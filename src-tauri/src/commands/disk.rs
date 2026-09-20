use sysinfo::Disks;
use std::sync::Mutex;
use crate::models::types::DiskInfo;

pub struct AppState {
    pub monitoring: Mutex<bool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self { monitoring: Mutex::new(false) }
    }
}

#[tauri::command]
pub async fn get_disks() -> Result<Vec<DiskInfo>, String> {
    let disks = Disks::new_with_refreshed_list();
    let mut result = Vec::new();

    let mut seen_root = false;
    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        let name = disk.name().to_string_lossy().to_string();

        // macOS APFS: skip /System/Volumes/Data — same physical disk as /
        if mount == "/System/Volumes/Data" && seen_root {
            continue;
        }
        if mount == "/" {
            seen_root = true;
        }

        let fs_type = match disk.file_system() {
            fs_str => fs_str.to_string_lossy().to_string(),
        };
        let is_removable = disk.is_removable();

        // Detect sparse bundle (APFS image)
        let is_sparse = mount.contains("AppData") || name.contains("AppData");

        result.push(DiskInfo {
            name,
            mount_point: mount,
            total_space: disk.total_space(),
            available_space: disk.available_space(),
            fs_type,
            is_removable,
            is_sparse,
        });
    }

    Ok(result)
}

#[tauri::command]
pub async fn get_disk_info(mount_point: String) -> Result<DiskInfo, String> {
    let disks = Disks::new_with_refreshed_list();
    for disk in disks.list() {
        if disk.mount_point().to_string_lossy() == mount_point {
            return Ok(DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                total_space: disk.total_space(),
                available_space: disk.available_space(),
                fs_type: disk.file_system().to_string_lossy().to_string(),
                is_removable: disk.is_removable(),
                is_sparse: false,
            });
        }
    }
    Err("Disk not found".to_string())
}
