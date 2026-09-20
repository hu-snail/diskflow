use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_space: u64,
    pub available_space: u64,
    pub fs_type: String,
    pub is_removable: bool,
    pub is_sparse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub file_count: u64,
    pub is_dir: bool,
    pub children: Vec<DirNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateResult {
    pub success: bool,
    pub source_size: u64,
    pub target_size: u64,
    pub file_count: u64,
    pub verify_passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub cpu_usage: f32,
    pub cpu_cores: u32,
    pub mem_total: u64,
    pub mem_used: u64,
    pub disk_read: u64,
    pub disk_write: u64,
    pub uptime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub modified: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub file_count_match: bool,
    pub size_match: bool,
    pub content_match: bool,
    pub source_files: u64,
    pub target_files: u64,
    pub source_size: u64,
    pub target_size: u64,
    pub differences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderItem {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub size_label: String,
    pub size_computed: bool,
    pub exists: bool,
    pub migrated: bool,
    pub has_backup: bool,
    pub symlink_target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupItem {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub category: String,
    pub file_count: u64,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppItem {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_app: bool,
    pub migrated: bool,
    pub has_backup: bool,
    pub symlink_target: String,
    pub category: String,
}
