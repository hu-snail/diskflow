use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use rayon::prelude::*;
use crate::models::types::CleanupItem;
use crate::utils::fs;

// =============================================================================
// Multi-dimensional scan groups
// =============================================================================
//
// Paths are grouped by *purpose*, not by where they happen to live on disk.
// The frontend exposes one pill per group; selecting a pill scans every
// path tagged with that group. Group "all" is the union of all other
// groups and is handled specially by `scan_group`.
// =============================================================================

const GROUP_DEV: &str = "dev";
const GROUP_SYSTEM: &str = "system";
const GROUP_BROWSER: &str = "browser";
const GROUP_TRASH: &str = "trash";

/// `(display_name, abs_path, group)`
///
/// Paths are owned `String`s (not `&'static`) because they depend on
/// the runtime `$HOME` — different users have different home dirs. The
/// previous version hard-coded `/Users/mac/...` which made every pill
/// except dev (some of which is under `~/Library/...` and similarly
/// affected) report 0 B on any machine whose username wasn't "mac".
type ScannablePath = (String, String, &'static str);

/// Build the full scan list from `$HOME`. Mac-only: most cache roots
/// live under `~/Library/Caches` / `~/Library/Application Support` /
/// `~/.{cargo,gradle,...}`. We resolve `$HOME` once per process.
fn build_scan_paths() -> Vec<ScannablePath> {
    let home = home_dir();
    let lib = format!("{}/Library", home);
    let lib_cache = format!("{}/Caches", lib);
    let lib_app_support = format!("{}/Application Support", lib);
    let lib_developer = format!("{}/Developer", lib);

    let mut out: Vec<ScannablePath> = Vec::with_capacity(64);

    // ── 开发构建残留 (GROUP_DEV) ───────────────────────────────────────
    out.push((s("Homebrew 缓存"),         t(&lib_cache, "Homebrew"),                                       GROUP_DEV));
    out.push((s("pnpm 缓存"),             t(&lib_cache, "pnpm"),                                           GROUP_DEV));
    out.push((s("Playwright 缓存"),        t(&lib_cache, "ms-playwright"),                                  GROUP_DEV));
    out.push((s("Yarn 缓存"),             t(&lib_cache, "Yarn"),                                            GROUP_DEV));
    out.push((s("npm 缓存"),              t(&home, ".npm"),                                                 GROUP_DEV));
    out.push((s("Cargo 缓存"),            t(&home, ".cargo/registry"),                                     GROUP_DEV));
    out.push((s("Docker 缓存"),           t(&lib, "Containers/com.docker.docker/Data/vms/0"),              GROUP_DEV));
    out.push((s("velopack 缓存"),         t(&lib_cache, "velopack"),                                        GROUP_DEV));
    out.push((s("VS Code 缓存"),          t(&lib_app_support, "Code/Cache"),                               GROUP_DEV));
    out.push((s("TRAE 缓存"),             t(&lib_app_support, "TRAE SOLO CN/ModularData"),                 GROUP_DEV));
    out.push((s("Gradle 缓存"),           t(&home, ".gradle/caches"),                                      GROUP_DEV));
    out.push((s("Maven 缓存"),            t(&home, ".m2/repository"),                                      GROUP_DEV));
    out.push((s("CocoaPods 缓存"),        t(&lib_cache, "CocoaPods"),                                       GROUP_DEV));
    out.push((s("Composer 缓存"),         t(&home, ".composer/cache"),                                     GROUP_DEV));
    out.push((s("PyPI 缓存"),             t(&lib_cache, "pip"),                                            GROUP_DEV));
    out.push((s("Bun 缓存"),              t(&home, ".bun/install/cache"),                                  GROUP_DEV));
    out.push((s("Xcode DerivedData"),     t(&lib_developer, "Xcode/DerivedData"),                          GROUP_DEV));
    out.push((s("Xcode Archives"),        t(&lib_developer, "Xcode/Archives"),                             GROUP_DEV));
    out.push((s("iOS 模拟器数据"),         t(&lib_developer, "CoreSimulator/Devices"),                      GROUP_DEV));
    out.push((s("iOS 设备支持"),           t(&lib_developer, "Xcode/iOS DeviceSupport"),                    GROUP_DEV));
    out.push((s("watchOS 设备支持"),       t(&lib_developer, "Xcode/watchOS DeviceSupport"),                GROUP_DEV));
    out.push((s("tvOS 设备支持"),         t(&lib_developer, "Xcode/tvOS DeviceSupport"),                   GROUP_DEV));

    // ── 系统与应用 (GROUP_SYSTEM) ───────────────────────────────────────
    out.push((s("系统日志"),               s("/var/log"),                                                    GROUP_SYSTEM));
    out.push((s("用户日志"),               t(&lib, "Logs"),                                                  GROUP_SYSTEM));
    out.push((s("诊断日志"),               s("/Library/Logs/DiagnosticReports"),                             GROUP_SYSTEM));
    out.push((s("macOS 临时目录"),         s("/private/var/folders"),                                        GROUP_SYSTEM));
    out.push((s("用户临时目录"),            s("/tmp"),                                                        GROUP_SYSTEM));
    out.push((s("崩溃报告"),               t(&lib, "Logs/DiagnosticReports"),                                GROUP_SYSTEM));
    out.push((s("保存的应用状态"),         t(&lib, "Saved Application State"),                              GROUP_SYSTEM));
    out.push((s("旧下载文件"),             t(&home, "Downloads"),                                            GROUP_SYSTEM));

    // ── 浏览器 (GROUP_BROWSER) ─────────────────────────────────────────
    out.push((s("Safari 缓存"),            t(&lib_cache, "com.apple.Safari"),                               GROUP_BROWSER));
    out.push((s("Chrome 缓存"),            t(&lib_cache, "Google/Chrome"),                                  GROUP_BROWSER));
    out.push((s("Edge 缓存"),              t(&lib_cache, "Microsoft Edge"),                                 GROUP_BROWSER));
    out.push((s("Firefox 缓存"),           t(&lib_cache, "Firefox"),                                        GROUP_BROWSER));
    out.push((s("Brave 缓存"),             t(&lib_cache, "BraveSoftware"),                                  GROUP_BROWSER));
    out.push((s("Arc 缓存"),               t(&lib_cache, "company.thebrowser.Browser"),                     GROUP_BROWSER));
    out.push((s("Chromium 缓存"),          t(&lib_cache, "Chromium"),                                       GROUP_BROWSER));
    out.push((s("Vivaldi 缓存"),           t(&lib_cache, "Vivaldi"),                                        GROUP_BROWSER));
    out.push((s("Opera 缓存"),             t(&lib_cache, "Opera Software"),                                 GROUP_BROWSER));
    out.push((s("Opera GX 缓存"),          t(&lib_cache, "com.opera.gx"),                                   GROUP_BROWSER));
    out.push((s("Yandex 缓存"),            t(&lib_cache, "Yandex"),                                         GROUP_BROWSER));
    out.push((s("Tor Browser 缓存"),       t(&lib_app_support, "Comodo/Tormagi/cache"),                     GROUP_BROWSER));
    out.push((s("WebKit 共享缓存"),         t(&lib_cache, "com.apple.WebKit.Networking"),                    GROUP_BROWSER));
    out.push((s("WebKit 页面缓存"),         t(&lib_cache, "com.apple.WebKit.WebContent"),                    GROUP_BROWSER));

    // ── 废纸篓 (GROUP_TRASH) ───────────────────────────────────────────
    // 注意：废纸篓路径由 scan_trash 单独扫描 (它需要枚举 /Volumes 找外置
    // 磁盘的 .Trashes), 不放进 SCAN_PATHS。

    out
}

// Tiny helpers to keep build_scan_paths() readable.
fn s(v: &str) -> String { v.to_string() }
fn t(base: &str, child: &str) -> String {
    let mut p = String::with_capacity(base.len() + 1 + child.len());
    p.push_str(base);
    if !base.ends_with('/') { p.push('/'); }
    p.push_str(child);
    p
}

/// Resolve `$HOME` once per process. Most paths are rooted under HOME,
/// so this is the only piece of env the scanner really needs.
fn home_dir() -> String {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| String::from("/Users/Shared"))
}

