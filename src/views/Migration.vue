<template>
  <div class="migration">
    <div class="page-title">应用迁移</div>

    <!-- Target config -->
    <div class="config-row">
      <span class="config-label">目标磁盘</span>
      <n-select v-model:value="targetDisk" :options="diskOptions" size="small" style="width: 200px" @update:value="onDiskChange" />
      <span class="config-label">路径</span>
      <n-input v-model:value="targetPath" size="small" style="width: 300px" />
      <n-button type="primary" size="small" :disabled="!selected.length || isRunning" :loading="isRunning" @click="startMigration">迁移 ({{ selected.length }})</n-button>
    </div>

    <!-- Category tabs -->
    <div class="tabs">
      <button :class="{ active: activeTab === 'all' }" @click="activeTab = 'all'">全部 ({{ apps.length }})</button>
      <button :class="{ active: activeTab === 'app' }" @click="activeTab = 'app'">应用 ({{ appCount }})</button>
      <button :class="{ active: activeTab === 'data' }" @click="activeTab = 'data'">数据 ({{ dataCount }})</button>
      <button :class="{ active: activeTab === 'migrated' }" @click="activeTab = 'migrated'">已迁移 ({{ migratedCount }})</button>
    </div>

    <!-- Left-right layout: app list | log panel -->
    <div class="main-layout">
      <!-- Left: app list + progress -->
      <div class="left-panel">
        <div v-if="scanning" class="scanning">扫描中...</div>
        <div v-else class="app-list">
          <div v-for="item in filteredApps" :key="item.path" class="app-item" :class="{ migrated: item.migrated }" :title="item.path">
            <n-checkbox :checked="selected.includes(item.path)" :disabled="item.migrated" @update:checked="toggleSelect(item.path)" />
            <div class="app-icon-wrapper">
              <img v-if="item.icon" :src="item.icon" class="app-icon" />
              <span v-else class="app-icon-placeholder" :class="categoryColor(item.category)"></span>
            </div>
            <span class="app-name">{{ item.name }}</span>
            <n-tag v-if="item.is_app" size="tiny" type="info" round>App</n-tag>
            <n-tag v-else size="tiny" :type="item.category === 'cache' ? 'warning' : 'default'" round>{{ categoryLabel(item.category) }}</n-tag>
            <span class="app-flex-grow"></span>
            <span class="app-size">{{ item.size > 0 ? formatBytes(item.size) : '...' }}</span>
            <div class="app-actions">
              <n-tag v-if="item.migrated" size="tiny" type="success" round>已迁移</n-tag>
              <n-button v-if="item.migrated" size="tiny" type="warning" quaternary :disabled="isRunning" @click.stop="rollbackApp(item)">还原</n-button>
              <n-button v-if="item.migrated && item.has_backup" size="tiny" type="error" quaterny :disabled="isRunning" @click.stop="cleanupBackup(item)">清理备份</n-button>
            </div>
          </div>
          <div v-if="!filteredApps.length && !scanning" class="empty">无数据</div>
        </div>

        <!-- Progress -->
        <div v-if="isRunning || progress.total > 0" class="progress-panel">
          <div class="progress-row">
            <span>{{ progress.app || '准备中' }}</span>
            <span>{{ progress.current }} / {{ progress.total }}</span>
          </div>
          <div class="progress-bar">
            <div class="progress-fill" :style="{ width: progressPercent + '%' }"></div>
          </div>
          <div v-if="progress.file" class="current-file">{{ progress.file }}</div>
        </div>
      </div>

      <!-- Right: log panel -->
      <div class="log-panel">
        <div class="log-header">
          <span>操作日志</span>
          <n-button size="tiny" quaterny @click="logs = []">清屏</n-button>
        </div>
        <div class="log-container">
          <div v-for="(log, i) in logs" :key="i" class="log-line" :class="'log-' + log.level">
            <span class="log-time">{{ log.time }}</span>
            <span>{{ log.message }}</span>
          </div>
          <div v-if="!logs.length" class="log-empty">等待操作...</div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { NSelect, NInput, NButton, NTag, NCheckbox } from 'naive-ui'
import { api, onProgress, onLog, onAppSize, formatBytes, type DiskInfo, type AppItem, type ProgressEvent, type LogEvent } from '../api/tauri'

interface AppItemWithIcon extends AppItem {
  icon: string
}

// Module-level cache: survives component re-mounts
let _cachedApps: AppItemWithIcon[] | null = null
let _iconCache: Map<string, string> = new Map()

