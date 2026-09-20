<template>
  <div class="analyzer">
    <div class="page-title">空间分析</div>

    <div class="scan-bar">
      <n-input v-model:value="scanPath" placeholder="输入要扫描的路径" style="width: 400px" size="small" />
      <n-button type="primary" size="small" @click="scan" :loading="scanning">扫描</n-button>
      <span v-if="scanning" class="scan-hint">扫描中... {{ scannedCount }} 个文件</span>
    </div>

    <div v-if="treeData" class="tree-view">
      <div class="tree-header">
        <span>目录名</span>
        <span>大小</span>
        <span>文件数</span>
      </div>
      <div v-for="node in sortedChildren" :key="node.path" class="tree-row" @click="drillInto(node)">
        <span class="tree-name">{{ node.name }}</span>
        <span class="tree-size">{{ formatBytes(node.size) }}</span>
        <span class="tree-count">{{ node.file_count }}</span>
      </div>
    </div>

    <div v-if="!treeData && !scanning" class="empty-state">
      输入路径开始扫描磁盘空间分布
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { NInput, NButton } from 'naive-ui'
import { api, formatBytes, type DirNode } from '../api/tauri'

const scanPath = ref('/Users/mac')
const treeData = ref<DirNode | null>(null)
const scanning = ref(false)
const scannedCount = ref(0)

const sortedChildren = computed(() => {
  if (!treeData.value) return []
  return [...treeData.value.children].sort((a, b) => b.size - a.size)
})

async function scan() {
  scanning.value = true
  treeData.value = null
  try {
    const result = await api.scanDirectory(scanPath.value, 2)
    treeData.value = result
  } catch (e) {
    console.error('scan error:', e)
  }
  scanning.value = false
}

function drillInto(node: DirNode) {
  scanPath.value = node.path
  scan()
}
</script>

<style scoped>
.analyzer { padding: 8px; }
.page-title { font-size: 20px; font-weight: 600; margin-bottom: 16px; }
.scan-bar { display: flex; gap: 8px; align-items: center; margin-bottom: 16px; }
.scan-hint { font-size: 12px; color: #888; }
.tree-view { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; overflow: hidden; }
.tree-header { display: flex; padding: 8px 12px; background: #222; font-size: 11px; color: #666; }
.tree-header span:first-child { flex: 1; }
.tree-header span:nth-child(2) { width: 100px; text-align: right; }
.tree-header span:nth-child(3) { width: 80px; text-align: right; }
.tree-row { display: flex; padding: 8px 12px; cursor: pointer; border-bottom: 1px solid #222; transition: background 0.15s; }
.tree-row:hover { background: #222; }
.tree-name { flex: 1; font-size: 13px; }
.tree-size { width: 100px; text-align: right; font-size: 12px; color: #63e2b7; }
.tree-count { width: 80px; text-align: right; font-size: 12px; color: #666; }
.empty-state { text-align: center; padding: 60px; color: #555; font-size: 14px; }
</style>
