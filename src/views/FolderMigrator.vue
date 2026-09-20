<template>
  <div class="folder-migrator">
    <div class="page-title">目录迁移</div>

    <div class="path-bar">
      <n-button size="tiny" quaterny @click="goUp">上级</n-button>
      <n-input v-model:value="currentPath" size="small" @keydown.enter="browse" />
      <n-button size="tiny" type="primary" @click="browse">前往</n-button>
    </div>

    <div class="quick-links">
      <n-button size="tiny" quaterny @click="navigate('/Users/mac/Desktop')">桌面</n-button>
      <n-button size="tiny" quaterny @click="navigate('/Users/mac/Downloads')">下载</n-button>
      <n-button size="tiny" quaterny @click="navigate('/Users/mac/Documents')">文档</n-button>
      <n-button size="tiny" quaterny @click="navigate('/Users/mac/Library')">资源库</n-button>
      <n-button size="tiny" quaterny @click="navigate('/Volumes')">磁盘</n-button>
    </div>

    <div class="folder-list">
      <div v-for="folder in folders" :key="folder.path" class="folder-item" @click="toggleSelect(folder.path)">
        <n-checkbox :checked="selectedFolders.includes(folder.path)" @update:checked="toggleSelect(folder.path)" @click.stop />
        <span class="folder-icon" :class="{ 'icon-migrated': folder.migrated, 'icon-normal': !folder.migrated }"></span>
        <span class="folder-name">{{ folder.name }}</span>
        <span class="folder-size">{{ folder.migrated ? folder.size_label : (folder.size_computed ? folder.size_label : '计算中...') }}</span>
        <n-tag v-if="folder.migrated" size="tiny" type="success" round>已迁移</n-tag>
        <n-button v-if="!folder.migrated" size="tiny" type="primary" quaterny :disabled="isRunning" @click.stop="migrateOne(folder)">迁移</n-button>
        <n-button v-if="folder.migrated" size="tiny" type="warning" quaterny :disabled="isRunning" @click.stop="rollbackOne(folder)">还原</n-button>
      </div>
    </div>

    <div v-if="selectedFolders.length" class="batch-bar">
      <span>已选 {{ selectedFolders.length }} 个目录</span>
      <n-button size="tiny" type="primary" :disabled="isRunning" @click="migrateBatch">批量迁移</n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { NButton, NInput, NTag, NCheckbox } from 'naive-ui'
import { api, type FolderItem } from '../api/tauri'

const currentPath = ref('/Users/mac/Desktop')
const folders = ref<FolderItem[]>([])
const selectedFolders = ref<string[]>([])
const isRunning = ref(false)

async function browse() {
  try {
    folders.value = await api.browseDirectory(currentPath.value)
  } catch (e) {
    console.error('browse error:', e)
  }
}

function navigate(path: string) {
  currentPath.value = path
  browse()
}

function goUp() {
  const parts = currentPath.value.split('/').filter(Boolean)
  parts.pop()
  currentPath.value = '/' + parts.join('/')
  if (!currentPath.value) currentPath.value = '/'
  browse()
}

function toggleSelect(path: string) {
  const idx = selectedFolders.value.indexOf(path)
  if (idx >= 0) selectedFolders.value.splice(idx, 1)
  else selectedFolders.value.push(path)
}

async function migrateOne(folder: FolderItem) {
  // Use first removable disk as target
  isRunning.value = true
  await api.migrateApp(folder.path, '/Volumes/AppData')
  isRunning.value = false
  browse()
}

async function migrateBatch() {
  isRunning.value = true
  for (const path of selectedFolders.value) {
    await api.migrateApp(path, '/Volumes/AppData')
  }
  isRunning.value = false
  selectedFolders.value = []
  browse()
}

async function rollbackOne(folder: FolderItem) {
  isRunning.value = true
  await api.rollbackApp(folder.path)
  isRunning.value = false
  browse()
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return (bytes / 1024 / 1024 / 1024).toFixed(1) + ' GB'
  if (bytes >= 1024 * 1024) return (bytes / 1024 / 1024).toFixed(0) + ' MB'
  if (bytes >= 1024) return (bytes / 1024).toFixed(0) + ' KB'
  return bytes + ' B'
}

browse()
</script>

<style scoped>
.folder-migrator { padding: 8px; }
.page-title { font-size: 20px; font-weight: 600; margin-bottom: 16px; }
.path-bar { display: flex; gap: 8px; align-items: center; margin-bottom: 8px; }
.quick-links { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 12px; }
.folder-list { background: #1a1a1a; border: 1px solid #2a2a2a; border-radius: 8px; overflow: hidden; }
.folder-item { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid #222; cursor: pointer; }
.folder-item:hover { background: #1f1f1f; }
.folder-icon { display: inline-block; width: 14px; height: 14px; border-radius: 3px; flex-shrink: 0; }
.icon-normal { background: #4080ff; }
.icon-migrated { background: #63e2b7; }
.folder-name { flex: 1; font-size: 13px; }
.folder-size { font-size: 11px; color: #666; }
.batch-bar { display: flex; justify-content: space-between; align-items: center; padding: 8px 12px; background: #1e3a21; border-radius: 6px; margin-top: 12px; }
</style>
