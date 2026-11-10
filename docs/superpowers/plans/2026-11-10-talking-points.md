# Talking Points Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A floating window that shows a live summary, action points and, when called for, suggested replies/questions during a meeting, with user-initiated Brave search fed back as context.

**Architecture:** Meeting mode moves from 30 s chunks to pause/turn-based stretches transcribed by a long-lived `whisper-server`. A new `talking` module builds a capped context bundle, schedules two kinds of model call (fast suggestions, slower notes) one at a time, parses lenient JSON, and publishes state into an `Arc<Mutex<…>>` that a new always-on-top egui viewport draws.

**Tech Stack:** Rust 2024, eframe/egui 0.37, ureq 3, serde_json, whisper.cpp (`whisper-server` / `whisper-cli`), any OpenAI-compatible or Ollama text server, Brave Search API, LM Studio `lms` CLI (optional).

**Spec:** `docs/superpowers/specs/2026-11-10-talking-points-design.md`

## Global Constraints

- No new runtime dependencies. `egui_kittest` (version matching egui 0.37) is allowed as a **dev-dependency** only.
- The repo has no tests yet. Tests live in-module as `#[cfg(test)] mod tests`, run with `cargo test`. Every task starts with a failing test (user's outside-in TDD rule).
- No comments that describe code. Intent goes in test names written as behaviour. When moving the hallucination-guard comment in `meeting.rs`, turn its measured cases (peak 0.43 speech, 0.036 and 0.021 noise tails) into test cases.
- Commit messages carry no AI/tool attribution lines of any kind.
- Each task is one PR, green and shippable on its own, in the order below.
- Memory gate: `physical_memory >= 16 * 1024 * 1024 * 1024` bytes, applied **only** with a local text model.
- Local means the text server host is `localhost`, `127.0.0.1` or `::1`.
- Exact copy:
  - Memory: `Needs 16 GB of memory to run the text model on this Mac`
  - Cloud opt-in label: `Send meeting transcripts to <host>`. The warning beneath it: `While Talking Points runs, the meeting transcript is sent to <host> every few seconds.`
  - No Brave key: `Add a Brave Search key in Models to search`
  - Unreachable model: `Can't reach the text model. Check it in Models.`
- Numbers: pause `500 ms`, forced cut `15 s`, recent window `180_000 ms`, bundle cap `12_000` chars (≈3k tokens), chip cap `4`, chip text cap `1_600` chars, Brave results `3`, notes interval `30_000 ms`.

## Review Focus

1. **The text server goes away mid-meeting** (LM Studio quit, Wi-Fi drop on cloud). Recording and transcript carry on, the window shows the unreachable message, and the next successful call clears it. Test in Task 5.
2. **One-sided or unsplittable capture** (mic only, or channels identical). The cutter still cuts at pauses and labels stretches `both`, with no speaker-change cuts. Test in Task 1.
3. **Model output that isn't clean JSON** (code fences, `<think>` blocks, prose before `{`, truncated reply). Parse leniently and keep the last good state on failure. Tests in Task 4 and Task 5.
4. **A two-hour meeting.** The bundle stays under the cap and the action list doesn't fill with duplicates. Tests in Task 4.
5. **Brave failures** (no network, 401, 429). An error chip or message appears, nothing else breaks, and the error never enters the bundle. Test in Task 7.

## File map

| File | Responsibility |
|---|---|
| `src/cut.rs` (new) | Pure pause/turn cutter: samples in, `Stretch`es out |
| `src/meeting.rs` | Uses the cutter; `handle_stretch`; starts and stops whisper-server and talking worker |
| `src/whisper_server.rs` (new) | Find, start, query and stop `whisper-server` |
| `src/talking.rs` (new) | Types, bundle, parsing, merge, scheduler, gate, worker |
| `src/search.rs` (new) | Brave call + response → chip |
| `src/lms.rs` (new) | LM Studio `lms` detection and start/load steps |
| `src/talking_view.rs` (new) | The floating window |
| `src/engines.rs` | `chat_json`, multipart helper extracted |
| `src/settings.rs`, `src/settings_view.rs` | New settings and their controls |
| `src/app.rs`, `src/main.rs` | Wire the viewport; declare modules |

---

### Task 1: Pause and turn cutter (mechanism, unused)

**Files:** Create `src/cut.rs`; Modify `src/main.rs` (add `mod cut;`).

**Interfaces — Produces:**
```rust
pub struct Stretch { pub voice: &'static str, pub samples: Vec<f32>, pub secs: f32, pub voiced_secs: f32 }
pub struct Cutter { /* private */ }
impl Cutter {
    pub fn new(rate: f64) -> Self;
    pub fn push(&mut self, mine: &[f32], theirs: &[f32], split_ok: bool) -> Vec<Stretch>;
    pub fn flush(&mut self) -> Option<Stretch>;
}
pub fn worth_transcribing(peak: f32, rms: f32, voiced_secs: f32) -> bool;
```
`samples` stays at the input rate. It's the chosen side, or the 0.5-weighted mix when both are speaking, which moves the side-choosing logic (2.5× RMS ratio, identical-channel check) out of `meeting::run` unchanged. Work in 20 ms frames. A frame is voiced when its RMS ≥ `0.01`. A stretch ends after 500 ms unvoiced, after 15 s, or when the dominant side changes and stays changed for 300 ms (only when `split_ok` and the channels aren't identical). Stretches with `voiced_secs == 0` are dropped.

