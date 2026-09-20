<template>
  <n-config-provider :theme="darkTheme" :locale="zhCN">
    <n-message-provider>
      <div class="app-layout">
        <!-- Top Bar -->
        <div class="top-bar">
          <div class="top-bar-left">
            <span class="app-icon"></span>
            <span class="app-title">DiskFlow</span>
            <span class="app-version">v1.0</span>
          </div>
          <div class="top-bar-center">
            <div class="metrics-mini" v-if="metrics">
              <span class="metric">CPU {{ metrics.cpu_usage.toFixed(0) }}%</span>
              <span class="metric">RAM {{ formatBytes(metrics.mem_used) }} / {{ formatBytes(metrics.mem_total) }}</span>
            </div>
          </div>
          <div class="top-bar-right">
            <n-tag size="tiny" :type="disks.length > 0 ? 'success' : 'error'" round>
              {{ disks.length }} 个磁盘
            </n-tag>
          </div>
        </div>

        <div class="body-layout">
          <!-- Sidebar -->
          <div class="sidebar">
            <div class="nav-menu">
              <div
                v-for="item in navItems"
                :key="item.key"
                class="nav-item"
                :class="{ active: activeView === item.key }"
                @click="activeView = item.key"
              >
                <span class="nav-label">{{ item.label }}</span>
              </div>
            </div>

            <div class="sidebar-divider"></div>

            <!-- Disk List -->
            <div class="disk-list">
              <div class="disk-list-title">磁盘</div>
              <div v-for="disk in disks" :key="disk.mount_point" class="disk-item" @click="activeView = 'dashboard'">
                <div class="disk-item-header">
                  <span class="disk-icon"></span>
                  <span class="disk-name">{{ disk.name || disk.mount_point }}</span>
                </div>
                <div class="disk-bar">
                  <div
                    class="disk-bar-fill"
                    :style="{ width: diskUsagePercent(disk) + '%', background: diskBarColor(disk) }"
                  ></div>
                </div>
                <div class="disk-item-meta">
                  {{ formatBytes(disk.available_space) }} 可用 / {{ formatBytes(disk.total_space) }}
                </div>
              </div>
            </div>
          </div>

          <!-- Main Content -->
          <div class="main-content">
            <component
              :is="currentView"
              :disks="disks"
              :metrics="metrics"
              @refresh-disks="loadDisks"
            />
          </div>
        </div>

        <!-- Bottom Status Bar -->
        <div class="status-bar">
          <span>{{ statusLabel }}</span>
          <span v-if="isRunning" class="status-running">● {{ currentTask }}</span>
        </div>
      </div>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, shallowRef } from 'vue'
import {
  NConfigProvider, NMessageProvider, NTag,
  darkTheme, zhCN,
} from 'naive-ui'
import { api, onMetrics, formatBytes, type DiskInfo, type MetricsEvent } from './api/tauri'

import Dashboard from './views/Dashboard.vue'
import Analyzer from './views/Analyzer.vue'
import Migration from './views/Migration.vue'
import FolderMigrator from './views/FolderMigrator.vue'
import Monitor from './views/Monitor.vue'
import Cleanup from './views/Cleanup.vue'

const navItems = [
  { key: 'dashboard', label: '磁盘概览' },
  { key: 'analyzer', label: '空间分析' },
  { key: 'migration', label: '应用迁移' },
  { key: 'folder', label: '目录迁移' },
  { key: 'monitor', label: '系统监控' },
  { key: 'cleanup', label: '空间清理' },
]

const activeView = ref('dashboard')
const disks = ref<DiskInfo[]>([])
const metrics = ref<MetricsEvent | null>(null)
const isRunning = ref(false)
const currentTask = ref('')
const statusLabel = ref('就绪')

let unlistenMetrics: (() => void) | null = null

const viewMap: Record<string, any> = {
  dashboard: shallowRef(Dashboard),
  analyzer: shallowRef(Analyzer),
  migration: shallowRef(Migration),
  folder: shallowRef(FolderMigrator),
  monitor: shallowRef(Monitor),
  cleanup: shallowRef(Cleanup),
}

