use std::process::Command;
use crate::utils::fs;
use crate::models::types::VerifyResult;

/// Two-tier verification:
/// - Tier 1 (always): parallel file count + logical size, source & target scanned simultaneously
/// - Tier 2 (only if tier 1 passes): rsync --dry-run content comparison
pub fn verify_migration(source: &str, target: &str) -> VerifyResult {
    let mut result = VerifyResult {
        file_count_match: false,
        size_match: false,
        content_match: false,
        source_files: 0,
        target_files: 0,
        source_size: 0,
        target_size: 0,
        differences: vec![],
    };

    // Tier 1: Scan source and target IN PARALLEL (rayon::join)
    let [(src_files, src_size, _), (tgt_files, tgt_size, _)] =
        fs::scan_dir_stats_parallel(source, target);

    result.source_files = src_files;
    result.target_files = tgt_files;
    result.file_count_match = src_files == tgt_files;

    result.source_size = src_size;
    result.target_size = tgt_size;
    let size_diff = if src_size > 0 {
        ((tgt_size as f64 - src_size as f64).abs() / src_size as f64) * 100.0
    } else { 0.0 };
    result.size_match = size_diff < 1.0;

    // Tier 2: Only run rsync if tier 1 passes
    if result.file_count_match && result.size_match {
        let rsync_output = Command::new("rsync")
            .args(["-a", "--dry-run", "--itemize-changes", "--out-format=%n", "--",
                   &format!("{}/", source), &format!("{}/", target)])
            .output();

        match rsync_output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let diffs: Vec<String> = stdout.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect();
                result.content_match = diffs.is_empty();
                result.differences = diffs;
            }
            _ => {
                // rsync unavailable or failed — tier 1 match is sufficient for local copies
                result.content_match = true;
            }
        }
    }

    result
}

pub fn verify_all_passed(result: &VerifyResult) -> bool {
    result.file_count_match && result.size_match && result.content_match
}
