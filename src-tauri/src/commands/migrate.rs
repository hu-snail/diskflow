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

    // Refuse to migrate a running app — its binary is mapped in memory and
    // renaming the source directory (or replacing it with a symlink) will
    // either fail mid-flight or leave the process holding a stale inode
    // that disappears the moment the app exits. UI already hides these, but
    // enforce it server-side as well.
    let running_apps = detect_running_apps();
    let app_name = bfs::basename(&source);
    if app_name.ends_with(".app") {
        let is_match = running_apps.iter().any(|p| {
            p == &source || p.ends_with(&format!("/{}", app_name))
        });
        if is_match {
            log(&app, "error", "应用正在运行中，请先退出后再迁移");
            return Ok(MigrateResult {
                success: false, source_size: 0, target_size: 0,
                file_count: 0, verify_passed: false,
                error: Some("App is currently running. Please quit it before migrating.".to_string()),
            });
        }
    }

    // Step 1: Parallel scan (single traversal via jwalk)
    log(&app, "info", "统计文件中...");
    let source_clone1 = source.clone();
    let (file_count, source_size, _) = tokio::task::spawn_blocking(move || {
        bfs::scan_dir_stats(&source_clone1)
    }).await.map_err(|e| e.to_string())?;
    log(&app, "info", &format!("  共 {} 个文件，{}", file_count, fs::format_size(source_size)));

    // Step 2: Copy with cp -av (batched per-file progress)
    log(&app, "info", "复制数据中...");
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
            log(&app, "error", &format!("复制失败: {}", e));
            let _ = std::fs::remove_dir_all(&target_full);
            return Ok(MigrateResult {
                success: false, source_size, target_size: 0,
                file_count, verify_passed: false,
                error: Some(e),
            });
        }
        Err(e) => {
            log(&app, "error", &format!("任务异常: {}", e));
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
    log(&app, "info", "数据校验中...");
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
        log(&app, "error", "数据校验失败");
        let _ = std::fs::remove_dir_all(&target_full);
        return Ok(MigrateResult {
            success: false, source_size, target_size: tgt_size,
            file_count, verify_passed: false,
            error: Some("Verification failed".to_string()),
        });
    }

    // Step 4: Backup (instant — just a rename)
    log(&app, "info", "备份原目录...");
    let backup_path = bfs::get_backup_path(&source);
    match bfs::rename(&source, &backup_path) {
        Ok(_) => log(&app, "success", &format!("✓ 备份完成: {}", backup_path)),
        Err(e) => {
            log(&app, "error", &format!("备份失败: {}", e));
            let _ = std::fs::remove_dir_all(&target_full);
            return Ok(MigrateResult {
                success: false, source_size, target_size: tgt_size,
                file_count, verify_passed: true,
                error: Some(format!("Backup failed: {}", e)),
            });
        }
    }

    // Step 5: Create symlink (instant)
    log(&app, "info", "创建符号链接...");
    match bfs::create_symlink(&target_full, &source) {
        Ok(_) => log(&app, "success", "✓ 符号链接创建成功"),
        Err(e) => {
            log(&app, "error", &format!("符号链接失败: {}", e));
            let _ = bfs::rename(&backup_path, &source);
            let _ = std::fs::remove_dir_all(&target_full);
            return Ok(MigrateResult {
                success: false, source_size, target_size: tgt_size,
                file_count, verify_passed: true,
                error: Some(format!("Symlink failed: {}", e)),
            });
        }
    }

    log(&app, "success", &format!("{} 迁移完成!", folder_name));

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
            log(&app, "error", &format!("恢复失败: {}", e));
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

    log(&app, "success", &format!("{} 回滚完成!", folder_name));
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
            log(&app, "success", &format!("释放 {}", fs::format_size(backup_size)));
            Ok(true)
        }
        Ok(Ok(out)) => {
            log(&app, "error", &format!("删除失败: {}", String::from_utf8_lossy(&out.stderr)));
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

/// Paths whose symlinks should NOT be treated as "migrated" — they are
/// macOS-managed system redirects (e.g. `/Applications/Safari.app` is a
/// symlink into `/System/Cryptexes/...`).
/// Only counts the prefix of a *resolved* absolute path.
fn is_system_symlink_target(target: &str) -> bool {
    const SYSTEM_PREFIXES: &[&str] = &[
        "/System/",
        "/System",
        "/private/var/",
        "/private/etc/",
        "/usr/libexec/",
        "/Library/Apple/",
        "/Library/Apple",
        "/Library/Developer/",
        "/Library/Developer",   // Xcode Command Line Tools (real apps stay; Apple-managed goes here)
    ];
    let t = if let Some(rest) = target.strip_prefix("../") {
        // Relative symlink (e.g. Safari.app -> ../System/Cryptexes/...)
        // Resolve against /Applications' parent (= /)
        format!("/{}", rest)
    } else {
        target.to_string()
    };
    SYSTEM_PREFIXES.iter().any(|p| t == *p || t.starts_with(p))
}

/// True if this item is something we should hide from the migration UI.
/// - System-provided symlinks (Safari.app -> /System/Cryptexes/...)
/// - Apple-managed frameworks under /Library
fn is_system_managed_item(path: &str, is_symlink: bool, symlink_target: &str) -> bool {
    if is_symlink && is_system_symlink_target(symlink_target) {
        return true;
    }
    // Apps that ship with macOS (defensive whitelist — these are in /Applications
    // and look like real apps but are not user-migratable)
    const APPLE_SYSTEM_APPS: &[&str] = &[
        "Safari.app",
        "Mail.app",
        "Messages.app",
        "FaceTime.app",
        "Photo Booth.app",
        "TextEdit.app",
        "Preview.app",
        "Calendar.app",
        "Contacts.app",
        "Reminders.app",
        "Notes.app",
        "Maps.app",
        "Music.app",
        "Podcasts.app",
        "TV.app",
        "App Store.app",
        "System Settings.app",
        "System Preferences.app",
        "Terminal.app",
        "Calculator.app",
        "Stickies.app",
        "Chess.app",
        "Dictionary.app",
        "Font Book.app",
        "Image Capture.app",
        "QuickTime Player.app",
        "Grapher.app",
        "Automator.app",
        "Script Editor.app",
        "Boot Camp Assistant.app",
        "Activity Monitor.app",
        "Console.app",
        "Disk Utility.app",
        "Migration Assistant.app",
        "Screenshot.app",
        "Voice Memos.app",
        "Books.app",
        "Freeform.app",
        "Home.app",
        "Find My.app",
        "Shortcuts.app",
        "Stocks.app",
        "Weather.app",
        "Clock.app",
        "News.app",
        "Translate.app",
        "Launchpad.app",
        "Siri.app",
        "Time Machine.app",
        "AirPort Utility.app",
        "Bluetooth File Exchange.app",
        "ColorSync Utility.app",
        "Digital Color Meter.app",
        "Grab.app",
        "Keychain Access.app",
        "RAID Utility.app",
        "System Information.app",
        "Terminal.app",
    ];
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    APPLE_SYSTEM_APPS.iter().any(|a| a == &name)
}

/// Detect "stub" apps. A `.app` is a stub if any of:
///   - No `Contents/` directory at all (empty shell)
///   - No `Contents/MacOS/` directory
///   - The largest binary inside `Contents/MacOS/` is < 1 MB
///
/// A real macOS app's main executable is always at least 1 MB (most are
/// much larger). A small "launcher" binary left behind after the real
/// app data was moved to an external disk is typically only ~225 KB.
fn is_stub_app(path: &str, is_app: bool) -> bool {
    if !is_app { return false; }
    let contents = format!("{}/Contents", path);
    if !Path::new(&contents).exists() { return true; }

    let macos = format!("{}/Contents/MacOS", path);
    let entries = match std::fs::read_dir(&macos) {
        Ok(it) => it,
        Err(_) => return true,
    };

    // Find the largest file inside Contents/MacOS/ — that's the main binary.
    let mut max_size: u64 = 0;
    for e in entries.flatten() {
        if let Ok(meta) = std::fs::symlink_metadata(e.path()) {
            if meta.is_file() {
                max_size = max_size.max(meta.len());
                if max_size >= 1024 * 1024 {
                    // 1 MB threshold — definitely not a stub
                    return false;
                }
            }
        }
    }
    // No file reached 1 MB → stub (or no executable at all).
    true
}

/// Detect App Store app by checking for `Contents/_MASReceipt/receipt`.
/// Returning true tells the UI to show a "商店" tag and warn the user that
/// migrating will break auto-update.
fn is_appstore_app(path: &str, is_app: bool) -> bool {
    if !is_app { return false; }
    let receipt = format!("{}/Contents/_MASReceipt/receipt", path);
    Path::new(&receipt).exists()
}

/// For stub apps, search common external-disk locations for the matching
/// basename and return the first one that exists. Candidates include
/// `/Volumes/<disk>/<name>.app` and `/Volumes/<disk>/App|Apps|AppData/<name>.app`.
fn find_external_stub_target(name: &str) -> String {
    let entries = match std::fs::read_dir("/Volumes") {
        Ok(e) => e,
        Err(_) => return String::new(),
    };
    for entry in entries.flatten() {
        let mount = entry.path();

        // Skip the system root mount. On macOS, `/Volumes/Macintosh HD` is a
        // symlink to `/` — searching it would match the local `/Applications`
        // stubs and report those as the "external" copy. Use `canonicalize`
        // and reject anything whose real path is on the boot volume.
        match std::fs::canonicalize(&mount) {
            Ok(canon) if canon == Path::new("/") => continue,
            Ok(_) => {}
            Err(_) => continue, // Unreadable mount — skip.
        }

        if !mount.is_dir() { continue; }
        // 1. Direct under mount
        let direct = mount.join(name);
        if direct.exists() { return direct.to_string_lossy().to_string(); }
        // 2. Common nested locations
        for sub in &[
            "",                    // mount root
            "App", "Apps", "AppData", "Applications",
            "Mac/Applications", "MacOS/Applications", "Macos/Applications",
            "MacOS/app", "Macos/app", "MacOS/Apps", "Macos/Apps",
            "macOS/Applications", "macOS/app",
        ] {
            let nested = mount.join(sub).join(name);
            if nested.exists() { return nested.to_string_lossy().to_string(); }
        }
    }
    String::new()
}

/// Detect which `.app` bundles currently have a process holding their
/// `Contents/MacOS/` binary open. Uses `lsof` — it lists every open file on
/// the system and reliably shows the actual binary path (including across
/// symlinks) for each running app. Returns a set of canonical bundle paths.
///
/// Notes:
///   - Runs are matched against the *resolved* path. Migrated apps keep
///     running against the external target, so we canonicalize each path
///     before insertion so callers can compare against either side.
///   - `.localized` etc. that mention MacOS but are not real apps get
///     filtered out by stripping the `Contents/MacOS/...` suffix and then
///     requiring the remaining path to end with `.app` and exist on disk.
///   - Cached with a short TTL so calling `scan_apps` doesn't spawn lsof
///     repeatedly on every refresh.
fn detect_running_apps() -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    use std::time::{Duration, Instant};
    use std::sync::Mutex;

    static CACHE: std::sync::LazyLock<Mutex<Option<(Instant, HashSet<String>)>>> =
        std::sync::LazyLock::new(|| Mutex::new(None));
    const TTL: Duration = Duration::from_secs(3);

    {
        let guard = CACHE.lock().unwrap();
        if let Some((t, set)) = guard.as_ref() {
            if t.elapsed() < TTL {
                return set.clone();
            }
        }
    }

    let mut set: HashSet<String> = HashSet::new();
    let out = Command::new("lsof")
        .args(["-nP", "-F", "n"])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            // -F n outputs one filename per line. Extract everything under
            // */Contents/MacOS/*, strip the suffix, and keep paths that
            // (after canonicalization) end with `.app`.
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                if !line.starts_with("n") { continue; }
                let path = &line[1..];
                // Trim the "n" prefix added by -F.
                let path = path.trim_start_matches('n');
                if !path.contains("/Contents/MacOS/") { continue; }
                let bundle = match path.split("/Contents/MacOS/").next() {
                    Some(b) => b,
                    None => continue,
                };
                if !bundle.ends_with(".app") { continue; }
                if let Ok(canon) = std::fs::canonicalize(bundle) {
                    set.insert(canon.to_string_lossy().to_string());
                }
                // Also keep the raw form so un-canonicalized callers can match.
                set.insert(bundle.to_string());
            }
        }
    }

    if let Ok(mut g) = CACHE.lock() {
        *g = Some((Instant::now(), set.clone()));
    }
    set
}

