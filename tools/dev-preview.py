#!/usr/bin/env python3
"""Rebuild and restart an isolated Open Dictate preview when source files change."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent.parent
PREVIEW_HOME = ROOT / "target" / "preview-home"
WATCHED = (ROOT / "src", ROOT / "assets", ROOT / "Cargo.toml", ROOT / "Cargo.lock")


def snapshot():
    files = (p for path in WATCHED for p in (path.rglob("*") if path.is_dir() else [path]))
    return {str(p): (p.stat().st_mtime_ns, p.stat().st_size) for p in files if p.is_file()}


def main():
    PREVIEW_HOME.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["HOME"] = str(PREVIEW_HOME)
    preview = None
    previous = None
    print(f"Preview data: {PREVIEW_HOME}\nWatching for changes. Press Ctrl+C to stop.", flush=True)
    try:
        while True:
            current = snapshot()
            if current != previous:
                previous = current
                print("Building preview…", flush=True)
                build = subprocess.run(["cargo", "build", "--locked"], cwd=ROOT)
                if build.returncode == 0:
                    if preview and preview.poll() is None:
                        preview.terminate()
                        try:
                            preview.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            preview.kill()
                            preview.wait()
                    preview = subprocess.Popen([str(ROOT / "target" / "debug" / "open-typeless"), "--preview", *sys.argv[1:]], cwd=ROOT, env=env)
                    print("Preview running. It will restart after the next successful build.", flush=True)
                else:
                    print("Build failed; keeping the previous preview open until the next change.", flush=True)
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        if preview and preview.poll() is None:
            preview.terminate()
            preview.wait()


if __name__ == "__main__":
    main()
