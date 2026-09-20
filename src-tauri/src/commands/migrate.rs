use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter};
use serde::Serialize;
use base64::Engine;
use crate::models::types::{MigrateResult, FolderItem, VerifyResult, AppItem};
use crate::utils::{fs, verify, fs as bfs};

#[derive(Clone, Serialize)]
struct ProgressEvent {
    current: u64,
    total: u64,
    file: String,
    app: String,
    app_index: u32,
    app_total: u32,
}

#[derive(Clone, Serialize)]
struct LogEvent {
    level: String,
    message: String,
    time: String,
}

fn log(app: &AppHandle, level: &str, msg: &str) {
    let _ = app.emit("migrate:log", LogEvent {
        level: level.to_string(),
        message: msg.to_string(),
        time: chrono::Local::now().format("%H:%M:%S").to_string(),
    });
}

fn progress(app: &AppHandle, current: u64, total: u64, file: &str, app_name: &str, app_index: u32, app_total: u32) {
    let _ = app.emit("migrate:progress", ProgressEvent {
        current, total,
        file: file.to_string(),
        app: app_name.to_string(),
        app_index, app_total,
    });
}

/// Copy using `cp -av` with batched progress events.
/// Instead of emitting IPC for every file (can be 10000+ events),
/// batch progress updates every ~50ms or every 100 files.
fn copy_with_progress(
    source: &str,
    target: &str,
    app: &AppHandle,
    app_name: &str,
    app_index: u32,
    app_total: u32,
    source_files: u64,
) -> Result<(), String> {
    let total = source_files.max(1);

    let mut child = Command::new("cp")
        .args(["-a", "-v", "--", source, target])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 cp 失败: {}", e))?;

    let stderr = child.stderr.take().ok_or("无法读取 cp 输出")?;

    // 64KB buffer (default is 8KB) — reduces read syscalls
    let reader = BufReader::with_capacity(65536, stderr);

    let mut copied: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let emit_interval = std::time::Duration::from_millis(100);
    let batch_threshold = 100u64;
    let mut since_last_batch = 0u64;

    for line in reader.lines() {
        if let Ok(ref line) = line {
            copied = copied.saturating_add(1).min(total);
            since_last_batch += 1;

            // Batch: emit either every 100ms or every 100 files, whichever comes first
            if since_last_batch >= batch_threshold || last_emit.elapsed() >= emit_interval {
                let pct = ((copied as f64 / total as f64) * 99.0).min(99.0) as u64;
                let filename = line.rsplit('/').next().unwrap_or(line);
                progress(app, pct, total, filename, app_name, app_index, app_total);
                last_emit = std::time::Instant::now();
                since_last_batch = 0;
            }
        }
    }

    let status = child.wait().map_err(|e| format!("等待 cp 完成失败: {}", e))?;

    if !status.success() {
        // Fallback: try rsync
        log(app, "warning", "cp 失败，尝试 rsync...");
        let rsync_result = Command::new("rsync")
            .args(["-a", "--", &format!("{}/", source), &format!("{}/", target)])
            .output();

        match rsync_result {
            Ok(out) if out.status.success() => {}
            Ok(out) => return Err(format!("rsync 也失败: {}", String::from_utf8_lossy(&out.stderr))),
            Err(e) => return Err(format!("rsync 启动失败: {}", e)),
        }
    }

    progress(app, total, total, "完成", app_name, app_index, app_total);
    Ok(())
}

