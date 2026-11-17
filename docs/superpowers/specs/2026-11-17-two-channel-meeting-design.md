# Two-channel meeting attribution

## Intent
Return meeting attribution to the microphone side (you) and the system-audio side (others), retaining both/unknown where the capture cannot reliably separate them. Do not count, cluster, name, or infer individual remote speakers.

## Approach
Keep the existing Core Audio capture, pause cutter and transcript voice labels. Remove the later individual speaker identification workflow and its controls rather than reverting whole commits, which would also remove later fixes. Preserve existing transcripts, summaries, saved speaker data and explicitly retained audio; old transcripts display their stored two-side labels. Preserve the existing sweep for audio held temporarily by older versions for labelling, without touching deliberately retained audio.

## Verification
Cover legacy settings that previously enabled speaker identification, audio retention, and rendering of the two-side labels. Run relevant capture, meeting and UI tests and the full feasible suite. Live capture must be verified separately on a real call; simultaneous speech and speaker playback bleed can make side attribution uncertain.
