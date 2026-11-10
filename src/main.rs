mod app;
mod audio;
mod cut;
mod engines;
mod icon;
mod lms;
mod meeting;
#[cfg(target_os = "macos")]
mod macos_look;
mod search;
mod settings;
mod settings_view;
mod stats;
mod talking;
mod talking_view;
mod tap;
mod theme;
mod tray;
mod views;
mod whisper_server;

#[cfg(target_os = "macos")]
mod macos_bundle {
    use std::path::Path;
    use std::process::Command;

    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>open-dictate</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleIdentifier</key><string>app.opendictate.desktop</string>
    <key>CFBundleName</key><string>Open Dictate</string>
    <key>CFBundleDisplayName</key><string>Open Dictate</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>VERSION</string>
    <key>CFBundleVersion</key><string>VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSAudioCaptureUsageDescription</key><string>Meeting mode records what your Mac is playing, so the other voices on a call are in the transcript alongside yours. The recording stays on this Mac.</string>
    <key>NSMicrophoneUsageDescription</key><string>Open Dictate listens while you hold or press your dictation shortcut, and sends the recording only to the speech model you chose.</string>
    <key>NSAppleEventsUsageDescription</key><string>Open Dictate pastes your dictated text into the app you are using.</string>
</dict>
</plist>
"#;