- [ ] **Step 1: Write failing tests** in `src/cut.rs` using synthetic audio (`speech(secs)` = 220 Hz sine at amplitude 0.3, `silence(secs)` = zeros, rate 48 000):
  - `cuts_after_half_a_second_of_silence_following_speech`: 2 s them-speech + 0.6 s silence → one stretch, `voice == "them"`, `secs ≈ 2.0..2.6`.
  - `does_not_cut_on_a_pause_shorter_than_half_a_second`: 1 s + 0.3 s silence + 1 s, then flush → exactly one stretch.
  - `forces_a_cut_after_fifteen_seconds_of_nonstop_speech`: 20 s speech → first stretch `secs ≈ 15.0`.
  - `cuts_when_the_speaker_changes_from_them_to_you`: 2 s them then 2 s you, no gap → two stretches, voices `["them", "you"]`.
  - `drops_stretches_that_are_all_silence`: 5 s silence + flush → none.
  - `labels_both_and_ignores_turns_when_sides_cannot_be_split`: same input as the turn test with `split_ok=false` → one stretch, `voice == "both"`.
  - `flush_returns_speech_still_in_progress_when_the_meeting_ends`.
  - `worth_transcribing` cases: `(0.43, 0.05, 3.0)` → true, `(0.036, 0.003, 0.0)` → false, `(0.021, 0.002, 0.0)` → false, `(0.08, 0.01, 1.2)` → true (a quiet short reply such as "Yes, Friday works"), `(0.02, 0.01, 2.0)` → false.
- [ ] **Step 2:** `cargo test cut::` → FAIL (not defined).
- [ ] **Step 3:** Implement. Guard rule: `peak >= 0.03 && rms >= 0.002 && (peak >= 0.10 || voiced_secs >= 1.0)`.
- [ ] **Step 4:** `cargo test cut::` → PASS, and `cargo build` is warning-free apart from `dead_code` on the new items (allow it at module level until Task 2).
- [ ] **Step 5:** Commit `Add a pause and turn cutter for meeting audio`.

### Task 2: Meeting mode uses the cutter

**Files:** Modify `src/meeting.rs` (`run`, remove `CHUNK_SECS`, extract `handle_stretch`).

**Interfaces:**
- Consumes: Task 1.
- Produces: `fn handle_stretch(dir: &Path, index: usize, at_ms: u64, st: &cut::Stretch, rate: f64, transcribe: &dyn Fn(&[u8]) -> Result<String, String>) -> Result<Option<Segment>, String>`. It does the resample, WAV write, guard, transcription, `append_segment` and WAV delete. `Ok(None)` means it was skipped, `Err` means transcription failed.

- [ ] **Step 1: Failing tests** (temp dir under `std::env::temp_dir()`):
  - `appends_a_segment_with_the_stretch_voice_to_the_transcript`: a fake transcriber returns `"hello"`. `transcript.jsonl` gets one line with `text == "hello"` and `voice == "them"`, and `audio/000.wav` is gone afterwards.
  - `skips_a_stretch_too_quiet_to_be_speech_without_calling_whisper`: the fake panics if called. Returns `Ok(None)`.
  - `reports_a_transcription_failure_and_still_deletes_the_audio`.
