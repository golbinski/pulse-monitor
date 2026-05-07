<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

interface ProcessInfo {
  name: string
  memory_mb: number
}

interface CpuProcess {
  name: string
  cpu_pct: number
}

interface Metrics {
  cpu: {
    total_usage: number
    per_core: number[]
    temperature: number | null
    throttling: boolean
    fan_speeds: number[]
  }
  top_cpu_processes: CpuProcess[]
  ram: {
    used_mb: number
    total_mb: number
    top_processes: ProcessInfo[]
  }
  battery: {
    level: number
    charging: boolean
    cycle_count: number | null
  } | null
  network: {
    download_kbps: number
    upload_kbps: number
  }
  disk: {
    used_gb: number
    total_gb: number
    name: string
  } | null
}

const metrics = ref<Metrics | null>(null)
const error = ref<string | null>(null)
const alwaysOnTop = ref(true)
const contextMenu = ref<{ x: number; y: number } | null>(null)
let pollTimer: ReturnType<typeof setInterval> | null = null

async function fetchMetrics() {
  try {
    metrics.value = await invoke<Metrics>('get_metrics')
    error.value = null
  } catch (e) {
    error.value = String(e)
  }
}

async function startPolling() {
  if (pollTimer) return
  await invoke('set_pulsing', { active: true })
  fetchMetrics()
  pollTimer = setInterval(fetchMetrics, 1000)
}

async function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer)
    pollTimer = null
  }
  await invoke('set_pulsing', { active: false })
}

async function hideWindow() {
  closeContextMenu()
  await stopPolling()
  await getCurrentWindow().hide()
}

async function toggleAlwaysOnTop() {
  alwaysOnTop.value = !alwaysOnTop.value
  await getCurrentWindow().setAlwaysOnTop(alwaysOnTop.value)
  closeContextMenu()
}

function openContextMenu(e: MouseEvent) {
  e.preventDefault()
  contextMenu.value = { x: e.clientX, y: e.clientY }
}

function closeContextMenu() {
  contextMenu.value = null
}

function bar(pct: number, width = 20): string {
  const filled = Math.round((pct / 100) * width)
  return '[' + '█'.repeat(filled) + '░'.repeat(width - filled) + ']'
}

function fmt(n: number, decimals = 1): string {
  return n.toFixed(decimals)
}

function fmtNet(kbps: number): string {
  if (kbps >= 1024) return fmt(kbps / 1024) + ' MB/s'
  return fmt(kbps) + ' KB/s'
}

onMounted(async () => {
  const win = getCurrentWindow()
  await win.show()
  await win.setAlwaysOnTop(true)
  await win.setFocus()
  startPolling()
})

onUnmounted(() => stopPolling())
</script>

