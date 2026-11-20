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
    fn dont_keep_history_does_not_create_future_history_or_backups() {
        assert!(!keeps_new_history(KeepHistory::Never));
        assert!(keeps_new_history(KeepHistory::Day1));
        assert!(keeps_new_history(KeepHistory::Forever));
    }

    #[test]
    fn history_backup_keeps_text_and_audio_before_trash_is_requested() {
        let root = std::env::temp_dir().join(format!("opendictate-safety-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let audio = root.join("original.wav");
        std::fs::write(&audio, b"recorded audio").unwrap();
        let items = vec![entry(42, Some(audio.to_string_lossy().into_owned()))];
        let result = archive_history_in(&root, &items, &|archive| {
            let json = std::fs::read_to_string(archive.join("history.json")).unwrap();
            assert!(json.contains("hello"));
            assert_eq!(std::fs::read(archive.join("audio/0.wav")).unwrap(), b"recorded audio");
            Err("Trash unavailable".into())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(audio).unwrap(), b"recorded audio");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn meeting_ids_cannot_escape_the_meeting_directory() {
        assert!(!valid_meeting_id("../history"));
        assert!(!valid_meeting_id(""));
        assert!(valid_meeting_id("2026-11-17-154247"));
    }

    #[test]
    fn history_is_not_committed_if_backup_or_save_fails() {
        let entries = vec![entry(1, None), entry(2, None)];
        let saved = std::cell::Cell::new(false);
        let result = prepare_removal(&entries, &|e| e.when == 1, &|_| Err("trash failed".into()), &|_| { saved.set(true); Ok(()) });
        assert!(result.is_err());
        assert!(!saved.get());
        assert_eq!(entries.len(), 2);
        let result = prepare_removal(&entries, &|e| e.when == 1, &|_| Ok(()), &|_| Err("disk full".into()));
        assert!(result.is_err());
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn history_commit_backs_up_only_removed_entries_before_saving() {
        let entries = vec![entry(1, None), entry(2, None)];
        let backed_up = std::cell::Cell::new(false);
        let (removed, kept) = prepare_removal(&entries, &|e| e.when == 1, &|e| {
            assert_eq!(e.len(), 1); assert_eq!(e[0].when, 1); backed_up.set(true); Ok(())
        }, &|e| {
            assert!(backed_up.get()); assert_eq!(e.len(), 1); assert_eq!(e[0].when, 2); Ok(())
        }).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn restore_merges_backup_and_copies_its_audio_without_overwriting_existing_history() {
        let root = std::env::temp_dir().join(format!("opendictate-restore-{}", std::process::id()));
        let backup = root.join("backup");
        let audio = root.join("live-audio");
        std::fs::create_dir_all(backup.join("audio")).unwrap();
        std::fs::create_dir_all(&audio).unwrap();
        std::fs::write(backup.join("audio/0.wav"), b"saved").unwrap();
        std::fs::write(backup.join("history.json"), serde_json::to_vec(&vec![entry(1, Some("audio/0.wav".into()))]).unwrap()).unwrap();
        let current = vec![entry(2, None)];
        let merged = restore_backup_in(&backup, &audio, &current, &|_| Ok(())).unwrap();
        assert_eq!(merged.len(), 2);
        let restored = merged.iter().find(|e| e.when == 1).unwrap();
        assert_eq!(std::fs::read(restored.audio.as_ref().unwrap()).unwrap(), b"saved");
        assert_eq!(std::fs::read(backup.join("audio/0.wav")).unwrap(), b"saved");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_rejects_audio_paths_outside_the_backup() {
        let root = std::env::temp_dir().join(format!("opendictate-restore-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("history.json"), serde_json::to_vec(&vec![entry(1, Some("../secret".into()))]).unwrap()).unwrap();
        assert!(restore_backup_in(&root, &root, &[], &|_| Ok(())).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_restore_does_not_leave_audio_copies_or_change_existing_entries() {
        let root = std::env::temp_dir().join(format!("opendictate-restore-fail-{}", std::process::id()));
        let backup = root.join("backup");
        let audio = root.join("live-audio");
        std::fs::create_dir_all(backup.join("audio")).unwrap();
        std::fs::create_dir_all(&audio).unwrap();
        std::fs::write(backup.join("audio/0.wav"), b"saved").unwrap();
        std::fs::write(backup.join("history.json"), serde_json::to_vec(&vec![entry(1, Some("audio/0.wav".into()))]).unwrap()).unwrap();
        let current = vec![entry(2, None)];
        assert!(restore_backup_in(&backup, &audio, &current, &|_| Err("disk full".into())).is_err());
        assert_eq!(std::fs::read_dir(&audio).unwrap().count(), 0);
        assert_eq!(current.len(), 1);
        assert!(backup.join("history.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "moves a temporary test folder through the real macOS Trash"]
    fn moves_a_temporary_folder_to_macos_trash() {
        let path = std::env::temp_dir().join(format!("opendictate-trash-test-{}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("test.txt"), "test").unwrap();
        let destination = trash_location(&path).expect("move test folder to Trash");
        assert!(!path.exists());
        assert!(destination.join("test.txt").exists());
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "moves a temporary history backup through the real macOS Trash"]
    fn backs_up_and_restores_temporary_history_from_macos_trash() {
        let root = std::env::temp_dir().join(format!("opendictate-history-trash-test-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let audio = root.join("original.wav");
        std::fs::write(&audio, b"audio").unwrap();
        let backup_path = std::cell::RefCell::new(None);
        archive_history_in(&root, &[entry(1, Some(audio.to_string_lossy().into_owned()))], &|folder| {
            *backup_path.borrow_mut() = Some(trash_location(folder)?);
            Ok(())
        }).unwrap();
        let backup = backup_path.into_inner().unwrap();
        let restored = restore_backup_in(&backup, &root, &[entry(2, None)], &|_| Ok(())).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(std::fs::read(restored[0].audio.as_ref().unwrap()).unwrap(), b"audio");
        std::fs::remove_dir_all(backup).unwrap();
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

pub fn prepare_removal(
    entries: &[Entry],
    remove: &dyn Fn(&Entry) -> bool,
    archive: &dyn Fn(&[Entry]) -> Result<(), String>,
    save: &dyn Fn(&[Entry]) -> Result<(), String>,
) -> Result<(Vec<Entry>, Vec<Entry>), String> {
    let (removed, kept): (Vec<Entry>, Vec<Entry>) = entries.iter().cloned().partition(remove);
    if !removed.is_empty() {
        archive(&removed)?;
        save(&kept)?;
    }
    Ok((removed, kept))
}

#[cfg(target_os = "macos")]
fn trash_location(path: &Path) -> Result<std::path::PathBuf, String> {
    use objc2_foundation::{NSFileManager, NSURL};
    let url = NSURL::from_file_path(path).ok_or("Could not prepare the item for Trash.")?;
    let mut destination = None;
    NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, Some(&mut destination))
        .map_err(|e| format!("Could not move the item to Trash: {e}"))?;
    destination.and_then(|url| url.to_file_path()).ok_or("The item moved to Trash, but its location could not be read.".into())
}

#[cfg(not(target_os = "macos"))]
fn trash_location(_path: &Path) -> Result<std::path::PathBuf, String> {
    Err("Moving items to Trash is not available on this system.".into())
}

#[cfg(target_os = "macos")]
pub fn trash(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSURL};
    let url = NSURL::from_file_path(path).ok_or("Could not prepare the item for Trash.")?;
    NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| format!("Could not move the item to Trash: {e}"))
}

#[cfg(not(target_os = "macos"))]
pub fn trash(path: &Path) -> Result<(), String> { trash_location(path).map(|_| ()) }

pub fn archive_history(entries: &[Entry]) -> Result<(), String> {
    if entries.is_empty() { return Ok(()); }
    let root = crate::settings::data_dir();
    archive_history_in(&root, entries, &|path| {
        let destination = trash_location(path)?;
        let index = root.join("latest-history-backup.json");
        let tmp = index.with_extension("tmp");
        let value = serde_json::to_vec(&destination).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, value).map_err(|e| format!("Could not record the backup location: {e}"))?;
        std::fs::rename(&tmp, &index).map_err(|e| format!("Could not record the backup location: {e}"))?;
        Ok(())
    })
}

pub fn latest_backup() -> Option<std::path::PathBuf> {
    let path = crate::settings::data_dir().join("latest-history-backup.json");
    let folder: std::path::PathBuf = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if !folder.file_name()?.to_str()?.starts_with("Open Dictate history backup ") || !folder.join("history.json").is_file() {
        return None;
    }
    Some(folder)
}

pub fn restore_backup_in(
    folder: &Path,
    audio_dir: &Path,
    current: &[Entry],
    save: &dyn Fn(&[Entry]) -> Result<(), String>,
) -> Result<Vec<Entry>, String> {
    use std::io::Write;
    struct Created(Vec<std::path::PathBuf>);
    impl Drop for Created { fn drop(&mut self) { for p in &self.0 { let _ = std::fs::remove_file(p); } } }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let data = std::fs::read(folder.join("history.json")).map_err(|e| format!("Could not read the backup: {e}"))?;
    let entries: Vec<Entry> = serde_json::from_slice(&data).map_err(|e| format!("The backup is invalid: {e}"))?;
    let mut merged = current.to_vec();
    let mut created = Created(Vec::new());
    for mut entry in entries {
        if merged.iter().any(|e| e.when == entry.when && e.action == entry.action && e.raw == entry.raw && e.text == entry.text && e.error == entry.error && e.selection == entry.selection) { continue; }
        if let Some(relative) = &entry.audio {
            let mut parts = Path::new(relative).components();
            let safe = matches!(parts.next(), Some(std::path::Component::Normal(p)) if p == "audio")
                && matches!(parts.next(), Some(std::path::Component::Normal(p)) if p.to_str().is_some_and(|n| n.strip_suffix(".wav").is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))))
                && parts.next().is_none();
            if !safe { return Err("The backup contains an invalid audio path.".into()); }
            let audio_folder = folder.join("audio");
            let source = folder.join(relative);
            if !std::fs::symlink_metadata(&audio_folder).map(|m| m.file_type().is_dir()).unwrap_or(false)
                || !std::fs::symlink_metadata(&source).map(|m| m.file_type().is_file()).unwrap_or(false) {
                return Err("A recording in the backup is missing or invalid.".into());
            }
            let target = loop {
                let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let target = audio_dir.join(format!("recovered-{}-{n}.wav", std::process::id()));
                match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
                    Ok(mut out) => {
                        created.0.push(target.clone());
                        let mut input = std::fs::File::open(&source).map_err(|e| e.to_string())?;
                        std::io::copy(&mut input, &mut out).map_err(|e| format!("Could not restore audio: {e}"))?;
                        out.flush().map_err(|e| e.to_string())?;
                        break target;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(format!("Could not restore audio: {e}")),
                }
            };
            entry.audio = Some(target.to_string_lossy().into_owned());
        }
        merged.push(entry);
    }
    merged.sort_by_key(|e| e.when);
    save(&merged)?;
    created.0.clear();
    Ok(merged)
}

fn archive_history_in(root: &Path, entries: &[Entry], move_to_trash: &dyn Fn(&Path) -> Result<(), String>) -> Result<(), String> {
    if entries.is_empty() { return Ok(()); }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let folder = loop {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = root.join(format!("Open Dictate history backup {}-{n}", std::process::id()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
        match builder.create(&path) {
            Ok(()) => break path,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not prepare a history backup: {e}")),
        }
    };
    let result = (|| {
        let mut copy = entries.to_vec();
        for (index, entry) in copy.iter_mut().enumerate() {
            if let Some(path) = entry.audio.clone() {
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.is_file() => {
                        std::fs::create_dir_all(folder.join("audio")).map_err(|e| e.to_string())?;
                        let name = format!("audio/{index}.wav");
                        std::fs::copy(&path, folder.join(&name)).map_err(|e| format!("Could not back up a recording: {e}"))?;
                        entry.audio = Some(name);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => entry.audio = None,
                    Ok(_) => return Err("Could not back up a recording: it is not a file.".into()),
                    Err(e) => return Err(format!("Could not back up a recording: {e}")),
                }
            }
        }
        let json = serde_json::to_vec_pretty(&copy).map_err(|e| e.to_string())?;
        std::fs::write(folder.join("history.json"), json).map_err(|e| format!("Could not back up history: {e}"))?;
        std::fs::write(folder.join("Read me.txt"), "These are history entries removed from Open Dictate. Text is in history.json; any retained failed recordings are in the audio folder. In History, open the More menu and choose Restore last history backup to restore the most recent backup.\n")
            .map_err(|e| e.to_string())?;
        move_to_trash(&folder)
    })();
    if result.is_err() { let _ = std::fs::remove_dir_all(&folder); }
    result
}