- [ ] **Step 2:** `cargo test meeting::` → FAIL.
- [ ] **Step 3:** Implement `handle_stretch`, and make `run` feed `cap.drain()` into `Cutter::push(…, cap.split_ok())`, calling `handle_stretch` with `|wav| engines::transcribe(&s, wav)`. On `ending`, use `flush()`. Keep the `Live` updates (`peak`, `chunks`, `heard_you`, `warning`) and the per-stretch `log` lines.
- [ ] **Step 4:** `cargo test` → PASS. Manual check: `cargo run -- --meeting` (the existing headless path in `main.rs:122`) for about 1 minute of talking. Segments should appear within ~1–3 s of each pause, and `log.txt` should show stretch lengths.
- [ ] **Step 5:** Commit `Cut meeting audio at pauses and turns instead of every 30 seconds`.

### Task 3: Keep Whisper loaded with whisper-server

**Files:** Create `src/whisper_server.rs`; Modify `src/engines.rs` (extract `fn multipart(fields: &[(&str, &str)], wav: &[u8]) -> (String /*content-type*/, Vec<u8>)` from `transcribe_remote`), `src/meeting.rs` (`Session` owns `Option<WhisperServer>`).

**Interfaces — Produces:**
```rust
pub fn find_server_bin(whisper_cli: &str) -> Option<PathBuf>;   // sibling `whisper-server` of the resolved cli, then PATH
pub struct WhisperServer { /* child, port */ }
impl WhisperServer {
    pub fn start(s: &Settings) -> Result<WhisperServer, String>; // -m <model> --host 127.0.0.1 --port <free>; waits ≤30 s for GET / == 200
    pub fn transcribe(&self, s: &Settings, wav: &[u8]) -> Result<String, String>; // POST /inference, file + response_format=json [+ language, prompt]; reads `text`, runs clean_transcript
}
impl Drop for WhisperServer { /* kill + wait */ }
```
`meeting::run` uses `server.transcribe` when a server exists, and otherwise `engines::transcribe`. If a server call fails, that stretch falls back to `engines::transcribe`, and the server stays dropped for the rest of the meeting (logged).

- [ ] **Step 1: Failing tests:**
  - `finds_whisper_server_next_to_the_configured_cli`: temp dir with executable files `whisper-cli` and `whisper-server`, configured path is the cli → returns the server path.
  - `returns_none_when_no_server_binary_exists`.
  - `multipart_body_carries_fields_and_wav` (the extracted helper): the body contains `name="response_format"`, then `json`, then the WAV bytes, ending with the closing boundary.
  - `falls_back_to_whisper_cli_when_the_server_request_fails`: test the selection function `fn transcribe_stretch(server: &mut Option<WhisperServer>, …)` with a server pointed at a closed port. The fake cli is called, and `server` is `None` afterwards.
  - `#[ignore] transcribes_a_wav_through_a_real_server`: needs `WHISPER_MODEL` env and a fixture WAV.
- [ ] **Step 2:** `cargo test whisper_server:: engines::` → FAIL.
- [ ] **Step 3:** Implement. `transcribe_remote` must keep behaving the same through the extracted helper.
- [ ] **Step 4:** `cargo test` → PASS. `WHISPER_MODEL=… cargo test -- --ignored transcribes_a_wav` → PASS (manual, on a dev Mac).
- [ ] **Step 5:** Commit `Keep whisper loaded for the length of a meeting`.

### Task 4: Talking Points core (pure, unused)

**Files:** Create `src/talking.rs`; Modify `src/settings.rs` (fields), `src/main.rs` (`mod talking;`).

**Interfaces — Produces:**
```rust
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)] pub struct Action { pub who: String, pub what: String }
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)] #[serde(rename_all = "lowercase")] pub enum Kind { Reply, Ask }
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)] pub struct Suggestion { pub kind: Kind, pub text: String }
#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)] pub struct Suggestions { pub reason: String, pub items: Vec<Suggestion> }
#[derive(Clone, Serialize, Deserialize, Default, Debug)] pub struct NotesReply { pub summary: String, pub actions: Vec<Action> }
#[derive(Clone, Serialize, Deserialize, Debug)] pub struct Chip { pub query: String, pub text: String }
#[derive(Clone, Serialize, Deserialize, Debug)] pub struct Tracked { pub action: Action, pub new: bool }

pub const SUGGEST_SYSTEM: &str; pub const NOTES_SYSTEM: &str;
pub fn suggestions_schema() -> serde_json::Value; pub fn notes_schema() -> serde_json::Value;
pub fn bundle(chips: &[Chip], summary: &str, actions: &[Tracked], segments: &[Segment], now_ms: u64) -> String;
pub fn parse_suggestions(raw: &str) -> Result<Suggestions, String>;
pub fn parse_notes(raw: &str) -> Result<NotesReply, String>;
pub fn merge_actions(list: &mut Vec<Tracked>, incoming: Vec<Action>);
pub fn add_chip(chips: &mut Vec<Chip>, chip: Chip);
pub fn is_local(url: &str) -> bool;
#[derive(Debug, PartialEq)] pub enum Availability { Ready, Disabled, NoTextModel, NeedsMemory, NeedsCloudOptIn { host: String } }
pub fn availability(s: &Settings, physical_memory: u64) -> Availability;
pub fn physical_memory() -> u64; // `sysctl -n hw.memsize`, 0 on failure
```
Settings additions (all `#[serde(default)]`): `talking_points: bool` (default `true`), `talking_cloud_host: String` (the host the user opted in for), `brave_key: String`.

