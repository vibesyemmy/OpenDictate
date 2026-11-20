#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Action, Entry, KeepHistory};

    fn entry(when: u64, audio: Option<String>) -> Entry {
        Entry { when, action: Action::Dictate, raw: "hello".into(), text: "hello".into(),
            seconds: 1.0, speech_model: String::new(), text_model: String::new(),
            error: None, audio, selection: String::new() }
    }

    #[test]
    fn retention_preview_counts_only_entries_that_will_be_removed() {
        let entries = vec![entry(100_000, None), entry(10_000, None)];
        assert_eq!(expired_count(&entries, KeepHistory::Day1, 100_000), 1);
        assert_eq!(expired_count(&entries, KeepHistory::Never, 100_000), 2);
        assert_eq!(expired_count(&entries, KeepHistory::Forever, 100_000), 0);
    }

    #[test]
    fn dont_keep_history_does_not_create_future_history() {
        assert!(!keeps_new_history(KeepHistory::Never));
        assert!(keeps_new_history(KeepHistory::Day1));
        assert!(keeps_new_history(KeepHistory::Forever));
    }

    #[test]
    fn meeting_ids_cannot_escape_the_meeting_directory() {
        assert!(!valid_meeting_id("../history"));
        assert!(!valid_meeting_id(""));
        assert!(valid_meeting_id("2026-11-17-154247"));
    }

    #[test]
    fn history_is_not_changed_if_save_fails() {
        let entries = vec![entry(1, None), entry(2, None)];
        let result = prepare_removal(&entries, &|e| e.when == 1, &|_| Err("disk full".into()));
        assert!(result.is_err());
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn removal_saves_only_the_kept_entries() {
        let entries = vec![entry(1, None), entry(2, None)];
        let (removed, kept) = prepare_removal(&entries, &|e| e.when == 1, &|e| {
            assert_eq!(e.len(), 1); assert_eq!(e[0].when, 2); Ok(())
        }).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn delete_permanently_removes_files_and_folders_for_good() {
        let root = std::env::temp_dir().join(format!("opendictate-delete-{}", std::process::id()));
        std::fs::create_dir_all(root.join("meeting/audio")).unwrap();
        std::fs::write(root.join("meeting/audio/1.wav"), b"audio").unwrap();
        std::fs::write(root.join("clip.wav"), b"audio").unwrap();
        delete_permanently(&root.join("meeting")).unwrap();
        delete_permanently(&root.join("clip.wav")).unwrap();
        assert!(!root.join("meeting").exists());
        assert!(!root.join("clip.wav").exists());
        assert!(delete_permanently(&root.join("missing")).is_ok(), "an item that is already gone is not an error");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn delete_permanently_removes_a_symlink_not_its_target() {
        let root = std::env::temp_dir().join(format!("opendictate-delete-link-{}", std::process::id()));
        std::fs::create_dir_all(root.join("outside")).unwrap();
        std::fs::write(root.join("outside/keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("link")).unwrap();
        delete_permanently(&root.join("link")).unwrap();
        assert!(root.join("outside/keep.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
use crate::settings::{Entry, KeepHistory};
use std::path::Path;

pub fn expired_count(entries: &[Entry], keep: KeepHistory, now: u64) -> usize {
    keep.max_age().map_or(0, |max| entries.iter().filter(|e| now.saturating_sub(e.when) >= max).count())
}

pub fn keeps_new_history(keep: KeepHistory) -> bool {
    keep != KeepHistory::Never
}

pub fn shortens_history(current: KeepHistory, next: KeepHistory) -> bool {
    match (current.max_age(), next.max_age()) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(a), Some(b)) => b < a,
    }
}

pub fn valid_meeting_id(id: &str) -> bool {
    !id.is_empty() && id != "." && !id.contains("..") && !id.contains('/') && !id.contains('\\')
}

/// Splits history into removed and kept entries and saves the kept ones.
/// Nothing is returned as removed unless the save succeeded, so a failed save
/// leaves history exactly as it was.
pub fn prepare_removal(
    entries: &[Entry],
    remove: &dyn Fn(&Entry) -> bool,
    save: &dyn Fn(&[Entry]) -> Result<(), String>,
) -> Result<(Vec<Entry>, Vec<Entry>), String> {
    let (removed, kept): (Vec<Entry>, Vec<Entry>) = entries.iter().cloned().partition(remove);
    if !removed.is_empty() {
        save(&kept)?;
    }
    Ok((removed, kept))
}

/// Deletes a file or folder for good; nothing goes to the Trash. The caller
/// confirms with the user first. A symlink is removed itself, never followed.
pub fn delete_permanently(path: &Path) -> Result<(), String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Could not delete: {e}")),
    };
    let result = if meta.file_type().is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    result.map_err(|e| format!("Could not delete: {e}"))
}
