# Launcher Options Parent Transaction Design

Date: 2026-09-01
Status: revision 1 self-approved under the autonomous Phase 14 goal after a fresh REVISE verdict; implementation remains blocked until a new fresh design review passes
Contract: `docs/contracts/2026-08-31-launcher-options-parent-implementation-contract.md` SHA-256 `CA8F090C91FFAEF8B666207DC2C8504758DB826808DB94D46114336881C14DEB` (fresh contract critic GREEN)

## Goal

Replace the main-menu launcher Options placeholder with the active-YR `0xD5` semantic parent transaction, including exact control projection/admission, immediate preview effects, ordered apply/teardown/commit, child handoff seams, and the smallest always-present Theme mechanism required by accepted Score zero.

This mechanism does not claim Network/Keyboard child-body closure or exact shell pixels. Those remain separate Phase 14 mechanisms/residuals and cannot inherit this design's GREEN verdict.

## Architecture Context

The active production path is app/UI-only. Main-menu action dispatch in `src/app/shell_main_menu.rs` currently constructs a unit `OptionsDialogState`; `src/ui/main_menu_dialogs.rs` draws an egui placeholder and returns only `Close`; `src/app/handler.rs` closes every egui-only modal on Escape and exits directly on `CloseRequested`.

Existing owners already provide most durable boundaries:

- `PersistenceState::options_profile` is the one process-owned `RetailOptionsProfile` and byte-preserving `RA2MD.INI` writer. The launcher dialog must remain a projection, not a second settings owner.
- `app::persistence::options` already uses a narrow operation adapter to test an ordered in-game Options transaction without constructing GPU-backed `AppState`. The launcher transaction follows that pattern but has different result and ordering semantics.
- `FrontendState::options_dialog` is the correct process-shell lifetime slot for transient parent control state.
- `AppAudioRuntime` is the process audio owner, but all Theme catalog, playlist, logical current/loop, and scenario-transition state currently lives inside `Option<MusicPlayer>`. `MusicPlayer::new` fails closed with output creation, so no logical Theme owner exists in SFX-only or fully headless configurations.
- `MusicPlayer::update` conflates missing physical output with completion if a logical current identity were retained. Native instead keeps logical active/retained/pending slots distinct from physical stream state.
- `SfxPlayer::play_sound_with_volume` supplies the call-local multiplier needed by `GenericBeep`; `GeneralRules` already owns `GenericClick`, `GUICheckboxSound`, and `GUIComboOpenSound` but not `GenericBeep`.
- `OfflineSkirmishRuntime` owns both its durable Skirmish snapshot and the six cached `[MultiPlayer]` preferences. Its existing loader/parser can be narrowed into the Network pre-route refresh without replacing snapshot, RNG, cooperative, or RMG state.
- Winit owns current-monitor mode facts; WGPU owns the rendered surface. The launcher may enumerate host dimension pairs but must not treat host bit depth as literal DirectDraw 16-bpp equivalence or resize the live surface.
- The skirmish shell has proven integer trackbar, checkbox, combo, ordered sound-queue, and operation-test patterns. Its resource IDs/layout/state remain skirmish-specific; the launcher reuses patterns and small pure primitives only, never its owner.
- Egui input and layout are in logical points. Native launcher input helpers consume integer control-client pixels, so one explicit conversion frame must sit at the custom launcher-control boundary.

The verified entry is ordinary active-YR main-menu case `5` at `Main__PrepareSession @ 0x0052D9A0`, which calls owner `0x0055FC80`. It is active without campaign, editor, online, map, faction, or TS-legacy gates.

### Component and authority flow

```text
main-menu action
  -> shell_main_menu opens retained OptionsDialogState
       inputs: RetailOptionsProfile + current-monitor mode pairs + local CSF fallbacks
               + immutable launcher_audio_available
  -> ui/main_menu_dialogs/options draws custom semantic controls
       output: ordered LauncherOptionsEvent list or one ParentResult
  -> shell_main_menu drains events
       preview -> retained profile -> concrete audio output -> cue
       parent result -> Pack -> ordered Apply -> DropPrimary
         Back/terminal -> one profile write
         Network -> six-field cache refresh -> distinct child route -> fresh parent on return
         Keyboard -> distinct child route -> fresh parent on return

AppAudioRuntime
  -> always-present ThemeRuntime (catalog + exact logical lifecycle)
  -> optional MusicPlayer (rodio output and gain projection only)
```

