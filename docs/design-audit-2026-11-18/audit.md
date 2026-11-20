# Typeless / Open Dictate — cohesion audit

18 November 2026 · First design pass · Proposed changes, not implementation

## Recommendation

Keep Paper & Ink's personality, but give it a hierarchy. Use the strong ink treatment for the main action and active selection; give reading surfaces quiet edges, and use simple rows for repeated content. Make Home a place to start, understand the current recording state, and recover the latest result. Keep usage insights secondary.

The current build is substantially ahead of the older saved screenshots. Start dictating is already prominent, model status is consolidated, a setup guide exists, meetings have a visible start action, and meeting detail exposes Rename and summary Edit. Those are foundations to retain.

## Evidence and limits

- Inspected the running **Open Dictate** app at its current window size, capturing eight screens: Home, History, Meetings, meeting detail, Dictionary, Settings, Models, and the setup guide. Screenshots are adjacent to this document; `review.html` adds numbered annotations.
- Reviewed the local Rust/egui source and November 17 UX sweep. Local repository base: `6b3a846`, with substantial pre-existing uncommitted work. The running binary's exact correspondence to this working tree was not established; visual observations and source findings are distinguished below.
- No product source, preferences, recordings, or user content were edited. Navigation was restored to Home. No audio was recorded or played, and no external provider was invoked.
- This is a heuristic review, not a usability study. Active recording, processing, failure, clean install, light appearance, minimum window size, and VoiceOver were not exercised. Their proposed behavior is a design recommendation, not a verified result.
- The native accessibility snapshot exposed the window and window controls, but no app content. This is an accessibility investigation signal, not proof that all assistive technology fails.

## Current screen annotations

### Home

1. **Keep the prominent action.** Start dictating and its shortcut are correctly near the headline. The earlier diagnosis that recording was buried in the right rail no longer applies.
2. **Reduce repeated emphasis.** Four stat cards, a large activity chart, the shortcut panel, readiness, and the sidebar all have bright outlines. The eye encounters many competing boundaries. Flatten statistics into a compact line and put the full activity view behind an Insights disclosure.
3. **Bring readiness next to recording.** The Ready panel is far from the action and leads with model names. Show capability language near the action, e.g. “Speech model available · Microphone not checked,” with technical details available on demand. Do not claim the entire flow is ready from model configuration alone.

### History

1. **Lead with the user's text.** Retention, privacy explanation, restore backup, and filters take approximately the first third of the content area before the first item. Keep a compact retention summary and a contextual recovery notice; put detailed policy controls in Privacy & data.
2. **Make long results scannable.** The first entry dominates the list with a long paragraph. Show a short preview with a clearly labeled expansion action. Preserve the full text for reading and copying.
3. **Use consistent action language.** Copy and overflow are visible icons, but the overall hierarchy needs a shared row pattern: timestamp/mode → text → actions. Retain accessible names and keyboard focus.

### Meetings

1. **Preserve the new start action and capture explanation.** The preflight block is useful, especially the distinction between call audio on this Mac and voices entering the microphone. Group it into a compact “Recording setup” summary, expandable before starting; never hide the active source choice.
2. **Make the library easier to scan.** Each saved meeting uses a large outlined, shadowed card and repeats a title, subtitle, date, source, and duration. Use aligned rows with title first, date/duration second, and optional subtitle. Retain an explicit hit target and hover/focus feedback.
3. **Avoid ambiguous privacy shorthand.** “Kept only on this Mac” describes storage, but can be mistaken for a processing guarantee. Prefer “Saved on this Mac,” with the actual processing route available nearby. A remote-provider state needs explicit destination language.

### Meeting detail

1. **Make this a document header.** The large generic “Meetings” page title, back link, long meeting title, Rename, metadata, and Delete occupy multiple levels. Use “All meetings” above the meeting title, then one coherent metadata/action row. Move Delete into the overflow menu while retaining its existing confirmation behavior.
2. **Bring evidence navigation closer.** Audio and Summary are separate large panels, while the transcript is below the visible fold. Provide local Summary / Transcript navigation and keep the audio control reachable. Keep the warning about partial speech clips.
3. **Constrain reading width.** Summary paragraphs span much of the 1,060-pixel content area. Use a reading column around 65–80 characters, with secondary actions outside that measure. Retain visible Edit and source-checking affordances.

### Settings

