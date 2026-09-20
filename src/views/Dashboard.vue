<template>
  <div class="dashboard">
    <div class="page-title">磁盘概览</div>

    <div class="stats-row">
      <div class="stat-card">
        <div class="stat-label">总容量</div>
        <div class="stat-value">{{ formatBytes(totalSpace) }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">已用</div>
        <div class="stat-value">{{ formatBytes(usedSpace) }}</div>
        <div class="stat-sub">{{ usedPercent }}%</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">可用</div>
        <div class="stat-value available">{{ formatBytes(availableSpace) }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">磁盘数量</div>
        <div class="stat-value">{{ disks.length }}</div>
      </div>
    </div>

    <div class="disk-grid">
      <div v-for="disk in disks" :key="disk.mount_point" class="disk-card">
        <div class="disk-card-header">
          <span class="disk-card-icon" :class="{ 'icon-removable': disk.is_removable, 'icon-internal': !disk.is_removable }"></span>
          <div>
            <div class="disk-card-name">{{ disk.name || disk.mount_point }}</div>
            <div class="disk-card-mount">{{ disk.mount_point }}</div>
          </div>
          <n-tag size="tiny" :type="disk.fs_type === 'apfs' ? 'success' : 'warning'" round>
            {{ disk.fs_type }}
          </n-tag>
        </div>

        <div class="ring-container">
          <svg viewBox="0 0 120 120" class="ring-svg">
            <circle cx="60" cy="60" r="50" fill="none" stroke="#2a2a2a" stroke-width="8" />
            <circle
              cx="60" cy="60" r="50" fill="none"
              :stroke="ringColor(disk)" stroke-width="8"
              stroke-linecap="round"
              :stroke-dasharray="circumference"
              :stroke-dashoffset="circumference * (1 - pct(disk) / 100)"
              transform="rotate(-90 60 60)"
            />
          </svg>
          <div class="ring-text">
            <div class="ring-percent">{{ pct(disk) }}%</div>
            <div class="ring-used">{{ formatBytes(disk.total_space - disk.available_space) }}</div>
          </div>
        </div>

        <div class="disk-card-footer">
          <span>{{ formatBytes(disk.available_space) }} 可用</span>
          <span>/ {{ formatBytes(disk.total_space) }}</span>
        </div>

        <div v-if="disk.is_sparse" class="sparse-badge">稀疏镜像</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { NTag } from 'naive-ui'
import { formatBytes, type DiskInfo } from '../api/tauri'

const props = defineProps<{ disks: DiskInfo[] }>()

const totalSpace = computed(() => props.disks.reduce((s, d) => s + d.total_space, 0))
const availableSpace = computed(() => props.disks.reduce((s, d) => s + d.available_space, 0))
const usedSpace = computed(() => totalSpace.value - availableSpace.value)
const usedPercent = computed(() => totalSpace.value ? Math.round((usedSpace.value / totalSpace.value) * 100) : 0)

const circumference = 2 * Math.PI * 50

function pct(disk: DiskInfo): number {
  if (!disk.total_space) return 0
  return Math.round(((disk.total_space - disk.available_space) / disk.total_space) * 100)
}

function ringColor(disk: DiskInfo): string {
  const p = pct(disk)
  if (p > 80) return '#f56c6c'
  if (p > 60) return '#e6a23c'
  return '#63e2b7'
}
</script>

<style scoped>
.dashboard { padding: 8px; }
.page-title { font-size: 20px; font-weight: 600; margin-bottom: 16px; }

.stats-row { display: flex; gap: 12px; margin-bottom: 20px; }
.stat-card {
  flex: 1; background: #1a1a1a; border: 1px solid #2a2a2a;
  border-radius: 10px; padding: 14px 16px;
}
.stat-label { font-size: 11px; color: #666; margin-bottom: 4px; }
.stat-value { font-size: 22px; font-weight: 700; color: #e0e0e0; }
.stat-value.available { color: #63e2b7; }
.stat-sub { font-size: 12px; color: #888; margin-top: 2px; }

.disk-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
.disk-card {
  background: #1a1a1a; border: 1px solid #2a2a2a;
  border-radius: 12px; padding: 16px;
  display: flex; flex-direction: column; align-items: center;
  position: relative;
}
.disk-card-header { display: flex; align-items: center; gap: 8px; width: 100%; margin-bottom: 12px; }
.disk-card-icon { display: inline-block; width: 14px; height: 14px; border-radius: 3px; flex-shrink: 0; }
.icon-internal { background: #4080ff; }
.icon-removable { background: #63e2b7; }
.disk-card-name { font-size: 14px; font-weight: 600; }
.disk-card-mount { font-size: 10px; color: #555; }

.ring-container { position: relative; width: 120px; height: 120px; margin: 8px 0; }
.ring-svg { width: 100%; height: 100%; }
.ring-text { position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); text-align: center; }
.ring-percent { font-size: 22px; font-weight: 700; }
.ring-used { font-size: 10px; color: #666; }

.disk-card-footer { font-size: 11px; color: #888; margin-top: 8px; display: flex; gap: 4px; }
.sparse-badge { position: absolute; top: 8px; right: 8px; font-size: 10px; color: #63e2b7; }
</style>