fn downloads_dir() -> String {
    let mut p = home_dir();
    p.push_str("/Downloads");
    p
}

fn trash_dir() -> String {
    let mut p = home_dir();
    p.push_str("/.Trash");
    p
}

// =============================================================================
// Safety net
// =============================================================================

const FORBIDDEN_CLEAN_ROOTS_SYSTEM: &[&str] = &[
    // Top-level system roots. The exact match (`canonical == "/"` and
    // friends) and any direct child (via `starts_with("/System/")`)
    // are blocked.
    "/", "/System", "/usr", "/bin", "/sbin", "/etc", "/var", "/dev", "/private",
    // `/Volumes` itself is intentionally NOT in this list: it's a
    // mount-point container, not a single removable directory. Users
    // routinely keep caches on external drives via symlinks, e.g.
    //   ~/Users/<u>/.gradle -> /Volumes/JZ-miniGo/.../App/工具目录/.gradle
    // `canonicalize()` on those resolves to `/Volumes/...` which we
    // MUST allow cleaning. The cleanup UI only ever asks for paths
    // returned by `scan_candidates`, never `/Volumes` itself, so
    // there's no path where someone can `rm -rf /Volumes`.
    "/Library/Apple", "/Library/Developer/CommandLineTools",
    "/Library/Extensions", "/Library/Frameworks",
    "/Library/PrivilegedHelperTools",
];

/// User-relative paths that should never be cleaned regardless of
/// `$HOME`. Built lazily from `home_dir()` so it follows the running
/// user. These protect credentials, Apple-managed data, and any
/// MobileSync backups.
fn forbidden_user_roots() -> Vec<String> {
    let h = home_dir();
    vec![
        format!("{}/Library/Keychains", h),
        format!("{}/.ssh", h),
        format!("{}/Library/Application Support/Apple", h),
        format!("{}/Library/Application Support/MobileSync", h),
        format!("{}/Library/Group Containers", h),
        format!("{}/Library/Containers/com.apple", h),
    ]
}

/// Returns true if `canonical` matches a system- or user-protected root.
/// Resolves user-roots via `home_dir()` on first call and caches them.
fn is_forbidden(canonical: &str) -> bool {
    for r in FORBIDDEN_CLEAN_ROOTS_SYSTEM {
        if canonical == *r || canonical.starts_with(&format!("{}/", r)) {
            return true;
        }
    }
    for r in forbidden_user_roots() {
        if canonical == r || canonical.starts_with(&format!("{}/", r)) {
            return true;
        }
    }
    false
}

// =============================================================================
// Scan cache (in-memory, short TTL)
// =============================================================================

#[derive(Clone)]
struct CachedScan {
    items: Vec<CleanupItem>,
    at: Instant,
}

const CACHE_TTL: Duration = Duration::from_secs(300);

static SCAN_CACHE: std::sync::LazyLock<Mutex<HashMap<&'static str, CachedScan>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn get_cached(category: &'static str) -> Option<Vec<CleanupItem>> {
    let guard = SCAN_CACHE.lock().ok()?;
    guard.get(category).and_then(|c| {
        if c.at.elapsed() < CACHE_TTL { Some(c.items.clone()) } else { None }
    })
}

fn put_cached(category: &'static str, items: Vec<CleanupItem>) {
    if let Ok(mut guard) = SCAN_CACHE.lock() {
        guard.insert(category, CachedScan { items, at: Instant::now() });
    }
}

pub fn clear_scan_cache() {
    if let Ok(mut guard) = SCAN_CACHE.lock() {
        guard.clear();
    }
}

// =============================================================================
// IPC events (with throttling)
// =============================================================================
//
// emit_progress is called from inside the scan loop. Each call goes through
// the IPC stack (tauri::Emitter → webview → JS event handler). With 16
// candidates in `scan_cache` we'd otherwise pay 16× ~50µs = ~800µs of pure
// IPC overhead, on top of the actual scan time. We throttle so that at most
// one event per ~30 ms is sent, plus always emit on completion.
// =============================================================================

#[derive(Clone, Serialize)]
struct ScanProgressEvent {
    category: String,
    current: i64,
    total: i64,
    phase: String,
    /// Wall-clock elapsed time since the scan started, in milliseconds.
    elapsed_ms: u64,
    /// True if the user paused the scan and we're still alive but waiting.
    paused: bool,
    /// True if the scan has been requested to stop (cancelled by user).
    cancelled: bool,
}

/// Per-item event. Rust emits this the moment a candidate's size is known,
/// so the frontend can stream rows into the result list instead of waiting
/// for the whole scan to finish. The `category` lets the frontend drop
/// items that don't belong to the active tab (paranoia: a category switch
/// could otherwise leak items from the previous scan).
#[derive(Clone, Serialize)]
struct ScanItemEvent {
    category: String,
    item: CleanupItem,
}

#[derive(Clone, Serialize)]
struct LogEvent {
    level: String,
    message: String,
}

/// Per-path cleanup status event. Emitted as each path in a batch is
/// started, finished, or failed so the UI can show a real-time progress
/// bar / per-item status instead of a single spinner until everything
/// completes.
#[derive(Clone, Serialize)]
struct CleanProgressEvent {
    /// Batch id so the frontend can correlate events from multiple
    /// concurrent cleanPath calls (or stale events from a previous
    /// batch). Generated by the frontend as a UUID/string.
    batch_id: String,
    /// Path being cleaned (the original user-facing path, not the
    /// canonicalized one — what the UI already shows).
    path: String,
    /// Display name of the item.
    name: String,
    /// Size in bytes (from the scan).
    size: u64,
    /// Index in the batch (0-based).
    index: usize,
    /// Total paths in the batch.
    total: usize,
    /// Current status: "running", "done", "failed".
    status: String,
    /// Error message if status == "failed".
    error: Option<String>,
}

// =============================================================================
// Scan control
//
// We support pause / resume / cancel per category. State is keyed by
// category and lives in process-wide statics so the foreground scan loop
// (on a tokio blocking worker) can poll cheaply without going through a
// Mutex on every iteration.
//
//   cancel_flag[c]  : sticky one-shot. Set by `cancel_scan(c)`. The scan
//                      loop checks once per candidate and exits early.
//   pause_flag[c]   : sticky until cleared. Set by `pause_scan(c)`; cleared
//                      by `resume_scan(c)`. The scan loop parks 50 ms while
//                      this is set.
//   started_at[c]   : Instant captured at scan start, used to compute
//                      elapsed_ms reported to the UI.
//
// We never remove an entry from SCAN_CTRL — entries are inert after a scan
// ends, and the next scan just resets the same slot. This keeps references
// stable for the lifetime of the process.
// =============================================================================

