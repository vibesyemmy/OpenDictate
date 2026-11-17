# Microphone fallback and two-key shortcuts

## Goal
Keep dictation and meeting capture working when a chosen microphone disappears, and allow global shortcuts consisting of two ordinary keys such as M + Space, as well as existing modifier shortcuts.

## Microphone fallback
At startup, if a selected input is missing or cannot be opened, try the system default and report which device is in use. During dictation, detect disconnection, retain the samples already captured, reopen the current system default, and join audio at a common sample rate. During meetings, detect the removal and rebuild the system-audio/microphone capture without discarding pending audio or resetting the transcript timeline. Notify the user if fallback fails, and do not claim capture is working when it is not. Keep the original preference so reconnecting it works on a later recording.

## Shortcuts
Keep the existing global shortcut implementation for modifier combinations and function keys. Add a macOS global key listener for ordinary two-key chords with key-down/up tracking, correct hold-to-talk and toggle behavior, conflict detection, and a visible permission/error state. Capture both keys in the shortcut editor without prematurely saving the first. Avoid recording or storing typed text. When both keys are held together, suppress the chord so it does not type into the active app. Either key pressed normally on its own must still type normally. Buffer and replay candidate key events when no chord forms, maintaining key order and ordinary typing behavior; handle focus changes, repeats and listener shutdown without lost or stuck keys.

## Verification
Use automated tests for device selection and recovery decisions, sample continuity and sample-rate changes, shortcut parsing, key order, repeat, release, conflicts, and existing shortcuts. Build and test the app. Verify macOS permission handling and actual unplug/reconnect and keyboard behavior on real hardware; report any hardware validation that cannot be performed.
