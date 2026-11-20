# Visible meeting capture

- Add tests for capture status: distinguish starting, sound, silence, stalled callbacks, missing microphone, and unseparated channels; audio activity must not imply speaker identity.
- Read independent channel activity directly from the audio callback so indicators stay current while speech transcription runs. Reset the monitor on microphone recovery and expose uncertainty when the layout cannot be separated.
- Add a Start meeting action and compact preflight to the empty and populated Meetings list. Let users indicate when other people are heard through the microphone; persist that choice and label microphone-only segments as mixed, including in model prompts.
- Show live activity and actionable, non-alarming guidance on the meeting screen, including when either channel is quiet. Preserve first-use introduction and current recording paths.
- Run focused tests, the regression suite, and release build; inspect screens with synthetic or isolated data without recording or modifying user meetings.
