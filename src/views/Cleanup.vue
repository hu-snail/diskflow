<template>
  <div class="cleanup">
    <!-- ── Header ───────────────────────────────────────────────────────── -->
    <header class="header">
      <div class="header-titles">
        <div class="title-row">
          <h1 class="page-title">空间清理</h1>
          <span class="mode-badge">
            <n-icon :component="iconRadar" :size="12" />
            <span>智能深度扫描模式</span>
          </span>
        </div>
        <p class="page-sub">多维度深度扫描 · 已扫描结果自动缓存 · 勾选需清理项后一键释放</p>
      </div>

      <div class="header-stats">
        <div class="stat-card">
          <span class="stat-label">已扫描项</span>
          <span class="stat-value">{{ formatBytes(stats.scannedBytes) }} <small>{{ stats.scannedCount.toLocaleString() }} 项</small></span>
        </div>
        <div class="stat-card">
          <span class="stat-label">可释放空间</span>
          <span class="stat-value accent">{{ formatBytes(stats.reclaimableBytes) }}</span>
        </div>
      </div>
    </header>

    <!-- ── Filter pills ──────────────────────────────────────────────────── -->
    <div class="pills">
      <button
        v-for="p in pills"
        :key="p.key"
        class="pill"
        :class="{ active: activePill === p.key }"
        @click="switchPill(p.key)"
      >
        <n-icon :component="p.icon" :size="14" class="pill-icon" />
        <span class="pill-label">{{ p.label }}</span>
        <span class="pill-size">{{ formatBytes(pillSize(p.key)) }}</span>
      </button>
    </div>

    <!-- ── Progress card (shown only while scanning) ────────────────────── -->
    <Transition name="card">
      <div v-if="scanning" class="progress-card">
        <div class="progress-icon">
          <n-icon :component="iconRadar" :size="22" />
        </div>
        <div class="progress-body">
          <div class="progress-top">
            <div class="progress-status">
              <span class="progress-title">实时深度扫描进行中</span>
              <span class="progress-speed">速度 {{ formatNumber(scanSpeed) }} 文件/秒</span>
            </div>
            <div class="progress-percent">{{ scanPercent }}<small>%</small></div>
          </div>
          <div class="progress-current">
            <span class="progress-current-label">当前</span>
            <span class="progress-current-path" :title="currentItem">{{ currentItem || currentPhase }}</span>
          </div>
          <div class="progress-bar">
            <div class="progress-bar-fill" :style="{ width: scanPercent + '%' }"></div>
          </div>
          <div class="progress-footer">
            <span>已分析 {{ formatNumber(scanCurrent) }} 个文件</span>
            <span v-if="etaText">剩余 ~{{ etaText }}</span>
          </div>
        </div>
      </div>
    </Transition>

    <!-- ── Cleanup progress card (shown only while cleaning) ────────────── -->
    <Transition name="card">
      <div v-if="cleaning" class="progress-card progress-card-clean">
        <div class="progress-icon">
          <n-icon :component="iconTrash" :size="22" />
        </div>
        <div class="progress-body">
          <div class="progress-top">
            <div class="progress-status">
              <span class="progress-title">正在清理缓存</span>
              <span class="progress-speed">
                成功 {{ cleanDone }} 项{{ cleanFailed ? ` · 失败 ${cleanFailed} 项` : '' }}
              </span>
            </div>
            <div class="progress-percent">{{ cleanPercent }}<small>%</small></div>
          </div>
          <div class="progress-current">
            <span class="progress-current-label">当前</span>
            <span class="progress-current-path" :title="cleanCurrentPath">{{ cleanCurrentPath || '准备…' }}</span>
          </div>
          <div class="progress-bar">
            <div class="progress-bar-fill progress-bar-fill-clean" :style="{ width: cleanPercent + '%' }"></div>
          </div>
          <div class="progress-footer">
            <span>已处理 {{ cleanDone + cleanFailed }} / {{ cleanTotal }}</span>
            <span v-if="cleanEtaText">剩余 ~{{ cleanEtaText }}</span>
          </div>
          <!-- Per-item event log. Auto-sticks to the bottom so the
               newest "正在清理 X / 完成 X / 失败 X" line is always
               visible while older lines scroll off the top. -->
          <div class="clean-log" ref="cleanLogEl">
            <div v-for="(line, idx) in cleanLog" :key="idx" class="clean-log-line" :class="`status-${line.status}`">
              <span class="clean-log-time">{{ line.time }}</span>
              <span class="clean-log-mark">
                <template v-if="line.status === 'running'">⟳</template>
                <template v-else-if="line.status === 'done'">✓</template>
                <template v-else>✗</template>
              </span>
              <span class="clean-log-name" :title="line.name">{{ line.name }}</span>
              <span v-if="line.error" class="clean-log-err" :title="line.error">{{ line.error }}</span>
            </div>
          </div>
        </div>
      </div>
    </Transition>

    <!-- ── Result list ───────────────────────────────────────────────────── -->
    <div class="cleanup-list">
      <div v-if="!items.length && !scanning" class="empty">
        <n-icon class="empty-icon" :component="iconEmpty" :size="48" />
        <div class="empty-title">未发现可清理项</div>
        <div class="empty-hint">扫描会自动启动,或点击下方按钮重新扫描</div>
      </div>

      <div v-if="items.length || scanning" class="list-header">
        <span>待清理分类项目 (勾选以释放存储)</span>
        <button class="link-btn" @click="toggleAll">{{ allSelected ? '取消全选' : '全选推荐项' }}</button>
      </div>

      <div v-for="item in items" :key="item.path" class="cleanup-item" @click="checked[item.path] = !checked[item.path]">
        <n-checkbox :checked="checked[item.path]" @update:checked="checked[item.path] = $event" @click.stop />
        <span class="cleanup-icon" :class="iconBgClass(item.category)">
          <n-icon :component="categoryIcon(item.category)" :size="18" />
        </span>
        <div class="cleanup-body">
          <div class="cleanup-row1">
            <span class="cleanup-name">{{ item.name }}</span>
            <span class="cleanup-tag" :class="tagClass(item.category)">{{ safetyTag(item.category) }}</span>
          </div>
          <div class="cleanup-desc">{{ descriptionFor(item) }}</div>
        </div>
        <div class="cleanup-meta">
          <span class="cleanup-size">{{ formatBytes(item.size) }}</span>
          <span class="cleanup-count">{{ formatNumber(item.file_count || 0) }} 文件</span>
          <n-icon class="cleanup-chevron" :component="iconChevron" :size="14" />
        </div>
      </div>
    </div>

    <!-- ── Bottom action bar (pinned to viewport bottom) ────────────────── -->
    <div class="action-bar">
      <div class="action-status">
        <span class="status-dot" :class="scanning ? 'scanning' : (items.length ? 'ready' : 'idle')"></span>
        <span class="status-text">
          <template v-if="scanning">正在分析深层冗余数据…</template>
          <template v-else-if="!items.length">点击右侧按钮启动深度扫描</template>
          <template v-else>已就绪 · 共 {{ items.length.toLocaleString() }} 项 · 预计可优化 {{ formatBytes(stats.reclaimableBytes) }}</template>
        </span>
      </div>
      <div class="action-buttons">
        <button class="ghost-btn" :disabled="scanning || cleaning" @click="rescanCurrent">
          <n-icon :component="iconRadar" :size="16" />
          <span>{{ scanning ? '扫描中…' : '重新扫描' }}</span>
        </button>
        <button class="primary-btn" :disabled="!items.length || scanning || cleaning" @click="cleanAll">
          <n-icon :component="iconTrash" :size="16" />
          <span>{{ cleaning ? `清理中 ${cleanPercent}%` : `立即清理 ${formatBytes(stats.reclaimableBytes)}` }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onBeforeUnmount, type Component } from 'vue'
