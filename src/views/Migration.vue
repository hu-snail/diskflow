<template>
  <div class="migration">
    <!-- Two-pane layout: local apps | external library -->
    <div class="pane-grid">
      <!-- LEFT: Mac local apps (/Applications) -->
      <div class="pane">
        <div class="pane-header">
          <div class="pane-title">
            <span class="dot dot-local"></span>
            <span class="title-text">Mac 本地应用</span>
            <span class="title-sub">/Applications</span>
          </div>
          <div class="pane-actions">
            <n-button size="tiny" quaternary @click="loadLocal">↻</n-button>
          </div>
        </div>

        <div class="pane-body">
          <div v-if="localScanning" class="pane-loading">扫描中…</div>
          <template v-else>
            <div
              v-for="item in sortedLocal"
              :key="item.path"
              class="app-row"
              :class="{ selected: selectedLocal.has(item.path) }"
              @click="toggleSelect('local', item.path)"
            >
              <n-checkbox
                :checked="selectedLocal.has(item.path)"
                @update:checked="toggleSelect('local', item.path)"
                @click.stop
              />
              <div class="app-icon-wrapper">
                <img v-if="iconFor(item.name)" :src="iconFor(item.name)" class="app-icon" />
                <span v-else class="app-icon-placeholder cat-app"></span>
              </div>
              <div class="app-meta">
                <div class="app-name">{{ item.name }}</div>
                <div class="app-state">
                  <n-tag size="tiny" type="info" round>App</n-tag>
                  <n-tag v-if="item.is_appstore" size="tiny" type="warning" round>商店</n-tag>
                  <n-tag v-if="item.is_running" size="tiny" type="error" round>运行中</n-tag>
                  <n-tag v-if="item.migrated" size="tiny" type="success" round>已迁移</n-tag>
                </div>
              </div>
              <div class="app-size-cell">
                <span :class="{ computing: computingSizes.has(item.path) }">
                  <template v-if="item.size > 0">{{ formatBytes(item.size) }}</template>
                  <template v-else-if="computingSizes.has(item.path)">计算中…</template>
                  <template v-else>—</template>
                </span>
              </div>
            </div>
            <div v-if="!sortedLocal.length" class="pane-empty">无应用</div>
          </template>
        </div>

        <div class="pane-footer">
          <n-button
            type="primary"
            block
            :disabled="!selectedLocal.size || isRunning || hasRunningSelected"
            :loading="isRunning"
            @click="migrateSelectedToExternal"
          >
            <span class="footer-label">迁移到外部</span>
            <span class="footer-meta" v-if="selectedLocal.size">已选 {{ selectedLocal.size }}</span>
            <span class="footer-arrow">→</span>
          </n-button>
        </div>
      </div>

      <!-- RIGHT: External library -->
      <div class="pane">
        <div class="pane-header">
          <div class="pane-title">
            <span class="dot dot-external"></span>
            <span class="title-text">外部应用库</span>
            <span class="title-sub" :title="externalDir">{{ externalDir }}</span>
          </div>
          <div class="pane-actions">
            <n-button size="tiny" quaternary @click="pickExternalDir">选择文件夹</n-button>
            <n-button size="tiny" quaternary @click="loadExternal">↻</n-button>
          </div>
        </div>

        <div class="pane-body">
          <div v-if="externalScanning" class="pane-loading">扫描中…</div>
          <template v-else>
            <div
              v-for="item in sortedExternal"
              :key="item.path"
              class="app-row"
              :class="{ selected: selectedExternal.has(item.path) }"
              @click="toggleSelect('external', item.path)"
            >
              <n-checkbox
                :checked="selectedExternal.has(item.path)"
                @update:checked="toggleSelect('external', item.path)"
                @click.stop
              />
              <div class="app-icon-wrapper">
                <img v-if="iconFor(item.name)" :src="iconFor(item.name)" class="app-icon" />
                <span v-else class="app-icon-placeholder cat-app"></span>
              </div>
              <div class="app-meta">
                <div class="app-name">{{ item.name }}</div>
                <div class="app-state">
                  <n-tag size="tiny" type="info" round>App</n-tag>
                  <n-tag v-if="localHasName(item.name)" size="tiny" type="success" round>已链接本地</n-tag>
                </div>
              </div>
              <div class="app-size-cell">
                <span :class="{ computing: computingSizes.has(item.path) }">
                  <template v-if="item.size > 0">{{ formatBytes(item.size) }}</template>
                  <template v-else-if="computingSizes.has(item.path)">计算中…</template>
                  <template v-else>—</template>
                </span>
              </div>
            </div>
            <div v-if="!sortedExternal.length" class="pane-empty">无应用</div>
          </template>
        </div>

        <div class="pane-footer pane-footer-split">
          <n-button
            type="warning"
            block
            :disabled="!selectedExternal.size || isRunning"
            :loading="isRunning"
            @click="linkExternalToLocal"
          >
            <span class="footer-arrow">←</span>
            <span class="footer-label">链接回本地</span>
            <span class="footer-meta" v-if="selectedExternal.size">已选 {{ selectedExternal.size }}</span>
          </n-button>
          <n-button
            type="primary"
            block
            :disabled="!selectedExternal.size || isRunning"
            :loading="isRunning"
            @click="migrateExternalToLocal"
          >
            <span class="footer-label">迁移回本地</span>
            <span class="footer-meta" v-if="selectedExternal.size">已选 {{ selectedExternal.size }}</span>
          </n-button>
        </div>
      </div>
    </div>

    <!-- Floating log panel: collapses to a small draggable circle when idle,
         expands into a regular drawer when active or clicked. -->
    <div
      class="log-panel"
      :class="{ open: drawerOpen, dragging: isDragging }"
      :style="panelStyle"
      @mouseenter="onDrawerEnter"
      @mouseleave="onDrawerLeave"
    >
      <!-- Collapsed circle button -->
      <div
        v-if="!drawerOpen"
        class="log-circle"
        @mousedown="onCircleMouseDown"
        @click="onCircleClick"
        title="操作日志"
      >
        <span v-if="taskActive" class="log-circle-pct">{{ progressPercent }}%</span>
        <span v-else class="log-circle-icon">≡</span>
      </div>

      <!-- Expanded drawer -->
      <div v-else class="log-drawer-inner">
        <div class="log-handle" @mousedown="onHandleMouseDown">
          <span class="log-handle-title">操作日志</span>
          <span class="log-handle-meta" v-if="taskActive">{{ progressPercent }}%</span>
          <span class="log-handle-toggle" @click.stop="closeDrawer">×</span>
        </div>
        <div class="log-body">
          <div class="log-header">
            <span>{{ logs.length }} 条日志</span>
            <n-button size="tiny" quaternary @click="logs = []">清屏</n-button>
          </div>
          <div class="log-container">
            <div v-for="(log, i) in logs" :key="i" class="log-line" :class="'log-' + log.level">
              <span class="log-time">{{ log.time }}</span>
              <span>{{ log.message }}</span>
            </div>
            <div v-if="!logs.length" class="log-empty">等待操作…</div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { NButton, NTag, NCheckbox, useMessage } from 'naive-ui'
