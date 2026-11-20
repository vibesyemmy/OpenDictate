//! How much disk space Open Dictate's own data uses, split into the parts a
//! person would recognise. Ollama's models live in Ollama's folder and are
//! not counted here.

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, bytes: usize) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![1u8; bytes]).unwrap();
    }

    #[test]
    fn files_are_counted_under_the_part_people_recognise() {
        let root = std::env::temp_dir().join(format!("opendictate-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write(&root, "models/ggml-small.bin", 9_000);
        write(&root, "meetings/2026-11-17-1/audio/000001000.wav", 5_000);
        write(&root, "meetings/2026-11-17-1/failed_audio/000002000.wav", 1_000);
        write(&root, "meetings/2026-11-17-1/transcript.jsonl", 300);
        write(&root, "meetings/2026-11-17-1/meta.json", 100);
        write(&root, "meetings/2026-11-17-1/meeting.log", 50);
        write(&root, "history.json", 200);
        write(&root, "audio/42.wav", 400);
        write(&root, "settings.json", 20);
        let usage = scan(&root);
        assert_eq!(usage.bytes(Part::SpeechModels), 9_000);
        assert_eq!(usage.bytes(Part::MeetingAudio), 6_000);
        assert_eq!(usage.bytes(Part::MeetingNotes), 400);
        assert_eq!(usage.bytes(Part::History), 600);
        assert_eq!(usage.bytes(Part::Other), 70);
        assert_eq!(usage.total(), 16_070);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed_so_nothing_outside_is_counted() {
        let root = std::env::temp_dir().join(format!("opendictate-usage-link-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("opendictate-usage-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        write(&outside, "big.bin", 50_000);
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("models/linked")).unwrap();
        assert_eq!(scan(&root).total(), 0);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn a_missing_folder_uses_nothing() {
        assert_eq!(scan(Path::new("/nonexistent/open-dictate")).total(), 0);
    }
}

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    SpeechModels,
    MeetingAudio,
    MeetingNotes,
    History,
    Other,
}

impl Part {
    /// Bar and legend order: biggest usual consumers first.
    pub const ALL: [Part; 5] = [Part::SpeechModels, Part::MeetingAudio, Part::MeetingNotes, Part::History, Part::Other];

    pub fn label(self) -> &'static str {
        match self {
            Part::SpeechModels => "Speech models",
            Part::MeetingAudio => "Meeting audio",
            Part::MeetingNotes => "Meeting transcripts & notes",
            Part::History => "Dictation history",
            Part::Other => "Settings, logs & other",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    parts: [u64; 5],
}

impl Usage {
    pub fn bytes(&self, part: Part) -> u64 { self.parts[part as usize] }
    pub fn total(&self) -> u64 { self.parts.iter().sum() }
}

fn part_of(relative: &Path) -> Part {
    let mut parts = relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned());
    match parts.next().as_deref() {
        Some("models") => Part::SpeechModels,
        Some("history.json") | Some("audio") => Part::History,
        Some("meetings") => {
            let rest: Vec<String> = parts.collect();
            let in_audio = rest.iter().any(|p| p == "audio" || p == "failed_audio");
            if in_audio { Part::MeetingAudio }
            else if rest.last().is_some_and(|name| name.ends_with(".log")) { Part::Other }
            else { Part::MeetingNotes }
        }
        _ => Part::Other,
    }
}

/// Walks the data folder once. Symlinks are never followed.
pub fn scan(root: &Path) -> Usage {
    let mut usage = Usage::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if meta.file_type().is_dir() {
                stack.push(path);
            } else if meta.file_type().is_file() {
                let part = part_of(path.strip_prefix(root).unwrap_or(&path));
                usage.parts[part as usize] += meta.len();
            }
        }
    }
    usage
}