import { NCheckbox, NIcon, useMessage } from 'naive-ui'
import {
  FlashOutline,
  GlobeOutline,
  CodeOutline,
  CloudDownloadOutline,
  TrashBinOutline,
  FolderOpenOutline,
  ArchiveOutline,
  RadioButtonOnOutline,
  ChevronForwardOutline,
} from '@vicons/ionicons5'
import {
  api, formatBytes,
  onCleanupProgress, onCleanupItem, onCleanupCleanProgress,
  type CleanupItem, type CleanupProgressEvent, type CleanProgressEvent,
} from '../api/tauri'

// ── Group (pill) definitions ────────────────────────────────────────────
// Mirrors the backend SCAN_PATHS groups.
type PillKey = 'all' | 'dev' | 'system' | 'browser' | 'trash'

const pills: { key: PillKey; label: string; icon: Component }[] = [
  { key: 'all',     label: '全部',         icon: FlashOutline },
  { key: 'dev',     label: '开发构建残留', icon: CodeOutline },
  { key: 'system',  label: '系统与应用',   icon: CloudDownloadOutline },
  { key: 'browser', label: '浏览器',       icon: GlobeOutline },
  { key: 'trash',   label: '废纸篓',       icon: TrashBinOutline },
]

const iconEmpty      = FolderOpenOutline
const iconArchive    = ArchiveOutline
const iconRadar      = RadioButtonOnOutline
const iconTrash      = TrashBinOutline
const iconChevron    = ChevronForwardOutline

const categoryIconMap: Record<string, Component> = {
  dev:     CodeOutline,
  system:  CloudDownloadOutline,
  browser: GlobeOutline,
  trash:   TrashBinOutline,
}

// Per-group tinted swatches for the list-icon block.
const iconBgClassMap: Record<string, string> = {
  dev:     'bg-blue',
  system:  'bg-cyan',
  browser: 'bg-orange',
  trash:   'bg-red',
}

// Per-group safety tag label + colour.
const safetyTagMap: Record<string, { label: string; cls: string }> = {
  dev:     { label: '安全可清理', cls: 'tag-green' },
  system:  { label: '安全推荐',   cls: 'tag-green' },
  browser: { label: '安全可清理', cls: 'tag-green' },
  trash:   { label: '将永久移除', cls: 'tag-red' },
}