import { open } from '@tauri-apps/plugin-dialog'
import { api, onProgress, onLog, onAppSize, formatBytes, type DiskInfo, type AppItem, type ProgressEvent, type LogEvent } from '../api/tauri'

interface AppItemWithIcon extends AppItem {
  icon: string
}

const props = defineProps<{ disks: DiskInfo[] }>()
useMessage()

const externalDir = ref('/Volumes/JZ-miniGo/Macos/App')

const localApps = ref<AppItemWithIcon[]>([])
const externalApps = ref<AppItemWithIcon[]>([])

const localScanning = ref(false)
const externalScanning = ref(false)

const selectedLocal = ref<Set<string>>(new Set())
const selectedExternal = ref<Set<string>>(new Set())

const isRunning = ref(false)
const progress = ref<ProgressEvent>({ current: 0, total: 0, file: '', app: '', app_index: 0, app_total: 0 })
const logs = ref<LogEvent[]>([])
const computingSizes = ref(new Set<string>())

// Drawer state
const drawerOpen = ref(false)
const drawerHover = ref(false)
const userClosed = ref(false)

// Floating panel position (px from the bottom-right of the container).
// The user can drag the circle or drawer header to reposition it; the
// position is preserved across open/close cycles.
const panelOffsetX = ref(16)
const panelOffsetY = ref(16)
const isDragging = ref(false)
let dragStartX = 0
let dragStartY = 0
let dragOriginX = 0
let dragOriginY = 0
let dragMoved = false
const DRAG_THRESHOLD = 4 // px — distinguish click vs drag