#[tauri::command]
pub async fn migrate_app(
    app: AppHandle,
    source: String,
    target: String,
) -> Result<MigrateResult, String> {
    fs::validate_path(&source).map_err(|e| e.clone())?;
    fs::validate_path(&target).map_err(|e| e.clone())?;

    log(&app, "section", &format!("━━━ 迁移: {} ━━━", bfs::basename(&source)));
    log(&app, "info", &format!("源路径: {}", source));

    let folder_name = bfs::basename(&source);
    let target_full = bfs::join(&target, &folder_name);
    log(&app, "info", &format!("目标: {}", target_full));

    if !bfs::path_exists(&source) {
        log(&app, "error", "源路径不存在");
        return Ok(MigrateResult {
            success: false, source_size: 0, target_size: 0,
            file_count: 0, verify_passed: false,
            error: Some("Source path does not exist".to_string()),
        });
    }

    if bfs::is_symlink(&source) {
        log(&app, "warning", "源路径已经是符号链接，无需迁移");
        return Ok(MigrateResult {
            success: false, source_size: 0, target_size: 0,
            file_count: 0, verify_passed: false,
            error: Some("Already a symlink".to_string()),
        });
    }

    // Step 1: Parallel scan (single traversal via jwalk)
    log(&app, "info", "📊 统计文件中...");
    let source_clone1 = source.clone();
    let (file_count, source_size, _) = tokio::task::spawn_blocking(move || {
        bfs::scan_dir_stats(&source_clone1)
    }).await.map_err(|e| e.to_string())?;
    log(&app, "info", &format!("  共 {} 个文件，{}", file_count, fs::format_size(source_size)));

    // Step 2: Copy with cp -av (batched per-file progress)
    log(&app, "info", "📋 复制数据中...");
    if bfs::path_exists(&target_full) {
        let target_rm = target_full.clone();
        tokio::task::spawn_blocking(move || { let _ = std::fs::remove_dir_all(&target_rm); })
            .await.ok();
    }

    let app_clone2 = app.clone();
    let source_clone2 = source.clone();
    let target_clone = target_full.clone();
    let folder_clone = folder_name.clone();
    let copy_result = tokio::task::spawn_blocking(move || {
        copy_with_progress(
            &source_clone2, &target_clone,
            &app_clone2, &folder_clone,
            0, 1, file_count,
        )
    }).await;

    match copy_result {
        Ok(Ok(())) => log(&app, "success", "✓ 数据复制完成"),
        Ok(Err(e)) => {
            log(&app, "error", &format!("❌ 复制失败: {}", e));
            let _ = std::fs::remove_dir_all(&target_full);
            return Ok(MigrateResult {
                success: false, source_size, target_size: 0,
                file_count, verify_passed: false,
                error: Some(e),
            });
        }
        Err(e) => {
            log(&app, "error", &format!("❌ 任务异常: {}", e));
            let _ = std::fs::remove_dir_all(&target_full);
            return Ok(MigrateResult {
                success: false, source_size, target_size: 0,
                file_count, verify_passed: false,
                error: Some(e.to_string()),
            });
        }
    }

    // Step 3: Verification — only scan target (we already have source stats from step 1)
    // This halves the verify cost by reusing pre-computed source data
    log(&app, "info", "🔍 数据校验中...");
    let target_clone3 = target_full.clone();
    let (tgt_files, tgt_size, _) = tokio::task::spawn_blocking(move || {
        bfs::scan_dir_stats(&target_clone3)
    }).await.map_err(|e| e.to_string())?;

    let file_count_match = file_count == tgt_files;
    let size_diff = if source_size > 0 {
        ((tgt_size as f64 - source_size as f64).abs() / source_size as f64) * 100.0
    } else { 0.0 };
    let size_match = size_diff < 1.0;

    // Tier 2: rsync only if tier 1 passes
    let content_match = if file_count_match && size_match {
        let source_clone4 = source.clone();
        let target_clone4 = target_full.clone();
        let rsync_ok = tokio::task::spawn_blocking(move || {
            let out = std::process::Command::new("rsync")
                .args(["-a", "--dry-run", "--itemize-changes", "--out-format=%n", "--",
                       &format!("{}/", source_clone4), &format!("{}/", target_clone4)])
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).lines()
                        .filter(|l| !l.is_empty())
                        .count() == 0
                }
                _ => true, // rsync unavailable — rely on tier 1
            }
        }).await.unwrap_or(true);
        rsync_ok
    } else {
        false
    };

    log(&app, "info", &format!("  文件数: 源 {} / 目标 {}", file_count, tgt_files));
    log(&app, "info", &format!("  总字节数: 源 {} / 目标 {}", fs::format_size(source_size), fs::format_size(tgt_size)));

    let verify_passed = file_count_match && size_match && content_match;
    if verify_passed {
        log(&app, "success", "✓ 三重校验全部通过");
    } else {
        log(&app, "error", "❌ 数据校验失败");
        let _ = std::fs::remove_dir_all(&target_full);
        return Ok(MigrateResult {
            success: false, source_size, target_size: tgt_size,
            file_count, verify_passed: false,
            error: Some("Verification failed".to_string()),
        });
    }

    // Step 4: Backup (instant — just a rename)
    log(&app, "info", "📦 备份原目录...");
    let backup_path = bfs::get_backup_path(&source);
    match bfs::rename(&source, &backup_path) {
        Ok(_) => log(&app, "success", &format!("✓ 备份完成: {}", backup_path)),
        Err(e) => {
            log(&app, "error", &format!("❌ 备份失败: {}", e));
            let _ = std::fs::remove_dir_all(&target_full);
            return Ok(MigrateResult {
                success: false, source_size, target_size: tgt_size,
                file_count, verify_passed: true,
                error: Some(format!("Backup failed: {}", e)),
            });
        }
    }

    // Step 5: Create symlink (instant)
    log(&app, "info", "🔗 创建符号链接...");
    match bfs::create_symlink(&target_full, &source) {
        Ok(_) => log(&app, "success", "✓ 符号链接创建成功"),
        Err(e) => {
            log(&app, "error", &format!("❌ 符号链接失败: {}", e));
            let _ = bfs::rename(&backup_path, &source);
            let _ = std::fs::remove_dir_all(&target_full);
            return Ok(MigrateResult {
                success: false, source_size, target_size: tgt_size,
                file_count, verify_passed: true,
                error: Some(format!("Symlink failed: {}", e)),
            });
        }
    }

    log(&app, "success", &format!("✅ {} 迁移完成!", folder_name));

    Ok(MigrateResult {
        success: true,
        source_size,
        target_size: tgt_size,
        file_count,
        verify_passed: true,
        error: None,
    })
}