// Fixed descriptions shown beneath each list-item title, keyed by group.
const descriptionMap: Record<string, string> = {
  dev:     'Xcode DerivedData、CocoaPods 缓存、node_modules 历史构建、Gradle 归档',
  system:  'Spotify 音乐缓存、Telegram 媒体、剪映渲染快照、微信临时缩略图',
  browser: 'Google Chrome、Safari WebKit、Arc 浏览器 离线页面缓存与视频缓存',
  trash:   '清空各磁盘废纸篓 .Trashes 及用户回收站项目',
}

// ── Reactive state ─────────────────────────────────────────────────────

const activePill = ref<PillKey>('all')
const items = ref<CleanupItem[]>([])
const scanning = ref(false)
const checked = ref<Record<string, boolean>>({})

// Per-pill cached scan results so switching pills is instant.
const cache = ref<Partial<Record<PillKey, CleanupItem[]>>>({})
const checkCache = ref<Partial<Record<PillKey, Record<string, boolean>>>>({})

// Progress / animation state
const currentPhase = ref('准备扫描…')
const currentItem  = ref('')
const scanCurrent  = ref(0)
const scanTotal    = ref(0)
const scanPercent  = computed(() => {
  if (scanTotal.value <= 0) return 0
  return Math.min(100, Math.round((scanCurrent.value / scanTotal.value) * 100))
})
const paused       = ref(false)
const cancelled    = ref(false)

const scanStartedAt = ref(0)
const nowTick       = ref(Date.now())
let nowTimer: ReturnType<typeof setInterval> | null = null

// ── Cleanup batch progress ────────────────────────────────────────────
//
// When `cleaning` is true the bottom progress card shows per-batch
// state instead of the scan progress. We track which batch we're
// listening for so stale events from a prior batch are ignored.
const cleaning        = ref(false)
const cleanBatchId    = ref('')
const cleanTotal      = ref(0)
const cleanDone       = ref(0)
const cleanFailed     = ref(0)
/// Number of items that are currently `running`. Each counts as half a
/// unit of progress in `cleanPercent` so the bar moves the moment the
/// first item is dispatched instead of waiting for it to finish — for
/// a single-item cleanup this is the difference between 0% → 100% and
/// 0% → 50% → 100%.
const cleanInFlight   = ref(0)
const cleanCurrentPath = ref('')
const cleanStartedAt  = ref(0)
/// Per-event log lines for the cleanup progress card. Each
/// `cleanup:clean-progress` event appends one entry — surfaced in the
/// card so the user sees what's happening to each item instead of
/// just a moving bar.
const cleanLog        = ref<{ time: string; status: 'running' | 'done' | 'failed'; name: string; error: string | null }[]>([])
const cleanLogEl      = ref<HTMLElement | null>(null)

// Auto-stick the log to the latest line so the user always sees the
// most recent state transition while older lines scroll off the top.
watch(() => cleanLog.value.length, async () => {
  await nextTick()
  if (cleanLogEl.value) cleanLogEl.value.scrollTop = cleanLogEl.value.scrollHeight
})

const cleanPercent = computed(() => {
  if (cleanTotal.value <= 0) return 0
  // Treat each in-flight item as half a unit of progress so the bar
  // moves the moment the first item is dispatched. With only one item
  // this means 0% → 50% (running) → 100% (done) instead of 0% → 100%.
  const weighted = cleanDone.value + cleanFailed.value + cleanInFlight.value * 0.5
  return Math.min(100, Math.round((weighted / cleanTotal.value) * 100))
})
const cleanEtaText = computed(() => {
  if (!cleanStartedAt.value || cleanTotal.value <= 0) return ''
  const finished = cleanDone.value + cleanFailed.value
  if (finished <= 0) return ''
  if (finished >= cleanTotal.value) return ''
  const elapsed = nowTick.value - cleanStartedAt.value
  const total = elapsed * (cleanTotal.value / finished)
  const remaining = total - elapsed
  if (remaining < 0 || !isFinite(remaining)) return ''
  if (remaining < 1000) return '<1s'
  return `${(remaining / 1000).toFixed(1)}s`
})

const etaText = computed(() => {
  if (!scanStartedAt.value || scanCurrent.value <= 0 || scanTotal.value <= 0) return ''
  if (scanPercent.value >= 100) return ''
  const elapsed = nowTick.value - scanStartedAt.value
  const total = elapsed * (scanTotal.value / scanCurrent.value)
  const remaining = total - elapsed
  if (remaining < 0 || !isFinite(remaining)) return ''
  if (remaining < 1000) return '<1s'
  return `${(remaining / 1000).toFixed(1)}s`
})

// Files/sec = scanCurrent / elapsed_seconds. Used in the progress card.
const scanSpeed = computed(() => {
  const elapsedSec = (nowTick.value - scanStartedAt.value) / 1000
  if (elapsedSec <= 0.1) return 0
  return Math.round(scanCurrent.value / elapsedSec)
})

// ── Computed aggregates for the header & pills ──────────────────────────

const stats = computed(() => {
  const visible = items.value
  const scannedCount = visible.length
  const scannedBytes = visible.reduce((s, it) => s + it.size, 0)
  const reclaimableBytes = visible
    .filter(it => checked.value[it.path])
    .reduce((s, it) => s + it.size, 0)
  return { scannedCount, scannedBytes, reclaimableBytes }
})