const props = defineProps<{ disks: DiskInfo[] }>()

const targetDisk = ref<string | null>(null)
const targetPath = ref('')
const selected = ref<string[]>([])
const isRunning = ref(false)
const scanning = ref(false)
const apps = ref<AppItemWithIcon[]>([])
const activeTab = ref('all')
const progress = ref<ProgressEvent>({ current: 0, total: 0, file: '', app: '', app_index: 0, app_total: 0 })
const logs = ref<LogEvent[]>([])

let unlistenProgress: (() => void) | null = null
let unlistenLog: (() => void) | null = null
let unlistenAppSize: (() => void) | null = null

const diskOptions = computed(() =>
  props.disks.map(d => ({
    label: `${d.name || d.mount_point} (${formatBytes(d.available_space)} 可用)`,
    value: d.mount_point,
  }))
)

const filteredApps = computed(() => {
  if (activeTab.value === 'all') return apps.value
  if (activeTab.value === 'migrated') return apps.value.filter(a => a.migrated)
  if (activeTab.value === 'app') return apps.value.filter(a => a.is_app)
  if (activeTab.value === 'data') return apps.value.filter(a => !a.is_app)
  return apps.value
})

const appCount = computed(() => apps.value.filter(a => a.is_app).length)
const dataCount = computed(() => apps.value.filter(a => !a.is_app).length)
const migratedCount = computed(() => apps.value.filter(a => a.migrated).length)

const progressPercent = computed(() => {
  if (!progress.value.total) return 0
  return Math.min(100, Math.round((progress.value.current / progress.value.total) * 100))
})

function categoryLabel(cat: string): string {
  const map: Record<string, string> = { app: 'App', data: '数据', cache: '缓存', container: '容器' }
  return map[cat] || cat
}

function categoryColor(cat: string): string {
  const map: Record<string, string> = { app: 'cat-app', data: 'cat-data', cache: 'cat-cache', container: 'cat-container' }
  return map[cat] || 'cat-data'
}

function onDiskChange(val: string) {
  const disk = props.disks.find(d => d.mount_point === val)
  if (disk) {
    const name = disk.name || ''
    targetPath.value = name === 'AppData' ? disk.mount_point : disk.mount_point + '/AppData'
  }
}

function toggleSelect(path: string) {
  const idx = selected.value.indexOf(path)
  if (idx >= 0) selected.value.splice(idx, 1)
  else selected.value.push(path)
}

async function loadApps() {
  scanning.value = true
  try {
    // Phase 1: Fast scan — get all items immediately, no sizes
    const items = await api.scanApps()
    apps.value = items.map(a => ({ ...a, icon: _iconCache.get(a.path) || '' }))
    _cachedApps = apps.value
    scanning.value = false

    // Phase 2: Async size calculation for items without size (apps with min_size=0)
    const needSizes = apps.value.filter(a => a.size === 0).map(a => a.path)
    if (needSizes.length) {
      api.computeAppSizes(needSizes).catch(() => {})
    }

    // Phase 3: Lazy load icons for ALL .app bundles (including migrated)
    for (const item of apps.value) {
      if (item.is_app && !item.icon) {
        api.getAppIcon(item.path).then(icon => {
          if (icon) {
            item.icon = icon
            _iconCache.set(item.path, icon)
          }
        }).catch(() => {})
      }
    }
  } catch (e) {
    console.error('scanApps error:', e)
    scanning.value = false
    // Restore from cache on error
    if (_cachedApps) apps.value = _cachedApps
  }
}

async function startMigration() {
  if (!selected.value.length || !targetPath.value) return
  isRunning.value = true
  logs.value = []
  try {
    for (const src of selected.value) {
      await api.migrateApp(src, targetPath.value)
    }
  } catch (e) {
    logs.value.push({ level: 'error', message: (e as Error).message, time: '' })
  }
  isRunning.value = false
  selected.value = []
  await loadApps()
}

async function rollbackApp(item: AppItemWithIcon) {
  isRunning.value = true
  try {
    await api.rollbackApp(item.path)
  } catch (e) {
    logs.value.push({ level: 'error', message: (e as Error).message, time: '' })
  }
  isRunning.value = false
  await loadApps()
}

async function cleanupBackup(item: AppItemWithIcon) {
  isRunning.value = true
  try {
    await api.cleanupBackup(item.path)
  } catch (e) {
    logs.value.push({ level: 'error', message: (e as Error).message, time: '' })
  }
  isRunning.value = false
  await loadApps()
}