/// Fast scan: only reads directory entries and detects symlinks. No size calculation.
/// For apps: include all. For data/cache/container: only include symlinks and dirs that
/// either have a .backup (migrated) or pass the size filter via quick `du`.
fn scan_directory_items_fast(dir: &str, items: &mut Vec<AppItem>, category: &str, min_size: u64, running: &std::collections::HashSet<String>) {
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
        // Mark as a true `.app` bundle when this is the app scan category
        // (`/Applications*`) or the external library scan category (a user-
        // chosen directory like `/Volumes/.../App`). Library data dirs
        // (`Application Support` / `Caches` / `Containers`) intentionally do
        // not match — they contain sandbox data folders whose names happen
        // to end in `.app` (e.g. `~/Library/Application Support/com.clashfx.app`).
        let is_app = (category == "app" || category == "external_app") && name.ends_with(".app");

        if !meta.is_dir() && !is_symlink { continue; }

        let symlink_target = if is_symlink {
            bfs::read_symlink(&path).unwrap_or_default()
        } else {
            String::new()
        };

        // Filter out macOS-managed symlinks pointing into /System or /private.
        // Symlinks pointing to user-migrated external disks are kept.
        if is_symlink && is_system_symlink_target(&symlink_target) {
            continue;
        }

        // A `.app` directory without `Contents/` is a stub — the real data was
        // moved out. Resolve its symlink target if any, otherwise treat the
        // empty shell as "migrated" so the UI shows it in the same group as
        // real symlinks. Also compute size from the resolved external path.
        let stub = is_stub_app(&path, is_app);
        let resolved_target: String = if is_symlink {
            // Absolute or relative symlink: read it; if relative, resolve
            // against the parent dir.
            let raw = symlink_target.clone();
            if raw.starts_with('/') {
                raw
            } else if !raw.is_empty() {
                let parent = Path::new(&path).parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                bfs::join(&parent, &raw)
            } else {
                String::new()
            }
        } else if stub {
            // Stub app: search external disks for the real copy.
            find_external_stub_target(&name)
        } else {
            String::new()
        };

        // migrated: real symlink OR stub (Contents missing) OR has .backup
        let has_backup = Path::new(&format!("{}.backup", path)).exists();
        let migrated = is_symlink || stub || has_backup;

        // Fast scan: do NOT compute size here. Sizes come from
        // `compute_app_sizes` (parallel) emitted as `app-size` events so the UI
        // can render immediately and update sizes in the background. We only
        // know a size upfront for non-migrated data categories, where `du` was
        // already batched once per parent dir.
        let size = if (is_symlink || stub) && !resolved_target.is_empty() {
            0
        } else {
            size_map.get(&path).copied().unwrap_or(0)
        };

        // For data categories: filter by size (keep symlinks/stub regardless)
        if min_size > 0 && !is_symlink && !is_app && !stub {
            if size < min_size { continue; }
        }

        // App Store detection (only meaningful for symlinks / real apps; a stub
        // won't have _MASReceipt because Contents/ is gone).
        let appstore = if is_symlink && !resolved_target.is_empty() {
            is_appstore_app(&resolved_target, true)
        } else if !stub {
            is_appstore_app(&path, is_app)
        } else {
            false
        };

        // For app/external_app categories, only surface real `.app` bundles
        // and their symlink/stub forms. Sibling directories like
        // `/Applications/Utilities` (a folder of system tools) or
        // `~/Applications/Chrome Apps.localized` (Chrome's local storage)
        // are not apps and must not be listed.
        if (category == "app" || category == "external_app") && !is_app {
            continue;
        }

        // Detect "running" state. For a real symlink/stub, the live process
        // is running against the *external* target — match against the
        // resolved path. For an unmigrated bundle, match against `path`.
        let is_running = if !resolved_target.is_empty() {
            running.contains(&resolved_target)
        } else {
            running.contains(&path)
        };

        items.push(AppItem {
            name,
            path,
            size,
            is_app,
            migrated,
            has_backup,
            symlink_target: resolved_target,
            category: category.to_string(),
            is_appstore: appstore,
            is_running,
        });
    }
}