const panelStyle = computed(() => ({
  right: panelOffsetX.value + 'px',
  bottom: panelOffsetY.value + 'px',
}))

let unlistenProgress: (() => void) | null = null
let unlistenLog: (() => void) | null = null
let unlistenAppSize: (() => void) | null = null
let closeTimer: number | null = null
let runningPollTimer: number | null = null

const taskActive = computed(() => isRunning.value || progress.value.total > 0)
const progressPercent = computed(() => {
  if (!progress.value.total) return 0
  return Math.min(100, Math.round((progress.value.current / progress.value.total) * 100))
})
const logDrawerClosed = computed(() => !drawerOpen.value)

// True if the user has selected any local app whose main process is
// currently running. Migrating such an app would either fail mid-flight
// or leave the process holding a soon-to-be-disconnected inode, so we
// block the action at the UI level. The backend also enforces this.
const hasRunningSelected = computed(() => {
  for (const path of selectedLocal.value) {
    const item = localApps.value.find(a => a.path === path)
    if (item?.is_running) return true
  }
  return false
})

// Build a name lookup for the local pane so the external pane can highlight
// apps that are already linked into /Applications.
const localNames = computed(() => {
  const m = new Set<string>()
  for (const a of localApps.value) m.add(a.name)
  return m
})
function localHasName(name: string): boolean {
  return localNames.value.has(name)
}

// Shared icon cache keyed by app basename. Icons are fetched once per
// unique name and reused on both panes — this means a single app shown
// on both sides uses one image, no extra backend call.
const iconCache = new Map<string, string>()
function iconFor(name: string): string {
  return iconCache.get(name) || ''
}
function setIconFor(name: string, data: string) {
  if (!data) return
  iconCache.set(name, data)
  // Push the icon onto any matching items in either pane so it shows
  // immediately without waiting for the next render cycle.
  for (const list of [localApps.value, externalApps.value]) {
    for (const it of list) {
      if (it.name === name && !it.icon) it.icon = data
    }
  }
}

// Sorted panes — alphabetical by name so the left and right panes line
// up row-for-row when both contain the same app.
const sortedLocal = computed(() =>
  [...localApps.value].sort((a, b) => a.name.localeCompare(b.name))
)
const sortedExternal = computed(() =>
  [...externalApps.value].sort((a, b) => a.name.localeCompare(b.name))
)

async function loadIconsFor(names: string[]) {
  // Fetch icons for any names we don't have yet, then write them into
  // the shared cache. Each `.app` only triggers one icon request even if
  // it appears on both panes.
  const missing = names.filter(n => !iconCache.has(n) && n.endsWith('.app'))
  for (const name of missing) {
    // Use the external path if we know it (the real app with full Info.plist)
    // — stub paths don't have Contents/Info.plist so the backend would
    // return an empty icon.
    let iconPath = ''
    const ext = externalApps.value.find(a => a.name === name)
    if (ext) iconPath = ext.path
    if (!iconPath) {
      const loc = localApps.value.find(a => a.name === name)
      if (loc) iconPath = loc.migrated && loc.symlink_target ? loc.symlink_target : loc.path
    }
    if (!iconPath) continue
    api.getAppIcon(iconPath)
      .then(data => { if (data) setIconFor(name, data) })
      .catch(() => {})
  }
}

