use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use sysinfo::{Components, Disks, Networks, System};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, State, WebviewWindowBuilder,
};

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct CpuInfo {
    pub total_usage: f32,
    pub per_core: Vec<f32>,
    pub temperature: Option<f32>,
    pub throttling: bool,
    pub fan_speeds: Vec<u32>,
}

#[derive(Serialize, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub memory_mb: u64,
}

#[derive(Serialize, Clone)]
pub struct RamInfo {
    pub used_mb: u64,
    pub total_mb: u64,
    pub top_processes: Vec<ProcessInfo>,
}

#[derive(Serialize, Clone)]
pub struct BatteryInfo {
    pub level: f32,
    pub charging: bool,
    pub cycle_count: Option<u32>,
}

#[derive(Serialize, Clone)]
pub struct NetworkInfo {
    pub download_kbps: f64,
    pub upload_kbps: f64,
}

#[derive(Serialize, Clone)]
pub struct DiskInfo {
    pub used_gb: f64,
    pub total_gb: f64,
    pub name: String,
}

#[derive(Serialize, Clone)]
pub struct Metrics {
    pub cpu: CpuInfo,
    pub ram: RamInfo,
    pub battery: Option<BatteryInfo>,
    pub network: NetworkInfo,
    pub disk: Option<DiskInfo>,
}

// ── App state ─────────────────────────────────────────────────────────────────

struct IconVariants {
    bright: (Vec<u8>, u32, u32),
    dim: (Vec<u8>, u32, u32),
}

struct AppState {
    sys: Mutex<System>,
    networks: Mutex<Networks>,
    pulsing: Arc<AtomicBool>,
    icons: Arc<IconVariants>,
}

// ── Icon helpers ──────────────────────────────────────────────────────────────

fn build_icon_variants() -> IconVariants {
    let png_bytes = include_bytes!("../icons/32x32.png");
    let img = image::load_from_memory(png_bytes)
        .expect("failed to decode icon")
        .to_rgba8();
    let (w, h) = (img.width(), img.height());
    let bright: Vec<u8> = img.into_raw();
    // dim: keep RGB, reduce alpha to ~35%
    let dim: Vec<u8> = bright
        .chunks(4)
        .flat_map(|px| [px[0], px[1], px[2], (px[3] as f32 * 0.35) as u8])
        .collect();
    IconVariants {
        bright: (bright, w, h),
        dim: (dim, w, h),
    }
}

fn make_image(data: &(Vec<u8>, u32, u32)) -> Image<'static> {
    Image::new_owned(data.0.clone(), data.1, data.2)
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
async fn get_metrics(state: State<'_, AppState>) -> Result<Metrics, String> {
    let (total_usage, per_core, used_mb, total_mb, procs, disk, download_kbps, upload_kbps, cpu_temp_sysinfo, battery) = {
        let mut sys = state.sys.lock().map_err(|e| e.to_string())?;
        sys.refresh_all();

        let total_usage = sys.global_cpu_usage();
        let per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let used_mb = sys.used_memory() / 1024 / 1024;
        let total_mb = sys.total_memory() / 1024 / 1024;

        let mut procs: Vec<ProcessInfo> = sys
            .processes()
            .values()
            .map(|p| ProcessInfo {
                name: p.name().to_string_lossy().to_string(),
                memory_mb: p.memory() / 1024 / 1024,
            })
            .collect();
        procs.sort_by(|a, b| b.memory_mb.cmp(&a.memory_mb));
        procs.truncate(5);

        let disks = Disks::new_with_refreshed_list();
        let disk = disks.list().first().map(|d| DiskInfo {
            used_gb: (d.total_space() - d.available_space()) as f64 / 1e9,
            total_gb: d.total_space() as f64 / 1e9,
            name: d.name().to_string_lossy().to_string(),
        });

        let components = Components::new_with_refreshed_list();
        let cpu_temp_sysinfo = components
            .list()
            .iter()
            .find(|c| {
                let label = c.label().to_lowercase();
                label.contains("cpu") || label.contains("core")
            })
            .and_then(|c| c.temperature());

        let battery = get_battery_info();
        drop(sys);

        let mut nets = state.networks.lock().map_err(|e| e.to_string())?;
        nets.refresh(false);
        let (rx, tx) = nets.list().values().fold((0u64, 0u64), |(r, t), n| {
            (r + n.received(), t + n.transmitted())
        });
        let download_kbps = rx as f64 / 1024.0;
        let upload_kbps = tx as f64 / 1024.0;

        (total_usage, per_core, used_mb, total_mb, procs, disk, download_kbps, upload_kbps, cpu_temp_sysinfo, battery)
    };

    #[cfg(target_os = "macos")]
    let (cpu_temp, throttling, fan_speeds) = match get_powermetrics().await {
        Ok(pm) => (pm.cpu_temp.or(cpu_temp_sysinfo), pm.throttling, pm.fan_speeds),
        Err(_) => (cpu_temp_sysinfo, false, vec![]),
    };

    #[cfg(not(target_os = "macos"))]
    let (cpu_temp, throttling, fan_speeds) = (cpu_temp_sysinfo, false, vec![]);

    Ok(Metrics {
        cpu: CpuInfo { total_usage, per_core, temperature: cpu_temp, throttling, fan_speeds },
        ram: RamInfo { used_mb, total_mb, top_processes: procs },
        battery,
        network: NetworkInfo { download_kbps, upload_kbps },
        disk,
    })
}

