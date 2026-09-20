<template>
  <div class="cleanup">
    <div class="page-title">空间清理</div>

    <div class="tabs">
      <button :class="{ active: tab === 'cache' }" @click="scanCache">缓存</button>
      <button :class="{ active: tab === 'log' }" @click="scanLog">日志</button>
      <button :class="{ active: tab === 'large' }" @click="scanLarge">大文件</button>
    </div>

    <div v-if="scanning" class="scanning">扫描中...</div>

    <div v-else class="cleanup-list">
      <div v-for="item in items" :key="item.path" class="cleanup-item">
        <n-checkbox v-model:checked="checked[item.path]" />
        <span class="cleanup-name">{{ item.name }}</span>
        <span class="cleanup-path">{{ item.path }}</span>
        <span class="cleanup-size">{{ formatBytes(item.size) }}</span>
        <n-button size="tiny" type="error" quaterny @click="clean(item)">删除</n-button>
      </div>
      <div v-if="!items.length && !scanning" class="empty">无数据</div>
    </div>

    <div v-if="items.length" class="batch-actions">
      <span>可释放 {{ formatBytes(totalCheckedSize) }}</span>
      <n-button size="small" type="error" @click="cleanAll">一键清理选中</n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { NButton, NCheckbox } from 'naive-ui'
import { api, formatBytes, type CleanupItem } from '../api/tauri'

const tab = ref('cache')
const items = ref<CleanupItem[]>([])
const scanning = ref(false)
const checked = ref<Record<string, boolean>>({})

const totalCheckedSize = computed(() => {
  return items.value.filter(i => checked.value[i.path]).reduce((s, i) => s + i.size, 0)
})

async function scanCache() {
  tab.value = 'cache'
  scanning.value = true
  items.value = await api.scanCache()
  scanning.value = false
}

async function scanLog() {
  tab.value = 'log'
  scanning.value = true
  items.value = await api.scanLogs()
  scanning.value = false
}

async function scanLarge() {
  tab.value = 'large'
  scanning.value = true
  // Use find_large_files with 100MB threshold
  const files = await api.findLargeFiles('/Users/mac', 100 * 1024 * 1024)
  items.value = files.map(f => ({
    name: f.name,
    path: f.path,
    size: f.size,
    category: 'large',
    file_count: 1,
    last_modified: f.modified,
  }))
  scanning.value = false
}

async function clean(item: CleanupItem) {
  await api.cleanPath(item.path)
  items.value = items.value.filter(i => i.path !== item.path)
}

async function cleanAll() {
  const toClean = items.value.filter(i => checked.value[i.path])
  for (const item of toClean) {
    await api.cleanPath(item.path)
  }
  items.value = items.value.filter(i => !checked.value[i.path])
}

scanCache()
</script>

<style scoped>
.cleanup { padding: 8px; }
.page-title { font-size: 20px; font-weight: 600; margin-bottom: 16px; }
.tabs { display: flex; gap: 4px; margin-bottom: 12px; }
.tabs button { padding: 4px 12px; background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 6px; color: #888; cursor: pointer; font-size: 12px; }
.tabs button.active { background: #1e3a21; color: #63e2b7; border-color: #63e2b7; }
.scanning { text-align: center; padding: 40px; color: #666; }
.cleanup-list { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; overflow: hidden; }
.cleanup-item { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid #222; }
.cleanup-name { font-size: 13px; }
.cleanup-path { flex: 1; font-size: 11px; color: #555; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cleanup-size { font-size: 12px; color: #63e2b7; }
.empty { text-align: center; padding: 40px; color: #555; }
.batch-actions { display: flex; justify-content: space-between; align-items: center; padding: 8px 12px; margin-top: 12px; background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; }
</style>