function toggleSelect(side: 'local' | 'external', path: string) {
  const set = side === 'local' ? selectedLocal.value : selectedExternal.value
  if (set.has(path)) set.delete(path)
  else set.add(path)
  // Trigger reactivity (Set mutations)
  if (side === 'local') selectedLocal.value = new Set(set)
  else selectedExternal.value = new Set(set)
}

async function loadLocal() {
  localScanning.value = true
  try {
    const items = await api.scanApps()
    localApps.value = items.map(a => ({ ...a, icon: iconFor(a.name) }))
    localScanning.value = false

    // Local pane: report the *stub/link* size — the bytes the entry
    // actually occupies on the local disk. For real symlinks that's
    // the symlink itself (a few bytes); for stub `.app` directories
    // it's the placeholder launcher. Pass the path with no target
    // override so the backend walks the local path, not the external
    // copy.
    const need = localApps.value.filter(a => a.size === 0)
    if (need.length) {
      const paths = need.map(a => a.path)
      const targets = paths.map(_ => '')
      for (const p of paths) computingSizes.value.add(p)
      computingSizes.value = new Set(computingSizes.value)
      api.computeAppSizes(paths, targets).catch(() => {})
    }

    // Lazy-load icons (deduped across both panes via iconCache).
    loadIconsFor(items.map(a => a.name))
  } catch (e) {
    console.error('scanApps error:', e)
    localScanning.value = false
  }
}

async function loadExternal() {
  externalScanning.value = true
  try {
    const items = await api.scanExternalApps(externalDir.value)
    externalApps.value = items.map(a => ({ ...a, icon: iconFor(a.name) }))
    externalScanning.value = false

    // External pane: report the *real* data size of the external copy.
    // We pass the path with an empty target so the backend walks the
    // external `.app` directly (it already lives on the external disk).
    const need = externalApps.value.filter(a => a.size === 0)
    if (need.length) {
      const paths = need.map(a => a.path)
      const targets = paths.map(_ => '')
      for (const p of paths) computingSizes.value.add(p)
      computingSizes.value = new Set(computingSizes.value)
      api.computeAppSizes(paths, targets).catch(() => {})
    }

    loadIconsFor(items.map(a => a.name))
  } catch (e) {
    console.error('scanExternalApps error:', e)
    externalScanning.value = false
  }
}

async function pickExternalDir() {
  try {
    const picked = await open({ directory: true, multiple: false, defaultPath: externalDir.value })
    if (typeof picked === 'string') {
      externalDir.value = picked
      await loadExternal()
    }
  } catch (e) {
    console.error('pickExternalDir error:', e)
  }
}

async function migrateSelectedToExternal() {
  if (!selectedLocal.value.size) return
  if (hasRunningSelected.value) {
    // Surface a clear log line so the user sees *why* the action was
    // blocked. The button itself is disabled, but a stray programmatic
    // call or stale UI state could otherwise silently no-op.
    if (!userClosed.value) drawerOpen.value = true
    logs.value.push({
      level: 'error',
      message: '所选应用中包含正在运行的应用，请先退出后再迁移',
      time: new Date().toLocaleTimeString(),
    })
    return
  }
  isRunning.value = true
  logs.value = []
  if (!userClosed.value) drawerOpen.value = true
  try {
    for (const src of selectedLocal.value) {
      await api.migrateApp(src, externalDir.value)
    }
  } catch (e) {
    logs.value.push({ level: 'error', message: (e as Error).message, time: '' })
  }
  isRunning.value = false
  selectedLocal.value = new Set()
  selectedExternal.value = new Set()
  await loadLocal()
  await loadExternal()
}

