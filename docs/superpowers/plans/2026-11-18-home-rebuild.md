# Home screen rebuild

- Build a testable Home view based on the approved sketch: equal action cards, setup/recovery notice, compact actual statistics and multi-month heatmap without an extra activity heading.
- Route actions through existing dictation, meeting, setup, recovery and settings flows. Respect in-progress recording and unavailable speech models; never silently stop a meeting.
- Cache actionable meeting failures when meetings are refreshed; update after retry/discard/delete and on Home entry. Do not access meeting content in the Home view.
- Keep configured shortcuts and real estimates; distinguish optional text processing, first-time state and checking/unavailable models. Follow Paper & Ink in both themes.
- Test state transitions, actions, notice placement, equal widths, heatmap calendar and existing regressions. Inspect light/dark and populated/first-time screens using isolated examples; do not modify the user's data.