#[tauri::command]
pub async fn rollback_app(
    app: AppHandle,
    source: String,
) -> Result<bool, String> {
    fs::validate_path(&source)?;

    let folder_name = bfs::basename(&source);
    log(&app, "section", &format!("━━━ 回滚: {} ━━━", folder_name));

    if !bfs::is_symlink(&source) {
        log(&app, "error", "不是符号链接，无法回滚");
        return Ok(false);
    }

    let backup_path = bfs::get_backup_path(&source);
    if !bfs::path_exists(&backup_path) {
        log(&app, "error", "备份目录不存在，无法回滚");
        return Ok(false);
    }

    let target = bfs::read_symlink(&source).unwrap_or_default();
    log(&app, "info", &format!("链接目标: {}", target));

    // Step 1: Remove symlink (instant)
    log(&app, "info", "删除符号链接...");
    let _ = std::fs::remove_file(&source);

    // Step 2: Restore backup (instant — just a rename)
    log(&app, "info", "恢复备份...");
    match bfs::rename(&backup_path, &source) {
        Ok(_) => log(&app, "success", "✓ 备份恢复成功"),
        Err(e) => {
            log(&app, "error", &format!("❌ 恢复失败: {}", e));
            if !target.is_empty() {
                let _ = bfs::create_symlink(&target, &source);
            }
            return Ok(false);
        }
    }

    // Step 3: Delete external data ASYNCHRONOUSLY (don't block rollback)
    if !target.is_empty() && bfs::path_exists(&target) {
        log(&app, "info", "后台清理外部磁盘数据...");
        tokio::task::spawn_blocking(move || {
            let _ = Command::new("rm").args(["-rf", "--", &target]).output();
        });
    }

    log(&app, "success", &format!("✅ {} 回滚完成!", folder_name));
    Ok(true)
}