async function linkExternalToLocal() {
  // For each selected external app, copy/link it back to /Applications.
  // Reuse migrate_app with the external app as source and /Applications as
  // target — this performs the standard copy + verify + symlink flow but in
  // reverse direction (target is local, source is external).
  if (!selectedExternal.value.size) return
  isRunning.value = true
  logs.value = []
  if (!userClosed.value) drawerOpen.value = true
  const localRoot = '/Applications'
  try {
    for (const src of selectedExternal.value) {
      await api.migrateApp(src, localRoot)
    }
  } catch (e) {
    logs.value.push({ level: 'error', message: (e as Error).message, time: '' })
  }
  isRunning.value = false
  selectedExternal.value = new Set()
  await loadLocal()
  await loadExternal()
}

async function migrateExternalToLocal() {
  // Same as linkExternalToLocal — pulls the external app into /Applications
  // and replaces any stub. Kept as a separate button so users can distinguish
  // intent ("link" vs "migrate") even though the underlying operation is the
  // same for now.
  return linkExternalToLocal()
}

function onDrawerEnter() {
  drawerHover.value = true
  if (closeTimer !== null) { clearTimeout(closeTimer); closeTimer = null }
}

function onDrawerLeave() {
  drawerHover.value = false
  if (userClosed.value) { drawerOpen.value = false; return }
  if (taskActive.value || logs.value.length) {
    closeTimer = window.setTimeout(() => {
      if (!drawerHover.value) drawerOpen.value = false
    }, 800)
  }
}

function closeDrawer() {
  drawerOpen.value = false
  userClosed.value = true
}

function openDrawer() {
  drawerOpen.value = true
  userClosed.value = false
}

// ---------- Drag handling ----------

function clampOffsets() {
  // Keep the panel fully inside the .migration container. We only know the
  // current pixel sizes after layout, so query the container and the panel.
  const container = document.querySelector('.migration') as HTMLElement | null
  const panel = document.querySelector('.log-panel') as HTMLElement | null
  if (!container || !panel) return
  const cw = container.clientWidth
  const ch = container.clientHeight
  const pw = panel.offsetWidth
  const ph = panel.offsetHeight
  const maxX = Math.max(0, cw - pw)
  const maxY = Math.max(0, ch - ph)
  if (panelOffsetX.value > maxX) panelOffsetX.value = maxX
  if (panelOffsetY.value > maxY) panelOffsetY.value = maxY
  if (panelOffsetX.value < 0) panelOffsetX.value = 0
  if (panelOffsetY.value < 0) panelOffsetY.value = 0
}

function beginDrag(clientX: number, clientY: number) {
  isDragging.value = true
  dragStartX = clientX
  dragStartY = clientY
  dragOriginX = panelOffsetX.value
  dragOriginY = panelOffsetY.value
  dragMoved = false
  mouseDownPos = { x: clientX, y: clientY }
  window.addEventListener('mousemove', onWindowMouseMove)
  window.addEventListener('mouseup', onWindowMouseUp)
}

function onWindowMouseMove(e: MouseEvent) {
  if (!isDragging.value) return
  const dx = e.clientX - dragStartX
  const dy = e.clientY - dragStartY
  if (!dragMoved && (Math.abs(dx) > DRAG_THRESHOLD || Math.abs(dy) > DRAG_THRESHOLD)) {
    dragMoved = true
  }
  if (dragMoved) {
    // Dragging right increases clientX; we want right-offset to *decrease*.
    panelOffsetX.value = Math.max(0, dragOriginX - dx)
    panelOffsetY.value = Math.max(0, dragOriginY - dy)
  }
}

function onWindowMouseUp() {
  window.removeEventListener('mousemove', onWindowMouseMove)
  window.removeEventListener('mouseup', onWindowMouseUp)
  isDragging.value = false
  if (dragMoved) clampOffsets()
}

function onCircleMouseDown(e: MouseEvent) {
  // Only left button starts drag.
  if (e.button !== 0) return
  beginDrag(e.clientX, e.clientY)
}

function onCircleClick(_e: MouseEvent) {
  // If a drag occurred, suppress the click. The drag flag is set by the
  // mousemove handler before mouseup.
  if (dragMoved) return
  openDrawer()
}

function onHandleMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  // Allow native click on the × toggle (it stops propagation).
  beginDrag(e.clientX, e.clientY)
}

watch(isRunning, (running, prev) => {
  if (running && !prev) {
    userClosed.value = false
    drawerOpen.value = true
  }
})

watch([panelOffsetX, panelOffsetY], () => {
  // Clamp after layout so we don't snap to invalid offsets if the window
  // shrinks while the panel is positioned near the edge.
  requestAnimationFrame(clampOffsets)
})

onMounted(async () => {
  unlistenProgress = await onProgress((e) => { progress.value = e })
  unlistenLog = await onLog((e) => { logs.value.push(e) })
  unlistenAppSize = await onAppSize((e) => {
    for (const list of [localApps.value, externalApps.value]) {
      const item = list.find(a => a.path === e.path)
      if (item) item.size = e.size
    }
    if (computingSizes.value.has(e.path)) {
      computingSizes.value.delete(e.path)
      computingSizes.value = new Set(computingSizes.value)
    }
  })

  await Promise.all([loadLocal(), loadExternal()])

  // Poll running state every 5s so the "运行中" tag appears/disappears
  // as the user launches or quits apps. We re-run only the lsof-backed
  // backend scan (lightweight — cached on the Rust side with a 3s TTL).
  runningPollTimer = window.setInterval(refreshRunningState, 5000)
})

async function refreshRunningState() {
  try {
    const [local, external] = await Promise.all([
      api.scanApps(),
      api.scanExternalApps(externalDir.value),
    ])
    // Merge sizes/icons we know locally into the freshly-scanned items so
    // we don't reset values the size callbacks haven't refreshed yet.
    const merge = (fresh: AppItemWithIcon[], prev: AppItemWithIcon[]) => {
      const prevByPath = new Map(prev.map(p => [p.path, p]))
      return fresh.map(f => {
        const old = prevByPath.get(f.path)
        return {
          ...f,
          size: f.size || old?.size || 0,
          icon: f.icon || old?.icon || '',
        }
      })
    }
    localApps.value = merge(local, localApps.value)
    externalApps.value = merge(external, externalApps.value)
  } catch (e) {
    // Silent — transient lsof failure shouldn't disrupt the UI.
  }
}

onUnmounted(() => {
  if (unlistenProgress) unlistenProgress()
  if (unlistenLog) unlistenLog()
  if (unlistenAppSize) unlistenAppSize()
  if (closeTimer !== null) clearTimeout(closeTimer)
  if (runningPollTimer !== null) clearInterval(runningPollTimer)
})
</script>

<style scoped>
.migration { padding: 8px; display: flex; flex-direction: column; height: 100%; position: relative; min-height: 0; }

/* Two-pane grid */
.pane-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  flex: 1;
  min-height: 0;
}
.pane {
  background: #141414;
  border: 1px solid #2a2a2a;
  border-radius: 10px;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}
