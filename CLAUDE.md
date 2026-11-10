# Meeting capture: decisions

- Stop the device before the final drain, or every pass picks up another quarter second and writes another stretch. The first version wrote 133 of them.
- Transcribe one side, not the mix. On speakers the microphone hears the call a few milliseconds late, so a mixed track carries everything twice and whisper sometimes writes it twice.
- The two channels can become identical mid-meeting when a microphone drops out of the aggregate. Measure that rather than trusting the channel count; the stretch is then "both".
- One transcription worker. Transcription is the slow part, and several at once starve the machine the meeting is on.
- The guard thresholds come from measured cases: speech at peak 0.43, noise tails at 0.036 and 0.021 that hallucinated a word and a whole sentence, quiet real speech at peak about 0.06 (fixtures in `tests/fixtures`). Short tails have to be obviously speech, because that is where the inventions live.
- Delete each stretch's audio once it is transcribed: it is 1.9 MB a minute and a week of meetings would fill a disk.
- A frame is voiced only above 1.5 times the room's noise floor on its side, and only in runs of 60 ms or more. A fixed threshold sat inside fan and HVAC levels (rms ~0.005), so every frame was voiced and whisper got noise every 15 s; single-frame typing clicks used to add up to enough voiced time to pass the guard.
