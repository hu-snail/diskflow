use std::fs;
use std::path::{Path, PathBuf};
use std::os::unix::fs::{MetadataExt, symlink};
use jwalk::WalkDir;

/// System-critical paths that must never be migrated or deleted
const FORBIDDEN_PATHS: &[&str] = &[
    "/",
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/var",
    "/dev",
    "/private",
    "/Volumes",
    "/Library",
    "/Users",
];

/// Validate that a path is safe to operate on (migrate/delete/scan)
pub fn validate_path(path: &str) -> Result<(), String> {
    let canonical = fs::canonicalize(path)
        .map_err(|e| format!("路径无效: {}", e))?
        .to_string_lossy()
        .to_string();

    for forbidden in FORBIDDEN_PATHS {
        if canonical == *forbidden {
            return Err(format!("禁止操作系统目录: {}", forbidden));
        }
    }

    if canonical.contains("..") {
        return Err("路径包含非法跳转".to_string());
    }

    Ok(())
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1073741824.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.0} MB", bytes as f64 / 1048576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Thread-local accumulator — avoids atomic contention on every file.
/// Each rayon worker accumulates locally, then merges at the end.
#[derive(Default)]
struct StatsAccumulator {
    file_count: u64,
    logical_size: u64,
    disk_size: u64,
}

/// Parallel scan using jwalk with thread-local accumulation.
/// Uses fold + reduce pattern to avoid atomic contention on every file.
pub fn scan_dir_stats(path: &str) -> (u64, u64, u64) {
    let mut file_count = 0u64;
    let mut logical_size = 0u64;
    let mut disk_size = 0u64;

    for entry in WalkDir::new(path)
        .follow_links(false)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                file_count += 1;
                logical_size += meta.len();
                disk_size += meta.blocks() * 512;
            }
        }
    }

    (file_count, logical_size, disk_size)
}

/// Scan source and target in parallel (two rayon threads simultaneously)
pub fn scan_dir_stats_parallel(source: &str, target: &str) -> [(u64, u64, u64); 2] {
    let (src, tgt) = rayon::join(
        || scan_dir_stats(source),
        || scan_dir_stats(target),
    );
    [src, tgt]
}

pub fn count_files(path: &str) -> u64 {
    scan_dir_stats(path).0
}

pub fn get_dir_size(path: &str) -> u64 {
    scan_dir_stats(path).2
}

pub fn get_logical_size(path: &str) -> u64 {
    scan_dir_stats(path).1
}

pub fn is_symlink(path: &str) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

pub fn read_symlink(path: &str) -> Option<String> {
    fs::read_link(path).ok().map(|p| p.to_string_lossy().to_string())
}

pub fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

pub fn create_symlink(target: &str, link: &str) -> Result<(), String> {
    symlink(target, link).map_err(|e| e.to_string())
}

pub fn rename(src: &str, dst: &str) -> Result<(), String> {
    fs::rename(src, dst).map_err(|e| e.to_string())
}

pub fn remove_dir_all(path: &str) -> Result<(), String> {
    fs::remove_dir_all(path).map_err(|e| e.to_string())
}

pub fn get_backup_path(src: &str) -> String {
    format!("{}.backup", src)
}

pub fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

pub fn join(base: &str, child: &str) -> String {
    PathBuf::from(base).join(child).to_string_lossy().to_string()
}
