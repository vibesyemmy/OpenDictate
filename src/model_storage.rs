#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("open-dictate-models-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn inventory_reports_real_files_and_sizes_and_deletion_stays_inside_root() {
        let dir = temp_dir("inventory");
        std::fs::write(dir.join("ggml-a.bin"), [1u8; 17]).unwrap();
        std::fs::write(dir.join("partial.bin.part"), [1u8; 9]).unwrap();
        let other = temp_dir("outside");
        std::fs::write(other.join("ggml-other.bin"), [0u8; 8]).unwrap();
        #[cfg(unix)] std::os::unix::fs::symlink(other.join("ggml-other.bin"), dir.join("link.bin")).unwrap();
        let models = whisper_inventory(&dir).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].size, 17);
        assert!(delete_whisper(&dir, &other.join("ggml-other.bin")).is_err());
        assert!(delete_whisper(&dir, &dir.join("link.bin")).is_err());
        delete_whisper(&dir, &dir.join("ggml-a.bin")).unwrap();
        assert!(!dir.join("ggml-a.bin").exists());
        assert!(other.join("ggml-other.bin").exists());
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(other).unwrap();
    }

    #[test]
    fn ollama_inventory_preserves_sizes_and_unknowns() {
        let json = serde_json::json!({"models":[{"name":"qwen2.5:3b","size":1234},{"name":"other:latest"}]});
        let models = parse_ollama_inventory(&json).unwrap();
        assert_eq!(models[0].size, Some(1234));
        assert_eq!(models[1].size, None);
        assert!(parse_ollama_inventory(&serde_json::json!({})).is_err());
    }

    #[test]
    fn sizes_read_the_way_finder_shows_them() {
        assert_eq!(format_size(0), "0 MB");
        assert_eq!(format_size(147_951_465), "148 MB");
        assert_eq!(format_size(999_000_000), "999 MB");
        assert_eq!(format_size(1_624_555_275), "1.6 GB");
        assert_eq!(format_size(12_000_000_000), "12 GB");
    }

    #[test]
    fn ollama_models_are_never_deleted_from_another_machine() {
        let err = delete_ollama("http://example.com:11434", "llama3.2").unwrap_err();
        assert!(err.contains("on this Mac"), "{err}");
        assert!(delete_ollama("http://evil.com@localhost:11434", "llama3.2").is_err());
    }

    #[test]
    fn model_identity_and_local_server_rules_are_safe() {
        assert!(same_ollama_model("llama3.2", "llama3.2:latest"));
        assert!(!same_ollama_model("llama3.2:3b", "llama3.2:latest"));
        assert!(local_ollama_url("http://localhost:11434"));
        assert!(local_ollama_url("http://127.0.0.1:11434"));
        assert!(!local_ollama_url("http://localhost.evil.com:11434"));
        assert!(!local_ollama_url("http://example.com:11434"));
        assert!(!local_ollama_url("http://evil.com@localhost:11434"));
    }
}

use std::path::{Path, PathBuf};

/// A downloaded whisper model file in the models folder.
#[derive(Clone, Debug, PartialEq)]
pub struct WhisperFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

/// A model reported by Ollama. Ollama may omit the size, so it stays unknown
/// rather than being shown as zero.
#[derive(Clone, Debug, PartialEq)]
pub struct OllamaModel {
    pub name: String,
    pub size: Option<u64>,
}

/// Lists finished whisper models: regular `.bin` files directly in `dir`.
/// Partial downloads and symlinks are left out, so the sizes shown are real
/// disk use in this folder.
pub fn whisper_inventory(dir: &Path) -> Result<Vec<WhisperFile>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Could not read the models folder: {e}")),
    };
    let mut models = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Could not read the models folder: {e}"))?;
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "bin") { continue; }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("Could not read a model file: {e}"))?;
        if !meta.file_type().is_file() { continue; }
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
        models.push(WhisperFile { name, path, size: meta.len() });
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

/// Deletes one whisper model. Only a regular `.bin` file directly inside
/// `dir` can be deleted: anything outside it, and symlinks (which could point
/// outside it), are refused.
pub fn delete_whisper(dir: &Path, file: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(file).map_err(|e| format!("Could not find the model: {e}"))?;
    if !meta.file_type().is_file() || file.extension().is_none_or(|x| x != "bin") {
        return Err("Only downloaded model files can be deleted.".into());
    }
    let root = dir.canonicalize().map_err(|e| format!("Could not read the models folder: {e}"))?;
    let parent = file.parent().ok_or("The model is not in the models folder.")?
        .canonicalize().map_err(|e| format!("Could not read the model's folder: {e}"))?;
    if parent != root { return Err("The model is not in the models folder.".into()); }
    std::fs::remove_file(file).map_err(|e| format!("Could not delete the model: {e}"))
}

/// Reads Ollama's `/api/tags` response. A reply without a `models` list is
/// an error rather than "no models", so a wrong server is not mistaken for an
/// empty one.
pub fn parse_ollama_inventory(json: &serde_json::Value) -> Result<Vec<OllamaModel>, String> {
    let list = json["models"].as_array().ok_or("The server's reply did not list any models.")?;
    Ok(list.iter().filter_map(|m| {
        let name = m["name"].as_str()?.to_owned();
        Some(OllamaModel { name, size: m["size"].as_u64() })
    }).collect())
}

/// Ollama treats a name without a tag as `:latest`; any other tag is a
/// different model.
pub fn same_ollama_model(a: &str, b: &str) -> bool {
    fn full(name: &str) -> String {
        let name = name.trim();
        let tagged = name.rsplit('/').next().is_some_and(|last| last.contains(':'));
        if tagged { name.to_owned() } else { format!("{name}:latest") }
    }
    full(a) == full(b)
}

/// True only for an Ollama address on this Mac. Addresses with a user part
/// (`name@host`) are refused outright, so nothing that looks like another
/// server can count as local.
pub fn local_ollama_url(url: &str) -> bool {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
    !authority.contains('@') && crate::talking::is_local(url)
}

/// A size in decimal units, as Finder shows it: whole MB below a gigabyte,
/// one decimal place below ten gigabytes, whole GB above.
pub fn format_size(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    const GB: f64 = 1_000_000_000.0;
    let b = bytes as f64;
    if b < GB { format!("{:.0} MB", b / MB) }
    else if b < 10.0 * GB { format!("{:.1} GB", b / GB) }
    else { format!("{:.0} GB", b / GB) }
}

fn ollama_base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// The models Ollama has downloaded, with their sizes.
pub fn ollama_inventory(url: &str) -> Result<Vec<OllamaModel>, String> {
    let resp = crate::engines::agent(4)
        .get(format!("{}/api/tags", ollama_base(url)))
        .call()
        .map_err(|_| "Ollama isn't running".to_string())?;
    parse_ollama_inventory(&crate::engines::read_json(resp)?)
}

/// Asks Ollama to delete a model. Only an Ollama on this Mac is asked: a
/// remote server's models belong to whoever runs it.
pub fn delete_ollama(url: &str, name: &str) -> Result<(), String> {
    if !local_ollama_url(url) {
        return Err("Models can only be deleted from Ollama on this Mac.".into());
    }
    let resp = crate::engines::agent(30)
        .delete(format!("{}/api/delete", ollama_base(url)))
        .force_send_body()
        .send_json(serde_json::json!({ "model": name }))
        .map_err(|_| "Ollama isn't running".to_string())?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        404 => Err(format!("Ollama no longer has {name}.")),
        code => Err(format!("Ollama could not delete {name} (error {code}).")),
    }
}
