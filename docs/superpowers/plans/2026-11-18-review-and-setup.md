# Reviewable meeting notes and guided first use

1. Add tests for backward-compatible, atomic edits to saved summaries and titles; ensure a user-edited summary is not silently replaced by regeneration.
2. Add a review surface for overview, decisions, actions, questions and points, with editable/removable items and honestly labeled possible transcript matches. Offer saved clip playback only when the matching clip exists.
3. Provide a first-use setup path showing the speech engine and recommended model, progress, microphone test, permissions and practice; preserve advanced provider configuration and existing settings.
4. Ensure release packaging has a usable speech engine or clearly reports its absence. Test the setup against a clean data directory and verify dependencies rather than assuming Homebrew exists.
5. Test navigation, cancellation, persistence, errors, empty/no-audio states and existing regression suite; inspect relevant screens in Paper & Ink. Do not modify the owner's stored meetings.
