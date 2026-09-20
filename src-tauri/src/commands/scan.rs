use std::path::Path;
use std::os::unix::fs::MetadataExt;
use tauri::AppHandle;
use jwalk::WalkDir;
use crate::models::types::{DirNode, FileInfo};
use crate::utils::fs;

#[tauri::command]
pub async fn scan_directory(
    app: AppHandle,
    path: String,
    depth: Option<usize>,
) -> Result<DirNode, String> {
    fs::validate_path(&path)?;

    let max_depth = depth.unwrap_or(3);
    let app_clone = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        scan_dir_recursive(&app_clone, &path, 0, max_depth)
    }).await;

    match result {
        Ok(r) => r,
        Err(e) => Err(e.to_string()),
    }
}

fn scan_dir_recursive(app: &AppHandle, path: &str, current_depth: usize, max_depth: usize) -> Result<DirNode, String> {
    let path_obj = Path::new(path);
    let name = path_obj.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    if current_depth >= max_depth {
        let (count, _, size) = fs::scan_dir_stats(path);
        return Ok(DirNode {
            name, path: path.to_string(), size, file_count: count,
            is_dir: true, children: vec![],
        });
    }

    let mut children = Vec::new();
    let mut total_size = 0u64;
    let mut total_files = 0u64;

    let entries = WalkDir::new(path).max_depth(1).follow_links(false);
    for entry in entries {
        match entry {
            Ok(e) => {
                if e.file_type().is_dir() && e.path().to_string_lossy() != path {
                    let child_path = e.path().to_string_lossy().to_string();
                    match scan_dir_recursive(app, &child_path, current_depth + 1, max_depth) {
                        Ok(child) => {
                            total_size += child.size;
                            total_files += child.file_count;
                            children.push(child);
                        }
                        Err(_) => {}
                    }
                } else if e.file_type().is_file() {
                    if let Ok(meta) = e.metadata() {
                        total_size += meta.blocks() * 512;
                        total_files += 1;
                    }
                }
            }
            Err(_) => {}
        }
    }

    // Sort children by size descending — largest first for UI
    children.sort_by(|a, b| b.size.cmp(&a.size));

    Ok(DirNode {
        name, path: path.to_string(), size: total_size,
        file_count: total_files, is_dir: true, children,
    })
}

#[tauri::command]
pub async fn get_dir_size(path: String) -> Result<u64, String> {
    fs::validate_path(&path)?;
    if !fs::path_exists(&path) {
        return Err("Path does not exist".to_string());
    }
    let result = tokio::task::spawn_blocking(move || fs::get_dir_size(&path)).await;
    Ok(result.map_err(|e| e.to_string())?)
}

/// Find large files — collect matching, sort by size desc, truncate to top 200
#[tauri::command]
pub async fn find_large_files(
    path: String,
    min_size: Option<u64>,
) -> Result<Vec<FileInfo>, String> {
    fs::validate_path(&path)?;

    let min = min_size.unwrap_or(100 * 1024 * 1024);
    const MAX_RESULTS: usize = 200;

    let result = tokio::task::spawn_blocking(move || -> Result<Vec<FileInfo>, String> {
        let mut files: Vec<FileInfo> = WalkDir::new(&path)
            .follow_links(false)
            .skip_hidden(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                let size = meta.len();
                if size < min { return None; }
                let modified = meta.modified()
                    .map(|t| {
                        let dt: chrono::DateTime<chrono::Local> = t.into();
                        dt.format("%Y-%m-%d %H:%M").to_string()
                    })
                    .ok();
                Some(FileInfo {
                    name: e.file_name().to_string_lossy().to_string(),
                    path: e.path().to_string_lossy().to_string(),
                    size,
                    modified: modified.unwrap_or_default(),
                    is_dir: false,
                })
            })
            .collect();

        // Partial sort: only need top 200, use select_nth_unstable for O(n)
        if files.len() > MAX_RESULTS {
            let pivot = files.select_nth_unstable_by(MAX_RESULTS - 1, |a, b| b.size.cmp(&a.size));
            let _ = pivot;
            files.truncate(MAX_RESULTS);
        } else {
            files.sort_by(|a, b| b.size.cmp(&a.size));
        }

        Ok(files)
    }).await;

    match result {
        Ok(r) => r,
        Err(e) => Err(e.to_string()),
    }
}
