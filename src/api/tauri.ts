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
  scanCache: (): Promise<CleanupItem[]> => invoke('scan_cache'),
  scanLogs: (): Promise<CleanupItem[]> => invoke('scan_logs'),
  cleanPath: (path: string): Promise<boolean> => invoke('clean_path', { path }),
  scanApps: (): Promise<AppItem[]> => invoke('scan_apps'),
  computeAppSizes: (paths: string[]): Promise<boolean> => invoke('compute_app_sizes', { paths }),
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

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return (bytes / 1024 / 1024 / 1024).toFixed(1) + ' GB'
  if (bytes >= 1024 * 1024) return (bytes / 1024 / 1024).toFixed(0) + ' MB'
  if (bytes >= 1024) return (bytes / 1024).toFixed(0) + ' KB'
  return bytes + ' B'
}