No edge enters `sim/`, replay, save serialization, match commands, deterministic hashes, or match RNG.

## Impact Analysis

Primary surfaces:

- `src/ui/main_menu_dialogs.rs` and new `src/ui/main_menu_dialogs/options.rs`: launcher-local labels, values, descriptor/group order, custom-control state, physical coordinate frame, input admission, draw output, and unit tests.
- `src/app/persistence/options.rs` and new `src/app/persistence/options/launcher.rs`: pure profile projection, immediate retained stores, packed snapshot, exact ordered parent transaction, and operation-ledger tests.
- `src/app/shell_main_menu.rs`: open facts, event/effect dispatch, parent destruction, Back/Network/Keyboard continuations, cues, and fresh re-entry.
- `src/app/handler.rs`: Options-specific Escape consumption and terminal-close transaction.
- `src/app/frontend/skirmish_session.rs`: one atomic six-field read-only preference refresh.
- `src/rules/ruleset.rs`: existing `[AudioVisual] GenericBeep` ownership and parser tests.
- `src/app/audio_runtime.rs`, `src/audio/mod.rs`, new `src/audio/theme.rs`, and `src/audio/music.rs`: always-present logical Theme owner plus optional output projection.
- Current Theme call sites in `src/app/state.rs`, `src/app/frame.rs`, `src/app/in_game.rs`, `src/app/loading/transitions.rs`, `src/app/match_runtime/sim_tick.rs`, and `src/app/shell_main_menu.rs`: route track-state mutations through `AppAudioRuntime`.
- `src/app/initialize.rs`: construct both physical outputs, freeze launcher-audio availability, initialize Theme data independently of music output, and apply retained gains.

Blast radius and controls:

- Theme migration spans shell and match transitions. All production track-state call sites move in the same dependency-coherent mechanism so no split owner or stale writer remains.
- Physical music behavior can regress if `Unavailable`, `Idle`, `Playing`, and `Finished` collapse. They remain distinct and are tested through a fake output seam.
- Egui stock widgets admit whole-label/whole-face clicks and point-space boundaries. Launcher controls use custom response rectangles and the one physical frame helper.
- Parent routing can accidentally persist child results or expose a stale dialog during post-result work. A consuming dispatcher and literal operation ledger make teardown structural.
- No persisted schema or snapshot format changes. No `sim/` or renderer dependency is introduced.

## Clarifications And Scope Decisions

The autonomous goal supplies exactness, active-YR, ordinary-shell, and publication criteria; no user-only design choice remains.

- TS/YR distinction is proven: all required owner/caller paths are active in the ordinary YR main menu. No TS legacy is designed.
- Network RT_DIALOG `0xD7` and keyboard RT_DIALOG `0xA3` bodies are excluded, but their distinct parent results, teardown order, Network preparation, and fresh-parent continuation are included. Until those child mechanisms land, their route adapters must remain explicit unavailable/stub boundaries rather than being mislabeled complete.
- Exact SHP/PCX paint, DLU pixels, warning reveal, combo-arrow art state, and legacy host 16-bpp capability are residuals. The semantic parent must not claim pixel parity.
- The output-failure choice is not a product preference. Strict native semantics win: after valid data and admitted logical start, native ignores `PlayFile` failure and stores active identity. Rust therefore logs/suppresses only the failed physical call and preserves logical success.

## Player-Experience Detail Ledger

### Milestone-blocking

