//! Optional automatic deletion of old recording folders (Settings → Saving). Off by default.
//! Runs shortly after the start and then every few hours; never touches the recording that is
//! open in the editor or still being recorded. Saved GIFs and videos are not affected.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

/// Settings key.
const KEY: &str = "recordingCleanup";
/// How often to look for old recordings while FrameNab runs.
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CleanupSettings {
    pub enabled: bool,
    /// Recordings unchanged for longer than this are deleted.
    pub days: u32,
}

impl Default for CleanupSettings {
    fn default() -> Self {
        Self { enabled: false, days: 30 }
    }
}

/// Recording folders (`*.framenab`) in `dir` with their last change. `project.json` changes
/// with every edit, the folder itself when frames are added.
pub fn recording_folders(dir: &Path) -> Vec<(SystemTime, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|e| e == "framenab"))
        .filter_map(|p| {
            let modified = std::fs::metadata(p.join("project.json")).or_else(|_| std::fs::metadata(&p));
            Some((modified.and_then(|m| m.modified()).ok()?, p))
        })
        .collect()
}

/// The folders unchanged for more than `days` at `now`.
fn older_than(folders: &[(SystemTime, PathBuf)], now: SystemTime, days: u32) -> Vec<PathBuf> {
    let age = Duration::from_secs(u64::from(days.max(1)) * 24 * 60 * 60);
    folders
        .iter()
        .filter(|(modified, _)| now.duration_since(*modified).is_ok_and(|d| d > age))
        .map(|(_, p)| p.clone())
        .collect()
}

fn folder_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => folder_size(&e.path()),
                    _ => e.metadata().map_or(0, |m| m.len()),
                })
                .sum()
        })
        .unwrap_or(0)
}

fn settings(app: &AppHandle) -> CleanupSettings {
    app.store("settings.json")
        .ok()
        .and_then(|s| s.get(KEY))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Folders that must stay: open in the editor, or being recorded.
fn in_use(app: &AppHandle) -> Vec<PathBuf> {
    [crate::editor::open_root(app), crate::recorder::recording_dir(app)]
        .into_iter()
        .flatten()
        .filter_map(|p| p.canonicalize().ok())
        .collect()
}

/// Deletes the old recordings if the setting is on. Returns how many were deleted.
pub fn run(app: &AppHandle) -> usize {
    let settings = settings(app);
    if !settings.enabled {
        return 0;
    }
    let keep = in_use(app);
    let dir = crate::recorder::recordings_dir(app);
    let mut deleted = 0;
    for path in older_than(&recording_folders(&dir), SystemTime::now(), settings.days) {
        if path.canonicalize().is_ok_and(|p| keep.contains(&p)) {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => deleted += 1,
            Err(err) => tracing::warn!(path = %path.display(), %err, "could not delete an old recording"),
        }
    }
    if deleted > 0 {
        tracing::info!(deleted, days = settings.days, "old recordings deleted");
    }
    deleted
}

/// Runs the cleanup shortly after the start and then every few hours.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("framenab-cleanup".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        loop {
            run(&app);
            std::thread::sleep(INTERVAL);
        }
    });
}

/// How many recordings (and bytes) are older than `days`, for the settings page.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OldRecordings {
    count: usize,
    bytes: u64,
}

#[tauri::command]
pub fn old_recordings(app: AppHandle, days: u32) -> OldRecordings {
    let dir = crate::recorder::recordings_dir(&app);
    let old = older_than(&recording_folders(&dir), SystemTime::now(), days);
    OldRecordings { count: old.len(), bytes: old.iter().map(|p| folder_size(p)).sum() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_folders_older_than_the_limit() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * 24 * 3600);
        let day = Duration::from_secs(24 * 3600);
        let folders = vec![
            (now - day * 40, PathBuf::from("old.framenab")),
            (now - day * 29, PathBuf::from("recent.framenab")),
            (now + day, PathBuf::from("future.framenab")),
        ];
        assert_eq!(older_than(&folders, now, 30), [PathBuf::from("old.framenab")]);
        assert_eq!(older_than(&folders, now, 1).len(), 2);
        assert_eq!(CleanupSettings::default(), CleanupSettings { enabled: false, days: 30 });
    }

    #[test]
    fn lists_only_recording_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("a.framenab")).unwrap();
        std::fs::create_dir(dir.path().join("other")).unwrap();
        std::fs::write(dir.path().join("b.framenab"), b"not a folder").unwrap();
        let folders = recording_folders(dir.path());
        assert_eq!(folders.len(), 1);
        assert!(folders[0].1.ends_with("a.framenab"));
    }
}