const currentView = computed(() => viewMap[activeView.value]?.value)

function diskUsagePercent(disk: DiskInfo): number {
  if (disk.total_space === 0) return 0
  return Math.round(((disk.total_space - disk.available_space) / disk.total_space) * 100)
}

function diskBarColor(disk: DiskInfo): string {
  const pct = diskUsagePercent(disk)
  if (pct > 80) return 'linear-gradient(90deg, #f56c6c, #ff7875)'
  if (pct > 60) return 'linear-gradient(90deg, #e6a23c, #fbbf24)'
  return 'linear-gradient(90deg, #63e2b7, #4ade80)'
}

async function loadDisks() {
  try {
    disks.value = await api.getDisks()
  } catch (e) {
    console.error('loadDisks error:', e)
  }
}

onMounted(async () => {
  await loadDisks()
  try {
    unlistenMetrics = await onMetrics((e) => { metrics.value = e })
    await api.startMonitoring()
  } catch (e) {
    console.error('monitoring error:', e)
  }
})

onUnmounted(() => {
  if (unlistenMetrics) unlistenMetrics()
  api.stopMonitoring()
})
</script>

<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body { height: 100%; overflow: hidden; }
body { font-family: -apple-system, 'SF Pro Text', 'PingFang SC', sans-serif; }

.app-layout {
  display: flex; flex-direction: column;
  height: 100vh;
  background: #0d0d0d; color: #e0e0e0;
}

.top-bar {
  display: flex; align-items: center; justify-content: space-between;
  padding: 0 16px; height: 44px;
  background: #1a1a1a; border-bottom: 1px solid #2a2a2a;
  -webkit-user-select: none;
}
.top-bar-left { display: flex; align-items: center; gap: 8px; }
.app-icon { display: inline-block; width: 10px; height: 10px; border-radius: 50%; background: #63e2b7; }
.app-title { font-size: 14px; font-weight: 600; color: #63e2b7; }
.app-version { font-size: 11px; color: #555; }
.top-bar-center { display: flex; align-items: center; gap: 16px; }
.metrics-mini { display: flex; gap: 12px; }
.metric { font-size: 11px; color: #888; }
.top-bar-right { display: flex; align-items: center; }

.body-layout { display: flex; flex: 1; overflow: hidden; }

.sidebar {
  width: 220px; flex-shrink: 0;
  background: #161616; border-right: 1px solid #2a2a2a;
  display: flex; flex-direction: column;
  overflow-y: auto;
}
.nav-menu { padding: 8px 0; }
.nav-item {
  display: flex; align-items: center; gap: 10px;
  padding: 8px 16px; cursor: pointer;
  transition: all 0.15s;
  font-size: 13px; color: #999;
}
.nav-item:hover { background: #1f1f1f; color: #ccc; }
.nav-item.active { background: #1e3a21; color: #63e2b7; border-left: 2px solid #63e2b7; }
.sidebar-divider { height: 1px; background: #2a2a2a; margin: 4px 0; }

.disk-list { padding: 8px; flex: 1; }
.disk-list-title { font-size: 11px; color: #555; padding: 4px 8px; margin-bottom: 4px; }
.disk-item { padding: 6px 8px; cursor: pointer; border-radius: 6px; transition: background 0.15s; margin-bottom: 4px; }
.disk-item:hover { background: #1f1f1f; }
.disk-item-header { display: flex; align-items: center; gap: 6px; margin-bottom: 4px; }
.disk-icon { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: #555; flex-shrink: 0; }
.disk-name { font-size: 11px; color: #aaa; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.disk-bar { height: 4px; background: #2a2a2a; border-radius: 2px; overflow: hidden; }
.disk-bar-fill { height: 100%; transition: width 0.3s; }
.disk-item-meta { font-size: 10px; color: #555; margin-top: 2px; }

.main-content { flex: 1; overflow: auto; padding: 16px; }

.status-bar {
  display: flex; align-items: center; justify-content: space-between;
  padding: 0 16px; height: 24px;
  background: #1a1a1a; border-top: 1px solid #2a2a2a;
  font-size: 11px; color: #555;
}
.status-running { color: #e6a23c; }
</style>