- Parent opens from the retained singleton with literal Detail/Difficulty/Scroll/checkbox/volume projection, including reject-to-zero range behavior and volume truncation. Trigger: every Options open. Player effect: wrong displayed/current settings and wrong accepted values. [report §§3, 5.2, 8; `0x0055FC80`]
- The parent exposes Display, Game, UI, Audio groups and Back/Network/Keyboard with exact local CSF fallback text. Detail/Difficulty/Scroll captions name the position from the start: the init's range and position changes send WM_HSCROLL (`0x0061E609..0x0061E6AF` → `0x0055FF68`), confirmed by retail stills (see docs/research/shell/2026-09-25-options-slides-evidence.md). Trigger: every open/slider use. [report §§5.2, 5.3, 8; retail RT_DIALOG `0xD5`; active `ra2md.csf`]
- Audio thumb changes store the retained profile before live output/cue and never write INI. Score is silent; Sound then plays `GenericBeep(1.0)`; Voice then plays `GenericBeep(selected_voice_multiplier)`. Accepted apply is cue-silent. Trigger: every audio drag/apply. [report §§5.3, 6; `0x005FA4A0/0x005FA510/0x005FA590`]
- Score-zero is Queue(active else retained) then Stop under Theme-ready/shared-audio/not-suppressed gates. Active-present clears all logical slots; active-absent preserves retained/pending; failed gates preserve all. Trigger: accepting Score zero. [live `0x0055FBE5..0x0055FC06`, `0x00720B20`, `0x00720EA0`]
- Theme logical `[active,retained,pending]` ownership survives without `MusicPlayer`; constructor, Queue, Play, AI, Next, Stop, repeat, menu INTRO, scenario handoff, failures, and completion preserve the frozen contract table. Trigger: every music/menu/scenario transition and SFX-only/no-output startup. [live `0x00720960`, `0x007209D0`, `0x00720A80`, `0x00720B20`, `0x00720BB0`, `0x00720EA0`]
- Every parent result performs Pack, exact interleaved Apply, then DropPrimary. Back/terminal write once after teardown; child paths do not write. Trigger: every parent exit. [report §§4, 6, 12; `0x0055FC80`, `0x0055FAA0`]

### Compounding

- Network and Keyboard remain distinct results. Network refreshes only cached Handle/Color/ColorEx/Side/SideEx/GameMode after parent teardown; Keyboard performs no speculative mutation; both rebuild from a fresh profile snapshot only after child return. [report §§4, 5.1; `0x006980C0`]
- Resolution list preserves duplicate dimension pairs, sorted width/height order, native bounds/whitelist, `w x h x 16` text, and last displayed matching duplicate. Valid selection stores retained width/height immediately but never writes or resizes. [report §§5.1, 5.2; `0x0055FF00..0x00560270`]
- Options Escape is consumed without close/apply/write. Window close still Pack/Apply/DropPrimary/Write before existing exit teardown. [report §§5.1, 12]
- Apply order is Detail(+changed redraw), Difficulty, UnitActionLines(+unconditional target-line refresh), ShowHidden, ToolTips, Scroll, Score(+zero Theme edge), Sound, Voice. No GameSpeed or sim command belongs to this parent. [report §6; `0x0055FAA0`]
- Checkbox only admits the 18x18 icon and cues after toggle. Combo cues on face mouse-down before admission but opens only for strict `x > width-20`. [report §9.1]
- Trackbars use integer physical coordinates, reserve `0` for Detail/Difficulty/Scroll and `50` for audio, strict initial lower-y gate, thumb capture without jump, x-driven captured motion, rail one-shot, literal span/clamp formula, changed-only notification, and non-audio-only `GenericClick`. [report §9.1; `0x0061D950`]
- Response edges and pointer coordinates round independently to nearest physical integer at the current finite positive pixels-per-point before local subtraction. Trigger: every non-100%-DPI interaction. [contract PhysicalControlFrame row; Winit/egui boundary]

### Exactification residuals