#[tauri::command]
async fn set_pulsing(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    active: bool,
) -> Result<(), String> {
    state.pulsing.store(active, Ordering::SeqCst);

    if active {
        let app_clone = app.clone();
        let pulsing = state.pulsing.clone();
        let icons = state.icons.clone();

        tokio::spawn(async move {
            let mut bright_frame = true;
            loop {
                if !pulsing.load(Ordering::SeqCst) {
                    // Restore full-brightness icon on stop
                    if let Some(tray) = app_clone.tray_by_id("pulse-tray") {
                        let _ = tray.set_icon(Some(make_image(&icons.bright)));
                    }
                    break;
                }
                if let Some(tray) = app_clone.tray_by_id("pulse-tray") {
                    let icon = if bright_frame {
                        make_image(&icons.bright)
                    } else {
                        make_image(&icons.dim)
                    };
                    let _ = tray.set_icon(Some(icon));
                }
                bright_frame = !bright_frame;
                tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;
            }
        });
    }

    Ok(())
}

// ── Battery (macOS) ───────────────────────────────────────────────────────────

fn get_battery_info() -> Option<BatteryInfo> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let out = Command::new("pmset").args(["-g", "batt"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        parse_pmset_battery(&text)
    }
    #[cfg(not(target_os = "macos"))]
    None
}

#[cfg(target_os = "macos")]
fn parse_pmset_battery(text: &str) -> Option<BatteryInfo> {
    let line = text.lines().find(|l| l.contains('%'))?;
    let pct_str = line.split('%').next()?.trim().split_whitespace().last()?;
    let level: f32 = pct_str.parse().ok()?;
    let charging = line.contains("charging") && !line.contains("discharging");
    Some(BatteryInfo { level, charging, cycle_count: get_battery_cycle_count() })
}

#[cfg(target_os = "macos")]
fn get_battery_cycle_count() -> Option<u32> {
    use std::process::Command;
    let out = Command::new("system_profiler").args(["SPPowerDataType"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.to_lowercase().contains("cycle count") {
            return line.split(':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

// ── powermetrics (macOS) ──────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
struct PowermetricsResult {
    cpu_temp: Option<f32>,
    throttling: bool,
    fan_speeds: Vec<u32>,
}

#[cfg(target_os = "macos")]
async fn get_powermetrics() -> Result<PowermetricsResult, String> {
    use tokio::process::Command;
    let out = Command::new("powermetrics")
        .args(["--samplers", "smc", "-n", "1", "-i", "100"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let text = String::from_utf8_lossy(&out.stdout);
    let mut cpu_temp: Option<f32> = None;
    let mut throttling = false;
    let mut fan_speeds: Vec<u32> = vec![];

    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("cpu die temperature") || lower.contains("cpu temperature") {
            if let Some(v) = parse_first_number(line) { cpu_temp = Some(v); }
        }
        if lower.contains("fan") && lower.contains("rpm") {
            if let Some(v) = parse_first_number(line) { fan_speeds.push(v as u32); }
        }
        if lower.contains("thermal level") || lower.contains("cpu throttle") {
            throttling = line.contains("1") || lower.contains("true") || lower.contains("yes");
        }
    }

    Ok(PowermetricsResult { cpu_temp, throttling, fan_speeds })
}

fn parse_first_number(s: &str) -> Option<f32> {
    s.split_whitespace()
        .find_map(|t| t.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-').parse().ok())
}

// ── Window ────────────────────────────────────────────────────────────────────

fn toggle_popover(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("popover") {
        let visible = win.is_visible().unwrap_or(false);
        if visible {
            let _ = win.hide();
        } else {
            let _ = win.set_always_on_top(true);
            let _ = win.show();
            let _ = win.set_focus();
        }
        return;
    }

    // First open — create the window
    let _ = WebviewWindowBuilder::new(app, "popover", tauri::WebviewUrl::App("/".into()))
        .title("Pulse")
        .inner_size(320.0, 580.0)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .always_on_top(true)
        .visible(false)
        .skip_taskbar(true)
        .build()
        .expect("failed to build popover window");
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let icons = Arc::new(build_icon_variants());

    tauri::Builder::default()
        .manage(AppState {
            sys: Mutex::new(System::new_all()),
            networks: Mutex::new(Networks::new_with_refreshed_list()),
            pulsing: Arc::new(AtomicBool::new(false)),
            icons,
        })
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let quit = MenuItem::with_id(app, "quit", "Quit Pulse", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;

            let state = app.state::<AppState>();
            let bright_icon = make_image(&state.icons.bright);

            TrayIconBuilder::with_id("pulse-tray")
                .icon(bright_icon)
                .tooltip("Pulse")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .build(app)?
                .on_tray_icon_event({
                    let app_handle = app.handle().clone();
                    move |_tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            toggle_popover(&app_handle);
                        }
                    }
                });

            app.on_menu_event({
                let app_handle = app.handle().clone();
                move |_app, event| {
                    if event.id().as_ref() == "quit" {
                        app_handle.exit(0);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_metrics, set_pulsing])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