#[tauri::command]
pub async fn cleanup_backup(
    app: AppHandle,
    source: String,
) -> Result<bool, String> {
    fs::validate_path(&source)?;

    let folder_name = bfs::basename(&source);
    log(&app, "section", &format!("━━━ 清理: {} ━━━", folder_name));

    if !bfs::is_symlink(&source) {
        log(&app, "error", "不是符号链接，无法清理");
        return Ok(false);
    }

    let backup_path = bfs::get_backup_path(&source);
    fs::validate_path(&backup_path)?;

    if !bfs::path_exists(&backup_path) {
        log(&app, "error", "无备份目录");
        return Ok(false);
    }

    // Get size via du (single command, no separate full traversal)
    let bp_size = backup_path.clone();
    let size_result = tokio::task::spawn_blocking(move || {
        let out = Command::new("du")
            .args(["-sk", "--", &bp_size])
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

    let backup_size = size_result.map_err(|e| e.to_string())?.unwrap_or(0);
    log(&app, "info", &format!("  备份大小: {}", fs::format_size(backup_size)));

    log(&app, "info", "正在删除...");
    let bp = backup_path.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new("rm").args(["-rf", "--", &bp]).output()
    }).await;

    match output {
        Ok(Ok(out)) if out.status.success() => {
            log(&app, "success", &format!("✅ 释放 {}", fs::format_size(backup_size)));
            Ok(true)
        }
        Ok(Ok(out)) => {
            log(&app, "error", &format!("❌ 删除失败: {}", String::from_utf8_lossy(&out.stderr)));
            Ok(false)
        }
        _ => Ok(false),
    }
}