- Exact `MNSCRN*`, `SDTP`, `SDBTNBKGD`, `SDBTNANM`, `SDBTM`, `LWSCRN*`, and 91-frame `SDWRNANM` paint/timing. Trigger/frequency: every Options presentation and animation frame, so frequent while the parent is open. Player effect: semantic behavior matches but shell art, layering, and animation timing remain visibly approximate. Downstream risk: a later renderer integration must preserve the pure control model and response rectangles, but cannot alter profile/control authority. [report §§9–9.3]
- Literal DirectDraw 16-bpp capability filtering. Trigger/frequency: mode enumeration on hosts whose advertised dimension set differs from the retail 16-bpp capability set; uncommon in ordinary play and most visible to expert/platform comparisons. Player effect: the resolution combo may offer or omit dimension pairs differently from retail. Downstream risk: later adoption can change the mode-list contents and selected row, but must not add live resize, change retained-profile authority, or change control admission. Modern WGPU/current-monitor dimensions are the explicit platform substitution. [report §5.2]
- Exact `DNARROWR` versus `UPARROWP` producer meaning. Trigger/frequency: every painted resolution-combo arrow, but normally noticed only during pixel comparison. Player effect: arrow artwork/state can differ while combo admission and selection remain exact. Downstream risk: later correction changes only renderer asset mapping; it cannot change the strict face/arrow hit boundary or profile ownership. [report §9.1]
- Release asset-CLI neutral-name winner. Trigger/frequency: never on the current production path; it becomes relevant only if production later consumes the release CLI lookup path. Player effect: none now, but a future consumer could resolve a different neutral-named asset. Downstream risk: later correction can change asset reachability/selection only and must not become a new settings or control authority. [report §9.2]

### Unknown-risk

None for the bounded semantic parent. Child bodies remain named separate mechanisms, not unknown behavior absorbed into this design.

## Approaches Considered

### A. Retained pure dialog model plus ordered app dispatcher — chosen

The UI module owns a pure bounded state machine and custom semantic controls. It emits an ordered event list and a consuming parent result. App persistence and shell adapters perform immediate stores, audio/cues, apply, teardown, routes, and writes. `AppAudioRuntime` separately coordinates a pure always-present Theme state machine with optional rodio output.

Architectural fit: follows existing `FrontendState`, `PersistenceState`, skirmish control-state, and operation-adapter patterns without importing app ownership into UI or sim. Every blocking/compounding ledger item has one explicit owner and test seam. Exact visual residuals remain isolated from later SHP/pixel work.

Trade-off: more explicit event and operation types than direct egui mutation, plus a real Theme/output split. That complexity is required to preserve native ordering, headless ownership, and testability.

### B. Let the egui draw function mutate `AppState` directly — rejected

This is short, but it couples point-space widgets to persistence/audio/routing, obscures profile-store/output/cue order, makes parent destruction dependent on UI borrows, and requires GPU/egui state for transaction tests. It also encourages stock widgets whose click admission differs from native.

Experience failure: frequent slider, combo, checkbox, Escape, and apply-order drift would remain likely. It fails the milestone and compounding ledger.

### C. Promote `0xD5` immediately into the shared SHP dialog/controller renderer — rejected for this mechanism

This could become the exact pixel implementation later, but doing it now couples semantic closure to DLU conversion, asset-atlas expansion, warning animation, combo-arrow uncertainty, and generic Win32-dialog substrate. It increases blast radius and mixes a later exactification mechanism with the reviewed parent transaction.

Experience fit: potentially best pixels, but unnecessary to close frequent control/persistence behavior and unsafe while the combo-arrow state and CLI lookup residuals remain. The semantic state/effect interfaces chosen by A remain reusable by a future SHP renderer.

### D. Keep Theme inside `Option<MusicPlayer>` and fake a headless player — rejected

This preserves the current file shape but leaves Theme data/lifecycle construction conditional on a rodio device, conflates unavailable with finished, and cannot represent SFX-only common-audio admission. It contradicts the active binary and the GREEN contract.

## Chosen Approach

Use Approach A. The parent surface is a retained pure model; the app is the transaction/effect owner; Theme logical state is always present; physical audio remains optional. The design creates no generic framework beyond concrete reusable seams already justified by multiple production callers.

## Design

### Components

#### Launcher UI state

