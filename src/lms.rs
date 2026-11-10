use crate::settings::{Settings, TextEngine};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const PORT: u16 = 1234;

pub fn find() -> Option<PathBuf> {
    let lms = PathBuf::from(std::env::var_os("HOME")?).join(".lmstudio/bin/lms");
    lms.is_file().then_some(lms)
}

fn port_of(url: &str) -> Option<u16> {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?;
    authority.rsplit_once(':').filter(|(_, p)| !p.contains(']')).and_then(|(_, p)| p.parse().ok())
}

pub fn is_lm_studio(s: &Settings) -> bool {
    s.text_engine == TextEngine::OpenAiCompatible && crate::talking::is_local(&s.text_url) && port_of(&s.text_url) == Some(PORT)
}

pub fn steps(server_running: bool, loaded: &[String], model: &str) -> Vec<Vec<String>> {
    let model = model.trim();
    let mut out = Vec::new();
    if !server_running {
        out.push(vec!["server".into(), "start".into()]);
    }
    if !model.is_empty() && !loaded.iter().any(|l| l == model) {
        out.push(vec!["load".into(), model.into(), "-y".into()]);
    }
    out
}

fn parse_running(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json).ok().and_then(|v| v["running"].as_bool()).unwrap_or(false)
}

fn parse_loaded(json: &str) -> Vec<String> {
    let Ok(serde_json::Value::Array(models)) = serde_json::from_str(json) else { return Vec::new() };
    models.iter().flat_map(|m| [m["identifier"].as_str(), m["modelKey"].as_str()]).flatten().map(String::from).collect()
}

fn ensure_with(s: &Settings, run: &dyn Fn(&[String]) -> Result<String, String>, log: &dyn Fn(&str)) -> Result<(), String> {
    let model = if s.meeting_model.trim().is_empty() { &s.text_model } else { &s.meeting_model };
    let model = model.trim();
    if model.starts_with('-') {
        return Err(format!("refusing model name {model:?}"));
    }
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if !parse_running(&run(&args(&["server", "status", "--json"]))?) {
        run(&args(&["server", "start"]))?;
        log("started the server");
    }
    let loaded = parse_loaded(&ps_with_retry(run, Duration::from_millis(500))?);
    for step in steps(true, &loaded, model) {
        run(&step)?;
        log(&format!("loaded {model}"));
    }
    Ok(())
}

fn ps_with_retry(run: &dyn Fn(&[String]) -> Result<String, String>, pause: Duration) -> Result<String, String> {
    let ps = ["ps".to_string(), "--json".to_string()];
    let mut result = run(&ps);
    for _ in 1..10 {
        if result.is_ok() {
            break;
        }
        std::thread::sleep(pause);
        result = run(&ps);
    }
    result
}

fn run_command(program: &Path, args: &[String], timeout: Duration, capture: bool) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{Receiver, channel};
    let shown = format!("lms {}", args.join(" "));
    let pipe = || if capture { Stdio::piped() } else { Stdio::null() };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(pipe())
        .stderr(pipe())
        .spawn()
        .map_err(|e| format!("{shown}: {e}"))?;
    fn drain(pipe: Option<impl Read + Send + 'static>) -> Receiver<String> {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_string(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    }
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| format!("{shown}: {e}"))? {
            Some(status) => break status,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{shown}: timed out after {}s", timeout.as_secs_f32()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    let (Ok(out), Ok(err)) = (out.recv_timeout(left), err.recv_timeout(left)) else {
        return Err(format!("{shown}: output pipe still held open after {}s", timeout.as_secs_f32()));
    };
    if status.success() { Ok(out) } else { Err(format!("{shown}: exit {status}: {}", err.trim())) }
}

fn exclusive(flag: &AtomicBool, work: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    struct Release<'a>(&'a AtomicBool);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    if flag.swap(true, Ordering::SeqCst) {
        return Err("already starting".into());
    }
    let _release = Release(flag);
    work()
}

static ENSURING: AtomicBool = AtomicBool::new(false);