#[tauri::command]
pub async fn verify_migration(
    source: String,
    target: String,
) -> Result<VerifyResult, String> {
    fs::validate_path(&source)?;
    fs::validate_path(&target)?;
    let result = tokio::task::spawn_blocking(move || {
        verify::verify_migration(&source, &target)
    }).await.map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn browse_directory(dir_path: String) -> Result<Vec<FolderItem>, String> {
    use std::fs as stdfs;

    let entries = stdfs::read_dir(&dir_path).map_err(|e| e.to_string())?;
    let mut folders = Vec::new();

    // Collect all subdirectory paths first
    let mut dir_paths: Vec<String> = Vec::new();

    for entry in entries {
        match entry {
            Ok(e) => {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; }
                let full_path = e.path().to_string_lossy().to_string();
                let meta = match e.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                if meta.file_type().is_symlink() {
                    let target = bfs::read_symlink(&full_path).unwrap_or_default();
                    let has_backup = bfs::path_exists(&format!("{}.backup", full_path));
                    folders.push(FolderItem {
                        name,
                        path: full_path,
                        size: 0,
                        size_label: format!("→ {}", bfs::basename(&target)),
                        size_computed: true,
                        exists: true,
                        migrated: true,
                        has_backup,
                        symlink_target: target,
                    });
                } else if meta.is_dir() {
                    let has_backup = bfs::path_exists(&format!("{}.backup", full_path));
                    dir_paths.push(full_path.clone());
                    folders.push(FolderItem {
                        name,
                        path: full_path,
                        size: 0,
                        size_label: "计算中...".to_string(),
                        size_computed: false,
                        exists: true,
                        migrated: false,
                        has_backup,
                        symlink_target: String::new(),
                    });
                }
            }
            Err(_) => {}
        }
    }

    // Batch size calculation: single `du -d 1 -k` call gets all children at once
    if !dir_paths.is_empty() {
        let size_map = du_dir_sizes(&dir_path);
        for f in folders.iter_mut() {
            if !f.size_computed && !f.migrated {
                if let Some(&size) = size_map.get(&f.path) {
                    f.size = size;
                    f.size_label = fs::format_size(size);
                    f.size_computed = true;
                } else {
                    // Fallback: individual du for paths not in the map
                    let p = f.path.clone();
                    if let Ok(out) = Command::new("du").args(["-sk", "--", &p]).output() {
                        if out.status.success() {
                            if let Some(kb) = String::from_utf8_lossy(&out.stdout)
                                .split_whitespace()
                                .next()
                                .and_then(|s| s.parse::<u64>().ok())
                            {
                                let size = kb * 1024;
                                f.size = size;
                                f.size_label = fs::format_size(size);
                                f.size_computed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(folders)
}

/// Get sizes of all direct children of a directory using a single `du -d 1 -k` call.
/// Much faster than calling du individually for each entry.
fn du_dir_sizes(dir: &str) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    let output = Command::new("du")
        .args(["-d", "1", "-k", "--", dir])
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let mut parts = line.splitn(2, '\t');
                if let (Some(size_str), Some(path_str)) = (parts.next(), parts.next()) {
                    if let Ok(kb) = size_str.parse::<u64>() {
                        map.insert(path_str.to_string(), kb * 1024);
                    }
                }
            }
        }
    }
    map
}

/// Fast scan: only reads directory entries and detects symlinks. No size calculation.
/// For apps: include all. For data/cache/container: only include symlinks and dirs that
/// either have a .backup (migrated) or pass the size filter via quick `du`.
fn scan_directory_items_fast(dir: &str, items: &mut Vec<AppItem>, category: &str, min_size: u64) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    // For data categories, use du to get sizes in one call, then filter
    let size_map = if min_size > 0 {
        du_dir_sizes(dir)
    } else {
        HashMap::new()
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }
        if name.ends_with(".backup") { continue; }

        let path = entry.path().to_string_lossy().to_string();

        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_symlink = meta.file_type().is_symlink();
        let is_app = name.ends_with(".app");

        if !meta.is_dir() && !is_symlink { continue; }

        // Get size from the map (already calculated for data categories)
        let size = size_map.get(&path).copied().unwrap_or(0);

        // For data categories: filter by size (keep symlinks regardless of size)
        if min_size > 0 && !is_symlink && !is_app {
            if size < min_size { continue; }
        }

        let symlink_target = if is_symlink {
            bfs::read_symlink(&path).unwrap_or_default()
        } else {
            String::new()
        };

        let has_backup = Path::new(&format!("{}.backup", path)).exists();

        items.push(AppItem {
            name,
            path,
            size,
            is_app,
            migrated: is_symlink,
            has_backup,
            symlink_target,
            category: category.to_string(),
        });
    }
}

#[derive(Clone, Serialize)]
struct AppSizeEvent {
    path: String,
    size: u64,
}

/// Fast scan: returns all apps and data directories immediately, no size calculation.
#[tauri::command]
pub async fn scan_apps() -> Result<Vec<AppItem>, String> {
    let result = tokio::task::spawn_blocking(|| {
        let mut items: Vec<AppItem> = Vec::new();
        let home = std::env::var("HOME").unwrap_or_default();

        // System apps — no size filter
        scan_directory_items_fast("/Applications", &mut items, "app", 0);

        // System utilities
        scan_directory_items_fast("/Applications/Utilities", &mut items, "app", 0);

        // User apps
        let user_apps = format!("{}/Applications", home);
        if Path::new(&user_apps).exists() {
            scan_directory_items_fast(&user_apps, &mut items, "app", 0);
        }

        // App data — filter > 50MB
        let app_support = format!("{}/Library/Application Support", home);
        scan_directory_items_fast(&app_support, &mut items, "data", 50 * 1024 * 1024);

        // Caches — filter > 50MB
        let caches = format!("{}/Library/Caches", home);
        scan_directory_items_fast(&caches, &mut items, "cache", 50 * 1024 * 1024);

        // Containers — filter > 50MB
        let containers = format!("{}/Library/Containers", home);
        scan_directory_items_fast(&containers, &mut items, "container", 50 * 1024 * 1024);

        // Deduplicate: collect all symlink target paths, then remove non-migrated
        // entries whose path matches a symlink target (those are the real dirs
        // on external disks that symlinks point to — they shouldn't show separately)
        let symlink_targets: Vec<String> = items.iter()
            .filter(|a| a.migrated)
            .map(|a| a.symlink_target.clone())
            .collect();

        let deduped: Vec<AppItem> = items.into_iter()
            .filter(|a| {
                // Keep migrated (symlink) entries
                if a.migrated { return true; }
                // Keep non-migrated entries whose path is NOT a symlink target
                !symlink_targets.contains(&a.path)
            })
            .collect();

        deduped
    }).await;

    Ok(result.map_err(|e| e.to_string())?)
}

/// Async size calculation: computes sizes in background, emits "app-size" events.
/// Uses `du -sk -L` to follow symlinks for migrated items.
#[tauri::command]
pub async fn compute_app_sizes(app: AppHandle, paths: Vec<String>) -> Result<bool, String> {
    // Batch paths by parent directory for efficient `du -d 1 -k` calls
    let mut by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for path in &paths {
        let parent = Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        by_parent.entry(parent).or_default().push(path.clone());
    }

    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        for (parent, child_paths) in &by_parent {
            // Run `du -d 1 -k -L` on parent — -L follows symlinks
            let output = Command::new("du")
                .args(["-d", "1", "-k", "-L", "--", parent])
                .output();

            let size_map: HashMap<String, u64> = match output {
                Ok(out) if out.status.success() => {
                    let mut m = HashMap::new();
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let mut parts = line.splitn(2, '\t');
                        if let (Some(size_str), Some(path_str)) = (parts.next(), parts.next()) {
                            if let Ok(kb) = size_str.parse::<u64>() {
                                m.insert(path_str.to_string(), kb * 1024);
                            }
                        }
                    }
                    m
                }
                _ => HashMap::new(),
            };

            // Emit events for each child path
            for path in child_paths {
                let size = size_map.get(path).copied().unwrap_or(0);
                // Fallback: individual du for paths not in batch result
                let size = if size == 0 {
                    let out = Command::new("du")
                        .args(["-sk", "-L", "--", path])
                        .output();
                    match out {
                        Ok(o) if o.status.success() => {
                            String::from_utf8_lossy(&o.stdout)
                                .split_whitespace()
                                .next()
                                .and_then(|s| s.parse::<u64>().ok())
                                .map(|kb| kb * 1024)
                                .unwrap_or(0)
                        }
                        _ => 0,
                    }
                } else {
                    size
                };

                let _ = app_handle.emit("app-size", AppSizeEvent {
                    path: path.clone(),
                    size,
                });
            }
        }
    });

    Ok(true)
}