`src/ui/main_menu_dialogs/options.rs` owns:

- launcher control IDs and the Display/Game/UI/Audio/right-rail descriptor;
- exact local `(CSF key, English fallback)` definitions;
- bounded integer positions, checkboxes, initial/dynamic captions, resolution rows/selection, and trackbar capture state;
- the immutable projected `launcher_audio_available` flag that disables interaction for all three audio trackbars as one group;
- `PhysicalControlFrame`, which validates scale, rounds global edges/pointer coordinates, and exposes local integer pixels;
- pure projection-facing value types, packed parent snapshot, and ordered `LauncherOptionsEvent`/`ParentResult` output;
- custom checkbox/combo/trackbar painting and response rectangles. Stock egui input admission is not semantic authority.

The dialog stores no Winit handle, profile reference, audio handle, INI path, or app callback.

Frame output is an ordered list because one admitted interaction can require multiple observable steps. Events distinguish live Score/Sound/Voice changes, valid resolution selection, shell cue requests, and Back/Network/Keyboard results. Internal Detail/Difficulty/Scroll caption changes occur before any corresponding cue event.

#### Launcher profile and transaction

`src/app/persistence/options/launcher.rs` owns:

- profile-to-dialog projection and accepted reverse mapping;
- current-monitor dimension filtering/sorting as a pure input transform;
- immediate, field-exact retained-profile stores for resolution and audio preview;
- the packed parent snapshot and exact interleaved apply operation;
- a narrow operation trait used by production and a recorder, following the existing in-game transaction seam.

It never reads the filesystem directly except through the existing final profile writer, never owns UI, and never routes children.

#### Shell orchestration

`src/app/shell_main_menu.rs` owns open, frame dispatch, and continuation:

- On open, gather a profile snapshot, current-monitor dimension pairs, exact launcher-local CSF strings, and the immutable process-start `launcher_audio_available`, then construct the dialog.
- On an ordinary frame, temporarily take the dialog, draw it, drain ordered effects, and reinsert it.
- On a parent result, keep the consumed dialog local through Pack and Apply. Then drop it and confirm `FrontendState::options_dialog` is empty before any persistence, preference refresh, route, return, or exit operation.
- Back persists once. Network refreshes six cached preferences then enters a distinct Network continuation. Keyboard enters a distinct Keyboard continuation. Neither child continuation persists. A later child completion calls the same open constructor and therefore gets a fresh snapshot.
- While child bodies are unavailable, their adapters report/log that exact unavailable boundary and return through the fresh-parent continuation; they do not masquerade as implemented child dialogs.

#### Terminal and keyboard routing

`src/app/handler.rs` checks launcher Options before the generic egui-modal Escape close. Escape requests redraw and returns with no event. `CloseRequested` consumes an open parent through the terminal result transaction before existing unrelated exit teardown and `event_loop.exit()`.

#### Theme and physical music split

`src/audio/theme.rs` owns device-independent configuration and logical behavior:

- an explicit catalog-loaded state, aliases, section-to-stem mapping, playlist/cursor, and menu identity/repeat;
- logical active and retained track identities plus pending `None`, `Auto`, `Hold`, or specific-track request;
- scenario transition/fade intent and the exact constructor/Queue/Play/AI/Next/Stop table from the contract;
- device-independent track resolution and data preparation so acquisition failure is known before physical submission.

`MusicPlayer` retains only the rodio device/player and user/output/focus/theme gain projection. It exposes physical stream observation (`Unavailable`, `Idle`, `Playing`, `Finished`), prepared-track submission, stop, and gain operations through a private testable output seam.

`AppAudioRuntime` owns both `ThemeRuntime` and `Option<MusicPlayer>`, plus immutable process-start `launcher_audio_available` and the currently fixed-false startup-suppression seam. Runtime methods sample physical state, ask Theme for exact logical transitions/physical commands, and apply commands in order. All existing production track-state callers migrate to these methods in the same mechanism. Gain-only callers may remain thin optional-output calls.