/// Per-pill size used in the pill label. Computed directly from
/// `items.value` by reading each item's `category` field (the backend
/// already tags every item with its source group: 'dev', 'system',
/// 'browser', or 'trash'). This works whether the active pill is
/// 'all' or a single group, and updates live as `cleanup:item`
/// events stream in — no need to wait for cache writes.
const pillSizes = computed<Record<PillKey, number>>(() => {
  const sizes = { all: 0, dev: 0, system: 0, browser: 0, trash: 0 } as Record<PillKey, number>
  for (const it of items.value) {
    const k = (it.category as PillKey)
    if (k in sizes) sizes[k] += it.size
    sizes.all += it.size
  }
  // The active pill is what the user sees right now. For an active
  // single-group view we prefer the live list's own total over the
  // category breakdown — but they should match anyway.
  return sizes
})

function pillSize(key: PillKey): number {
  return pillSizes.value[key] ?? 0
}

const allSelected = computed(() => {
  if (!items.value.length) return false
  return items.value.every(i => checked.value[i.path])
})

// ── Display helpers ────────────────────────────────────────────────────

function categoryIcon(cat: string): Component {
  return categoryIconMap[cat] ?? iconArchive
}

function iconBgClass(cat: string): string {
  return iconBgClassMap[cat] ?? 'bg-blue'
}

function safetyTag(cat: string): string {
  return safetyTagMap[cat]?.label ?? '安全可清理'
}

function tagClass(cat: string): string {
  return safetyTagMap[cat]?.cls ?? 'tag-green'
}

function descriptionFor(item: CleanupItem): string {
  // If the backend already tagged this row with a known group, use that
  // group's canned description; otherwise fall back to the path tail.
  const desc = descriptionMap[item.category]
  if (desc) return desc
  return item.path
}

function formatNumber(n: number): string {
  return n.toLocaleString()
}

// ── Pill switching & scan lifecycle ─────────────────────────────────────

async function switchPill(key: PillKey) {
  if (scanning.value) return
  // Persist current pill's checked state
  checkCache.value[activePill.value] = { ...checked.value }
  activePill.value = key

  const cached = cache.value[key]
  if (cached) {
    items.value = cached
    checked.value = checkCache.value[key] ?? {}
  } else {
    items.value = []
    checked.value = {}
    await runScan(key, false)
  }
}

async function runScan(key: PillKey, force: boolean) {
  scanning.value   = true
  paused.value      = false
  cancelled.value   = false
  scanCurrent.value = 0
  scanTotal.value   = 0
  currentItem.value = ''
  items.value = []
  currentPhase.value = `准备扫描 · ${pills.find(p => p.key === key)?.label ?? key}`
  scanStartedAt.value = Date.now()
  startNowTimer()
  // Show the progress card immediately even at 0% so the user sees feedback
  scanning.value = true

  // Watchdog: if scanGroup never resolves/rejects within 90s the UI gets
  // stuck on "正在分析深层冗余数据…" forever. Force-finish so the user
  // can retry / switch pill. 90s is well above a clean 22-path scan
  // (typically < 6s) and well below a hung tauri invoke.
  let watchdog: ReturnType<typeof setTimeout> | null = null
  const armWatchdog = () => {
    if (watchdog) clearTimeout(watchdog)
    watchdog = setTimeout(() => {
      if (scanning.value) {
        // eslint-disable-next-line no-console
        console.warn(`[cleanup] scanGroup("${key}") timed out after 90s, forcing UI reset`)
        scanning.value = false
        currentPhase.value = '扫描超时,请重试'
        message.warning('扫描超时,点击"重新扫描"重试')
      }
    }, 90_000)
  }
  armWatchdog()

  try {
    const result = await api.scanGroup(key, force)
    if (watchdog) { clearTimeout(watchdog); watchdog = null }

    if (cancelled.value) {
      items.value = []
      return
    }

    // Items may already have streamed in via onCleanupItem events while
    // the invoke was in flight. Merge instead of overwriting so nothing
    // emitted after the command returns but before the listener tears
    // down is dropped.
    const seen = new Set(items.value.map(it => it.path))
    for (const it of result) {
      if (!seen.has(it.path)) items.value.push(it)
    }
    items.value = [...items.value].sort((a, b) => b.size - a.size)
    // eslint-disable-next-line no-console
    console.debug(`[cleanup] scanGroup("${key}") returned ${result.length} items`)
    cache.value[key] = items.value
    // Restore previously-checked paths (if any are still in the result)
    const prev = checkCache.value[key] ?? {}
    const next: Record<string, boolean> = {}
    for (const it of items.value) {
      if (prev[it.path]) next[it.path] = true
    }
    checked.value = next
    checkCache.value[key] = next
    currentPhase.value = '扫描完成'
  } catch (e) {
    if (watchdog) { clearTimeout(watchdog); watchdog = null }
    message.error(`扫描失败: ${e}`)
    items.value = []
  } finally {
    scanning.value = false
    paused.value    = false
    cancelled.value = false
    stopNowTimer()
  }
}

