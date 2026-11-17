# Microphone Fallback and Key Chords Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans. Implement the approved design incrementally with tests.

**Goal:** Continue active dictation and meeting capture on the system microphone after a selected input disappears; support two ordinary keys as a global shortcut without breaking ordinary typing.

**Architecture:** Retain recording samples across input streams and rebuild meeting capture when the source disappears, preserving elapsed offsets. Add a macOS event-tap chord state machine that buffers/replays candidate key events, alongside existing modifier-key shortcuts. Explicitly report missing permissions and fallback failures.

**Tech Stack:** Rust, egui, cpal, Core Audio and macOS Core Graphics.

- [ ] Write failing tests for device fallback selection, audio continuity and recovery; implement and run focused tests.
- [ ] Write failing tests for chord parsing, press/release, candidate buffering/replay, key repeat, conflicts and capture; implement listener and UI and run focused tests.
- [ ] Run full suite and build; validate real-device disconnect and global shortcut behavior where possible, reporting limits.
