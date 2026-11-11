//! Opening Open Dictate at login.
//!
//! macOS 13 has `SMAppService` for this, but the bundle declares
//! `LSMinimumSystemVersion` 11.0 and a LaunchAgent works on every version the
//! app claims to run on, with no new dependency and nothing to ask the system
//! for. The result is one XML file in the person's own `~/Library/LaunchAgents`
//! that launchd reads at login; deleting it by hand undoes this completely,
//! which is the kind of switch worth preferring.
//!
//! The file is written but never loaded with `launchctl`. Loading it would
//! honour `RunAtLoad` at once and start a second copy of the app on top of the
//! one you just ticked the box in. Login is when this is meant to happen, so
//! login is when it happens.
//!
//! macOS 13 and later will tell the person an item was added and offer to turn
//! it off under Login Items. That switch is theirs; this code never fights it.

use std::path::PathBuf;

/// Matches CFBundleIdentifier, so the Login Items list names the app and not
/// some invented label.
const LABEL: &str = "app.opendictate.desktop";

fn agents_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/LaunchAgents"))
}

pub fn plist_path() -> Option<PathBuf> {
    agents_dir().map(|d| d.join(format!("{LABEL}.plist")))
}

/// The XML launchd reads. `exe` is the binary inside the bundle, which is what
/// gives the launched process its bundle identity - and so its microphone and
/// accessibility grants. Launching the `.app` directory instead would need
/// `open`, and a login item that is really `/usr/bin/open` is harder to read
/// and harder to trust.
///
/// `--hidden` keeps the window shut. The app lives in the menu bar and works
/// by shortcut, so logging in should hand you the shortcuts, not a window.
fn plist(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>--hidden</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
    )
}

/// Writes or removes the login item.
///
/// Writing is not conditional on the file's absence: the path to the app is
/// baked into it, so an app that has moved since the box was ticked needs the
/// file rewritten, not left alone.
pub fn set(enabled: bool) -> Result<(), String> {
    let path = plist_path().ok_or("No home directory")?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Could not remove the login item: {e}")),
        };
    }
    let exe = std::env::current_exe().map_err(|e| format!("Could not find this app: {e}"))?;
    let exe = exe.to_str().ok_or("This app's path is not valid text")?;
    // An ampersand or an angle bracket in the path would break the XML. A Mac
    // app lives somewhere its own installer put it, so this is a guard rather
    // than an expectation - better a clear refusal than a file launchd cannot
    // parse and never reports on.
    if exe.contains(['&', '<', '>', '"']) {
        return Err("This app's path has characters a login item cannot carry".into());
    }
    let dir = agents_dir().ok_or("No home directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not open {}: {e}", dir.display()))?;
    std::fs::write(&path, plist(exe)).map_err(|e| format!("Could not write the login item: {e}"))
}

/// Brings the file into line with the setting at launch.
///
/// The setting is what the person chose, so it wins. This also repoints the
/// file after the app has been moved - from the Downloads folder to
/// Applications, most often - which otherwise leaves a login item quietly
/// aimed at a binary that is no longer there.
pub fn sync(enabled: bool) {
    let _ = set(enabled);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_names_the_bundle_and_the_binary() {
        let x = plist("/Applications/Open Dictate.app/Contents/MacOS/open-dictate");
        assert!(x.contains("<string>app.opendictate.desktop</string>"));
        assert!(x.contains("<string>/Applications/Open Dictate.app/Contents/MacOS/open-dictate</string>"));
        assert!(x.contains("<string>--hidden</string>"), "a login item should not open a window");
        assert!(x.contains("<key>RunAtLoad</key><true/>"));
    }

    #[test]
    fn the_file_sits_beside_the_other_login_items() {
        let p = plist_path().unwrap();
        assert!(p.ends_with("Library/LaunchAgents/app.opendictate.desktop.plist"), "{}", p.display());
    }
}