<template>
  <div class="pulse" @contextmenu="openContextMenu" @click="closeContextMenu">

    <!-- Drag handle -->
    <div class="drag-bar" data-tauri-drag-region>
      <span class="drag-title" data-tauri-drag-region>PULSE</span>
      <span class="drag-hint" data-tauri-drag-region>⠿</span>
    </div>

    <!-- Context menu -->
    <Teleport to="body">
      <div
        v-if="contextMenu"
        class="ctx-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
        @click.stop
      >
        <div class="ctx-item" @click="toggleAlwaysOnTop">
          {{ alwaysOnTop ? 'Send to Background' : 'Bring to Front' }}
        </div>
        <div class="ctx-separator"></div>
        <div class="ctx-item" @click="hideWindow">Hide</div>
      </div>
    </Teleport>

    <div v-if="error" class="err">ERR: {{ error }}</div>

    <template v-if="metrics">
      <!-- CPU -->
      <div class="section-header">CPU</div>
      <div class="row">
        <span class="label">Total</span>
        <span class="bar">{{ bar(metrics.cpu.total_usage) }}</span>
        <span class="val">{{ fmt(metrics.cpu.total_usage) }}%</span>
      </div>
      <div v-for="(usage, i) in metrics.cpu.per_core" :key="i" class="row core-row">
        <span class="label">c{{ i }}</span>
        <span class="bar">{{ bar(usage, 14) }}</span>
        <span class="val">{{ fmt(usage) }}%</span>
      </div>
      <div v-if="metrics.cpu.temperature !== null" class="row">
        <span class="label">Temp</span>
        <span class="val accent">{{ fmt(metrics.cpu.temperature) }}°C</span>
        <span v-if="metrics.cpu.throttling" class="warn"> THROTTLING</span>
      </div>
      <div v-if="metrics.cpu.fan_speeds.length" class="row">
        <span class="label">Fans</span>
        <span class="val">{{ metrics.cpu.fan_speeds.map(f => f + ' RPM').join('  ') }}</span>
      </div>

      <!-- Top CPU processes -->
      <div class="section-header">CPU PROCESSES</div>
      <div v-for="(proc, i) in metrics.top_cpu_processes" :key="i" class="row proc-row">
        <span class="proc-name">{{ proc.name.slice(0, 18).padEnd(18) }}</span>
        <span class="val dim">{{ fmt(proc.cpu_pct, 1) }}%</span>
      </div>

      <!-- RAM -->
      <div class="section-header">MEMORY</div>
      <div class="row">
        <span class="label">RAM</span>
        <span class="bar">{{ bar((metrics.ram.used_mb / metrics.ram.total_mb) * 100) }}</span>
        <span class="val">{{ fmt(metrics.ram.used_mb / 1024) }}/{{ fmt(metrics.ram.total_mb / 1024) }} GB</span>
      </div>
      <div v-for="(proc, i) in metrics.ram.top_processes" :key="i" class="row proc-row">
        <span class="proc-name">{{ proc.name.slice(0, 18).padEnd(18) }}</span>
        <span class="val dim">{{ proc.memory_mb }} MB</span>
      </div>

      <!-- Network -->
      <div class="section-header">NETWORK</div>
      <div class="row">
        <span class="label">↓</span>
        <span class="val">{{ fmtNet(metrics.network.download_kbps) }}</span>
      </div>
      <div class="row">
        <span class="label">↑</span>
        <span class="val">{{ fmtNet(metrics.network.upload_kbps) }}</span>
      </div>

      <!-- Disk -->
      <template v-if="metrics.disk">
        <div class="section-header">DISK</div>
        <div class="row">
          <span class="label">{{ metrics.disk.name.slice(0, 10) }}</span>
          <span class="bar">{{ bar((metrics.disk.used_gb / metrics.disk.total_gb) * 100) }}</span>
          <span class="val">{{ fmt(metrics.disk.used_gb) }}/{{ fmt(metrics.disk.total_gb) }} GB</span>
        </div>
      </template>

      <!-- Battery -->
      <template v-if="metrics.battery">
        <div class="section-header">BATTERY</div>
        <div class="row">
          <span class="label">Level</span>
          <span class="bar">{{ bar(metrics.battery.level) }}</span>
          <span class="val">{{ fmt(metrics.battery.level, 0) }}%{{ metrics.battery.charging ? ' ⚡' : '' }}</span>
        </div>
        <div v-if="metrics.battery.cycle_count !== null" class="row">
          <span class="label">Cycles</span>
          <span class="val">{{ metrics.battery.cycle_count }}</span>
        </div>
      </template>
    </template>

    <div v-else-if="!error" class="loading">loading…</div>
  </div>
</template>

<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body {
  width: 320px;
  background: transparent;
  overflow: hidden;
  user-select: none;
}
</style>

<style scoped>
.pulse {
  width: 320px;
  background: #0d0d0d;
  border: 1px solid #1e3a1e;
  border-radius: 8px;
  padding: 0 12px 12px;
  font-family: 'Menlo', 'Consolas', 'DejaVu Sans Mono', monospace;
  font-size: 11px;
  line-height: 1.6;
  color: #4af54a;
  overflow: hidden;
}

.drag-bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 0 4px;
  margin-bottom: 2px;
  cursor: grab;
  border-bottom: 1px solid #1a3a1a;
}
.drag-bar:active { cursor: grabbing; }

.drag-title {
  font-size: 10px;
  letter-spacing: 0.15em;
  color: #2db82d;
}

.drag-hint {
  color: #1a5a1a;
  font-size: 13px;
}

.section-header {
  color: #2a7a2a;
  margin-top: 6px;
  margin-bottom: 2px;
  font-size: 10px;
  letter-spacing: 0.05em;
}

.row {
  display: flex;
  align-items: baseline;
  gap: 4px;
  white-space: nowrap;
  overflow: hidden;
}

.label  { color: #2db82d; min-width: 38px; flex-shrink: 0; }
.bar    { color: #1a6b1a; font-size: 10px; flex-shrink: 0; }
.val    { color: #5fff5f; flex-shrink: 0; }
.accent { color: #ffdd44; }
.warn   { color: #ff4444; font-weight: bold; }
.dim    { color: #2a7a2a; }

.core-row .label { min-width: 20px; }
.proc-row        { padding-left: 4px; }
.proc-name       { color: #39a839; min-width: 130px; font-size: 10px; }

.loading { color: #2a7a2a; text-align: center; padding: 20px; }
.err     { color: #ff4444; font-size: 10px; word-break: break-all; }

.ctx-menu {
  position: fixed;
  background: #111;
  border: 1px solid #2a5a2a;
  border-radius: 5px;
  padding: 4px 0;
  z-index: 9999;
  min-width: 160px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.7);
}

.ctx-item {
  padding: 6px 14px;
  font-family: 'Menlo', 'Consolas', monospace;
  font-size: 11px;
  color: #4af54a;
  cursor: pointer;
  white-space: nowrap;
}
.ctx-item:hover {
  background: #1a3a1a;
  color: #7fff7f;
}

.ctx-separator {
  height: 1px;
  background: #1e3a1e;
  margin: 3px 0;
}
</style>
