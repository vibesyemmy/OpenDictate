# Talking Points: design

A floating window that runs alongside a meeting and shows, as the meeting goes
on, a short summary, action points and, when something calls for it,
suggested replies and follow-up questions. It builds on Meeting mode
(`docs/meeting-capture.md`, `src/meeting.rs`).

## Why

During a call there's no time to take notes and think up a good reply at the
same time. A local text model fast enough to keep up (Phi 4 Mini at ~37 tok/s
on the author's Mac) can do the note-taking and offer replies while the
meeting is still running, and nothing leaves the Mac unless the person
chooses to search.

## Goals

- While a meeting runs, show a live summary and action points that keep up
  with what is being said.
- When someone asks the user something, or a decision or commitment comes up,
  offer suggested replies and follow-up questions within about 3–6 s of the
  speaker pausing.
- Let the user select words in the transcript and search them on the web, with
  the result fed to the model as context.
- Ship in the DMG and work on other people's Macs, with whatever text server
  they already use.

## Non-goals (this version)

- No context from outside the meeting: no agendas, notes, documents or
  calendar. Only what has been said, plus searches the user runs.
- No model deciding when to search. Search is always started by the user.
- No automatic highlighting of terms to search. The user selects words.
- No word-by-word live transcript. One sentence at a time, after each pause, is
  enough.
- No talking to the meeting app (Zoom, Meet…) beyond the audio Meeting mode
  already captures.

## Anti-goals: what would make it a failure even if it works

- **Wrong suggestions.** A confident, irrelevant suggestion is worse than
  none. When nothing clearly calls for a suggestion, show nothing.
- **Late suggestions.** A reply that shows up after the moment has passed is
  useless. The latency targets below are part of the feature, not polish.
- Sending meeting content off the Mac without the user asking. With a local
  text model, only the words the user chooses to search are sent, and only to
  Brave. A cloud text model gets the transcript only after the explicit
  opt-in (section 5).

## Who decided what

Decided by the user:
- Layout B: one wide floating window, transcript on the left, talking points
  on the right, a context strip along the bottom.
- Suggestions (replies **and** follow-up questions) stay hidden behind a
  notification until there are some, and clicking it opens them.
- Search: the user selects words in the transcript, a menu offers "Search",
  the app calls the **Brave Search API**, and the result is added to the
  model's context. Context must be managed carefully.
- Pause-based transcription (a sentence lands after each pause) instead of
  30-second chunks.
- Works with **any text server** the app already supports (Ollama or an
  OpenAI-compatible server such as LM Studio).
- On Macs with **less than 16 GB of memory, Talking Points does not run** with
  a local text model. With an opted-in cloud text model, the rule doesn't
  apply.
- Distributed to other people via the DMG.

Proposed by Claude and accepted without objection (the user said "this is fine
for this system" to the pipeline playback):
- Whisper kept loaded as a server, cut-at-pause transcription, Phi returning a
  fixed JSON shape, skip-to-latest instead of queuing, `lms` auto-start, the
  context bundle shape, Brave key in Models.

Claude's calls, which the user hasn't confirmed (see the end):
- Talking points saved next to the transcript when the meeting ends.
- Exact thresholds (pause length, max stretch, chip caps, window sizes).

## How it works

### 1. Transcription: cut at pauses, not every 30 s

Today `meeting::run` cuts audio into 30 s chunks (`CHUNK_SECS`) and launches
`whisper-cli` per chunk, which reloads the model every time. Worst case, a
question reaches the model ~40 s after it was asked.

Change:
- **Cut at pauses.** Track the level of the chosen side (`you` / `them` /
  `both`, as today). When speech is followed by ~500 ms below a silence
  threshold, cut there. Force a cut after ~15 s of nonstop speech. Drop
  stretches that are all silence instead of transcribing them.
- **Keep Whisper loaded** for the local engine: run whisper.cpp's
  `whisper-server` (next to the configured `whisper-cli`, or found the same
  way `find_whisper_bin` does), started when a meeting starts and stopped when
  it ends, and POST each stretch to it. If no server binary can be found, fall
  back to today's `whisper-cli`-per-stretch path, which is slower but keeps
  working.
- **Cut on a change of speaker too** *(Claude's call)*. In turn-taking the
  combined signal never goes quiet: "them" stops and "you" start straight
  away. So the cutter watches each side on its own and also cuts when the
  speaking side changes (them → you, or you → them). Each stretch is labelled
  with the side that was speaking, which keeps who asked whom. That turn
  change is also when suggestions should fire.
- **Rework the made-up-text guard** *(Claude's call)*. The current
  `secs < 5.0 && peak < 0.10` filter in `meeting.rs` would drop short real
  replies such as "Yes, Friday works." once stretches get short. Replace it
  with a check that still blocks the hallucinations it was tuned for (e.g.
  based on how much of the stretch was actually voiced, plus whisper's usual
  silence phrases). Tests should cover both a short real reply and a
  near-silent stretch.
- The remote speech engine (`transcribe_remote`) gets the same pause-cut
  stretches.
- The output format doesn't change: each stretch is one `Segment` appended to
  `transcript.jsonl`, so Meetings, history and end-of-meeting summarising keep
  working. Segments just get shorter.

Target: a sentence shows up ~1 s after the speaker pauses.

This step is useful without Talking Points (a live transcript that keeps up)
and should ship first.

### 2. The talking-points loop

**Two calls, not one** *(Claude's call)*. At ~37 tok/s, a single reply with
summary, actions and suggestions is about 200+ tokens, roughly 5 s of
output, which would miss the target. So:
- **Suggestions call:** runs after every new segment, preferably at a change
  of speaker. It's short: it returns only `suggestions` (often empty), so it's
  fast. This is the call held to the 3–6 s target.
- **Notes call:** refreshes `summary` and `actions` less often, about every
  30–60 s or after several new segments, and never at the same time as a
  suggestions call. Suggestions get priority.

Both go through the existing `engines::chat_as` path (Ollama or
OpenAI-compatible). For each call, if segments arrive while one is running,
the app keeps only the newest pending call and skips the rest. It never
queues.

**The app owns the notes** *(Claude's call)*. The app keeps the action list
and the summary, not the model:
- The current action list (short) goes into the notes call.
- The model returns the updated summary plus **new or changed** actions. The
  app merges them into its list and removes duplicates.
- "NEW" means added since the last update the user saw. Existing items keep
  their order.
- "So far" covers the whole meeting. The summary may grow to about 5–8
  sentences / bullets for long meetings, which still fits the token budget.

**Context bundle**, in this order, so the start stays stable and the server
can reuse prompt work it has already done:
1. Fixed instructions (role, rules, output shape).
2. Pinned search results (capped, see Search).
3. Rolling summary of everything before the recent window (the model's own
   last `summary`).
4. Recent transcript word for word: roughly the last 3 minutes, with `you` /
   `them` labels from `Segment.voice`.

Aim to keep the bundle under ~3k tokens. Small models get worse and slower as
the prompt grows, and on base chips reading the prompt is the slow part.

For the suggestions call, keep items 1–3 identical between calls and put the
fast-changing transcript last, so the server can reuse the prompt work it
already did on the start of the prompt.

**Output**: a fixed JSON shape per call, enforced with structured output /
JSON schema where the server supports it, and parsed leniently where it
doesn't:

```json
// notes call
{ "summary": "string", "actions": [{ "who": "string", "what": "string" }] }

// suggestions call
{
  "reason": "string, why now, e.g. 'You were asked for an estimate by Friday'",
  "items": [{ "kind": "reply" | "ask", "text": "string" }]
}
```

The instructions require `suggestions.items` to be empty unless something
clearly calls for one: a question aimed at the user, a decision, or a
commitment. Suggestions may only use the transcript and the pinned search
results.

If the model's reply doesn't parse, keep the last good state on screen and
log it.

Target: suggestions visible ~3–6 s after the pause, on the recommended
hardware.

### 3. The window

A separate always-on-top egui viewport, opened when a meeting starts (if
Talking Points is enabled and allowed on this Mac) and closed when it stops.
Mockups (layout B and the suggestions flow) are in
`.superpowers/brainstorm/72534-1794290861/content/talking-points-layout.html`
(option B) and `talking-points-suggestions.html` in the same folder.

- Title bar: recording dot, elapsed time, "updated Ns ago".
- **Left:** live transcript with `YOU` / `THEM` labels. Text can be selected.
  Selecting text shows a small menu with **Search "…"** and **Copy**.
- **Right, always visible:** **So far** (summary) and **Action points**.
  Items that are new since the last update get a small "NEW" marker. Items
  already on screen shouldn't jump around while the user is reading.
- **Right, suggestions:** when `suggestions.items` is non-empty, a
  notification shows the count and `reason`. Clicking it opens a panel listing
  each item labelled REPLY or ASK, with a copy button, and a close button.
  When later updates bring no suggestions, an open panel stays as it is until
  the user closes it.
- **Bottom: Context strip:** one chip per pinned search result, each removable
  with ✕.
- Follow the app's existing look (`theme.rs`, `macos_look.rs` backdrop).

### 4. Search

- Selecting words and choosing **Search** calls the Brave Search API
  (`ureq`, already a dependency) with exactly the selected text.
- Keep the top ~3 results, trimmed to title + snippet, ~400 tokens total per
  search. Add them as one chip labelled with the search text.
- Cap at ~4 chips. Adding a fifth drops the oldest. Removing a chip removes it
  from the next bundle.
- **Key:** each person enters their own Brave key in Models. Without a key,
  the Search item is greyed out and says why.
- Errors (no network, bad key, rate limit) show on the chip or as a short
  message. They never block the window.

### 5. Running the models

- **Text model:** whatever the user has set as the text engine. Phi 4 Mini
  Instruct is recommended in the Models page, but not required. Use
  `meeting_model` if it's set, otherwise the clean-up model (that's the
  builder's call, see below).
- **LM Studio convenience:** if the text engine points at LM Studio and `lms`
  is installed (`~/.lmstudio/bin/lms` or on `PATH`), the app starts the server
  (`lms server start`) and loads the model (`lms load <model>`) when a meeting
  starts, if they aren't already running. Otherwise it changes nothing.
- **Cloud text servers need an explicit opt-in** (user's decision). If the
  text engine's address isn't on this Mac (not `localhost` / `127.0.0.1` /
  `::1`), Talking Points stays off until the user turns on a separate
  setting. That setting shows a clear warning: the meeting transcript is
  sent to that server every few seconds while Talking Points runs. The opt-in is
  tied to that host, so changing the address asks again. Without it, the
  window shows the transcript and explains why talking points are off.
  Cloud speech (`transcribe_remote`) is out of scope for this version.
- **Unreachable text model:** the window still shows the transcript, and the
  talking-points side says plainly that no text model is reachable and links
  to Models. Recording carries on.
- **Whisper:** see section 1.

### 6. Hardware gate

- Requirements: macOS 14.2+ (already needed by Meeting mode's process taps).
  Apple Silicon is in practice needed for local models fast enough to keep up.
- The 3–6 s target applies to the recommended hardware. On a base 16 GB Mac,
  late suggestions don't block shipping *(Claude's call)*.
- **With a local text model, under 16 GB of physical memory (`hw.memsize`),
  Talking Points doesn't run.** The setting is shown disabled with a reason
  ("Needs 16 GB of memory to run the text model on this Mac"). Dictation and
  meeting recording still work.
- **With a cloud text model (opted in, section 5), the 16 GB rule doesn't
  apply.** Only Whisper runs locally, which Meeting mode already does on
  these Macs.
- Recommended: a Pro/Max chip with 32 GB and Phi 4 Mini at 8-bit. A base chip
  with 16 GB and a 4-bit model should work with somewhat later suggestions.
  This is an estimate: time it on a real base machine before promising it in
  release notes.

### 7. When the meeting ends

The last talking-points state (summary, actions, suggestions shown, search
chips) is saved as `talking_points.json` in the meeting's folder, next to
`transcript.jsonl`, and deleted with the meeting. *Claude's call, not
confirmed by the user.* Showing it on the Meetings page is out of scope for
this version.

## Settings added

- Talking Points on/off (default on where allowed, disabled under 16 GB with a local text model).
- "Send meeting transcripts to `<host>`" opt-in, shown only for a cloud text
  server.
- Brave Search API key.

## Suggested PR order

1. Pause-based cutting + `whisper-server` in Meeting mode (no UI change beyond
   faster transcript).
2. Talking-points loop + context bundle + JSON parsing, behind the setting,
   with the hardware gate (state only, no window yet, or a minimal one).
3. The floating window (layout B, suggestions notification).
4. Brave search + context chips.
5. `lms` auto-start.

## Left to the builder

- Exact silence threshold and pause length. Tune on real meetings.
- Choosing between `meeting_model` and the clean-up model, and how the prompt
  is worded.
- How text selection is done in egui.
- How the window is created (`egui` viewport vs. an extra NSWindow), as long as
  it floats above other apps.
- Window size/position memory.
- Tests: per the repo's TDD rule. The pause cutter, the bundle builder (order
  and caps), JSON parsing (including bad output) and the skip-to-latest logic
  are pure enough to test without audio or a model.