The bundle is the user message, in fixed order: `Search results:` (chips), `So far:` (summary), `Action points:`, `Recent transcript:` (segments with `at_ms >= now_ms - 180_000`, one per line as `[you] …` / `[them] …` / `[both] …`). When the result is over 12 000 chars, drop the oldest transcript lines first. `SUGGEST_SYSTEM` must say that `items` is empty unless the latest lines contain a question aimed at `[you]`, a decision, or a commitment, and that only the transcript and search results may be used. Wording is the implementer's call. `merge_actions` dedupes on lowercased, whitespace-collapsed `what`, marks newly added items `new = true`, and never reorders existing items. `parse_*` runs `engines::strip_thinking` (make it `pub(crate)`), strips code fences, takes the outermost `{…}`, then `serde_json`.

- [ ] **Step 1: Failing tests:**
  - `bundle_puts_search_results_then_summary_then_actions_then_recent_transcript` (assert the index order of the four headings).
  - `bundle_labels_each_line_with_who_spoke`.
  - `bundle_leaves_out_transcript_older_than_three_minutes`.
  - `bundle_stays_under_the_cap_in_a_two_hour_meeting`: 2 000 segments of 100 chars → `len <= 12_000`, and the newest line is present.
  - `parses_suggestions_wrapped_in_code_fences_and_thinking`.
  - `parse_fails_on_truncated_json` (an `Err`, not a panic).
  - `merging_the_same_action_twice_keeps_one_and_marks_only_the_first_as_new`.
  - `merging_keeps_existing_actions_in_place`.
  - `items_marked_new_lose_the_mark_on_the_next_merge`.
  - `adding_a_fifth_chip_drops_the_oldest`, and `chip_text_is_cut_to_1600_chars`.
  - `availability` cases: local Ollama with 8 GiB → `NeedsMemory`; local with 16 GiB → `Ready`; `https://api.openai.com/v1` with 8 GiB and no opt-in → `NeedsCloudOptIn{host:"api.openai.com"}`; same with `talking_cloud_host = "api.openai.com"` → `Ready`; opted in for `api.groq.com` but the URL is OpenAI → `NeedsCloudOptIn`; `TextEngine::Off` → `NoTextModel`; `talking_points=false` → `Disabled`.
  - `is_local` for `http://localhost:1234/v1`, `http://127.0.0.1:11434`, `http://[::1]:8080` → true; `https://api.groq.com` → false.
- [ ] **Step 2:** `cargo test talking::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` → PASS.
- [ ] **Step 5:** Commit `Add the Talking Points bundle, parsing and availability rules`.

### Task 5: Talking Points worker, settings controls, saved state

**Files:** Modify `src/talking.rs` (scheduler, live state, worker), `src/engines.rs` (`chat_json`), `src/meeting.rs` (`Session` starts and stops the worker), `src/settings_view.rs` (controls in `models_tab`).