No output is `Unavailable`, never `Finished`. Asset lookup/load/decode failure follows native pre-Play failure. After valid data and admitted logical start, a missing or failed sink call preserves logical success exactly as native does after ignoring `PlayFile`'s return.

#### Network preference refresh

`OfflineSkirmishRuntime` gains one method that constructs a fresh `LocalMultiplayerPreferences` from constructor defaults plus the current `[MultiPlayer]` section, then atomically replaces only that field. Missing/unreadable/malformed input takes the same existing defaulted reader path. Snapshot, Scenario RNG, RMG options, cooperative state, and persistence bytes remain untouched.

### Interfaces And Contracts

Dialog contract:

- Construction receives owned projected values, owned labels, owned resolution rows, and immutable `launcher_audio_available`.
- UI events are ordered, deterministic for a given egui input frame, and side-effect free outside dialog state.
- When `launcher_audio_available` is false, all three audio controls paint disabled and reject press, capture, drag, rail, keyboard, and synthetic-event admission without mutating local or retained values.
- Pack consumes no external authority and includes only parent-owned accepted controls.
- Escape emits nothing. Invalid scale/input/selection emits nothing.

Preview contract:

- UI admission and shell dispatch both require immutable `launcher_audio_available` for Score/Sound/Voice preview events; the dispatch recheck rejects forged/stale events before any store, output call, or cue.
- After that recheck, Score/Sound/Voice change first stores the exact retained profile float.
- The corresponding concrete output setter follows when present.
- Score ends there. Sound then requests `GenericBeep` with local multiplier `1.0`; Voice uses the selected voice multiplier.
- Initialization and accepted reapply never request the beep. No preview or resolution event writes INI.
- Parent Pack/Apply remains unconditional and applies accepted packed values even when launcher audio is unavailable; the common flag gates live control admission/preview only.

Parent transaction contract:

- Operation order is literal and test-recorded.
- Apply sees the dialog still alive but only an immutable packed snapshot.
- DropPrimary occurs before every post-result operation.
- Final paths write once; child paths write zero times.
- No rollback exists.

Theme contract:

- The frozen implementation-contract lifecycle table is normative.
- Common admission is `theme_initialized && launcher_audio_available && !theme_startup_suppressed`.
- Logical branches inspect logical slots, never rodio activity.
- Physical output commands cannot rewrite the logical result after admission.
- Output observation never fabricates completion for an unavailable device.

### Data Flow

Open:

1. Main-menu Options action borrows the retained profile and immutable process-start `launcher_audio_available`.
2. The window adapter collects current-monitor mode dimensions without resizing.
3. Pure projection filters/sorts rows and chooses the last displayed matching duplicate.
4. The local CSF table resolves every exact key/fallback.
5. `OptionsDialogState` is installed in `FrontendState` with the common audio-admission flag.

Frame and preview:

1. Shell orchestration takes the dialog and draws one custom semantic frame.
2. UI converts admitted pointer geometry through `PhysicalControlFrame`.
3. UI mutates local control state and emits ordered events only after an exact value change/admission, including the common audio flag for every audio trackbar.
4. Shell rechecks the immutable common audio flag, then drains each admitted event to profile, audio, and cues in its verified order.
5. If no parent result occurred, the dialog is reinserted.

Parent result:

1. Pack the consumed dialog.
2. Apply fields/consumers in native order, including the exact Score-zero Theme edge.
3. Drop the dialog and clear frontend ownership.
4. Back/terminal writes once, or Network/Keyboard enters its distinct post-teardown continuation without a write.
5. Child return constructs a new parent from the then-current retained profile.

Theme update:

1. Caller supplies assets and wall time to an `AppAudioRuntime` method.
2. Runtime samples optional physical stream state; no output yields `Unavailable`.
3. Theme applies the native logical transition and returns ordered physical work.
4. Runtime applies optional stop/start/gain work without changing the logical outcome.

### Error Handling

