# Two-channel meeting attribution Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement task-by-task.

**Goal:** Preserve you/them/both capture while removing individual remote-speaker identification.

**Architecture:** Keep capture, cutter, transcript voice fields and legacy audio sweeping. Remove diarization, its settings and views, background worker and dependencies. Preserve existing JSON data without migration or deletion.

**Tech Stack:** Rust, egui, Core Audio, serde.

- [ ] Add regression checks for legacy settings and ordinary audio retention; run them red.
- [ ] Remove speaker identification execution, controls and model dependencies; preserve old metadata and audio safety. Simplify transcript display to channel labels.
- [ ] Run focused and full tests, build, and inspect changes. State live-call verification limitation.
