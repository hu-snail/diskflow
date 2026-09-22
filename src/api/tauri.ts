import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export interface DiskInfo {
  name: string
  mount_point: string
  total_space: number
  available_space: number
  fs_type: string
  is_removable: boolean
  is_sparse: boolean
}

export interface DirNode {
  name: string
  path: string
  size: number
  file_count: number
  is_dir: boolean
  children: DirNode[]
}

export interface MigrateResult {
  success: boolean
  source_size: number
  target_size: number
  file_count: number
  verify_passed: boolean
  error: string | null
}

export interface SystemInfo {
  cpu_usage: number
  cpu_cores: number
  mem_total: number
  mem_used: number
  disk_read: number
  disk_write: number
  uptime: number
}

export interface FileInfo {
  name: string
  path: string
  size: number
  modified: string
  is_dir: boolean
}

export interface FolderItem {
  name: string
  path: string
  size: number
  size_label: string
  size_computed: boolean
  exists: boolean
  migrated: boolean
  has_backup: boolean
  symlink_target: string
}

export interface CleanupItem {
  name: string
  path: string
  size: number
  category: string
  file_count: number
  last_modified: string | null
}

export interface AppItem {
  name: string
  path: string
  size: number
  is_app: boolean
  migrated: boolean
  has_backup: boolean
  symlink_target: string
  category: string
  is_appstore?: boolean
  is_running?: boolean
}

export interface ProgressEvent {
  current: number
  total: number
  file: string
  app: string
  app_index: number
  app_total: number
}

export interface LogEvent {
  level: string
  message: string
  time: string
}

export interface MetricsEvent {
  cpu_usage: number
  mem_used: number
  mem_total: number
  disk_read: number
  disk_write: number
}

export const api = {
  getDisks: (): Promise<DiskInfo[]> => invoke('get_disks'),
  getDiskInfo: (mountPoint: string): Promise<DiskInfo> => invoke('get_disk_info', { mountPoint }),
  scanDirectory: (path: string, depth?: number): Promise<DirNode> => invoke('scan_directory', { path, depth }),
  getDirSize: (path: string): Promise<number> => invoke('get_dir_size', { path }),
  findLargeFiles: (path: string, minSize?: number): Promise<FileInfo[]> => invoke('find_large_files', { path, minSize }),
  migrateApp: (source: string, target: string): Promise<MigrateResult> => invoke('migrate_app', { source, target }),
  rollbackApp: (source: string): Promise<boolean> => invoke('rollback_app', { source }),
  cleanupBackup: (source: string): Promise<boolean> => invoke('cleanup_backup', { source }),
  verifyMigration: (source: string, target: string) => invoke('verify_migration', { source, target }),
  browseDirectory: (dirPath: string): Promise<FolderItem[]> => invoke('browse_directory', { dirPath }),
  getSystemInfo: (): Promise<SystemInfo> => invoke('get_system_info'),
  startMonitoring: (): Promise<boolean> => invoke('start_monitoring'),
  stopMonitoring: (): Promise<boolean> => invoke('stop_monitoring'),
  scanGroup: (group?: string, forceRefresh?: boolean): Promise<CleanupItem[]> => invoke('scan_group', { group, forceRefresh }),
  scanDownloads: (forceRefresh?: boolean, minAgeDays?: number): Promise<CleanupItem[]> => invoke('scan_downloads', { forceRefresh, minAgeDays }),
  scanTrash: (forceRefresh?: boolean): Promise<CleanupItem[]> => invoke('scan_trash', { forceRefresh }),
  pauseScan: (category: string): Promise<void> => invoke('pause_scan', { category }),
  resumeScan: (category: string): Promise<void> => invoke('resume_scan', { category }),
  cancelScan: (category: string): Promise<void> => invoke('cancel_scan', { category }),
  cleanPath: (path: string): Promise<boolean> => invoke('clean_path', { path }),
  cleanPathsBatch: (batchId: string, paths: string[], sizes?: number[]): Promise<CleanResult[]> =>
    invoke('clean_paths_batch', { batchId, paths, sizes }),
  scanApps: (): Promise<AppItem[]> => invoke('scan_apps'),
  scanExternalApps: (dir: string): Promise<AppItem[]> => invoke('scan_external_apps', { dir }),
  getRunningApps: (): Promise<string[]> => invoke('get_running_apps'),
  computeAppSizes: (paths: string[], targets?: string[]): Promise<boolean> => invoke('compute_app_sizes', { paths, targets }),
  getAppIcon: (appPath: string): Promise<string> => invoke('get_app_icon', { appPath }),
}

export function onProgress(callback: (e: ProgressEvent) => void): Promise<UnlistenFn> {
  return listen<ProgressEvent>('migrate:progress', (event) => callback(event.payload))
}

export function onLog(callback: (e: LogEvent) => void): Promise<UnlistenFn> {
  return listen<LogEvent>('migrate:log', (event) => callback(event.payload))
}

export function onMetrics(callback: (e: MetricsEvent) => void): Promise<UnlistenFn> {
  return listen<MetricsEvent>('system:metrics', (event) => callback(event.payload))
}

export interface AppSizeEvent {
  path: string
  size: number
}

export function onAppSize(callback: (e: AppSizeEvent) => void): Promise<UnlistenFn> {
  return listen<AppSizeEvent>('app-size', (event) => callback(event.payload))
}

export interface CleanupProgressEvent {
  category: string
  current: number
  total: number
  phase: string
  elapsed_ms: number
  paused: boolean
  cancelled: boolean
}

export interface CleanupItemEvent {
  category: string
  item: CleanupItem
}

export interface CleanResult {
  path: string
  ok: boolean
  error: string | null
}

export interface CleanProgressEvent {
  batch_id: string
  path: string
  name: string
  size: number
  index: number
  total: number
  status: 'running' | 'done' | 'failed'
  error: string | null
}

/// Final summary emitted at the end of a `clean_paths_batch` call.
/// Carries the authoritative freed-bytes total so the UI doesn't have
/// to re-accumulate from per-item events (which could drift if any
/// tick was dropped or batch_id was mismatched).
export interface CleanFinishedEvent {
  batch_id: string
  freed_bytes: number
  success_count: number
  failed_count: number
}

export function onCleanupProgress(callback: (e: CleanupProgressEvent) => void): Promise<UnlistenFn> {
  return listen<CleanupProgressEvent>('cleanup:progress', (event) => callback(event.payload))
}

export function onCleanupItem(callback: (e: CleanupItemEvent) => void): Promise<UnlistenFn> {
  return listen<CleanupItemEvent>('cleanup:item', (event) => callback(event.payload))
}

export function onCleanupCleanProgress(callback: (e: CleanProgressEvent) => void): Promise<UnlistenFn> {
  return listen<CleanProgressEvent>('cleanup:clean-progress', (event) => callback(event.payload))
}

export function onCleanupCleanFinished(callback: (e: CleanFinishedEvent) => void): Promise<UnlistenFn> {
  return listen<CleanFinishedEvent>('cleanup:clean-finished', (event) => callback(event.payload))
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return (bytes / 1024 / 1024 / 1024).toFixed(1) + ' GB'
  if (bytes >= 1024 * 1024) return (bytes / 1024 / 1024).toFixed(0) + ' MB'
  if (bytes >= 1024) return (bytes / 1024).toFixed(0) + ' KB'
  return bytes + ' B'
}