#[derive(Clone, Serialize)]
struct AppSizeEvent {
    path: String,
    size: u64,
}

/// Fast scan: returns all apps and data directories immediately, no size calculation.
///
/// NOTE: This scan only enumerates `.app` bundles. Library data dirs
/// (`Application Support` / `Caches` / `Containers`) are intentionally NOT
/// scanned here — those belong to data migration and cleanup features, not
/// application migration, and they each contain hundreds of entries which
/// caused the previous version of this command to take 30+ seconds.
#[tauri::command]
pub async fn scan_apps() -> Result<Vec<AppItem>, String> {
    let result = tokio::task::spawn_blocking(|| {
        let mut items: Vec<AppItem> = Vec::new();
        let home = std::env::var("HOME").unwrap_or_default();

        // Snapshot of currently-running app bundles. Used to mark items
        // that should be hidden from migration until the user quits them.
        let running = detect_running_apps();

        // System apps — no size filter
        scan_directory_items_fast("/Applications", &mut items, "app", 0, &running);

        // System utilities
        scan_directory_items_fast("/Applications/Utilities", &mut items, "app", 0, &running);

        // User apps
        let user_apps = format!("{}/Applications", home);
        if Path::new(&user_apps).exists() {
            scan_directory_items_fast(&user_apps, &mut items, "app", 0, &running);
        }

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

/// Scan an external library directory (e.g. `/Volumes/JZ-miniGo/Macos/App`)
/// for `.app` bundles. These are the "real" copies the local symlinks/stubs
/// point at. Returns one `AppItem` per `.app` found, with size = 0 (use
/// `compute_app_sizes` to populate).
#[tauri::command]
pub async fn scan_external_apps(dir: String) -> Result<Vec<AppItem>, String> {
    fs::validate_path(&dir).map_err(|e| e.clone())?;
    let result = tokio::task::spawn_blocking(move || {
        let mut items: Vec<AppItem> = Vec::new();
        let running = detect_running_apps();
        if Path::new(&dir).exists() {
            scan_directory_items_fast(&dir, &mut items, "external_app", 0, &running);
        }
        // Only keep .app entries (filter out any sibling files accidentally
        // picked up — scan_directory_items_fast already keeps dirs).
        items.retain(|i| i.is_app);
        items
    }).await;

    Ok(result.map_err(|e| e.to_string())?)
}

/// Lightweight refresh of which `.app` bundles are currently running.
/// Returns the set of canonical bundle paths; the UI matches them against
/// either the local `path` or the migrated `symlink_target` (whichever is
/// non-empty). Cheap to call repeatedly; the heavy `lsof -nP` is cached
/// for 3 seconds inside `detect_running_apps`.
#[tauri::command]
pub async fn get_running_apps() -> Result<Vec<String>, String> {
    let result = tokio::task::spawn_blocking(|| {
        detect_running_apps().into_iter().collect()
    }).await;
    Ok(result.map_err(|e| e.to_string())?)
}

/// Compute size of a single path. Tries `du -sk -L` (follow symlinks) first so
/// migrated apps report the size of their external copy. Falls back to
/// `du -sk` (don't follow) when the target is unreadable (e.g. system symlinks
/// under SIP). Returns 0 if both fail.
fn du_size_for_path(path: &str) -> u64 {
    let parse_kb = |out: &std::process::Output| -> u64 {
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .unwrap_or(0)
    };

    // Try following symlinks — works for migrated apps (target is on external disk)
    if let Ok(out) = Command::new("du").args(["-sk", "-L", "--", path]).output() {
        if out.status.success() {
            let sz = parse_kb(&out);
            if sz > 0 {
                return sz;
            }
        }
    }

    // Fallback: do not follow symlinks
    if let Ok(out) = Command::new("du").args(["-sk", "--", path]).output() {
        if out.status.success() {
            return parse_kb(&out);
        }
    }

    0
}

/// Async size calculation: computes sizes in background, emits "app-size"
/// events. Each path is sized with jwalk + `blocks()` accounting. This is
/// far faster than `du` (which spawns a process per path and walks the
/// whole tree separately) and avoids the cold-start cost of every du
/// invocation.
///
/// We serialize walks across paths to avoid USB-bus contention when the
/// targets live on the same external drive — benchmarks showed that
/// concurrent walks on an exFAT external drive starved all but the first
/// thread (most returned 0). Serial is also more I/O-friendly overall.
#[tauri::command]
pub async fn compute_app_sizes(
    app: AppHandle,
    paths: Vec<String>,
    targets: Option<Vec<String>>,
) -> Result<bool, String> {
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        use std::collections::HashMap;

        // Map: scanned path -> resolved target (may be empty)
        let target_map: HashMap<String, String> = match targets {
            Some(ts) if ts.len() == paths.len() => paths.iter().cloned().zip(ts).collect(),
            _ => HashMap::new(),
        };

        // Filter out system symlinks — their targets live under SIP and
        // walking them fails or returns 0 anyway.
        let filtered: Vec<String> = paths.into_iter()
            .filter(|p| {
                let is_sym = bfs::is_symlink(p);
                if !is_sym { return true; }
                let t = bfs::read_symlink(p).unwrap_or_default();
                !is_system_symlink_target(&t)
            })
            .collect();

        // For each path, pick the target to walk.
        let jobs: Vec<(String, String)> = filtered.into_iter().map(|p| {
            let target = target_map.get(&p).cloned().unwrap_or_default();
            if !target.is_empty() && std::path::Path::new(&target).exists() {
                (p, target)
            } else {
                (p.clone(), p)
            }
        }).collect();

        // Serial walk, emit progress as each path completes so the UI can
        // update incrementally. jwalk emits items lazily, so the first
        // paths show their size long before the big ones (like Xcode) finish.
        for (path, target) in jobs {
            let size = walk_size_bytes(&target);
            let _ = app_handle.emit("app-size", AppSizeEvent { path, size });
        }
    });

    Ok(true)
}

/// Walk a directory tree using jwalk and sum the *logical* file size
/// (`len()`), not the on-disk block allocation. This matches what macOS
/// Finder reports in "Get Info" and what users intuitively expect.
///
/// Why not `blocks() * 512` (which is what `du` reports)? On external
/// drives with large block sizes (128 KB on exFAT by default), block-
/// accounting inflates the number significantly — wpsoffice reports as
/// 5.9 GB with `blocks()` but only 2.77 GB logical. Users comparing
/// against Finder see the discrepancy as a bug.
///
/// jwalk itself reads dir entries with `getdents` in bulk and uses a
/// Rayon-style worker pool internally, so this is much faster than
/// shelling out to `du` per path.
fn walk_size_bytes(path: &str) -> u64 {
    use jwalk::WalkDir;
    use std::os::unix::fs::MetadataExt;

    // If the root path itself is a symlink, jwalk will silently follow it
    // and walk the target's contents (its special-case at depth 0).
    // For migrated apps we only want the bytes the stub occupies locally —
    // a real symlink like `/Applications/Chrome.app -> /Volumes/...` is
    // just the symlink itself (a few dozen bytes). Return its lstat size.
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return meta.len();
        }
    }

    let mut total: u64 = 0;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() { continue; }
        if let Ok(meta) = entry.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    total
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

        // Convert with sips at a larger size and remove any solid background
        // so the icon looks at home on the dark UI. We use `sips -z 144 144`
        // to output a 144x144 PNG, then run ImageMagick (`magick`) if
        // available to knock out the white background. If ImageMagick is
        // not installed we fall back to a raw PNG and the UI handles the
        // background via CSS `mix-blend-mode`.
        let convert_result = Command::new("sips")
            .args(["-z", "144", "144", &icon_path, "--out", &tmp_path])
            .output();

        if let Ok(out) = convert_result {
            if out.status.success() {
                let final_path = knock_out_background(&tmp_path);
                if let Ok(data) = std::fs::read(&final_path) {
                    return Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&data)));
                }
            }
        }

        Ok(String::new())
    }).await;
    result.map_err(|e| e.to_string())?
}

/// Try to remove a solid white/black background from a PNG using ImageMagick
/// (`magick`). If ImageMagick isn't installed or the conversion fails, the
/// original path is returned unchanged so the caller falls back to the raw
/// PNG (the UI handles the background visually with CSS).
fn knock_out_background(png_path: &str) -> String {
    let out_path = png_path.replace(".png", "_nobg.png");
    // `-fuzz 8%%` tolerates slight anti-aliasing on icon edges.
    let result = Command::new("magick")
        .args([
            png_path,
            "-bordercolor", "white",
            "-border", "1",
            "-fuzz", "8%",
            "-trim",
            "+repage",
            "-alpha", "on",
            "-background", "none",
            "-channel", "A",
            "-evaluate", "multiply", "1.0",
            "+channel",
            &out_path,
        ])
        .output();
    match result {
        Ok(o) if o.status.success() && std::path::Path::new(&out_path).exists() => out_path,
        _ => png_path.to_string(),
    }
}