.pane-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 10px 12px;
  background: #1a1a1a;
  border-bottom: 1px solid #2a2a2a;
  flex-shrink: 0;
}
.pane-title { display: flex; align-items: center; gap: 8px; min-width: 0; flex: 1; }
.dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.dot-local { background: #4080ff; }
.dot-external { background: #63e2b7; }
.title-text { font-size: 14px; font-weight: 600; color: #e0e0e0; }
.title-sub { font-size: 11px; color: #666; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 220px; }
.pane-actions { display: flex; gap: 4px; }

.pane-body {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  padding: 4px 0;
}
.pane-loading, .pane-empty {
  text-align: center;
  padding: 32px;
  color: #555;
  font-size: 12px;
}

.app-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  border-bottom: 1px solid #1f1f1f;
  cursor: pointer;
  transition: background 0.12s;
  min-height: 52px;
}
.app-row:hover { background: #1c1c1c; }
.app-row.selected { background: #1e3a21; }
.app-row.selected:hover { background: #234a26; }

.app-icon-wrapper {
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}
.app-icon {
  width: 40px;
  height: 40px;
  object-fit: contain;
  border-radius: 8px;
  background: #1f1f1f;
  /* Subtle inner border that gives the icon depth on the dark UI
     without adding a heavy container. */
  box-shadow: inset 0 0 0 1px #2a2a2a;
}
.app-icon-placeholder { display: block; width: 32px; height: 32px; border-radius: 8px; background: #4080ff; }
.app-meta { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.app-name { font-size: 13px; color: #e0e0e0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.app-state { display: flex; gap: 4px; flex-wrap: wrap; }
.app-size-cell { font-size: 12px; color: #63e2b7; min-width: 70px; text-align: right; white-space: nowrap; flex-shrink: 0; }
.app-size-cell .computing { color: #888; font-style: italic; }

.pane-footer {
  padding: 8px;
  background: #1a1a1a;
  border-top: 1px solid #2a2a2a;
  flex-shrink: 0;
}
.pane-footer-split {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}
.footer-label { margin-right: 6px; }
.footer-meta { font-size: 11px; opacity: 0.85; margin: 0 6px; }
.footer-arrow { font-weight: 600; }

/* Floating log panel — collapsed circle, expandable drawer, draggable. */
.log-panel {
  position: absolute;
  z-index: 20;
  display: flex;
  flex-direction: column;
  /* `right` and `bottom` come from inline panelStyle (user-draggable). */
  user-select: none;
}
.log-panel.dragging { cursor: grabbing; }

/* Collapsed circle button (idle state) */
.log-circle {
  width: 56px;
  height: 56px;
  border-radius: 50%;
  background: #1a1a1a;
  border: 1px solid #2a2a2a;
  color: #63e2b7;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  transition: transform 0.12s ease, box-shadow 0.12s ease, border-color 0.12s ease;
  flex-shrink: 0;
}
.log-circle:hover {
  border-color: #63e2b7;
  transform: scale(1.05);
  box-shadow: 0 6px 16px rgba(0, 0, 0, 0.55);
}
.log-circle:active { cursor: grabbing; transform: scale(0.97); }
.log-circle-pct {
  font-size: 11px;
  font-weight: 600;
  color: #63e2b7;
}
.log-circle-icon {
  font-size: 22px;
  color: #888;
  line-height: 1;
}
.log-circle:hover .log-circle-icon { color: #ccc; }

/* Expanded drawer */
.log-drawer-inner {
  width: 360px;
  height: 340px;
  background: #1a1a1a;
  border: 1px solid #2a2a2a;
  border-radius: 10px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
}
.log-handle {
  display: flex; align-items: center; gap: 8px;
  padding: 8px 12px;
  background: #222;
  cursor: grab;
  flex-shrink: 0;
}
.log-handle:active { cursor: grabbing; }
.log-handle-title { font-size: 12px; color: #ccc; flex: 1; }
.log-handle-meta { font-size: 11px; color: #63e2b7; font-weight: 600; }
.log-handle-toggle {
  font-size: 16px;
  color: #888;
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  cursor: pointer;
  flex-shrink: 0;
}
.log-handle-toggle:hover { color: #f56c6c; background: #2a2a2a; }

.log-body { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.log-header { display: flex; justify-content: space-between; align-items: center; padding: 6px 12px; background: #1f1f1f; font-size: 11px; color: #888; flex-shrink: 0; }
.log-container { flex: 1; overflow-y: auto; padding: 8px 12px; font-family: 'SF Mono', monospace; font-size: 11px; line-height: 1.6; min-height: 0; }
.log-line { display: flex; gap: 8px; }
.log-time { color: #444; flex-shrink: 0; }
.log-success { color: #63e2b7; }
.log-error { color: #f56c6c; }
.log-warning { color: #e6a23c; }
.log-section { color: #4080ff; font-weight: 600; }
.log-info { color: #ccc; }
.log-empty { color: #555; text-align: center; padding: 20px; }
</style>
