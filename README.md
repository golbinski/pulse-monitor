# Pulse

A conky-inspired system monitor that lives in your macOS menubar or Windows system tray. Built with Tauri v2 and Vue 3.

## What it shows

- CPU total usage + per-core bars
- CPU temperature and throttling state
- Fan speeds (macOS: via powermetrics)
- RAM used/total with top 5 memory-consuming processes
- Battery level, charge state, cycle count
- Network download/upload speed
- Disk usage for the main volume

## One-time setup (macOS) — passwordless powermetrics

CPU temperature and fan data on macOS require `powermetrics`, which normally demands sudo. Grant it without a password:

```
sudo visudo -f /etc/sudoers.d/pulse-powermetrics
```

Add this line (replace `yourusername` with your actual macOS username):

```
yourusername ALL=(root) NOPASSWD: /usr/bin/powermetrics
```

Save and exit. Pulse will then be able to read SMC sensor data (temperature, fans) without prompting.

> Without this step the app still works — temperature and fan fields will be absent, everything else is unaffected.

## Development

Prerequisites: Rust, Node 18+, Tauri v2 system deps (see https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

The signed `.app` (macOS) or `.exe`/`.msi` (Windows) ends up in `src-tauri/target/release/bundle/`.

## Releasing

Create and push a tag, then publish a GitHub release from it — the Actions workflow builds macOS (Apple Silicon + Intel) and Windows installers automatically and attaches them to the release.

```bash
git tag v0.1.0
git push origin v0.1.0
```

## How it works

- The app hides from the Dock and lives entirely in the system tray.
- Clicking the tray icon toggles the popover open/closed.
- The window is draggable — grab the `PULSE` header bar to reposition it anywhere on screen.
- Right-click the window for options: **Send to Background** (removes always-on-top), **Bring to Front**, and **Hide**.
- The Rust backend polls `sysinfo` for CPU, RAM, disk, and network. On macOS it spawns `powermetrics --samplers smc -n 1` to read SMC data (temperatures, fans). Battery info is read via `pmset` and `system_profiler`.
- The tray icon pulses while stats are streaming; resets when the panel is hidden.
- Polling is active only while the popover is visible — zero background CPU when closed.
- Update interval: 1 second.

## Recommended IDE setup

VS Code + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