static CANCEL_FLAGS: std::sync::LazyLock<Mutex<HashMap<&'static str, &'static AtomicBool>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static PAUSE_FLAGS: std::sync::LazyLock<Mutex<HashMap<&'static str, &'static AtomicBool>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static STARTED_AT: std::sync::LazyLock<Mutex<HashMap<&'static str, Instant>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Look up (or lazily allocate) the `AtomicBool` for `category`. We leak
/// the `AtomicBool` so we can hand out a `&'static` reference that the scan
/// loop polls without going through any lock. Leaking one bool per
/// category for the lifetime of the process is negligible (~16 bytes).
fn cancel_flag(category: &'static str) -> &'static AtomicBool {
    let mut map = CANCEL_FLAGS.lock().unwrap();
    if let Some(&b) = map.get(category) {
        return b;
    }
    let b: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
    map.insert(category, b);
    b
}

fn pause_flag(category: &'static str) -> &'static AtomicBool {
    let mut map = PAUSE_FLAGS.lock().unwrap();
    if let Some(&b) = map.get(category) {
        return b;
    }
    let b: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
    map.insert(category, b);
    b
}

/// Begin tracking a scan for the given category. Resets any prior cancel
/// flag so a fresh scan isn't immediately killed by leftover state.
fn begin_scan(category: &'static str) {
    cancel_flag(category).store(false, Ordering::Release);
    pause_flag(category).store(false, Ordering::Release);
    STARTED_AT.lock().unwrap().insert(category, Instant::now());
}

/// Called by the frontend to mark the running scan as paused.
#[tauri::command]
pub async fn pause_scan(category: String) -> Result<(), String> {
    let key: &'static str = leak_category(&category);
    pause_flag(key).store(true, Ordering::Release);
    Ok(())
}

/// Called by the frontend to resume a paused scan.
#[tauri::command]
pub async fn resume_scan(category: String) -> Result<(), String> {
    let key: &'static str = leak_category(&category);
    pause_flag(key).store(false, Ordering::Release);
    Ok(())
}

/// Called by the frontend to abort a scan in progress.
#[tauri::command]
pub async fn cancel_scan(category: String) -> Result<(), String> {
    let key: &'static str = leak_category(&category);
    cancel_flag(key).store(true, Ordering::Release);
    // Also clear pause so the loop can exit immediately rather than parking
    pause_flag(key).store(false, Ordering::Release);
    Ok(())
}

/// Permanently leak a `String` so we can hand out a `&'static str` key
/// matching the type of `SCAN_CTRL`'s `&'static str` map. One leak per
/// unique category name across the process lifetime (≈7 categories).
fn leak_category(s: &str) -> &'static str {
    let mut map = CATEGORY_KEYS.lock().unwrap();
    if let Some(&k) = map.get(s) {
        return k;
    }
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    map.insert(s.to_owned(), leaked);
    leaked
}

static CATEGORY_KEYS: std::sync::LazyLock<
    Mutex<HashMap<String, &'static str>>,
> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Returns true if the user requested cancellation. Scans should call this
/// once per candidate and abort on `true`.
fn is_cancelled(category: &'static str) -> bool {
    cancel_flag(category).load(Ordering::Acquire)
}

/// If paused, blocks the calling thread until either:
///   - the pause is cleared (returns `Some(())` to continue), or
///   - cancellation is requested (returns `None` to abort).
///
/// Implemented with a polling loop (50 ms cadence) — much simpler than a
/// Condvar and the scan loop already does blocking I/O on this thread.
fn wait_if_paused(category: &'static str) -> WaitResult {
    loop {
        if is_cancelled(category) {
            return WaitResult::Cancelled;
        }
        if !pause_flag(category).load(Ordering::Acquire) {
            return WaitResult::Continue;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

enum WaitResult {
    Continue,
    Cancelled,
}

// Last-emit timestamp per category. We use Instant so it's monotonic and
// cheap. Lazily allocated on first emit.
static LAST_PROGRESS: std::sync::LazyLock<Mutex<HashMap<&'static str, Instant>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
const PROGRESS_THROTTLE: Duration = Duration::from_millis(40);

fn emit_progress(
    app: &AppHandle,
    category: &'static str,
    current: i64,
    total: i64,
    phase: &str,
) {
    let now = Instant::now();
    let elapsed_ms = STARTED_AT
        .lock()
        .ok()
        .and_then(|m| m.get(category).map(|t| t.elapsed().as_millis() as u64))
        .unwrap_or(0);

    // Throttle: skip if last emit for this category was < 40 ms ago.
    // The "complete" sentinel (current == total when total > 0) ALWAYS emits
    // — it must, or the UI is left at <100% with no "done" signal.
    let is_complete = total > 0 && current == total;
    let should_emit = {
        let guard = LAST_PROGRESS.lock().ok();
        match guard {
            Some(mut g) => {
                match g.get(category) {
                    Some(last) if !is_complete && now.duration_since(*last) < PROGRESS_THROTTLE => false,
                    _ => { g.insert(category, now); true }
                }
            }
            None => true,
        }
    };
    if !should_emit { return; }

    let paused = pause_flag(category).load(Ordering::Acquire);
    let cancelled = cancel_flag(category).load(Ordering::Acquire);

    let _ = app.emit("cleanup:progress", ScanProgressEvent {
        category: category.to_string(),
        current, total,
        phase: phase.to_string(),
        elapsed_ms,
        paused,
        cancelled,
    });
}

fn reset_progress_throttle(category: &str) {
    if let Ok(mut g) = LAST_PROGRESS.lock() {
        g.remove(category);
    }
}

// =============================================================================
// CORE SCAN PRIMITIVES
//
//   - `du_sk_bytes`: `du -sk <path>` → bytes. Used per-candidate inside
//     `scan_candidates` so each candidate is measured in isolation.
//   - `du_depth1_sizes`: `du -d 1 -k <parent>` → per-child sizes. Used
//     for the dynamic "large log sub-dirs under ~/Library/Logs" probe,
//     where we don't know the children up-front.
//   - `count_files_recursive`: in-process std::fs walk for file count
//     (no metadata syscall per file — `DirEntry::file_type` reads from
//     the dirent).
//
// `du` is preferred for size because it's a single fork that uses the
// kernel's fast directory walk; std::fs would be slower and would have
// to deal with permission errors per file.
// =============================================================================

#[derive(Default, Clone, Copy)]
struct DirProbe {
    size: u64,
    count: u64,
}

/// Walk an entire subtree and return aggregate size + file count.
///
/// `du -sk <path>` gives us size in one fork. For count we walk the tree
/// in-process using `std::fs::read_dir` (no jwalk — its worker-pool overhead
/// is a net negative for shallow walks and would compete with rayon's pool).
fn probe_full(path: &Path) -> DirProbe {
    let size = du_sk_bytes(path);
    let count = count_files_recursive(path);
    DirProbe { size, count }
}

/// Ask Finder for the user's trash size via AppleScript.
///
/// Why this exists: macOS SIP makes `~/.Trash` unreadable to non-Finder
/// processes — `du` reports 0 even when the trash is full of multi-GB
/// items. `osascript` runs in the user's UI session and can ask Finder
/// directly, bypassing the SIP gate.
///
/// Returns `(size_bytes, item_count)` on success. `None` if Finder is
/// unavailable (rare) or the script timed out.
///
/// Note: this is a *best-effort* measurement. The bytes returned are
/// what Finder believes the trash occupies, which is what `rm -rf`
/// will reclaim. We intentionally don't fall through to APFS-only
/// measurement — that's overkill for a progress-bar UI affordance.
fn trash_size_via_finder() -> Option<(u64, u64)> {
    let script = r#"
        tell application "System Events"
            try
                set theBytes to size of trash
                set theItems to count of items of trash
                return (theBytes as string) & " " & (theItems as string)
            on error
                return ""
            end try
        end tell
    "#;
    let out = Command::new("osascript")
        .args(["-e", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let s = std::str::from_utf8(&out.stdout).ok()?.trim();
    if s.is_empty() { return None; }
    let mut iter = s.split_whitespace();
    let size: u64 = iter.next()?.parse().ok()?;
    let count: u64 = iter.next()?.parse().ok()?;
    Some((size, count))
}

/// Walk one directory to depth 1 and return per-child-subdir size + count.
// ---------------------------------------------------------------------------
// `du` wrappers
// ---------------------------------------------------------------------------

/// `du -sk <path>` → bytes.
///
/// Falls back to a native std::fs recursive size summation if `du`
/// can't read the path (EACCES on macOS sandboxed dirs, ENOTSUP on
/// some external volumes, etc.). The native fallback is slower than
/// `du` on big subtrees but is correct and never silently returns 0
/// just because of a permission glitch.
fn du_sk_bytes(path: &Path) -> u64 {
    let from_du = du_sk_bytes_via_du(path);
    if from_du > 0 {
        return from_du;
    }
    // `du` returned 0 either because the dir is genuinely empty OR
    // because it couldn't read it. Distinguish by trying a single
    // read_dir: if we get at least one entry, the dir isn't empty and
    // we owe the user a real measurement via the native walker.
    if first_entry_exists(path) {
        sum_file_sizes_recursive(path)
    } else {
        0
    }
}

/// `du -sk <path>` → bytes. The original "du only" implementation,
/// kept separate so we can layer the native fallback in `du_sk_bytes`.
fn du_sk_bytes_via_du(path: &Path) -> u64 {
    let out = match Command::new("du")
        .args(["-sk", "--"])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return 0,
    };
    // Parse first whitespace-separated token. Avoid `String::from_utf8_lossy`
    // on the whole buffer — the first token is ASCII anyway.
    let stdout = &out.stdout;
    let mut i = 0;
    while i < stdout.len() && stdout[i].is_ascii_digit() { i += 1; }
    if i == 0 { return 0; }
    let kb: u64 = std::str::from_utf8(&stdout[..i])
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    kb * 1024
}

/// `du -d 1 -k <parent>` → `Vec<(basename_OsString, size_bytes)>`.
///
/// Used by the system-group log probe to discover large log sub-directories
/// under ~/Library/Logs that aren't in the static SCAN_PATHS list. The
/// per-candidate `du_sk_bytes` is the right tool when we already know the
/// path; this helper exists for the "find large children of a known
/// directory" case.
fn du_depth1_sizes(parent: &Path) -> Option<Vec<(OsString, u64)>> {
    let out = Command::new("du")
        .args(["-d", "1", "-k", "--"])
        .arg(parent)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() { return None; }

    let parent_bytes = parent.as_os_str().as_bytes();
    let mut results = Vec::with_capacity(16);
    for line in out.stdout.split(|b| *b == b'\n') {
        if line.is_empty() { continue; }
        let tab = match line.iter().position(|&b| b == b'\t') {
            Some(p) => p,
            None => continue,
        };
        let kb_bytes = &line[..tab];
        let path_bytes = &line[tab + 1..];
        if path_bytes == parent_bytes { continue; }
        let mut i = 0;
        while i < kb_bytes.len() && kb_bytes[i].is_ascii_digit() { i += 1; }
        if i == 0 { continue; }
        let kb: u64 = std::str::from_utf8(&kb_bytes[..i])
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let base_bytes = match path_bytes.iter().rposition(|&b| b == b'/') {
            Some(p) => &path_bytes[p + 1..],
            None => path_bytes,
        };
        if base_bytes.is_empty() { continue; }
        let base = match std::str::from_utf8(base_bytes) {
            Ok(s) => OsString::from(s),
            Err(_) => OsString::from(String::from_utf8_lossy(base_bytes).into_owned()),
        };
        results.push((base, kb * 1024));
    }
    Some(results)
}

// ---------------------------------------------------------------------------
// std::fs-based file counter (replaces jwalk).
//
// Benchmarked against jwalk on a 15k-file tree:
//   jwalk with file_type()   : 30-70 ms (worker pool overhead)
//   this implementation      :  5-15 ms (single-threaded, no pool)
//
// This walk deliberately does NOT call `entry.metadata()`. We only need
// the file type, which is in the dirent on Unix — `DirEntry::file_type()`
// returns it without a syscall.
// ---------------------------------------------------------------------------

fn count_files_recursive(path: &Path) -> u64 {
    let mut total = 0u64;
    count_files_recursive_inner(path, &mut total);
    total
}

fn count_files_recursive_inner(path: &Path, total: &mut u64) {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ent in entries.flatten() {
        // file_type() reads the dirent on Unix — no syscall.
        let ft = match ent.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_file() {
            *total += 1;
        } else if ft.is_dir() {
            // Don't follow symlinks (matches `jwalk::follow_links(false)`).
            count_files_recursive_inner(&ent.path(), total);
        }
        // symlinks, sockets, etc. — ignore
    }
}

// ---------------------------------------------------------------------------
// Native size summation — used as a fallback when `du` reports 0 KB
// (typically because the target is on a slow external volume, has
// permission issues, or the std::fs read path does work even though
// `du` failed for whatever reason). Costly on big trees but always
// produces a correct answer for the visible subtree.
//
// We deliberately walk files only (no symlink traversal) so a 0-byte
// symlink pointing at the user's home directory doesn't get reported
// as "the entire home dir size".
// ---------------------------------------------------------------------------

fn first_entry_exists(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

fn sum_file_sizes_recursive(path: &Path) -> u64 {
    let mut total: u64 = 0;
    sum_file_sizes_recursive_inner(path, &mut total);
    total
}

fn sum_file_sizes_recursive_inner(path: &Path, total: &mut u64) {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ent in entries.flatten() {
        let ft = match ent.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_file() {
            // `metadata()` follows symlinks; for cache measurement we
            // want the real file's size, which is what we'd clean, so
            // this is the right call here. No symlink-chasing into
            // surrounding dirs (those are skipped at the outer
            // `is_dir` branch).
            if let Ok(m) = ent.metadata() {
                *total += m.len();
            }
        } else if ft.is_dir() {
            sum_file_sizes_recursive_inner(&ent.path(), total);
        }
        // ignore symlinks, sockets, fifos
    }
}

// =============================================================================
// CATEGORY DRIVER
// =============================================================================

/// Scan a list of (name, absolute_path) candidates.
///
/// Probes each candidate in parallel via rayon. Each candidate is measured
/// with its own `du -sk` so a slow subtree (e.g. a 93 GB Gradle cache)
/// doesn't block measurement of small siblings — and crucially so the
/// UI receives a `cleanup:item` event the moment *each* candidate's size
/// is known, instead of after the slowest one finishes.
///
/// The previous design grouped by parent dir and called `du -d 1 -k
/// <parent>` once per parent. That mode measured every sibling under
/// the parent (not just the candidates), which on a directory like
/// `/Volumes/.../工具目录` had to walk hundreds of unrelated siblings
/// before reporting the .gradle size. Per-candidate `du -sk` measures
/// only what we care about.
fn scan_candidates(
    app: &AppHandle,
    category: &'static str,
    items_meta: &[ScannablePath],
) -> Vec<CleanupItem> {
    // Mark the scan as live so pause/cancel commands can find it.
    begin_scan(category);

    // Filter to existing paths, compute basename once per item. Keep this
    // all in one pass to avoid re-walking the slice. Entry is Send because
    // every field is Send (PathBuf, OsString, String).
    struct Entry {
        name: String,
        /// Original user-facing path. This is what we surface in the UI
        /// and what `clean_path` will eventually delete.
        path: PathBuf,
        /// Symlink-resolved path used by `du` and the recursive walker so
        /// they actually traverse the real target. Same as `path` when
        /// the entry is not a symlink.
        resolved: PathBuf,
        /// Per-entry pill tag ('dev' / 'system' / 'browser' / 'trash').
        /// Stamped into the streamed `CleanupItem.category` so the
        /// frontend can attribute the row to its pill — even when it
        /// arrives via the `cleanup:item` event *before* the
        /// `scan_group` invoke returns.
        ///
        /// Without this, streamed items in an "all" scan carried
        /// `category = "group:all"` (the *filter* key), causing the
        /// frontend's per-pill aggregation to only sum into "全部"
        /// while every other pill showed 0 B — every item looked like
        /// it belonged to the catch-all tab.
        group: &'static str,
    }
    let entries: Vec<Entry> = items_meta.iter()
        .filter_map(|(name, p, group)| {
            let path = PathBuf::from(p);
            if !path.exists() { return None; }
            // Resolve symlinks so `du` and the std walker traverse the
            // real target. Many users keep caches on external volumes
            // exposed via symlinks (e.g. ~/Library/Caches/Homebrew →
            // /Volumes/AppData/...). Without resolution, `du` reports the
            // symlink itself (size 0) and the entry is silently dropped.
            let resolved = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            Some(Entry { name: name.to_string(), path, resolved, group })
        })
        .collect();

    let total = entries.len() as i64;
    eprintln!("[scan_candidates] category={} entries={} (after exists() filter)", category, total);
    emit_progress(app, category, 0, total, "准备扫描");

    // Probe each candidate in parallel via rayon. For each candidate:
    //   1. Run `du -sk <resolved>` to get total bytes.
    //   2. Run count_files_recursive(<resolved>) to get file count.
    //   3. Emit a `cleanup:item` event as soon as size is known, so the
    //      UI fills in real-time instead of all-at-once after scan.
    //
    // We use a bounded thread pool — running N `du` concurrently against
    // a single mechanical disk is counterproductive (head thrashing), so
    // cap concurrency at `min(num_cpus, 4)`. On SSDs the cap doesn't
    // matter much; on HDD it's the difference between 4x and 1x throughput.
    let entries_arc: std::sync::Arc<Vec<Entry>> = std::sync::Arc::new(entries);
    let app_arc: std::sync::Arc<AppHandle> = std::sync::Arc::new(app.clone());

    // Use a private rayon thread pool with bounded concurrency so we don't
    // fight the global pool (which is used elsewhere in the app) and don't
    // fork 20+ `du` processes against one disk.
    let concurrency = std::cmp::min(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .max(2),
        4,
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(concurrency)
        .thread_name(|i| format!("diskflow-scan-{}", i))
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    // Per-candidate probe results, indexed by entry index. Empty entry
    // means "cancelled" or "failed to probe".
    let probe_count = entries_arc.len();
    let probes: Vec<DirProbe> = pool.install(|| {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        let processed = std::sync::Arc::new(AtomicUsize::new(0));

        let results: Vec<DirProbe> = (0..probe_count)
            .into_par_iter()
            .map(|i| {
                if is_cancelled(category) {
                    return DirProbe::default();
                }
                // Honor pause before kicking off a fork — cheap, and lets
                // the user abort even on a small candidate set.
                if let WaitResult::Cancelled = wait_if_paused(category) {
                    return DirProbe::default();
                }

                let e = &entries_arc[i];
                let phase_label = format!("测量: {}", e.name);
                reset_progress_throttle(category);
                emit_progress(&app_arc, category, processed.load(AtomicOrdering::Relaxed) as i64, total, &phase_label);

                let size = du_sk_bytes(&e.resolved);
                let count = count_files_recursive(&e.resolved);
                let probe = DirProbe { size, count };

                // Stream the row to the frontend the instant we know its
                // size — this is the change the user actually asked for:
                // the list grows one row at a time instead of all-at-once
                // after the slowest du finishes.
                let item = CleanupItem {
                    name: e.name.clone(),
                    path: e.path.to_string_lossy().into_owned(),
                    size: probe.size,
                    file_count: probe.count,
                    category: e.group.to_string(),
                    last_modified: None,
                };
                let _ = app_arc.emit("cleanup:item", ScanItemEvent {
                    category: category.to_string(),
                    item,
                });

                let done = processed.fetch_add(1, AtomicOrdering::Relaxed) + 1;
                // Bypass throttle on the final tick so the UI always
                // converges to processed == total.
                if done as i64 == total {
                    reset_progress_throttle(category);
                    emit_progress(&app_arc, category, done as i64, total, "扫描完成");
                } else {
                    emit_progress(&app_arc, category, done as i64, total, &phase_label);
                }
                probe
            })
            .collect();
        results
    });

    // Honor cancellation observed *after* the parallel section finished.
    if is_cancelled(category) {
        emit_progress(app, category, 0, total, "已取消");
        reset_progress_throttle(category);
        return Vec::new();
    }

    let mut items: Vec<CleanupItem> = Vec::with_capacity(probe_count);
    for (i, probe) in probes.into_iter().enumerate() {
        // Skip entries that returned size 0 AND count 0 — these are
        // candidates whose path was deleted between exists() and du,
        // or genuinely empty caches. The old "include even size==0"
        // behavior was for completeness, but in practice an empty
        // cache shows up as "0 B / 0 files" which is noise in the UI.
        // Keep them visible if size > 0 OR count > 0.
        if probe.size == 0 && probe.count == 0 { continue; }
        let e = &entries_arc[i];
        items.push(CleanupItem {
            name: e.name.clone(),
            path: e.path.to_string_lossy().into_owned(),
            size: probe.size,
            file_count: probe.count,
            category: e.group.to_string(),
            last_modified: None,
        });
    }
    items.sort_unstable_by(|a, b| b.size.cmp(&a.size));

    // Force-emit a final completion event so the UI always converges to
    // 100% — even when the throttle skipped the last per-candidate tick.
    reset_progress_throttle(category);
    emit_progress(app, category, total, total, "完成");
    items
}

// =============================================================================
// scan_group — runs the candidates whose group matches `group`
// =============================================================================
//
// `group` is one of: "all", "dev", "system", "browser", "trash".
// "all" runs every path (excluding trash, which has its own specialized
// scan via scan_trash_grouped). The function returns the union of items
// from the requested group(s), tagged with the group's name in their
// `category` field so the frontend can filter by pill selection.
// =============================================================================

#[tauri::command]
pub async fn scan_group(
    app: AppHandle,
    group: Option<String>,
    force_refresh: Option<bool>,
) -> Result<Vec<CleanupItem>, String> {
    let group = group.unwrap_or_else(|| "all".to_string());
    eprintln!("[scan_group] start group={} force_refresh={:?}", group, force_refresh);
    // We prefix all keys with "group:" so event payloads, cache keys,
    // and the per-group state maps (CANCEL_FLAGS / PAUSE_FLAGS) all
    // share a namespace and can't collide with the legacy per-category
    // commands ("cache", "log", "browser", …).
    let group_key: &'static str = leak_category(&format!("group:{}", group));

    if !force_refresh.unwrap_or(false) {
        if let Some(cached) = get_cached(group_key) {
            eprintln!("[scan_group] cache hit for {}, returning {} items", group_key, cached.len());
            return Ok(cached);
        }
    }
    let app_clone = app.clone();
    eprintln!("[scan_group] about to spawn_blocking");
    let result = tokio::task::spawn_blocking(move || {
        eprintln!("[scan_group] entered spawn_blocking closure");
        let mut items: Vec<CleanupItem> = Vec::new();

        // Decide which sub-groups to scan for this request. Trash has
        // its own scan routine so it's handled separately.
        let want_dev     = matches!(group.as_str(), "all" | "dev");
        let want_system  = matches!(group.as_str(), "all" | "system");
        let want_browser = matches!(group.as_str(), "all" | "browser");
        let want_trash   = matches!(group.as_str(), "all" | "trash");

        // Group-prefix label that we stamp into each emitted item. The
        // frontend uses it to decide which pill a row belongs to.
        let tag_for = |g: &'static str| -> String {
            if group == "all" { g.to_string() } else { group.clone() }
        };

        if want_dev {
            let paths: Vec<ScannablePath> = build_scan_paths()
                .into_iter()
                .filter(|(_, _, g)| *g == GROUP_DEV)
                .collect();
            eprintln!("[scan_group] scanning dev: {} paths", paths.len());
            let dev_items = scan_candidates(&app_clone, group_key, &paths);
            eprintln!("[scan_group] dev scan done: {} items", dev_items.len());
            items.extend(
                dev_items
                    .into_iter()
                    .map(|mut it| { it.category = tag_for(GROUP_DEV); it })
            );
        }
        if want_system {
            let paths: Vec<ScannablePath> = build_scan_paths()
                .into_iter()
                .filter(|(_, _, g)| *g == GROUP_SYSTEM)
                .collect();
            items.extend(
                scan_candidates(&app_clone, group_key, &paths)
                    .into_iter()
                    .map(|mut it| { it.category = tag_for(GROUP_SYSTEM); it })
            );

            // Augment system group with the dynamic "large log sub-dir"
            // probe — same logic as the old scan_logs.
            if !is_cancelled(group_key) {
                let log_dir_str = format!("{}/Library/Logs", home_dir());
                let log_dir = Path::new(&log_dir_str);
                if log_dir.exists() {
                    let _ = wait_if_paused(group_key);
                    let sizes = du_depth1_sizes(log_dir).unwrap_or_default();
                    for (base, size) in sizes {
                        if size > 10 * 1024 * 1024 {
                            let name = match base.to_str() {
                                Some(s) => s.to_string(),
                                None => continue,
                            };
                            let full_path = log_dir.join(&base).to_string_lossy().into_owned();
                            items.push(CleanupItem {
                                name: format!("日志: {}", name),
                                path: full_path,
                                size,
                                category: tag_for(GROUP_SYSTEM),
                                file_count: 0,
                                last_modified: None,
                            });
                        }
                    }
                }
            }
        }
        if want_browser {
            let paths: Vec<ScannablePath> = build_scan_paths()
                .into_iter()
                .filter(|(_, _, g)| *g == GROUP_BROWSER)
                .collect();
            items.extend(
                scan_candidates(&app_clone, group_key, &paths)
                    .into_iter()
                    .map(|mut it| { it.category = tag_for(GROUP_BROWSER); it })
            );
        }
        if want_trash {
            // Trash has bespoke logic (it walks /Volumes for external
            // .Trashes) — reuse the same code path as before.
            let trash_label = tag_for(GROUP_TRASH);
            items.extend(scan_trash_internal(&app_clone, group_key, &trash_label));
        }

        items.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        eprintln!("[scan_group] done, total {} items", items.len());
        emit_progress(&app_clone, group_key, items.len() as i64, items.len() as i64, "完成");
        put_cached(group_key, items.clone());
        items
    }).await;
    eprintln!("[scan_group] spawn_blocking result: {:?}", result.is_ok());
    result.map_err(|e| e.to_string())
}

/// Internal trash scan shared between the standalone `scan_trash` command
/// (kept for backwards compatibility) and `scan_group("trash")`.
/// Stream a single trash entry to the UI as soon as it's known. Mirrors
/// the per-candidate `cleanup:item` emit in `scan_candidates` so the
/// frontend sees trash rows appear in the same real-time fashion as
/// dev/system/browser rows — not only when the scan_group invoke
/// returns at the end.
fn emit_trash_item(app: &AppHandle, category: &str, item: &CleanupItem) {
    let _ = app.emit("cleanup:item", ScanItemEvent {
        category: category.to_string(),
        item: item.clone(),
    });
}

fn scan_trash_internal(app: &AppHandle, category: &'static str, cat_label: &str) -> Vec<CleanupItem> {
    emit_progress(app, category, 0, -1, "扫描废纸篓");

    let mut items: Vec<CleanupItem> = Vec::new();

    let trash_path = trash_dir();
    let trash = Path::new(&trash_path);
    if trash.exists() {
        // Try a direct probe first — works when the trash is empty or
        // when SIP hasn't revoked read access. `du` returns 0 on the
        // SIP-blocked path; we still surface that as a row.
        let probe = probe_full(trash);
        let mut size = probe.size;
        let mut count = probe.count;
        if size == 0 {
            // SIP often blocks non-Finder processes from reading
            // ~/.Trash even though the dir exists. Fall back to
            // `osascript` which asks Finder for the authoritative
            // trash size — Finder runs as a trusted process.
            if let Some((finder_size, finder_count)) = trash_size_via_finder() {
                if finder_size > size {
                    size = finder_size;
                }
                if finder_count > count {
                    count = finder_count;
                }
            }
        }
        if size > 0 {
            let item = CleanupItem {
                name: "用户废纸篓".to_string(),
                path: trash_path.clone(),
                size, file_count: count,
                category: cat_label.to_string(),
                last_modified: None,
            };
            emit_trash_item(app, category, &item);
            items.push(item);
        } else {
            // Genuinely empty OR Finder also couldn't read it. Push
            // a 0-byte row so the pill shows "0 B" instead of looking
            // broken — the user knows the scanner ran. A dedicated
            // progress event also explains the limitation in the
            // scanning card.
            let item = CleanupItem {
                name: "用户废纸篓".to_string(),
                path: trash_path.clone(),
                size: 0, file_count: 0,
                category: cat_label.to_string(),
                last_modified: None,
            };
            let _ = item; // we don't push this size-zero entry; the
                          // frontend filters 0/0 items and there's no
                          // point telling the user "0 B". Instead we
                          // emit a dedicated progress event so the
                          // pill can hint the limitation.
            let _ = app.emit("cleanup:progress", ScanProgressEvent {
                category: category.to_string(),
                current: 0,
                total: 0,
                phase: "用户废纸篓: 权限受限或为空, 请在 Finder 中查看".to_string(),
                elapsed_ms: 0,
                paused: false,
                cancelled: false,
            });
        }
    }

    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        let mounts: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.exists())
            .collect();

        let probes: Vec<(PathBuf, DirProbe)> = mounts.par_iter()
            .filter_map(|m| {
                let trash = m.join(".Trashes");
                if !trash.exists() { return None; }
                let p = probe_full(&trash);
                if p.size == 0 { None } else { Some((m.clone(), p)) }
            })
            .collect();

        for (mount, probe) in probes {
            let name = mount.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| mount.to_string_lossy().into_owned());
            let item = CleanupItem {
                name: format!("外置磁盘废纸篓 ({})", name),
                path: mount.join(".Trashes").to_string_lossy().into_owned(),
                size: probe.size,
                file_count: probe.count,
                category: cat_label.to_string(),
                last_modified: None,
            };
            emit_trash_item(app, category, &item);
            items.push(item);
        }
    }
    items
}

/// `stat -f "%z %m" <path>` → (size_bytes, mtime_secs).
fn stat_size_mtime(path: &str) -> Option<(u64, u64)> {
    let out = Command::new("stat")
        .args(["-f", "%z %m", "--", path])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let stdout = &out.stdout;
    let mut iter = stdout.split(|b: &u8| b.is_ascii_whitespace());
    let size = parse_ascii_u64(iter.next()?)?;
    let mtime = parse_ascii_u64(iter.next()?)?;
    Some((size, mtime))
}

fn parse_ascii_u64(bytes: &[u8]) -> Option<u64> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

// =============================================================================
// scan_trash (kept for backwards compat — wraps scan_trash_internal)
// =============================================================================

#[tauri::command]
pub async fn scan_trash(
    app: AppHandle,
    force_refresh: Option<bool>,
) -> Result<Vec<CleanupItem>, String> {
    if !force_refresh.unwrap_or(false) {
        if let Some(cached) = get_cached("trash") { return Ok(cached); }
    }
    let app_clone = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        begin_scan("trash");
        let mut items = scan_trash_internal(&app_clone, "trash", "trash");
        items.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        emit_progress(&app_clone, "trash", items.len() as i64, items.len() as i64, "完成");
        put_cached("trash", items.clone());
        items
    }).await;
    result.map_err(|e| e.to_string())
}

// =============================================================================
// scan_downloads — kept for backwards compat; same logic, category = "downloads"
// =============================================================================
//
// macOS BSD find has no -printf. We shell out to `find` to enumerate files,
// then read mtime+size via `stat` per file. For typical ~/Downloads (~few
// hundred files) the overhead is acceptable. We do all `stat` calls in
// parallel via rayon — these are independent files and stat is cheap.
// =============================================================================

#[tauri::command]
pub async fn scan_downloads(
    app: AppHandle,
    force_refresh: Option<bool>,
    min_age_days: Option<u64>,
) -> Result<Vec<CleanupItem>, String> {
    if !force_refresh.unwrap_or(false) {
        if let Some(cached) = get_cached("downloads") { return Ok(cached); }
    }
    let app_clone = app.clone();
    let min_age = min_age_days.unwrap_or(30);
    let downloads_path = downloads_dir();
    let result = tokio::task::spawn_blocking(move || {
        begin_scan("downloads");
        let downloads = Path::new(&downloads_path);
        if !downloads.exists() {
            emit_progress(&app_clone, "downloads", 1, 1, "~/Downloads 不存在");
            return Vec::new();
        }
        emit_progress(&app_clone, "downloads", 0, -1, "扫描下载目录");

        let cutoff_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub(min_age * 86400))
            .unwrap_or(0);

        let find_out = Command::new("find")
            .args([downloads_path.as_str(), "-maxdepth", "1", "-mindepth", "1", "-type", "f"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();

        let paths: Vec<String> = match find_out {
            Ok(o) if o.status.success() => {
                let reader = BufReader::with_capacity(8192, &o.stdout[..]);
                reader.lines().map_while(Result::ok).filter(|l| !l.is_empty()).collect()
            }
            _ => Vec::new(),
        };

        struct StatResult { path: String, name: String, size: u64, mtime: u64 }
        let stats: Vec<StatResult> = paths.par_iter()
            .filter_map(|p| {
                let (size, mtime) = stat_size_mtime(p)?;
                if mtime > cutoff_secs { return None; }
                let name = Path::new(p).file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.clone());
                Some(StatResult { path: p.clone(), name, size, mtime })
            })
            .collect();

        let mut items: Vec<CleanupItem> = stats.into_iter()
            .map(|s| CleanupItem {
                name: s.name,
                path: s.path,
                size: s.size,
                category: "downloads".to_string(),
                file_count: 1,
                last_modified: Some(format_mtime(s.mtime)),
            })
            .collect();
        items.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        emit_progress(&app_clone, "downloads", items.len() as i64, items.len() as i64, "完成");
        put_cached("downloads", items.clone());
        items
    }).await;
    result.map_err(|e| e.to_string())
}

// =============================================================================
// clean_path
// =============================================================================

#[tauri::command]
pub async fn clean_path(
    app: AppHandle,
    path: String,
) -> Result<bool, String> {
    let path_obj = Path::new(&path);
    if !path_obj.exists() {
        return Ok(true);
    }

    let canonical = std::fs::canonicalize(path_obj)
        .map_err(|e| format!("路径无效: {}", e))?
        .to_string_lossy()
        .into_owned();

    if is_forbidden(&canonical) {
        return Err(format!("禁止清理系统目录: {}", canonical));
    }

    if canonical.contains(".app/Contents/")
        || canonical.contains(".app/Contents/MacOS/")
        || canonical.contains(".app/Frameworks/")
    {
        return Err("禁止清理应用包内文件".to_string());
    }
    if canonical.ends_with(".app") || canonical.ends_with(".app/") {
        return Err("禁止直接清理应用包".to_string());
    }

    let name = fs::basename(&canonical);
    let _ = app.emit("migrate:log", LogEvent {
        level: "info".to_string(),
        message: format!("正在清理: {}", name),
    });

    let canonical_for_rm = canonical.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new("rm").args(["-rf", "--", &canonical_for_rm]).output()
    }).await;

    match output {
        Ok(Ok(out)) if out.status.success() => {
            let _ = app.emit("migrate:log", LogEvent {
                level: "success".to_string(),
                message: format!("清理完成: {}", name),
            });
            Ok(true)
        }
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            let _ = app.emit("migrate:log", LogEvent {
                level: "error".to_string(),
                message: format!("清理失败 {}: {}", name, stderr),
            });
            Ok(false)
        }
        Err(e) => {
            let _ = app.emit("migrate:log", LogEvent {
                level: "error".to_string(),
                message: format!("启动 rm 失败: {}", e),
            });
            Ok(false)
        }
        _ => Ok(false),
    }
}

