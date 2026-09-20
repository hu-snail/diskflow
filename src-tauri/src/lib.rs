mod commands;
mod models;
mod utils;

use commands::{disk, scan, migrate, system, cleanup};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .invoke_handler(tauri::generate_handler![
            disk::get_disks,
            disk::get_disk_info,
            scan::scan_directory,
            scan::get_dir_size,
            scan::find_large_files,
            migrate::migrate_app,
            migrate::rollback_app,
            migrate::cleanup_backup,
            migrate::verify_migration,
            migrate::browse_directory,
            migrate::scan_apps,
            migrate::scan_external_apps,
            migrate::get_running_apps,
            migrate::compute_app_sizes,
            migrate::get_app_icon,
            system::get_system_info,
            system::start_monitoring,
            system::stop_monitoring,
            cleanup::scan_cache,
            cleanup::scan_logs,
            cleanup::clean_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DiskFlow");
}