**Interfaces:**
- Consumes: Tasks 2 and 4.
- Produces:
```rust
// engines.rs: like chat_as, adds response_format {type:"json_schema", json_schema:{name, schema, strict:true}}; on a 4xx retries once without it
pub fn chat_json(s: &Settings, model_override: Option<&str>, system: &str, user: &str, name: &str, schema: &Value) -> Result<String, String>;

// talking.rs
#[derive(Clone, Copy, PartialEq, Debug)] pub enum Job { Suggest, Notes }
pub struct Scheduler { /* private */ }
impl Scheduler {
    pub fn new() -> Self;
    pub fn on_segment(&mut self);
    pub fn next(&mut self, now_ms: u64) -> Option<Job>;  // None while a job runs
    pub fn done(&mut self, job: Job, now_ms: u64);
}
#[derive(Default)] pub struct Live {
    pub summary: String, pub actions: Vec<Tracked>, pub suggestions: Option<Suggestions>,
    pub chips: Vec<Chip>, pub status: Option<String>, pub updated_ms: Option<u64>, pub availability: Option<Availability>,
}
pub fn step(live: &Mutex<Live>, sched: &mut Scheduler, segments: &[Segment], now_ms: u64,
            chat: &dyn Fn(Job, &str /*system*/, &str /*user*/) -> Result<String, String>);
pub fn spawn(s: Settings, meeting: Arc<Mutex<meeting::Live>>, stop: Arc<AtomicBool>) -> Arc<Mutex<Live>>;
pub fn save(dir: &Path, live: &Live);  // writes talking_points.json
```
Scheduler rules: one job at a time. `Suggest` is due after any `on_segment` since the last suggest started, and segments that arrive while it runs fold into one more run. `Notes` is due once `now - last_notes >= 30_000` and at least one segment has arrived since then. When both are due, `Suggest` goes first. `step` sets `suggestions` only when `items` is non-empty, and keeps the last good state plus a `status` on a parse or chat error. A chat error sets status to the unreachable copy, and the next success clears it. `spawn` checks `availability(&s, physical_memory())` first. If it isn't `Ready`, it stores that and makes no calls. Otherwise it loops every 200 ms until `stop`, calling `step` with `chat_json` (`meeting_model` override when set). `Session::start` spawns it when `availability` is `Ready`, and also stores the `Arc` in `Session.talking: Option<Arc<Mutex<talking::Live>>>`. `finish` calls `save`.

Settings controls in `models_tab`, under the text engine: a "Talking Points" switch, disabled with the memory copy when the gate says `NeedsMemory`. For a non-local text URL, a checkbox with the opt-in label and warning copy that sets or clears `talking_cloud_host` to the current host.

- [ ] **Step 1: Failing tests:**
  - Scheduler: `a_new_segment_starts_a_suggestion`, `segments_arriving_during_a_suggestion_fold_into_one_more_run`, `never_starts_a_second_job_while_one_runs`, `notes_wait_thirty_seconds_between_runs`, `suggestions_go_before_notes_when_both_are_due`.
  - `step` with a fake chat: `a_question_to_you_shows_suggestions_with_their_reason`; `an_empty_suggestion_reply_leaves_the_last_suggestions_alone`; `unparseable_output_keeps_the_last_good_notes_and_sets_a_status`; `an_unreachable_model_shows_the_message_and_the_next_success_clears_it` (Review Focus 1); `the_notes_reply_merges_actions_instead_of_replacing_them`.
  - `spawn_makes_no_calls_when_talking_points_is_unavailable` (8 GiB local settings: `Live.availability == Some(NeedsMemory)`).
  - `save_writes_talking_points_json_next_to_the_transcript`, and `deleting_a_meeting_removes_its_talking_points` (via `meeting::delete`).
  - `chat_json_retries_without_a_schema_when_the_server_rejects_it`: a local `TcpListener` fake server answers 400 then 200.
- [ ] **Step 2:** `cargo test talking:: engines::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` → PASS. Manual check: with LM Studio serving Phi 4 Mini, run `cargo run -- --meeting`, ask a question aloud from another device, stop, and read `talking_points.json`.
- [ ] **Step 5:** Commit `Run Talking Points during meetings and save them with the meeting`.

### Task 6: The floating window

**Files:** Create `src/talking_view.rs`; Modify `src/app.rs` (show the viewport while `self.meeting` has `talking`), `Cargo.toml` (`[dev-dependencies] egui_kittest`).

**Interfaces:**
- Consumes: `talking::Live`, `meeting::Live`.
- Produces:
```rust
pub struct ViewState { pub panel_open: bool, pub seen_suggestions: usize, pub selection: Option<String> }
pub fn draw(ui: &mut egui::Ui, meeting: &meeting::Live, talking: &talking::Live, vs: &mut ViewState) -> Option<ViewAction>;
pub enum ViewAction { Search(String), Copy(String) }
```
Viewport: `ViewportId::from_hash_of("talking")`, `with_window_level(AlwaysOnTop)`, ~640×460, resizable, the same backdrop treatment as the main window (`macos_look`). Layout B as in the mockup `.superpowers/brainstorm/72534-1794290861/content/talking-points-suggestions.html`:
- Title bar shows elapsed time and "updated Ns ago".
- Left is the transcript in a read-only selectable `TextEdit` with `YOU`/`THEM` labels. A selection shows a menu with `Search “…”` (disabled with the no-key copy when `brave_key` is empty) and `Copy`.
- Right has `So far`, `Action points` with a `NEW` mark, and the suggestions notification (count + reason). Clicking the notification opens a panel of `REPLY`/`ASK` rows with copy buttons and a close button. The panel stays open, unchanged, when a later update has no suggestions.
- Bottom is the context strip of chips with ✕.
- When `availability` isn't `Ready`, or `status` is set, the right side shows that message instead.

