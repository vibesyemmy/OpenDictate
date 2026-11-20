# Meeting detail and synchronized audio

- Preserve meeting data, corrections, recovery, summary regeneration, and the confirmed delete flow.
- Derive saved audio ranges from existing timestamped WAV clips; keep gaps and first-clip offsets explicit.
- Add a seekable, pausable macOS player reporting its actual playback position; prepare audio off the UI thread and clean temporary files.
- Replace the detail layout with a meeting header, Summary and Transcript tabs, a persistent player, and a readable transcript that indicates the currently playing saved passage.
- Make timestamps seek only when corresponding audio exists; retain transcript correction as a separate action and do not imply word-level timing.
- Test clip gaps and timeline offsets, playback transitions, existing data compatibility, detail UI states and navigation. Verify with synthetic meeting data, light/dark views, and appropriate app tests.