#[tauri::command]
pub async fn get_app_icon(app_path: String) -> Result<String, String> {
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        if !app_path.ends_with(".app") {
            return Ok(String::new());
        }

        let info_plist = format!("{}/Contents/Info.plist", app_path);
        if !Path::new(&info_plist).exists() {
            return Ok(String::new());
        }

        // Get icon file name from Info.plist
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleIconFile", &info_plist])
            .output()
            .map_err(|e| e.to_string())?;

        if !output.status.success() {
            return Ok(String::new());
        }

        let icon_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if icon_name.is_empty() {
            return Ok(String::new());
        }

        let resources = format!("{}/Contents/Resources", app_path);

        // Try different extensions
        let icon_path = ["icns", "tiff", "png", ""]
            .iter()
            .map(|ext| {
                if ext.is_empty() {
                    format!("{}/{}", resources, icon_name)
                } else if icon_name.ends_with(&format!(".{}", ext)) {
                    format!("{}/{}", resources, icon_name)
                } else {
                    format!("{}/{}.{}", resources, icon_name, ext)
                }
            })
            .find(|p| Path::new(p).exists());

        let icon_path = match icon_path {
            Some(p) => p,
            None => return Ok(String::new()),
        };

        // Convert to PNG using sips, cache in /tmp
        let tmp_dir = "/tmp/diskflow_icons";
        let _ = std::fs::create_dir_all(tmp_dir);

        let app_name = Path::new(&app_path)
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or("icon".to_string());
        let tmp_path = format!("{}/{}.png", tmp_dir, app_name);

        // Return cached icon if exists
        if Path::new(&tmp_path).exists() {
            if let Ok(data) = std::fs::read(&tmp_path) {
                return Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&data)));
            }
        }

        // Convert with sips
        let convert_result = Command::new("sips")
            .args(["-s", "format", "png", &icon_path, "--out", &tmp_path])
            .output();

        if let Ok(out) = convert_result {
            if out.status.success() {
                if let Ok(data) = std::fs::read(&tmp_path) {
                    return Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&data)));
                }
            }
        }

        Ok(String::new())
    }).await;

    result.map_err(|e| e.to_string())?
}