pub fn ensure(s: &Settings, log: &dyn Fn(&str)) -> Result<(), String> {
    let lms = find().ok_or("lms is not installed")?;
    exclusive(&ENSURING, || {
        ensure_with(
            s,
            &|args| {
                let first = args.first().map(String::as_str);
                let quiet = matches!(first, Some("load")) || args.get(..2).is_some_and(|a| a == ["server", "start"]);
                run_command(&lms, args, Duration::from_secs(if first == Some("load") { 180 } else { 15 }), !quiet)
            },
            log,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn lm(url: &str, model: &str, meeting: &str) -> Settings {
        Settings {
            text_engine: TextEngine::OpenAiCompatible,
            text_url: url.into(),
            text_model: model.into(),
            meeting_model: meeting.into(),
            ..Settings::default()
        }
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn starts_the_server_and_loads_the_model_when_neither_is_running() {
        assert_eq!(steps(false, &[], "phi"), vec![strs(&["server", "start"]), strs(&["load", "phi", "-y"])]);
    }

    #[test]
    fn only_loads_the_model_when_the_server_is_already_up() {
        assert_eq!(steps(true, &strs(&["other"]), "phi"), vec![strs(&["load", "phi", "-y"])]);
    }

    #[test]
    fn does_nothing_when_the_model_is_already_loaded() {
        assert!(steps(true, &strs(&["phi"]), "phi").is_empty());
    }

    #[test]
    fn loads_nothing_when_no_model_is_chosen() {
        assert_eq!(steps(false, &[], " "), vec![strs(&["server", "start"])]);
    }

    #[test]
    fn is_not_lm_studio_for_ollama_or_a_cloud_url() {
        let mut ollama = lm("http://localhost:1234/v1", "m", "");
        ollama.text_engine = TextEngine::Ollama;
        assert!(!is_lm_studio(&ollama));
        assert!(!is_lm_studio(&lm("https://api.openai.com/v1", "m", "")));
        assert!(!is_lm_studio(&lm("http://localhost:11434/v1", "m", "")));
        assert!(!is_lm_studio(&lm("http://localhost/v1", "m", "")));
    }

    #[test]
    fn is_lm_studio_for_a_local_openai_compatible_server_on_1234() {
        assert!(is_lm_studio(&lm("http://localhost:1234/v1", "m", "")));
        assert!(is_lm_studio(&lm("http://127.0.0.1:1234", "m", "")));
        assert!(is_lm_studio(&lm("http://[::1]:1234/v1", "m", "")));
    }

    #[test]
    fn reads_whether_the_server_runs_from_status_json() {
        assert!(parse_running(r#"{"running":true,"port":1234}"#));
        assert!(!parse_running(r#"{"running":false}"#));
        assert!(!parse_running("The server is not running."));
    }

    #[test]
    fn reads_identifiers_and_model_keys_of_loaded_models() {
        let json = r#"[{"identifier":"phi-4-mini-instruct-chat-mlx","modelKey":"phi-4-mini","path":"x/y"}]"#;
        assert_eq!(parse_loaded(json), strs(&["phi-4-mini-instruct-chat-mlx", "phi-4-mini"]));
        assert!(parse_loaded("[]").is_empty());
        assert!(parse_loaded("garbage").is_empty());
    }

    fn recorded(s: &Settings, status: &str, ps: &str) -> Vec<Vec<String>> {
        let calls = RefCell::new(Vec::new());
        let run = |a: &[String]| -> Result<String, String> {
            calls.borrow_mut().push(a.to_vec());
            Ok(match a[0].as_str() {
                "server" if a[1] == "status" => status.to_string(),
                "ps" => ps.to_string(),
                _ => String::new(),
            })
        };
        ensure_with(s, &run, &|_| {}).unwrap();
        calls.into_inner()
    }

    #[test]
    fn ensure_runs_no_start_and_no_load_when_the_model_is_up() {
        let calls = recorded(&lm("http://localhost:1234/v1", "phi", ""), r#"{"running":true}"#, r#"[{"identifier":"phi","modelKey":"phi"}]"#);
        assert_eq!(calls, vec![strs(&["server", "status", "--json"]), strs(&["ps", "--json"])]);
    }

    #[test]
    fn ensure_starts_the_server_and_loads_the_meeting_model_over_the_text_model() {
        let calls = recorded(&lm("http://localhost:1234/v1", "small", "big"), r#"{"running":false}"#, "[]");
        assert_eq!(calls, vec![strs(&["server", "status", "--json"]), strs(&["server", "start"]), strs(&["ps", "--json"]), strs(&["load", "big", "-y"])]);
    }

    #[test]
    fn ensure_reports_a_failing_step() {
        let run = |a: &[String]| -> Result<String, String> {
            if a[0] == "load" { Err("no such model".into()) } else { Ok(r#"{"running":true}"#.into()) }
        };
        assert!(ensure_with(&lm("http://localhost:1234/v1", "x", ""), &run, &|_| {}).unwrap_err().contains("no such model"));
    }

    #[test]
    fn a_model_already_loaded_while_the_server_was_off_is_not_loaded_again() {
        let calls = recorded(&lm("http://localhost:1234/v1", "phi", ""), r#"{"running":false}"#, r#"[{"identifier":"phi","modelKey":"phi"}]"#);
        assert_eq!(calls, vec![strs(&["server", "status", "--json"]), strs(&["server", "start"]), strs(&["ps", "--json"])]);
    }

    #[test]
    fn a_model_name_that_looks_like_a_flag_is_refused_before_anything_runs() {
        let run = |a: &[String]| -> Result<String, String> { panic!("ran {a:?}") };
        assert!(ensure_with(&lm("http://localhost:1234/v1", "--engine-cwd", ""), &run, &|_| {}).is_err());
    }

    #[test]
    fn logs_what_it_started_and_loaded() {
        let logged = RefCell::new(Vec::new());
        let run = |a: &[String]| -> Result<String, String> { Ok(if a[0] == "server" && a[1] == "status" { r#"{"running":false}"#.into() } else { "[]".into() }) };
        ensure_with(&lm("http://localhost:1234/v1", "phi", ""), &run, &|l| logged.borrow_mut().push(l.to_string())).unwrap();
        assert_eq!(logged.into_inner(), strs(&["started the server", "loaded phi"]));
    }

    #[test]
    fn a_second_ensure_while_one_runs_returns_already_starting() {
        let flag = AtomicBool::new(false);
        let inner = exclusive(&flag, || exclusive(&flag, || Ok(())));
        assert_eq!(inner, Err("already starting".into()));
        assert!(exclusive(&flag, || Ok(())).is_ok());
    }

    #[test]
    fn the_exclusive_flag_is_released_when_the_work_fails() {
        let flag = AtomicBool::new(false);
        assert!(exclusive(&flag, || Err("boom".into())).is_err());
        assert!(exclusive(&flag, || Ok(())).is_ok());
    }

    fn script(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("lms-test-{}-{name}", std::process::id()));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_command_that_outlives_its_timeout_is_killed_and_reported() {
        let sleeper = script("sleep", "exec sleep 30");
        let started = std::time::Instant::now();
        let err = run_command(&sleeper, &strs(&["ps"]), Duration::from_millis(200), true).unwrap_err();
        std::fs::remove_file(&sleeper).ok();
        assert!(err.contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_command_reads_no_stdin_and_returns_its_output() {
        let echo = script("echo", "cat >/dev/null; echo \"$1 ok\"");
        let out = run_command(&echo, &strs(&["ps"]), Duration::from_secs(5), true);
        std::fs::remove_file(&echo).ok();
        assert_eq!(out.unwrap().trim(), "ps ok");
    }

    #[test]
    fn a_failing_command_reports_its_stderr() {
        let bad = script("bad", "echo nope >&2; exit 3");
        let err = run_command(&bad, &strs(&["load"]), Duration::from_secs(5), true).unwrap_err();
        std::fs::remove_file(&bad).ok();
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn the_flag_is_released_when_the_work_panics() {
        let flag = AtomicBool::new(false);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| exclusive(&flag, || panic!("runner blew up"))));
        assert!(outcome.is_err());
        assert!(exclusive(&flag, || Ok(())).is_ok());
    }

    #[test]
    fn a_daemon_holding_the_pipes_does_not_delay_a_quiet_command() {
        let starter = script("daemon", "sleep 30 &\nexit 0");
        let started = std::time::Instant::now();
        let out = run_command(&starter, &strs(&["server", "start"]), Duration::from_secs(10), false);
        std::fs::remove_file(&starter).ok();
        assert!(out.is_ok(), "{out:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_daemon_holding_the_pipes_does_not_hang_a_captured_command_past_its_timeout() {
        let starter = script("daemon-captured", "sleep 30 &\nexit 0");
        let started = std::time::Instant::now();
        let _ = run_command(&starter, &strs(&["ps"]), Duration::from_millis(300), true);
        std::fs::remove_file(&starter).ok();
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_quiet_command_that_fails_is_reported_by_its_exit_status() {
        let bad = script("quiet-bad", "exit 3");
        let err = run_command(&bad, &strs(&["load", "x", "-y"]), Duration::from_secs(5), false).unwrap_err();
        std::fs::remove_file(&bad).ok();
        assert!(err.contains("exit"), "{err}");
    }

    #[test]
    fn ps_is_retried_while_the_server_comes_up() {
        let tries = std::cell::Cell::new(0);
        let run = |_: &[String]| -> Result<String, String> {
            tries.set(tries.get() + 1);
            if tries.get() < 3 { Err("not up yet".into()) } else { Ok("[]".into()) }
        };
        assert_eq!(ps_with_retry(&run, Duration::from_millis(1)), Ok("[]".into()));
        assert_eq!(tries.get(), 3);
    }

    #[test]
    fn ps_gives_up_after_ten_tries() {
        let tries = std::cell::Cell::new(0);
        let run = |_: &[String]| -> Result<String, String> {
            tries.set(tries.get() + 1);
            Err("down".into())
        };
        assert!(ps_with_retry(&run, Duration::from_millis(1)).is_err());
        assert_eq!(tries.get(), 10);
    }
}
