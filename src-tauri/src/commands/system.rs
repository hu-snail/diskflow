use sysinfo::System;
use tauri::{AppHandle, Emitter};
use std::sync::Mutex;
use serde::Serialize;
use crate::models::types::SystemInfo;

#[derive(Clone, Serialize)]
struct MetricsEvent {
    cpu_usage: f32,
    mem_used: u64,
    mem_total: u64,
    disk_read: u64,
    disk_write: u64,
}

static MONITORING: std::sync::LazyLock<Mutex<bool>> = std::sync::LazyLock::new(|| Mutex::new(false));
// Reuse a single System instance to avoid reallocating on every refresh
static SYS: std::sync::LazyLock<Mutex<System>> = std::sync::LazyLock::new(|| Mutex::new(System::new()));

#[tauri::command]
pub async fn get_system_info() -> Result<SystemInfo, String> {
    let result = tokio::task::spawn_blocking(|| {
        let mut sys = SYS.lock().map_err(|e| e.to_string())?;
        sys.refresh_cpu_all();
        // Minimal sleep for first CPU sample
        std::thread::sleep(std::time::Duration::from_millis(200));
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let cpu_usage: f32 = sys.cpus().iter()
            .map(|c| c.cpu_usage())
            .sum::<f32>() / sys.cpus().len().max(1) as f32;

        Ok(SystemInfo {
            cpu_usage,
            cpu_cores: sys.cpus().len() as u32,
            mem_total: sys.total_memory(),
            mem_used: sys.used_memory(),
            disk_read: 0,
            disk_write: 0,
            uptime: sysinfo::System::uptime(),
        })
    }).await;

    result.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn start_monitoring(app: AppHandle) -> Result<bool, String> {
    {
        let mut monitoring = MONITORING.lock().map_err(|e| e.to_string())?;
        if *monitoring {
            return Ok(true);
        }
        *monitoring = true;
    }

    tokio::spawn(async move {
        loop {
            let should_run = {
                match MONITORING.lock() {
                    Ok(m) => *m,
                    Err(_) => false,
                }
            };
            if !should_run { break; }

            // Refresh in spawn_blocking to not hold the async thread
            let metrics = tokio::task::spawn_blocking(|| {
                let mut sys = match SYS.lock() {
                    Ok(s) => s,
                    Err(_) => return None,
                };
                sys.refresh_cpu_usage();
                sys.refresh_memory();

                let cpu_usage: f32 = sys.cpus().iter()
                    .map(|c| c.cpu_usage())
                    .sum::<f32>() / sys.cpus().len().max(1) as f32;

                Some(MetricsEvent {
                    cpu_usage,
                    mem_used: sys.used_memory(),
                    mem_total: sys.total_memory(),
                    disk_read: 0,
                    disk_write: 0,
                })
            }).await;

            if let Ok(Some(m)) = metrics {
                let _ = app.emit("system:metrics", m);
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });

    Ok(true)
}

#[tauri::command]
pub async fn stop_monitoring() -> Result<bool, String> {
    let mut monitoring = MONITORING.lock().map_err(|e| e.to_string())?;
    *monitoring = false;
    Ok(true)
}
