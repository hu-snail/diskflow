use std::path::Path;
use jwalk::WalkDir;
use rayon::prelude::*;
use tauri::{AppHandle, Emitter};
use crate::models::types::CleanupItem;
use crate::utils::fs;

const CACHE_PATHS: &[(&str, &str)] = &[
    ("Homebrew 缓存", "/Users/mac/Library/Caches/Homebrew"),
    ("pnpm 缓存", "/Users/mac/Library/Caches/pnpm"),
    ("Playwright 缓存", "/Users/mac/Library/Caches/ms-playwright"),
    ("Yarn 缓存", "/Users/mac/Library/Caches/Yarn"),
    ("npm 缓存", "/Users/mac/.npm"),
    ("Cargo 缓存", "/Users/mac/.cargo/registry"),
    ("Docker 缓存", "/Users/mac/Library/Containers/com.docker.docker/Data/vms/0"),
    ("velopack 缓存", "/Users/mac/Library/Caches/velopack"),
    ("VS Code 缓存", "/Users/mac/Library/Application Support/Code/Cache"),
    ("TRAE 缓存", "/Users/mac/Library/Application Support/TRAE SOLO CN/ModularData"),
];

const LOG_PATHS: &[(&str, &str)] = &[
    ("系统日志", "/var/log"),
    ("用户日志", "/Users/mac/Library/Logs"),
    ("诊断日志", "/Library/Logs/DiagnosticReports"),
];

/// Scan all cache paths in parallel using rayon::par_iter
#[tauri::command]
pub async fn scan_cache() -> Result<Vec<CleanupItem>, String> {
    let result = tokio::task::spawn_blocking(|| {
        // Collect existing paths first (fast, no traversal)
        let to_scan: Vec<(&str, &str)> = CACHE_PATHS.iter()
            .filter(|(_, path)| Path::new(path).exists())
            .map(|(n, p)| (*n, *p))
            .collect();

        // Scan each path in parallel across CPU cores
        let items: Vec<CleanupItem> = to_scan.par_iter()
            .filter_map(|(name, path)| {
                let (count, _, size) = fs::scan_dir_stats(path);
                if size > 0 {
                    Some(CleanupItem {
                        name: name.to_string(),
                        path: path.to_string(),
                        size,
                        category: "cache".to_string(),
                        file_count: count,
                        last_modified: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        let mut items = items;
        items.sort_by(|a, b| b.size.cmp(&a.size));
        items
    }).await;

    Ok(result.map_err(|e| e.to_string())?)
}

#[tauri::command]
pub async fn scan_logs() -> Result<Vec<CleanupItem>, String> {
    let result = tokio::task::spawn_blocking(|| {
        // Fixed paths scanned in parallel
        let to_scan: Vec<(&str, &str)> = LOG_PATHS.iter()
            .filter(|(_, path)| Path::new(path).exists())
            .map(|(n, p)| (*n, *p))
            .collect();

        let mut items: Vec<CleanupItem> = to_scan.par_iter()
            .filter_map(|(name, path)| {
                let (count, _, size) = fs::scan_dir_stats(path);
                if size > 0 {
                    Some(CleanupItem {
                        name: name.to_string(),
                        path: path.to_string(),
                        size,
                        category: "log".to_string(),
                        file_count: count,
                        last_modified: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Scan user log subdirectories in parallel
        let log_dir = "/Users/mac/Library/Logs";
        if Path::new(log_dir).exists() {
            let sub_dirs: Vec<String> = WalkDir::new(log_dir)
                .max_depth(1)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_dir() && e.path().to_string_lossy() != log_dir)
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();

            let sub_items: Vec<CleanupItem> = sub_dirs.par_iter()
                .filter_map(|dir_path| {
                    let (count, _, size) = fs::scan_dir_stats(dir_path);
                    if size > 10 * 1024 * 1024 {
                        let name = Path::new(dir_path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| dir_path.clone());
                        Some(CleanupItem {
                            name: format!("日志: {}", name),
                            path: dir_path.clone(),
                            size,
                            category: "log".to_string(),
                            file_count: count,
                            last_modified: None,
                        })
                    } else {
                        None
                    }
                })
                .collect();

            items.extend(sub_items);
        }

        items.sort_by(|a, b| b.size.cmp(&a.size));
        items
    }).await;

    Ok(result.map_err(|e| e.to_string())?)
}

#[tauri::command]
pub async fn clean_path(
    app: AppHandle,
    path: String,
) -> Result<bool, String> {
    use serde::Serialize;

    #[derive(Clone, Serialize)]
    struct LogEvent {
        level: String,
        message: String,
    }

    fs::validate_path(&path)?;

    let name = fs::basename(&path);

    // Get size via du (single command — avoids full Rust traversal)
    let path_clone = path.clone();
    let size = tokio::task::spawn_blocking(move || {
        let out = std::process::Command::new("du")
            .args(["-sk", "--", &path_clone])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|kb| kb * 1024)
            }
            _ => None,
        }
    }).await;

    let size = size.map_err(|e| e.to_string())?.unwrap_or(0);

    let _ = app.emit("migrate:log", LogEvent {
        level: "info".to_string(),
        message: format!("🗑️ 正在清理: {} ({})", name, fs::format_size(size)),
    });

    let path_clone2 = path.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("rm")
            .args(["-rf", "--", &path_clone2])
            .output()
    }).await;

    match output {
        Ok(Ok(out)) if out.status.success() => {
            let _ = app.emit("migrate:log", LogEvent {
                level: "success".to_string(),
                message: format!("✅ 清理完成，释放 {}", fs::format_size(size)),
            });
            Ok(true)
        }
        Ok(Ok(out)) => {
            let _ = app.emit("migrate:log", LogEvent {
                level: "error".to_string(),
                message: format!("❌ 清理失败: {}", String::from_utf8_lossy(&out.stderr)),
            });
            Ok(false)
        }
        _ => Ok(false),
    }
}