onMounted(async () => {
  // Restore from cache immediately if available
  if (_cachedApps) {
    apps.value = _cachedApps
    scanning.value = false
  }

  // Auto-select first removable disk
  const removable = props.disks.find(d => d.is_removable || d.is_sparse)
  if (removable) {
    targetDisk.value = removable.mount_point
    onDiskChange(removable.mount_point)
  }

  unlistenProgress = await onProgress((e) => { progress.value = e })
  unlistenLog = await onLog((e) => { logs.value.push(e) })
  unlistenAppSize = await onAppSize((e) => {
    // Update size for matching app item
    const item = apps.value.find(a => a.path === e.path)
    if (item) item.size = e.size
    // Also update cache
    if (_cachedApps) {
      const cached = _cachedApps.find(a => a.path === e.path)
      if (cached) cached.size = e.size
    }
  })

  // Only do a fresh scan if no cache
  if (!_cachedApps) {
    await loadApps()
  } else {
    // Refresh sizes in background for items without size
    const needSizes = apps.value.filter(a => a.size === 0).map(a => a.path)
    if (needSizes.length) {
      api.computeAppSizes(needSizes).catch(() => {})
    }
  }
})

onUnmounted(() => {
  if (unlistenProgress) unlistenProgress()
  if (unlistenLog) unlistenLog()
  if (unlistenAppSize) unlistenAppSize()
})
</script>

<style scoped>
.migration { padding: 8px; display: flex; flex-direction: column; height: 100%; }
.page-title { font-size: 20px; font-weight: 600; margin-bottom: 12px; }
.config-row { display: flex; gap: 8px; align-items: center; margin-bottom: 8px; flex-wrap: wrap; }
.config-label { font-size: 12px; color: #888; }

.tabs { display: flex; gap: 4px; margin-bottom: 8px; }
.tabs button { padding: 4px 12px; background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 6px; color: #888; cursor: pointer; font-size: 12px; }
.tabs button.active { background: #1e3a21; color: #63e2b7; border-color: #63e2b7; }

/* Left-right main layout */
.main-layout { display: flex; gap: 8px; flex: 1; min-height: 0; }
.left-panel { flex: 1; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
.log-panel { width: 360px; display: flex; flex-direction: column; background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; overflow: hidden; flex-shrink: 0; }

.scanning { text-align: center; padding: 40px; color: #666; }

.app-list { flex: 1; background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; overflow-y: auto; }
.app-item { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid #222; }
.app-item:hover { background: #1f1f1f; }
.app-item.migrated { opacity: 0.6; }
.app-icon-wrapper { width: 28px; height: 28px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; }
.app-icon { width: 24px; height: 24px; object-fit: contain; border-radius: 5px; }
.app-icon-placeholder { display: block; width: 20px; height: 20px; border-radius: 5px; }
.cat-app { background: #4080ff; }
.cat-data { background: #63e2b7; }
.cat-cache { background: #e6a23c; }
.cat-container { background: #9c27b0; }
.app-name { font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.app-flex-grow { flex: 1; }
.app-size { font-size: 12px; color: #63e2b7; min-width: 60px; text-align: right; white-space: nowrap; }
.app-actions { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
.empty { text-align: center; padding: 40px; color: #555; }

.progress-panel { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; padding: 12px; flex-shrink: 0; }
.progress-row { display: flex; justify-content: space-between; margin-bottom: 6px; font-size: 12px; }
.progress-bar { height: 8px; background: #2a2a2a; border-radius: 4px; overflow: hidden; }
.progress-fill { height: 100%; background: linear-gradient(90deg, #2080f0, #63e2b7); transition: width 0.3s; }
.current-file { font-size: 11px; color: #666; margin-top: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

.log-header { display: flex; justify-content: space-between; align-items: center; padding: 8px 12px; background: #222; font-size: 12px; flex-shrink: 0; }
.log-container { flex: 1; overflow-y: auto; padding: 8px 12px; font-family: 'SF Mono', monospace; font-size: 12px; line-height: 1.6; }
.log-line { display: flex; gap: 8px; }
.log-time { color: #444; flex-shrink: 0; }
.log-success { color: #63e2b7; }
.log-error { color: #f56c6c; }
.log-warning { color: #e6a23c; }
.log-section { color: #4080ff; font-weight: 600; }
.log-info { color: #ccc; }
.log-empty { color: #555; text-align: center; padding: 20px; }
</style>