function startNowTimer() {
  stopNowTimer()
  nowTimer = setInterval(() => {
    if (paused.value) return
    nowTick.value = Date.now()
  }, 100)
}
function stopNowTimer() {
  if (nowTimer !== null) {
    clearInterval(nowTimer)
    nowTimer = null
  }
}

// Force a fresh scan of the currently active pill. Bypasses the 5-minute
// in-memory cache on the backend. Used by the "重新扫描" button.
async function rescanCurrent() {
  if (scanning.value) return
  await runScan(activePill.value, true)
}

// ── Cleanup actions ────────────────────────────────────────────────────

async function cleanAll() {
  const toClean = items.value.filter(i => checked.value[i.path])
  if (!toClean.length) {
    // No explicit selection → clean everything recommended
    const all = items.value
    if (!all.length) return
    await cleanPaths(all)
    return
  }
  await cleanPaths(toClean)
}

async function cleanPaths(targets: CleanupItem[]) {
  if (!targets.length || cleaning.value) return

  // Initialize batch progress state before issuing the command so the
  // progress card flips on immediately (no flash of empty state).
  cleaning.value = true
  const batchId = `b_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
  cleanBatchId.value = batchId
  cleanTotal.value = targets.length
  cleanDone.value = 0
  cleanFailed.value = 0
  cleanInFlight.value = 0
  cleanLog.value = []
  cleanCurrentPath.value = targets[0].name
  cleanStartedAt.value = Date.now()
  startNowTimer()

  let results: Awaited<ReturnType<typeof api.cleanPathsBatch>>
  try {
    results = await api.cleanPathsBatch(batchId, targets.map(t => t.path))
  } catch (e) {
    message.error(`清理失败: ${e}`)
    cleaning.value = false
    stopNowTimer()
    return
  }

  // The backend already emitted per-path progress events during the
  // batch, so the UI's cleanDone / cleanFailed counters reflect reality.
  // The return value here is the authoritative per-path result list.
  const successSet = new Set(results.filter(r => r.ok).map(r => r.path))
  const failedItems = results.filter(r => !r.ok)
  const fail = failedItems.length

  items.value = items.value.filter(i => !successSet.has(i.path))
  const c = cache.value[activePill.value]
  if (c) cache.value[activePill.value] = c.filter(i => !successSet.has(i.path))
  for (const k of Object.keys(checked.value)) delete checked.value[k]

  if (fail === 0) {
    message.success(`已清理 ${successSet.size} 项`)
  } else {
    const firstErr = failedItems.find(r => r.error)?.error ?? '未知原因'
    message.warning(`清理完成: 成功 ${successSet.size}，失败 ${fail}（首个失败原因: ${firstErr}）`)
  }

  cleaning.value = false
  cleanCurrentPath.value = ''
  // Leave cleanStartedAt alone — cleanEtaText short-circuits when
  // finished == total anyway.
  stopNowTimer()
}

function toggleAll() {
  if (allSelected.value) {
    checked.value = {}
  } else {
    const next: Record<string, boolean> = {}
    for (const i of items.value) next[i.path] = true
    checked.value = next
  }
  checkCache.value[activePill.value] = { ...checked.value }
}

// ── Lifecycle ──────────────────────────────────────────────────────────

let unlistenProgress: (() => void) | null = null
let unlistenItem: (() => void) | null = null
let unlistenClean: (() => void) | null = null
const message = useMessage()

onMounted(async () => {
  unlistenClean = await onCleanupCleanProgress((e: CleanProgressEvent) => {
    // Stale events from a previous batch? Drop them. The backend tags
    // each batch with the id the frontend generated at kickoff, so we
    // only react to events for *this* batch.
    if (e.batch_id !== cleanBatchId.value) return
    cleanCurrentPath.value = e.name
    const stamp = new Date().toLocaleTimeString('zh-CN', { hour12: false })
    if (e.status === 'running') {
      // Each running item counts as half a unit of progress (see
      // cleanPercent) so the bar fills smoothly while `rm -rf` is
      // chewing through a multi-GB directory instead of jumping 0% →
      // 100% when the first item returns.
      cleanInFlight.value += 1
      cleanLog.value = [...cleanLog.value, {
        time: stamp, status: 'running', name: e.name, error: null,
      }]
      return
    }
    if (e.status === 'done') {
      cleanDone.value += 1
      cleanInFlight.value = Math.max(0, cleanInFlight.value - 1)
      cleanLog.value = [...cleanLog.value, {
        time: stamp, status: 'done', name: e.name, error: null,
      }]
      return
    }
    if (e.status === 'failed') {
      cleanFailed.value += 1
      cleanInFlight.value = Math.max(0, cleanInFlight.value - 1)
      cleanLog.value = [...cleanLog.value, {
        time: stamp, status: 'failed', name: e.name, error: e.error,
      }]
      // eslint-disable-next-line no-console
      console.warn(`[cleanup] clean failed: ${e.path} — ${e.error}`)
      return
    }
  })

  unlistenProgress = await onCleanupProgress((e: CleanupProgressEvent) => {
    // Only consume events for the active pill. The backend emits with
    // group keys like "group:dev", "group:all", "group:trash".
    // eslint-disable-next-line no-console
    console.debug('[cleanup] progress event:', JSON.stringify(e))
    if (e.category !== `group:${activePill.value}`) return
    if (e.current > 0 && e.total > 0 && e.current <= e.total) {
      currentItem.value = e.phase
    }
    currentPhase.value  = e.phase
    scanCurrent.value   = e.current
    scanTotal.value     = e.total
    paused.value        = e.paused
    cancelled.value     = e.cancelled || cancelled.value
  })

  unlistenItem = await onCleanupItem((e) => {
    if (e.category !== `group:${activePill.value}`) return
    if (items.value.some(it => it.path === e.item.path)) return
    items.value = [...items.value, e.item].sort((a, b) => b.size - a.size)
  })

  // Auto-scan the "all" pill so the user sees results immediately, just
  // like the screenshot suggests.
  await runScan('all', false)
})

onBeforeUnmount(() => {
  stopNowTimer()
  if (unlistenProgress) unlistenProgress()
  if (unlistenItem) unlistenItem()
  if (unlistenClean) unlistenClean()
})
</script>

<style scoped>
.cleanup {
  padding: 0 4px 110px 4px;
  max-width: 980px;
  margin: 0 auto;
  position: relative;
}

/* =========================================================================
   Header
   ========================================================================= */
.header {
  display: flex; align-items: flex-end; justify-content: space-between;
  gap: 16px;
  margin-bottom: 14px;
}
.header-titles { display: flex; flex-direction: column; gap: 4px; }
.title-row { display: flex; align-items: center; gap: 10px; }
.page-title {
  font-size: 22px; font-weight: 600; color: #f5f5f5;
  margin: 0;
  letter-spacing: -0.2px;
}
.mode-badge {
  display: inline-flex; align-items: center; gap: 4px;
  padding: 3px 10px;
  border-radius: 999px;
  background: rgba(99, 226, 183, 0.10);
  color: #63e2b7;
  font-size: 11px;
  border: 1px solid rgba(99, 226, 183, 0.25);
}
.page-sub { font-size: 12px; color: #666; margin: 0; }

.header-stats { display: flex; gap: 10px; }
.stat-card {
  display: flex; flex-direction: column; gap: 2px;
  padding: 8px 14px;
  background: #1a1a1a;
  border: 1px solid #2a2a2a;
  border-radius: 10px;
  min-width: 130px;
}
.stat-label { font-size: 11px; color: #666; }
.stat-value { font-size: 16px; font-weight: 600; color: #e0e0e0; font-family: 'SF Mono', Menlo, monospace; }
.stat-value small { font-size: 11px; font-weight: 400; color: #888; margin-left: 4px; }
.stat-value.accent { color: #63e2b7; }

/* =========================================================================
   Filter pills
   ========================================================================= */
.pills {
  display: flex; gap: 8px;
  margin-bottom: 16px;
  flex-wrap: wrap;
}
.pill {
  display: inline-flex; align-items: center; gap: 6px;
  padding: 7px 14px;
  background: #161616;
  border: 1px solid #2a2a2a;
  border-radius: 999px;
  color: #aaa;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.18s;
  font-family: inherit;
}
.pill:hover {
  background: #1c1c1c;
  color: #ddd;
  border-color: #3a3a3a;
}
.pill.active {
  background: linear-gradient(135deg, #14532d, #166534);
  border-color: rgba(99, 226, 183, 0.5);
  color: #63e2b7;
  box-shadow: 0 0 0 1px rgba(99, 226, 183, 0.2), 0 4px 14px rgba(99, 226, 183, 0.12);
}
.pill-icon { color: inherit; opacity: 0.85; }
.pill-label { font-weight: 500; }
.pill-size {
  font-family: 'SF Mono', Menlo, monospace;
  font-size: 11px;
  color: #888;
  padding-left: 4px;
  border-left: 1px solid rgba(255,255,255,0.08);
  margin-left: 2px;
}
.pill.active .pill-size { color: #b6f5d8; border-color: rgba(99, 226, 183, 0.25); }

/* =========================================================================
   Progress card
   ========================================================================= */
.progress-card {
  display: flex; gap: 14px;
  padding: 16px 18px;
  margin-bottom: 16px;
  background: linear-gradient(135deg, #142019 0%, #0f1812 100%);
  border: 1px solid rgba(99, 226, 183, 0.25);
  border-radius: 14px;
  box-shadow:
    inset 0 1px 0 rgba(99, 226, 183, 0.06),
    0 8px 22px rgba(0, 0, 0, 0.35);
  position: relative;
  overflow: hidden;
}
/* Cleanup variant — orange/red tint to distinguish from the green
   scan-progress card. Same layout, different accent color. */
.progress-card-clean {
  background: linear-gradient(135deg, #251a12 0%, #1a120c 100%);
  border-color: rgba(255, 145, 90, 0.30);
  box-shadow:
    inset 0 1px 0 rgba(255, 145, 90, 0.06),
    0 8px 22px rgba(0, 0, 0, 0.35);
}
.progress-card-clean::before {
  background: radial-gradient(ellipse at 0% 50%, rgba(255, 145, 90, 0.10) 0%, transparent 60%);
}
.progress-card-clean .progress-icon {
  background: rgba(255, 145, 90, 0.15);
  border-color: rgba(255, 145, 90, 0.30);
  color: #ff915a;
}
.progress-card-clean .progress-percent {
  color: #ff915a;
}
.progress-bar-fill-clean {
  background: linear-gradient(90deg, #ff915a, #f56c6c);
  box-shadow: 0 0 12px rgba(255, 145, 90, 0.5);
}
.progress-card::before {
  content: '';
  position: absolute; inset: 0;
  background: radial-gradient(ellipse at 0% 50%, rgba(99, 226, 183, 0.10) 0%, transparent 60%);
  pointer-events: none;
}
.progress-icon {
  width: 48px; height: 48px;
  border-radius: 12px;
  background: rgba(99, 226, 183, 0.15);
  border: 1px solid rgba(99, 226, 183, 0.3);
  color: #63e2b7;
  display: flex; align-items: center; justify-content: center;
  flex-shrink: 0;
  animation: spin-slow 3s linear infinite;
}
@keyframes spin-slow { to { transform: rotate(360deg); } }

.progress-body { flex: 1; min-width: 0; position: relative; }
.progress-top { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
.progress-status { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.progress-title { font-size: 14px; color: #f0f0f0; font-weight: 600; }
.progress-speed { font-size: 11px; color: #888; font-family: 'SF Mono', Menlo, monospace; }
.progress-percent {
  font-size: 28px; font-weight: 700;
  color: #63e2b7;
  font-family: 'SF Mono', Menlo, monospace;
  line-height: 1;
}
.progress-percent small { font-size: 14px; opacity: 0.7; margin-left: 2px; }

.progress-current {
  margin-top: 10px;
  display: flex; gap: 8px;
  font-size: 11px;
  color: #888;
  font-family: 'SF Mono', Menlo, monospace;
  min-width: 0;
}
.progress-current-label { color: #555; flex-shrink: 0; }
.progress-current-path {
  color: #ccc;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  direction: rtl; text-align: left;
}

.progress-bar {
  margin-top: 10px;
  height: 6px;
  background: rgba(255, 255, 255, 0.04);
  border-radius: 999px;
  overflow: hidden;
  position: relative;
}
.progress-bar-fill {
  height: 100%;
  background: linear-gradient(90deg, #63e2b7, #4ade80);
  box-shadow: 0 0 12px rgba(99, 226, 183, 0.5);
  border-radius: 999px;
  transition: width 0.25s ease;
}
.progress-footer {
  margin-top: 6px;
  display: flex; justify-content: space-between;
  font-size: 11px;
  color: #666;
  font-family: 'SF Mono', Menlo, monospace;
}

/* Per-item event log for the cleanup progress card. Sticks to the
   bottom so the most recent line is visible while older ones scroll
   off the top. Keep the visual style muted — this is debug-style
   output, not the headline. */
.clean-log {
  margin-top: 10px;
  max-height: 96px;
  overflow-y: auto;
  border-top: 1px solid rgba(255, 145, 90, 0.15);
  padding-top: 8px;
  display: flex; flex-direction: column; gap: 3px;
  font-family: 'SF Mono', Menlo, monospace;
  font-size: 11px;
}
.clean-log-line {
  display: flex; align-items: center; gap: 6px;
  color: #aaa;
  min-width: 0;
}
.clean-log-time { color: #555; flex-shrink: 0; }
.clean-log-mark { flex-shrink: 0; width: 14px; text-align: center; }
.clean-log-name {
  color: #ddd;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  min-width: 0;
}
.clean-log-err {
  color: #ff7875;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  max-width: 50%;
  flex-shrink: 1;
}
.clean-log-line.status-running .clean-log-mark { color: #ff915a; }
.clean-log-line.status-running .clean-log-name { color: #ffba8e; }
.clean-log-line.status-done    .clean-log-mark { color: #4ade80; }
.clean-log-line.status-done    .clean-log-name { color: #c8e6c9; }
.clean-log-line.status-failed  .clean-log-mark { color: #ff7875; }
.clean-log-line.status-failed  .clean-log-name { color: #ffb4b4; }

.card-enter-active, .card-leave-active {
  transition: opacity 0.25s, transform 0.25s;
}
.card-enter-from, .card-leave-to {
  opacity: 0; transform: translateY(-6px);
}

/* =========================================================================
   Cleanup list
   ========================================================================= */
.cleanup-list {
  background: transparent;
  display: flex; flex-direction: column;
  gap: 8px;
}

.list-header {
  display: flex; justify-content: space-between; align-items: center;
  padding: 4px 4px;
  font-size: 12px;
  color: #888;
}
.link-btn {
  background: none; border: none; color: #63e2b7;
  font-size: 12px; cursor: pointer; padding: 0;
  font-family: inherit;
}
.link-btn:hover { color: #4ade80; }

.cleanup-item {
  display: flex; align-items: center; gap: 14px;
  padding: 14px 16px;
  background: #1a1a1a;
  border: 1px solid #2a2a2a;
  border-radius: 12px;
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s, transform 0.1s;
}
.cleanup-item:hover {
  border-color: #3a3a3a;
  background: #1e1e1e;
}
.cleanup-item:active { transform: scale(0.998); }

.cleanup-icon {
  width: 40px; height: 40px;
  border-radius: 10px;
  display: flex; align-items: center; justify-content: center;
  flex-shrink: 0;
  color: #fff;
}
.bg-blue   { background: linear-gradient(135deg, #3b82f6, #1e40af); }
.bg-cyan   { background: linear-gradient(135deg, #06b6d4, #0e7490); }
.bg-orange { background: linear-gradient(135deg, #f59e0b, #b45309); }
.bg-red    { background: linear-gradient(135deg, #ef4444, #991b1b); }

.cleanup-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
.cleanup-row1 { display: flex; align-items: center; gap: 8px; min-width: 0; }
.cleanup-name { font-size: 13px; color: #f0f0f0; font-weight: 500; }
.cleanup-tag {
  font-size: 10px;
  padding: 2px 7px;
  border-radius: 4px;
  font-weight: 500;
  border: 1px solid;
}
.tag-green { color: #63e2b7; background: rgba(99, 226, 183, 0.10); border-color: rgba(99, 226, 183, 0.25); }
.tag-red   { color: #ff7875; background: rgba(245, 108, 108, 0.10); border-color: rgba(245, 108, 108, 0.25); }

.cleanup-desc {
  font-size: 11px;
  color: #777;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.cleanup-meta {
  display: flex; align-items: center; gap: 8px;
  flex-shrink: 0;
}
.cleanup-size {
  font-size: 14px; font-weight: 600;
  color: #63e2b7;
  font-family: 'SF Mono', Menlo, monospace;
}
.cleanup-count { font-size: 11px; color: #888; font-family: 'SF Mono', Menlo, monospace; }
.cleanup-chevron { color: #444; }

.empty {
  text-align: center; padding: 60px 20px;
  background: #1a1a1a;
  border: 1px dashed #2a2a2a;
  border-radius: 12px;
  color: #666;
}
.empty-icon { display: inline-flex; opacity: 0.5; margin-bottom: 8px; color: #888; }
.empty-title { font-size: 13px; }
.empty-hint { font-size: 11px; color: #555; margin-top: 6px; }

/* =========================================================================
   Bottom action bar
   ========================================================================= */
.action-bar {
  position: fixed;
  left: 50%; bottom: 22px;
  transform: translateX(-50%);
  z-index: 50;
  display: flex; align-items: center; gap: 14px;
  padding: 10px 14px;
  max-width: calc(100vw - 32px);
  white-space: nowrap;
  background: rgba(20, 22, 28, 0.82);
  border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 999px;
  backdrop-filter: blur(18px) saturate(140%);
  -webkit-backdrop-filter: blur(18px) saturate(140%);
  box-shadow:
    0 1px 0 rgba(255, 255, 255, 0.05) inset,
    0 14px 36px rgba(0, 0, 0, 0.55),
    0 4px 12px rgba(0, 0, 0, 0.35);
}
.action-status {
  display: flex; align-items: center; gap: 8px;
  font-size: 12px;
  color: #aaa;
  padding: 0 10px;
  border-right: 1px solid rgba(255,255,255,0.08);
  max-width: 360px;
  min-width: 0;
}
.status-dot {
  width: 8px; height: 8px; border-radius: 50%;
  background: #555; flex-shrink: 0;
}
.status-dot.scanning { background: #63e2b7; box-shadow: 0 0 8px #63e2b7; animation: pulse 1.5s infinite; }
.status-dot.ready    { background: #4ade80; }
.status-dot.idle     { background: #555; }
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50%      { opacity: 0.5; }
}
.status-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.action-buttons { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }

.ghost-btn {
  display: inline-flex; align-items: center; gap: 8px;
  padding: 10px 16px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 999px;
  color: #cfd6dc;
  font-size: 13px; font-weight: 500;
  cursor: pointer;
  transition: all 0.18s;
  font-family: inherit;
  white-space: nowrap;
  flex-shrink: 0;
}
.ghost-btn:hover:not(:disabled) {
  background: rgba(99, 226, 183, 0.08);
  border-color: rgba(99, 226, 183, 0.45);
  color: #63e2b7;
}
.ghost-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.primary-btn {
  display: inline-flex; align-items: center; gap: 8px;
  padding: 10px 18px;
  background: linear-gradient(135deg, #63e2b7 0%, #4ade80 100%);
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 999px;
  color: #0a1612;
  font-size: 13px; font-weight: 600;
  cursor: pointer;
  transition: all 0.18s;
  font-family: inherit;
  white-space: nowrap;
  flex-shrink: 0;
  box-shadow:
    0 6px 18px rgba(99, 226, 183, 0.35),
    inset 0 1px 0 rgba(255, 255, 255, 0.3);
}
.primary-btn:hover:not(:disabled) {
  transform: translateY(-1px);
  box-shadow:
    0 8px 22px rgba(99, 226, 183, 0.45),
    inset 0 1px 0 rgba(255, 255, 255, 0.35);
}
.primary-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
  filter: saturate(0.5);
}
</style>
