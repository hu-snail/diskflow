<template>
  <div class="monitor">
    <div class="page-title">系统监控</div>

    <div class="metrics-grid">
      <!-- CPU -->
      <div class="metric-card">
        <div class="metric-header">
          <span class="metric-dot" :style="{ background: cpuColor }"></span>
          <span>CPU</span>
          <span class="metric-value">{{ metrics?.cpu_usage?.toFixed(1) ?? '--' }}%</span>
        </div>
        <div class="metric-bar">
          <div class="metric-bar-fill" :style="{ width: (metrics?.cpu_usage ?? 0) + '%', background: cpuColor }"></div>
        </div>
      </div>

      <!-- Memory -->
      <div class="metric-card">
        <div class="metric-header">
          <span class="metric-dot" :style="{ background: memColor }"></span>
          <span>内存</span>
          <span class="metric-value">{{ memPercent }}%</span>
        </div>
        <div class="metric-bar">
          <div class="metric-bar-fill" :style="{ width: memPercent + '%', background: memColor }"></div>
        </div>
        <div class="metric-detail">{{ formatBytes(metrics?.mem_used ?? 0) }} / {{ formatBytes(metrics?.mem_total ?? 0) }}</div>
      </div>
    </div>

    <!-- CPU History Chart -->
    <div class="chart-panel">
      <div class="chart-title">CPU 使用率（实时）</div>
      <div class="chart-container">
        <svg viewBox="0 0 400 100" class="chart-svg">
          <polyline
            :points="cpuHistoryPoints"
            fill="none"
            stroke="#63e2b7"
            stroke-width="2"
          />
          <polyline
            :points="cpuHistoryArea"
            fill="rgba(99, 226, 183, 0.1)"
            stroke="none"
          />
        </svg>
      </div>
    </div>

    <!-- System Info -->
    <div class="sys-info">
      <div class="info-row">
        <span class="info-label">系统运行时间</span>
        <span class="info-value">{{ uptime }}</span>
      </div>
      <div class="info-row">
        <span class="info-label">磁盘总数</span>
        <span class="info-value">{{ disks.length }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { api, onMetrics, formatBytes, type DiskInfo, type MetricsEvent } from '../api/tauri'

const props = defineProps<{ disks: DiskInfo[]; metrics: MetricsEvent | null }>()

const cpuHistory = ref<number[]>(Array(60).fill(0))
let unlisten: (() => void) | null = null

const memPercent = computed(() => {
  if (!props.metrics || !props.metrics.mem_total) return 0
  return Math.round((props.metrics.mem_used / props.metrics.mem_total) * 100)
})

const cpuColor = computed(() => {
  const v = props.metrics?.cpu_usage ?? 0
  if (v > 80) return '#f56c6c'
  if (v > 60) return '#e6a23c'
  return '#63e2b7'
})

const memColor = computed(() => {
  if (memPercent.value > 80) return '#f56c6c'
  if (memPercent.value > 60) return '#e6a23c'
  return '#4080ff'
})

const cpuHistoryPoints = computed(() => {
  return cpuHistory.value.map((v, i) => `${(i / 60) * 400},${100 - v}`).join(' ')
})

const cpuHistoryArea = computed(() => {
  const pts = cpuHistory.value.map((v, i) => `${(i / 60) * 400},${100 - v}`).join(' ')
  return `0,100 ${pts} 400,100`
})

const uptime = computed(() => {
  const s = props.metrics?.uptime ?? 0
  const days = Math.floor(s / 86400)
  const hours = Math.floor((s % 86400) / 3600)
  if (days > 0) return `${days}天 ${hours}小时`
  return `${hours}小时`
})

onMounted(async () => {
  unlisten = await onMetrics((e) => {
    cpuHistory.value.shift()
    cpuHistory.value.push(e.cpu_usage)
  })
})

onUnmounted(() => { if (unlisten) unlisten() })
</script>

<style scoped>
.monitor { padding: 8px; }
.page-title { font-size: 20px; font-weight: 600; margin-bottom: 16px; }
.metrics-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-bottom: 16px; }
.metric-card { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; padding: 14px; }
.metric-header { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.metric-dot { display: inline-block; width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0; }
.metric-value { margin-left: auto; font-size: 18px; font-weight: 700; }
.metric-bar { height: 8px; background: #2a2a2a; border-radius: 4px; overflow: hidden; }
.metric-bar-fill { height: 100%; transition: width 0.5s; }
.metric-detail { font-size: 11px; color: #666; margin-top: 4px; }
.chart-panel { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; padding: 14px; margin-bottom: 12px; }
.chart-title { font-size: 13px; color: #888; margin-bottom: 8px; }
.chart-container { height: 100px; }
.chart-svg { width: 100%; height: 100%; }
.sys-info { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; padding: 12px; }
.info-row { display: flex; justify-content: space-between; padding: 4px 0; font-size: 13px; }
.info-label { color: #888; }
.info-value { color: #e0e0e0; }
</style>
