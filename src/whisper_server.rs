use crate::settings::{Settings, SpeechEngine};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub fn find_server_bin(whisper_cli: &str) -> Option<PathBuf> {
    let beside = crate::engines::find_whisper_bin(whisper_cli)
        .and_then(|cli| cli.parent().map(|d| d.join("whisper-server")))
        .filter(|p| p.is_file());
    beside.or_else(|| crate::engines::find_whisper_bin("whisper-server"))
}

fn inference_fields<'a>(language: &'a str, prompt: &'a str) -> Vec<(&'static str, &'a str)> {
    let language = if language.is_empty() { "auto" } else { language };
    let mut fields = vec![("response_format", "json"), ("language", language)];
    if !prompt.is_empty() {
        fields.push(("prompt", prompt));
    }
    fields
}

fn spawn_watchdog(app: u32, server: u32) -> std::io::Result<std::process::Child> {
    let script = r#"while kill -0 "$1" 2>/dev/null && kill -0 "$2" 2>/dev/null; do sleep 1; done; kill "$2" 2>/dev/null"#;
    std::process::Command::new("sh")
        .args(["-c", script, "sh", &app.to_string(), &server.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

struct Failure {
    message: String,
    server_still_good: bool,
}

pub struct WhisperServer {
    child: Option<std::process::Child>,
    watchdog: Option<std::process::Child>,
    port: u16,
    rejected: u8,
}

impl WhisperServer {
    pub fn start(s: &Settings, stop: &AtomicBool, dir: &Path) -> Result<WhisperServer, String> {
        let bin = find_server_bin(&s.whisper_bin).ok_or("whisper-server not found")?;
        if !Path::new(&s.whisper_model).is_file() {
            return Err("No speech model chosen".into());
        }
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map_err(|e| format!("no free port: {e}"))?
            .port();
        let stderr = std::fs::File::create(dir.join("whisper-server.log")).map(std::process::Stdio::from).unwrap_or_else(|_| std::process::Stdio::null());
        let child = std::process::Command::new(bin)
            .arg("-m")
            .arg(&s.whisper_model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|e| format!("could not run whisper-server: {e}"))?;
        let watchdog = spawn_watchdog(std::process::id(), child.id()).ok();
        let mut server = WhisperServer { child: Some(child), port, watchdog, rejected: 0 };
        server.wait_until_ready(stop, Duration::from_secs(30))?;
        Ok(server)
    }

    fn wait_until_ready(&mut self, stop: &AtomicBool, patience: Duration) -> Result<(), String> {
        let probe = crate::engines::agent(2);
        let deadline = Instant::now() + patience;
        while Instant::now() < deadline {
            if stop.load(Ordering::Relaxed) {
                return Err("stopped while whisper-server was loading".into());
            }
            if let Some(status) = self.child.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
                return Err(format!("whisper-server exited: {status}"));
            }
            if let Ok(mut r) = probe.get(format!("http://127.0.0.1:{}/", self.port)).call() {
                let ours = r.status() == 200
                    && r.body_mut().read_to_string().is_ok_and(|b| b.contains("Whisper.cpp Server"))
                    && self.child.as_mut().is_none_or(|c| c.try_wait().is_ok_and(|s| s.is_none()));
                if ours {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("whisper-server did not become ready in time".into())
    }

    #[cfg(test)]
    pub fn transcribe(&self, s: &Settings, wav: &[u8]) -> Result<String, String> {
        self.attempt(s, wav).map_err(|f| f.message)
    }

    fn attempt(&self, s: &Settings, wav: &[u8]) -> Result<String, Failure> {
        let prompt = crate::engines::dictionary_prompt(s);
        let fields = inference_fields(s.language.trim(), &prompt);
        let (content_type, body) = crate::engines::multipart(&fields, wav);
        let down = |message: String| Failure { message, server_still_good: false };
        let resp = crate::engines::agent(180)
            .post(format!("http://127.0.0.1:{}/inference", self.port))
            .header("Content-Type", content_type)
            .send(&body[..])
            .map_err(|e| down(format!("whisper-server unreachable: {e}")))?;
        let rejected = resp.status().is_client_error();
        let v = crate::engines::read_json(resp).map_err(|m| Failure { message: m, server_still_good: rejected })?;
        Ok(crate::engines::clean_transcript(v["text"].as_str().unwrap_or_default()))
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        for mut c in [self.child.take(), self.watchdog.take()].into_iter().flatten() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

pub fn transcribe_stretch(
    server: &mut Option<WhisperServer>,
    s: &Settings,
    wav: &[u8],
    fallback: &dyn Fn(&[u8]) -> Result<String, String>,
    note: &dyn Fn(&str),
) -> Result<String, String> {
    if s.speech_engine == SpeechEngine::WhisperLocal {
        if let Some(srv) = server.as_mut() {
            match srv.attempt(s, wav) {
                Ok(text) => {
                    srv.rejected = 0;
                    return Ok(text);
                }
                Err(f) if f.server_still_good && srv.rejected + 1 < 3 => {
                    srv.rejected += 1;
                    return Err(f.message);
                }
                Err(f) if f.server_still_good => {
                    note(&format!("rejected three stretches in a row ({}); using whisper-cli for the rest of the meeting", f.message));
                    *server = None;
                }
                Err(f) => {
                    note(&format!("{}; using whisper-cli for the rest of the meeting", f.message));
                    *server = None;
                }
            }
        }
    }
    fallback(wav)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("od-ws-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn executable(dir: &std::path::Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn finds_whisper_server_next_to_the_configured_cli() {
        let dir = temp_dir("sibling");
        let cli = executable(&dir, "whisper-cli");
        let server = executable(&dir, "whisper-server");
        assert_eq!(find_server_bin(cli.to_str().unwrap()), Some(server));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn picks_no_sibling_when_none_sits_beside_the_cli() {
        let dir = temp_dir("none");
        let cli = executable(&dir, "whisper-cli");
        let found = find_server_bin(cli.to_str().unwrap());
        assert!(found.is_none_or(|p| !p.starts_with(&dir)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_whisper_cli_when_the_server_request_fails() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut server = Some(WhisperServer { child: None, port: closed, watchdog: None, rejected: 0 });
        let calls = std::cell::Cell::new(0);
        let cli = |_: &[u8]| {
            calls.set(calls.get() + 1);
            Ok("from the cli".to_string())
        };
        let out = transcribe_stretch(&mut server, &Settings::default(), b"wav", &cli, &|_| {});
        assert_eq!(out, Ok("from the cli".to_string()));
        assert_eq!(calls.get(), 1);
        assert!(server.is_none());
    }

    #[test]
    fn uses_the_cli_when_there_is_no_server() {
        let mut server = None;
        let cli = |_: &[u8]| Ok("cli".to_string());
        assert_eq!(transcribe_stretch(&mut server, &Settings::default(), b"w", &cli, &|_| {}), Ok("cli".to_string()));
    }

    #[test]
    fn asks_the_server_to_detect_the_language_when_set_to_auto() {
        for lang in ["", "auto"] {
            assert!(inference_fields(lang, "").contains(&("language", "auto")));
        }
    }

    #[test]
    fn passes_an_explicit_language_through() {
        let fields = inference_fields("fr", "Vocabulary: x.");
        assert!(fields.contains(&("language", "fr")));
        assert!(fields.contains(&("prompt", "Vocabulary: x.")));
    }

    #[test]
    fn stopping_while_the_server_loads_returns_promptly() {
        let dir = temp_dir("loading");
        let cli = executable(&dir, "whisper-cli");
        let server = dir.join("whisper-server");
        std::fs::write(&server, "#!/bin/sh\nexec sleep 60\n").unwrap();
        std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
        let model = dir.join("model.bin");
        std::fs::write(&model, "x").unwrap();
        let mut s = Settings::default();
        s.whisper_bin = cli.to_str().unwrap().into();
        s.whisper_model = model.to_str().unwrap().into();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let began = Instant::now();
        let result = WhisperServer::start(&s, &stop, &dir);
        assert!(result.is_err());
        assert!(began.elapsed() < Duration::from_secs(2), "{:?}", began.elapsed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fake_http(status: &str, body: &'static str) -> u16 {
        fake_http_in_turn(&[(status, body)])
    }

    fn fake_http_in_turn(replies: &[(&str, &'static str)]) -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let replies: Vec<(String, &'static str)> = replies.iter().map(|(s, b)| (s.to_string(), *b)).collect();
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().flatten().enumerate() {
                let (status, body) = &replies[n % replies.len()];
                let mut stream = stream;
                stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let mut request = Vec::new();
                let mut buf = [0u8; 65536];
                let head_end = loop {
                    if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break request.len(),
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&request[..head_end]).to_lowercase();
                let wanted = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while request.len() < head_end + wanted {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        port
    }

    #[test]
    fn does_not_accept_an_unrelated_server_that_answers_on_the_port() {
        let port = fake_http("200 OK", "<html>some other service</html>");
        let mut server = WhisperServer { child: None, port, watchdog: None, rejected: 0 };
        let began = Instant::now();
        assert!(server.wait_until_ready(&AtomicBool::new(false), Duration::from_secs(1)).is_err());
        assert!(began.elapsed() >= Duration::from_secs(1));
    }

    #[test]
    fn accepts_a_server_that_serves_whisper_servers_own_page() {
        let port = fake_http("200 OK", "<html><title>Whisper.cpp Server</title></html>");
        let mut server = WhisperServer { child: None, port, watchdog: None, rejected: 0 };
        assert!(server.wait_until_ready(&AtomicBool::new(false), Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn keeps_the_server_when_it_rejects_one_stretch() {
        let port = fake_http("400 Bad Request", r#"{"error":"Invalid request"}"#);
        let mut server = Some(WhisperServer { child: None, port, watchdog: None, rejected: 0 });
        let out = transcribe_stretch(&mut server, &Settings::default(), b"w", &|_| panic!("cli must not run"), &|_| {});
        assert!(out.unwrap_err().contains("Invalid request"));
        assert!(server.is_some());
    }

    fn calls(server: &mut Option<WhisperServer>, n: usize, noted: &std::cell::RefCell<String>) -> Vec<Result<String, String>> {
        (0..n)
            .map(|_| transcribe_stretch(server, &Settings::default(), b"w", &|_| Ok("cli".into()), &|m| noted.borrow_mut().push_str(m)))
            .collect()
    }

    #[test]
    fn falls_back_to_the_cli_after_the_server_rejects_three_stretches_in_a_row() {
        let port = fake_http("400 Bad Request", "not json");
        let mut server = Some(WhisperServer { child: None, port, watchdog: None, rejected: 0 });
        let noted = std::cell::RefCell::new(String::new());
        let out = calls(&mut server, 3, &noted);
        assert!(out[0].is_err() && out[1].is_err());
        assert_eq!(out[2], Ok("cli".to_string()));
        assert!(server.is_none());
        assert!(noted.borrow().contains("rejected"), "{}", noted.borrow());
    }

    #[test]
    fn a_success_between_rejections_resets_the_count() {
        let ok = r#"{"text":"hi"}"#;
        let port = fake_http_in_turn(&[("400 Bad Request", "no"), ("400 Bad Request", "no"), ("200 OK", ok)]);
        let mut server = Some(WhisperServer { child: None, port, watchdog: None, rejected: 0 });
        let out = calls(&mut server, 8, &std::cell::RefCell::new(String::new()));
        assert!(server.is_some());
        assert_eq!(out.iter().filter(|r| r.as_deref() == Ok("hi")).count(), 2);
    }

    #[test]
    fn drops_the_server_and_says_why_when_it_answers_with_a_server_error() {
        let port = fake_http("500 Internal Server Error", r#"{"error":"boom"}"#);
        let mut server = Some(WhisperServer { child: None, port, watchdog: None, rejected: 0 });
        let noted = std::cell::RefCell::new(String::new());
        let out = transcribe_stretch(&mut server, &Settings::default(), b"w", &|_| Ok("cli".into()), &|m| noted.borrow_mut().push_str(m));
        assert_eq!(out, Ok("cli".to_string()));
        assert!(server.is_none());
        assert!(noted.borrow().contains("boom"), "{}", noted.borrow());
    }

    fn sleeper() -> std::process::Child {
        std::process::Command::new("sleep").arg("30").spawn().unwrap()
    }

    fn gone_within(c: &mut std::process::Child, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if c.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn the_server_is_killed_when_the_app_that_started_it_dies() {
        let (mut app, mut server) = (sleeper(), sleeper());
        let mut dog = spawn_watchdog(app.id(), server.id()).unwrap();
        app.kill().unwrap();
        app.wait().unwrap();
        let killed = gone_within(&mut server, 3);
        let _ = server.kill();
        let _ = dog.kill();
        assert!(killed);
    }

    #[test]
    fn the_watchdog_exits_when_the_server_stops_normally() {
        let (mut app, mut server) = (sleeper(), sleeper());
        let mut dog = spawn_watchdog(app.id(), server.id()).unwrap();
        server.kill().unwrap();
        server.wait().unwrap();
        let exited = gone_within(&mut dog, 3);
        let _ = app.kill();
        let _ = dog.kill();
        assert!(exited);
    }

    fn real_settings() -> Settings {
        let mut s = Settings::default();
        s.whisper_model = std::env::var("WHISPER_MODEL").expect("set WHISPER_MODEL to a ggml model file");
        s
    }

    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/question.wav")).unwrap()
    }

    #[test]
    #[ignore = "needs WHISPER_MODEL and whisper-server"]
    fn transcribes_a_wav_through_a_real_server() {
        let s = real_settings();
        let server = WhisperServer::start(&s, &AtomicBool::new(false), &std::env::temp_dir()).unwrap();
        let text = server.transcribe(&s, &fixture()).unwrap();
        assert!(text.to_lowercase().contains("cost estimate"), "{text}");
    }

    #[test]
    #[ignore = "needs WHISPER_MODEL, whisper-server and whisper-cli"]
    fn times_server_calls_against_cli_calls() {
        let s = real_settings();
        let wav = fixture();
        let server = WhisperServer::start(&s, &AtomicBool::new(false), &std::env::temp_dir()).unwrap();
        server.transcribe(&s, &wav).unwrap();
        let t = std::time::Instant::now();
        for _ in 0..10 {
            server.transcribe(&s, &wav).unwrap();
        }
        let server_avg = t.elapsed() / 10;
        let t = std::time::Instant::now();
        for _ in 0..3 {
            crate::engines::transcribe(&s, &wav).unwrap();
        }
        let cli_avg = t.elapsed() / 3;
        println!("server avg {server_avg:?} over 10, cli avg {cli_avg:?} over 3");
        assert!(server_avg < cli_avg);
    }
}