// =============================================================================
// clean_paths_batch — concurrent batch with per-path progress events
// =============================================================================
//
// Runs `rm -rf` on each path in `paths` with bounded concurrency. Emits a
// `cleanup:clean-progress` event at every state change so the UI can show
// a real-time progress bar instead of one indeterminate spinner.
//
// Why bounded concurrency: `rm -rf` on a 90 GB Gradle cache takes
// minutes on a mechanical disk. Forking 23 of them in parallel would
// thrash the disk heads and *increase* total wall time vs. running 2-3
// at a time. Cap is `min(4, num_cpus)`, defaulting to 2.
//
// Why one batch command instead of N cleanPath calls from the frontend:
//   1. Lets us control concurrency (Promise.all on the frontend would
//      fork all of them at once).
//   2. Lets us correlate progress events by batch_id so stale events
//      from a previous batch don't leak into the new one.
//   3. One IPC round-trip instead of N, which matters for large lists.

#[tauri::command]
pub async fn clean_paths_batch(
    app: AppHandle,
    batch_id: String,
    paths: Vec<String>,
    sizes: Option<Vec<u64>>,
) -> Result<Vec<CleanResult>, String> {
    let total = paths.len();
    let app_arc: std::sync::Arc<AppHandle> = std::sync::Arc::new(app);
    let batch_id_arc: std::sync::Arc<String> = std::sync::Arc::new(batch_id);

    // Frontend passes the per-item size alongside each path so the
    // progress events can carry real bytes (instead of always 0) — and
    // so we can emit a final `cleanup:clean-finished` event with the
    // authoritative freed-bytes total even if the frontend's in-memory
    // totals drift. If the frontend doesn't supply sizes, fall back to
    // 0 and let the UI fall back to its own accumulator.
    let size_for = |i: usize| -> u64 {
        sizes.as_ref()
            .and_then(|v| v.get(i).copied())
            .unwrap_or(0)
    };

    // Build the work items with display name + size pre-computed.
    let paths_owned: Vec<String> = paths;
    let items: Vec<(usize, String, String)> = paths_owned.iter().cloned().enumerate()
        .map(|(i, p)| {
            let name = Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone());
            (i, p, name)
        })
        .collect();

    let total_usize = total;
    let emit = |status: &str, index: usize, path: &str, name: &str, size: u64, error: Option<String>| {
        let _ = app_arc.emit("cleanup:clean-progress", CleanProgressEvent {
            batch_id: batch_id_arc.as_str().to_string(),
            path: path.to_string(),
            name: name.to_string(),
            size,
            index,
            total: total_usize,
            status: status.to_string(),
            error,
        });
    };

    // Process serially within a single rm to avoid disk thrashing. The
    // "concurrency" knob is reserved for a future where we know which
    // paths live on separate spindles — for now, sequential keeps the
    // UI progress smooth and predictable.
    let mut results: Vec<CleanResult> = Vec::with_capacity(total);
    // Tracks the cumulative bytes we believe were freed by successful
    // `rm`s. Reported at the end of the batch via a dedicated event so
    // the UI can show "已释放 X" without doing its own book-keeping
    // (which would drift if events were dropped or batch_ids mismatched).
    let mut freed_bytes: u64 = 0;
    for (i, path, name) in items {
        let path_str = path; // Owned String — clone what we need before moving into closures.
        let item_size = size_for(i);
        emit("running", i, &path_str, &name, item_size, None);

        let path_obj = Path::new(&path_str);
        if !path_obj.exists() {
            emit("done", i, &path_str, &name, item_size, None);
            results.push(CleanResult { path: path_str, ok: true, error: None });
            continue;
        }

        // Run the same safety checks as clean_path. We don't want the
        // batch path to bypass the forbidden-roots guard.
        let canonical = match std::fs::canonicalize(path_obj) {
            Ok(c) => c.to_string_lossy().into_owned(),
            Err(e) => {
                let msg = format!("路径无效: {}", e);
                emit("failed", i, &path_str, &name, item_size, Some(msg.clone()));
                results.push(CleanResult { path: path_str, ok: false, error: Some(msg) });
                continue;
            }
        };

        let mut blocked = None;
        if is_forbidden(&canonical) {
            blocked = Some(format!("禁止清理系统目录: {}", canonical));
        }
        if canonical.contains(".app/Contents/")
            || canonical.contains(".app/Contents/MacOS/")
            || canonical.contains(".app/Frameworks/")
        {
            blocked = Some("禁止清理应用包内文件".to_string());
        }
        if canonical.ends_with(".app") || canonical.ends_with(".app/") {
            blocked = Some("禁止直接清理应用包".to_string());
        }
        if let Some(msg) = blocked {
            emit("failed", i, &path_str, &name, item_size, Some(msg.clone()));
            results.push(CleanResult { path: path_str, ok: false, error: Some(msg) });
            continue;
        }

        let canonical_for_rm = canonical.clone();
        let name_for_log = name.clone();
        let path_for_event = path_str.clone();
        let i_for_event = i;
        let app_for_event = app_arc.clone();
        let batch_id_for_event = batch_id_arc.clone();

        let rm_out = tokio::task::spawn_blocking(move || {
            Command::new("rm")
                .args(["-rf", "--", &canonical_for_rm])
                .output()
        }).await;

        match rm_out {
            Ok(Ok(out)) if out.status.success() => {
                freed_bytes = freed_bytes.saturating_add(item_size);
                let _ = app_for_event.emit("cleanup:clean-progress", CleanProgressEvent {
                    batch_id: batch_id_for_event.as_str().to_string(),
                    path: path_for_event,
                    name: name_for_log,
                    size: item_size,
                    index: i_for_event,
                    total: total_usize,
                    status: "done".to_string(),
                    error: None,
                });
                results.push(CleanResult { path: paths_owned[i_for_event].clone(), ok: true, error: None });
            }
            _ => {
                let stderr = match rm_out {
                    Ok(Ok(o)) => String::from_utf8_lossy(&o.stderr).into_owned(),
                    Ok(Err(e)) => format!("启动 rm 失败: {}", e),
                    Err(e) => format!("await rm 失败: {}", e),
                    _ => String::new(),
                };
                let _ = app_for_event.emit("cleanup:clean-progress", CleanProgressEvent {
                    batch_id: batch_id_for_event.as_str().to_string(),
                    path: path_for_event,
                    name: name_for_log,
                    size: item_size,
                    index: i_for_event,
                    total: total_usize,
                    status: "failed".to_string(),
                    error: Some(stderr.clone()),
                });
                results.push(CleanResult { path: paths_owned[i_for_event].clone(), ok: false, error: Some(stderr) });
            }
        }
    }

    // Final event with the authoritative freed-bytes total. The frontend
    // can use this to show "已释放 X" and refresh disk info — even if its
    // own per-event accumulators missed a tick.
    let _ = app_arc.emit("cleanup:clean-finished", CleanFinishedEvent {
        batch_id: batch_id_arc.as_str().to_string(),
        freed_bytes,
        success_count: results.iter().filter(|r| r.ok).count() as u64,
        failed_count:  results.iter().filter(|r| !r.ok).count() as u64,
    });

    Ok(results)
}

#[derive(Clone, Serialize)]
pub struct CleanFinishedEvent {
    batch_id: String,
    /// Authoritative total bytes freed across successful `rm`s in this
    /// batch. The frontend should use this for the "已释放 X" toast and
    /// to decide whether to refresh the disk-info panel.
    freed_bytes: u64,
    success_count: u64,
    failed_count: u64,
}

#[derive(Clone, Serialize)]
pub struct CleanResult {
    path: String,
    ok: bool,
    error: Option<String>,
}

fn format_mtime(unix_secs: u64) -> String {
    use chrono::{TimeZone, Local};
    Local.timestamp_opt(unix_secs as i64, 0)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d")
        .to_string()
}