    fn same_file(a: &Path, b: &Path) -> bool {
        match (std::fs::read(a), std::fs::read(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }

    /// Copies this program into an app bundle (when it changed) and opens that instead.
    /// Returns true when the bundled copy was started and this process should exit.
    pub fn relaunch() -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        if exe.to_string_lossy().contains(".app/Contents/MacOS/") {
            return false; // already running as an app
        }
        const ICNS: &[u8] = include_bytes!("../assets/AppIcon.icns");
        let bundle = crate::settings::data_dir().join("Open Dictate.app");
        let macos = bundle.join("Contents/MacOS");
        let icon = bundle.join("Contents/Resources/AppIcon.icns");
        let target = macos.join("open-dictate");
        let plist = PLIST.replace("VERSION", env!("CARGO_PKG_VERSION"));
        let plist_path = bundle.join("Contents/Info.plist");
        let fresh = !same_file(&exe, &target) || std::fs::read_to_string(&plist_path).ok().as_deref() != Some(plist.as_str());
        if fresh {
            // Remove before copying so a running copy keeps its own file.
            let _ = std::fs::remove_file(&target);
            if std::fs::create_dir_all(&macos).is_err()
                || std::fs::create_dir_all(bundle.join("Contents/Resources")).is_err()
                || std::fs::write(&plist_path, &plist).is_err()
                || std::fs::write(&icon, ICNS).is_err()
                || std::fs::copy(&exe, &target).is_err()
            {
                return false;
            }
            // Sign the bundle so macOS ties the microphone permission to it.
            let _ = Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&bundle).output();
        }
        Command::new("open").arg(&bundle).status().is_ok_and(|s| s.success())
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--summarise") {
        // Re-reads a meeting already on disk. The transcript is the
        // expensive part and it survives, so extraction can be changed and
        // measured against a real meeting without recording another.
        let s = settings::load_settings();
        let id = match args.get(i + 1).map(String::as_str) {
            Some("latest") | None => meeting::list().first().map(|m| m.id.clone()),
            Some(id) => Some(id.to_string()),
        };
        let Some(id) = id else {
            println!("no meetings");
            return Ok(());
        };
        let progress = std::sync::Mutex::new(String::new());
        match meeting::summarise(&s, &id, &progress) {
            Ok(sum) => {
                println!("{id} · {}\n\n{}", sum.model, sum.overview);
                for (k, v) in [("Decided", &sum.decisions), ("To do", &sum.actions), ("Still open", &sum.questions)] {
                    println!("\n{k} ({}):", v.len());
                    for i in v {
                        println!("  - {i}");
                    }
                }
            }
            Err(e) => println!("failed: {e}"),
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--meeting-check") {
        // Records a meeting for a few seconds and prints what came of it,
        // levels and all. A meeting that transcribes to nothing has to be
        // explainable without a rebuild.
        let secs: u64 = args.get(i + 1).and_then(|a| a.parse().ok()).unwrap_or(40);
        let s = settings::load_settings();
        // Also written to a file: the run that says anything about the
        // microphone's permission is one launched as the bundle, and that
        // run has nowhere to print.
        let report = settings::data_dir().join("meeting-check.txt");
        match meeting::Session::start(&s) {
            Err(e) => println!("could not start: {e}"),
            Ok(session) => {
                println!("recording {secs}s: {}", session.source);
                let dir = session.dir.clone();
                std::thread::sleep(std::time::Duration::from_secs(secs));
                let talking = session.talking.clone();
                let id = session.stop();
                if let Some(t) = talking {
                    let began = std::time::Instant::now();
                    while !t.lock().map_or(true, |l| l.done) && began.elapsed() < std::time::Duration::from_secs(35) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                let levels = std::fs::read_to_string(dir.join("levels.log")).unwrap_or_default();
                let _ = std::fs::write(&report, format!("meeting {id}\n{levels}"));
                println!("\n{levels}");
                for seg in meeting::transcript(&id) {
                    println!("[{:>7}ms {:4.1}s] {}", seg.at_ms, seg.secs, seg.text);
                }
                if let Ok(points) = std::fs::read_to_string(dir.join("talking_points.json")) {
                    println!("\n--- talking points ---\n{points}");
                }
                if !meeting::transcript(&id).is_empty() {
                    let progress = std::sync::Mutex::new(String::new());
                    match meeting::summarise(&s, &id, &progress) {
                        Ok(sum) => {
                            println!("\n--- summary ({}) ---\n{}", sum.model, sum.overview);
                            for (k, v) in [("Decided", &sum.decisions), ("To do", &sum.actions), ("Still open", &sum.questions)] {
                                println!("\n{k}:");
                                for i in v {
                                    println!("  - {i}");
                                }
                            }
                        }
                        Err(e) => println!("\nsummary failed: {e}"),
                    }
                }
            }
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--tap-check") {
        // Writes as well as prints: the only run that proves anything about
        // the permission is one launched by LaunchServices as the signed
        // bundle, and that run has nowhere to print to.
        let report = tap::check(6);
        print!("{report}");
        let path = settings::data_dir().join("tap-check.txt");
        let _ = std::fs::create_dir_all(settings::data_dir());
        let _ = std::fs::write(&path, &report);
        return Ok(());
    }
    if args.iter().any(|a| a == "--mic-check") {
        // Records two seconds from every microphone and prints how loud it was.
        for name in audio::input_devices() {
            match audio::Recording::start(&name, 1.0) {
                Ok(r) => {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let info = r.info();
                    let clip = r.finish();
                    let peak = clip.samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    println!("{name}: {info} -> {:.1}s, peak {peak:.4}", clip.seconds());
                }
                Err(e) => println!("{name}: {e}"),
            }
        }
        return Ok(());
    }
    let screenshot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);

    // macOS only lets proper app bundles ask for the microphone; a bare program
    // just gets silence. Re-open ourselves as "Open Dictate.app" for normal use.
    #[cfg(target_os = "macos")]
    if screenshot.is_none() && !args.iter().any(|a| a == "--no-bundle") && macos_bundle::relaunch() {
        return Ok(());
    }

    // egui only produces frames while the system is drawing the root window,
    // and macOS stops drawing one that is closed or buried behind other apps.
    // That made the recording indicator impossible to create at the moment it
    // was most wanted. So the root is a permanent 1x1 window tucked under the
    // menu bar, where nothing ever covers it: frames keep arriving, and the
    // real window hangs off it as a child viewport.
    //
    // Screenshot mode keeps the old shape, with the real window as the root,
    // because that is what gets captured.
    let options = eframe::NativeOptions {
        viewport: if screenshot.is_some() {
            eframe::egui::ViewportBuilder::default()
                .with_title("Open Dictate")
                .with_inner_size([1360.0, 920.0])
                .with_min_inner_size([980.0, 640.0])
                // Matches the real window, so a screenshot shows the page at
                // the alpha it actually has rather than flattened onto black.
                .with_transparent(true)
        } else {
            eframe::egui::ViewportBuilder::default()
                .with_title("Open Dictate")
                .with_inner_size([1.0, 1.0])
                .with_position([0.0, 0.0])
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_active(false)
                // Floating above everything is what makes this work. A window
                // macOS considers covered stops being drawn, and a root that is
                // not drawn produces no frames at all - which is how the
                // indicator went missing whenever the real window was buried.
                .with_window_level(eframe::egui::WindowLevel::AlwaysOnTop)
                .with_mouse_passthrough(true)
                .with_taskbar(false)
        },
        ..Default::default()
    };
    eframe::run_native(
        "Open Dictate",
        options,
        Box::new(move |cc| Ok(Box::new(app::DictateApp::new(cc, screenshot)))),
    )
}