1. **Clarify the information architecture.** “Settings” opens a sheet with another tab called “Settings.” Rename that tab General. Consider ordering the tabs General, Audio input, Models, Personalization, Privacy & data, About.
2. **Bring high-use controls into reach.** Appearance and Startup consume most of the first viewport; the first shortcut row is cut by the lower edge. Put Shortcuts and Output ahead of cosmetic/startup options, or use explicit subsection navigation. Tighten section spacing without shrinking targets.
3. **Reuse the pinned sheet header.** The title, description, and close control already stay together. Preserve that pattern and standardize its padding across other dialogs.

### Models

1. **Keep plain-language role descriptions.** Speech-to-text versus optional text cleanup is explained. Use capability headings as the first organizing level, with the chosen provider and health beneath them.
2. **Separate normal setup from troubleshooting.** Engine selection, executable path, and implementation terms arrive early. A recommended local setup can lead; program paths and compatible-server details belong in advanced sections, automatically opened when relevant to a failure.
3. **Keep routing visible.** The audio/text destination line and Privacy & data link are useful additions. Reuse their wording in Home's recording context instead of inventing a second privacy vocabulary.

### Dictionary

1. **Reuse this screen's restraint.** Its content uses quiet rows without heavy card outlines. This is an existing example to extend to History and Meetings.
2. **Give search a stable, labeled entry.** Search is an isolated icon. A compact labeled Search field becomes useful as the dictionary grows; validate at both three and many entries. Keep the current filter categories.
3. **Clarify how entries are used.** Add a brief line connecting terms to recognition and corrections, reflecting any actual limitations by mode. Don't promise identical meeting/dictation behavior without evidence.

### First-dictation setup guide

1. **Retain the safe practice area.** The guide clearly says practice stays in the window. This is a good model for a Home-initiated result.
2. **Make progress reflect completion.** Engine and model have green status; permissions and practice are instructions. Emphasize the next unverified step and collapse completed checks. Distinguish “configured” from “tested.”
3. **Make the final action truthful.** Done is available without practice. “Close setup” is more accurate until the completion conditions have been met; after a successful test, use a success-specific action. Access to advanced settings should remain available.

## Ranked fixes

| Rank | Change | Why first | Evidence / scope |
|---|---|---|---|
| 1 | Define strong, quiet, and unframed surface roles | Biggest cross-screen visual improvement with one shared rule | Observed on all screens; `theme.rs:197–216` currently uses ink outlines for both reading and interactive surfaces |
| 2 | Consolidate Home's recording action, capability status, and latest result | Gives the page one coherent daily purpose | Observed Home; source `views.rs:1009`, `1251`, `1280`; result behavior is proposed |
| 3 | Adopt one page header and list-row pattern | Makes History/Meetings predictable and easier to scan | Observed list screens and detail header |
| 4 | Reduce Settings section spacing and improve grouping | Makes essential configuration easier to find in the existing sheet | Observed Settings; shared helpers `settings_view.rs:217`, `238` |
| 5 | Separate configuration from verified readiness | Avoids a reassuring “Ready” label that exceeds what was checked | Source `app.rs:1219`: local check tests binary/model presence; compatible server checks nonempty model name, not microphone/paste success |
| 6 | Make meeting review a readable document with nearby transcript navigation | Supports checking and correcting generated output | Observed detail first viewport; source contains transcript further down |

Ranks are suggested design sequence, not measured defect severity. Accessibility and recording failures should take precedence if validation reveals blockers.

## Proposed shared rules

**Intent:** A person moving between writing and calls needs to capture a thought quickly, know what is being recorded, and retrieve usable text. The experience should feel like a dependable writing instrument: quiet while idle, explicit while listening, clear about where the result went.

**Domain:** spoken thought, listening, dictation, a writing cursor, transcript, correction, personal vocabulary, meeting notes.

**Color world:** warm paper `#F4F3EF`, near-black ink `#14140F`, graphite `#4C4B46`, pencil-line gray `#D9D8D5`, marker yellow `#F2C31A`, editorial blue `#2B5A9B`; preserve the existing dark counterparts. Green and red keep their semantic status roles. These colors already belong to the project; no new palette is needed.

**Signature:** a single voice-to-text area that changes from prepared → listening → processing → result while keeping its placement. Existing shortcut keycaps reinforce the connection to dictation in other apps. This makes the primary interaction recognizable, rather than relying on decorative statistics.