- Missing CSF/key: launcher-local exact English fallback; unrelated CSF lookup semantics do not change.
- No current monitor or no admitted modes: empty resolution list and no selection; other controls remain usable.
- Invalid pixels-per-point: reject interaction for that frame; never guess integer geometry.
- Invalid resolution row: preserve retained pair and emit no effect.
- Missing profile path or write failure: final persistence remains nonfatal and logs through the existing owner.
- Missing Theme INIs/assets: explicit catalog-load result and pre-Play failure; no empty-alias retry sentinel.
- Missing physical music output: keep logical state and suppress physical work; never auto-advance as finished.
- Physical submission failure after valid data: log it and preserve native logical success.
- Missing SFX output: profile/live logical ordering still completes; only the concrete cue/output call is suppressed.
- Child body unavailable: retain a named route boundary and rebuild the parent; never write or claim child closure.

### Testing Strategy

All tests are library tests. Every Cargo invocation carries `--lib` and follows a `cargo/rustc` ownership check.

- UI projection tables for defaults and every out-of-range/non-finite edge.
- Exact key/fallback and descriptor/group/caption lifecycle tests.
- Resolution duplicate/filter/last-match/live-store tests with no resize/write.
- Physical frame tables at 1.0/1.25/1.5/2.0 and fractional origins.
- Checkbox/combo/trackbar boundary, capture, threshold, caption, and cue-order tests.
- Audio-control matrix proving `-NOAUDIO` and neither-output state disable all three controls, music-only/SFX-only/both-output states use the one common enabled predicate, disabled controls reject drag/rail/capture without mutation, and forged/stale preview events are rejected by dispatch before store/output/cue.
- Preview operation recorder proving profile -> output -> cue and cue-silent initialization/accept, while accepted parent Apply remains unconditional in disabled-audio configurations.
- Parent operation recorder proving both Detail branches, exact full apply order, DropPrimary, child preparation/routes, final write, and terminal exit.
- Headless production-wiring tests through the helpers called by `shell_main_menu`: main-menu action `5` constructs the retained snapshot with mode/CSF/audio inputs, a frame's ordered preview and parent results reach the shell dispatcher, ordinary frames reinsert the parent, and every terminal path commits exactly once after DropPrimary.
- Handler regression tests proving Options Escape is consumed with the same parent instance and no apply/write, while Escape retains the existing close behavior for neighboring egui-only dialogs; `CloseRequested` still enters the Options terminal transaction exactly once before unrelated exit teardown.
- Six-field Network refresh fixture proving every unrelated runtime owner and INI byte remains unchanged.
- Pure Theme lifecycle table tests covering every sentinel/transition, common-gate cross product, menu/scenario paths, Score-zero cases, and physical-output matrix.
- Fake output tests proving `Unavailable != Finished`, valid headless starts remain logically active, pre-Play failures follow native rows, and sink failure cannot rewrite logical success.
- Production call-site/source scan proving every track-state writer routes through `AppAudioRuntime` and no `sim/`, replay, save, second profile/parser, or display-resize dependency was added.
- Focused commands are module filters such as `main_menu_dialogs`, `launcher_options`, `music/theme`, Rules audio-visual parsing, and skirmish-session preference refresh. The Phase-wide full library suite remains reserved for the final Phase 14 gate.

## Architectural Decisions

- Retained pure model over direct UI mutation: preserves ordering and enables GPU-free tests.
- Process profile remains sole persisted authority; immediate stores are live in-memory profile mutation, not a second cache.
- Consuming parent dispatcher over callback-driven close: makes DropPrimary-before-post-result structural.
- Always-present Theme state over a fake headless `MusicPlayer`: preserves native ownership and exact logical/physical separation.
- Concrete internal output seam over a public audio framework: one real rodio implementation plus test fake, no speculative abstraction.
- Launcher-local custom controls over stock egui widgets: preserves native admission while retaining the current semantic egui shell style.
- Current-monitor dimension substitution over host 16-bpp filtering: keeps ordinary lists viable and labels the residual honestly.
- No sim/state-hash changes: all work remains process UI/audio/persistence orchestration.

## Adversarial Self-Review And Approval

Objection: the Theme split is much larger than a settings dialog and could create expensive regressions.