The window opens on meeting start and closes on stop.

- [ ] **Step 1: Failing `egui_kittest` tests:** `shows_a_notification_with_count_and_reason_when_suggestions_arrive`, `clicking_the_notification_opens_reply_and_ask_items`, `the_panel_stays_open_when_the_next_update_has_no_suggestions`, `search_is_disabled_with_a_reason_when_there_is_no_brave_key`, `shows_why_talking_points_are_off_when_memory_is_too_low`, `removing_a_chip_returns_it_from_the_view` (or assert via `ViewAction`, implementer's call).
- [ ] **Step 2:** `cargo test talking_view::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` → PASS. Manual check: a real meeting, where the window floats over the call app, selection works and copy works.
- [ ] **Step 5:** Commit `Show Talking Points in a floating window during meetings`.

### Task 7: Brave search into context

**Files:** Create `src/search.rs`; Modify `src/app.rs` (handle `ViewAction::Search` on a thread), `src/settings_view.rs` (Brave key field in `models_tab`).

**Interfaces — Produces:**
```rust
pub fn brave(key: &str, query: &str) -> Result<talking::Chip, String>; // GET https://api.search.brave.com/res/v1/web/search?q=<enc>&count=3, headers Accept: application/json, X-Subscription-Token
pub fn chip_from_response(query: &str, body: &serde_json::Value) -> Result<talking::Chip, String>; // web.results[..3] → "title — description" lines, HTML tags stripped, ≤1600 chars
```
A success calls `talking::add_chip` on the live state. A failure sets a short message in `talking::Live.status` for that search, with the wording: `Search failed: <reason>`, plus for 401 `check the Brave key in Models` and for 429 `Brave rate limit, try again shortly`. Errors never become chips.

- [ ] **Step 1: Failing tests:** `builds_a_chip_from_the_top_three_results_without_html`, `a_response_with_no_results_is_an_error`, `a_401_is_reported_as_a_bad_key_and_adds_no_chip`, `a_429_is_reported_as_a_rate_limit` (a fake `TcpListener` server for the status cases; make the base URL a private `fn brave_at(base: &str, …)` so tests can point at it).
- [ ] **Step 2:** `cargo test search::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` → PASS. Manual check with a real key: select a term, a chip appears, and the next suggestion can use it.
- [ ] **Step 5:** Commit `Search selected words with Brave and add them as meeting context`.

### Task 8: Start LM Studio automatically

**Files:** Create `src/lms.rs`; Modify `src/meeting.rs` (`Session::start` runs it on a thread before the talking worker's first call; the worker's unreachable status covers the wait).

**Interfaces — Produces:**
```rust
pub fn find() -> Option<PathBuf>;                          // ~/.lmstudio/bin/lms, then PATH
pub fn is_lm_studio(s: &Settings) -> bool;                 // OpenAiCompatible + local + port 1234
pub fn steps(server_running: bool, loaded: &[String], model: &str) -> Vec<Vec<String>>; // ["server","start"], ["load", model, "-y"] as needed
pub fn ensure(s: &Settings) -> Result<(), String>;         // status via `lms server status`, `lms ps`; runs steps
```
- [ ] **Step 1: Failing tests:** `starts_the_server_and_loads_the_model_when_neither_is_running`, `only_loads_the_model_when_the_server_is_already_up`, `does_nothing_when_the_model_is_already_loaded`, `is_not_lm_studio_for_ollama_or_a_cloud_url`.
- [ ] **Step 2:** `cargo test lms::` → FAIL.
- [ ] **Step 3:** Implement. Run `lms server status` / `lms ps` once on a dev Mac first to confirm their output format, and parse that.
- [ ] **Step 4:** `cargo test` → PASS. Manual check: quit LM Studio, start a meeting, and Phi loads with suggestions starting once it's up.
- [ ] **Step 5:** Commit `Start LM Studio and load the model when a meeting starts`.