**Defaults to replace:** dashboard-first Home → capture and recent text; card around every group → quiet sections and aligned rows; persistent technical rail → capability status at the action plus expandable details.

- **Geometry:** retain the roughly 216-point navigation allocation; use a shared 32-point page inset at wide sizes and 24 at the minimum supported size. Use a 4-point spacing unit: 8 within controls, 12–16 within related groups, 24 between sections, 32 between major regions. Treat these as prototype starting values.
- **Widths:** page max 1,080 points; long text 65–80 characters. Remove Home's independently offset 186-point rail. At 980 × 640, stack supporting sections and keep the primary action visible without horizontal overflow. This size is declared in `main.rs:230`; it was not visually tested during this audit.
- **Hierarchy:** page title 32–38, section title 18–22, body 14–16, metadata 12–13. Keep Marsipan for selective display headings and Nunito for reading. Use sentence case for most controls; reserve display styling for one dominant action.
- **Surface roles:** strong outline/short hard shadow for the main action and selected navigation; quiet 1-point hairline and no shadow for ordinary reading surfaces; no container for simple groupings. Preserve visible keyboard focus independently of the quiet styling.
- **Headers:** page title + short context + one primary action. Details use back navigation + item title + metadata, rather than repeating the parent page heading at equal prominence.
- **States:** each core action needs idle, capture, processing, result, and recoverable error. Use specific labels (“Transcribing”, “Cleaning up”) when the phase is known. Show result delivery independently of saved-history status.

## Home proposal

Recommended order: concise page title → recording area with shortcut and honest capability status → recent text → compact usage summary. Put the full usage chart, detailed shortcut reference, and model identifiers behind disclosures or their existing settings destinations.

Two approaches were considered:

- **Visual cleanup only:** change borders, spacing, and type while preserving the existing dashboard. Smallest change, but statistics and model details still occupy the main information hierarchy.
- **Capture-centered Home (recommended):** apply the same visual cleanup and reorganize Home around the complete recording cycle. More work because Home-initiated capture needs an explicit result destination, but it addresses both visual cohesion and daily use.

The companion mockup illustrates the second approach using synthetic text and sample status. It is not connected to the microphone, clipboard, stored history, or external services. Its “Ready” state is illustrative, not a claim about the running app.

### Destination contract to settle during implementation

- Global shortcut: preserve dictation into the user's active destination and report paste/copy outcomes accurately.
- Home button: proposed capture into a Home result area with explicit Copy; do not imply that an unspecified previously active app will receive text. This is a proposed behavior change, not merely a layout edit.
- Only show “Saved to History” if retention policy actually saved the item. With history off, keep the result temporary and say so.
- A recent-results section must honor retention; do not create a new hidden copy of deleted or expired text.

### Acceptance checks for the next pass

1. A first-time user can identify the primary action and explain where its result will appear.
2. A returning user can find/copy the latest result without scrolling past usage charts or policy prose.
3. A person can distinguish model configuration, microphone activity, processing, and successful delivery.
4. Repeated elements use the same headers, row geometry, spacing, typography, focus treatment, and surface roles.
5. At 1360 × 920 and 980 × 640, key actions remain visible and text/controls do not overlap; repeat in light and dark appearance.
6. Keyboard and VoiceOver paths can reach and name the primary controls; exercise success, silence, failure, and recovery with a controlled test recording.

## Next implementation slice

Figma workspace supplied by the owner: [Open Dictate](https://www.figma.com/design/OcHziKpLwLDY82p1581l4k/Open-Dictate). Inspected in Figma Desktop on 18 November: one empty page named Page 1, with no layers visible. Use this existing file for editable design iterations. The Figma Console Desktop Bridge was not connected during inspection; follow the project bridge instructions when authoring.

Start with Home and the shared surface roles, keeping the current navigation destinations. Review the concrete layout and destination contract before implementing; then carry the accepted patterns into lists and Settings. Do not combine this design pass with unrelated changes to audio capture or model infrastructure.

## Deliverable checks

The annotated HTML review was opened locally and the Home screenshot markers were visually inspected. All eight image paths and annotation links were checked. The separate Home mockup was visually inspected in the browser; Start → Listening → Processing → Result, simulated Copy, and Error → Setup transitions were exercised. The mockup's JavaScript syntax was checked. These checks validate the review artifacts, not production dictation behavior. No production test suite was run because no product code changed.