Resolution: accepted Score zero directly mutates Theme slots. Keeping Theme inside optional output would make the parent wrong in SFX-only/no-output states and leave multiple writers. The design migrates only the existing track-state cone, preserves gain-only callers, and freezes every transition with a fake-output test seam. It is the smallest dependency-coherent prerequisite, not a general audio rewrite.

Objection: an egui semantic parent will still look visibly unlike retail.

Resolution: that is true and explicitly remains an exactification residual. The selected mechanism closes frequent settings, input, audio, and persistence behavior without claiming pixel parity. Its state/effect interfaces are renderer-independent, so a later SHP/pixel mechanism can replace drawing without rewriting authority.

Objection: Network/Keyboard buttons still lack child bodies, so the ordinary journey is incomplete.

Resolution: their bodies are separate Phase 14 mechanisms. This branch must prove distinct results, teardown, Network preparation, no-write, and fresh-parent continuation, but must not absorb or falsely close the children. The Phase row stays open after this parent PR.

Objection: treating a reported sink failure as logical success seems less recoverable than retrying.

Resolution: native ignores `PlayFile`'s return and writes active identity. A retry rewrite would be a new semantic policy. The design distinguishes earlier asset/decode failure, where native has verified failure transitions, from post-admission physical failure, where strict parity wins.

Objection: taking/reinserting dialog state each frame may accidentally expose stale ownership during a result.

Resolution: the result path consumes the state and never reinserts it; the operation seam records Pack/Apply/DropPrimary before all continuations. Ordinary frame reinsertion is a separate branch.

Objection: current-monitor enumeration and DPI conversion may become platform-specific debt.

Resolution: platform facts remain plain inputs. Pure filters and `PhysicalControlFrame` contain the substitution; UI/persistence code stores no Winit handles. Boundary tables prevent silent scale drift.

Approval decision: approve Approach A for fresh design review. Every milestone-blocking and compounding item has an owner, ordering boundary, and executable test. No unknown-risk item remains. Residuals are bounded, non-sim, and do not force a later authority rewrite.

## Alternatives Considered

Approaches B–D above are rejected. A future exact shell renderer may reuse the chosen model/effect contracts, but neither direct `AppState` mutation nor Theme-under-output ownership is an acceptable intermediate state.

## Sources

- [docs/research/LAUNCHER_OPTIONS_0XD5_BACKUP_RESET_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAUNCHER_OPTIONS_0XD5_BACKUP_RESET_GHIDRA_REPORT.md), GREEN SHA-256 `3B47ABD9EC6EB285687F4548C8AA4FEEBA540EE15B32118C9785C0D6246340BB`.
- Bounded active-binary Theme lifecycle zero-add recheck: constructor `0x00720960`, AI `0x007209D0`, Next `0x00720A80`, Queue `0x00720B20`, Play `0x00720BB0`, Stop `0x00720EA0`, and live menu/scenario/main-tick/launcher callers.
- [docs/research/OPTIONS_PROFILE_TRANSACTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OPTIONS_PROFILE_TRANSACTION_GHIDRA_REPORT.md) and merged PR #202.
- `docs/contracts/2026-08-31-launcher-options-parent-implementation-contract.md`, frozen GREEN hash cited above.
- Active `gamemd.exe` SHA-256 `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`.
- Retail RT_DIALOG `0xD5`, active `ra2md.csf`, `[AudioVisual] GenericBeep`, and current-monitor Winit facts.
- Exact `origin/main c9b97157f42e932c9360fdad436e91a6fe5e0835` source reads across UI, shell, handler, persistence, audio, Rules, skirmish-session, and Theme production callers.
- `docs/plans/2026-06-02-shell-substrate-slice5b-options-design.md` only as a pattern/source map for the separate in-game Options surface; none of its `0xBBB/0xF5` result semantics are generalized to launcher `0xD5`.
- `docs/plans/2026-08-15-engine-domain-boundaries-design.md` for the established process-wide `AppAudioRuntime` and `PersistenceState` ownership.
