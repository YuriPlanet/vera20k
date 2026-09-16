# Initial Main Menu Dialog 0xE2 Implementation Plan

> **For Claude:** Execute this plan task-by-task. Each task is self-contained.

**Goal:** Replace the current egui main-menu surface with a faithful native render path for Yuri's Revenge initial shell dialog `0xE2`: looping RA2TS Bink playback, shell owner-draw right buttons, localized labels, main-button click sound, and original button action identity.

**Architecture:** Pure dialog geometry and hit testing live under `ui/main_menu_shell`; static PCX button chrome and dynamic Bink movie texture live under `render`; app-level glue composes, routes input, loads startup shell assets, and plays UI audio. No `sim/` module participates.

**Design Doc:** [docs/plans/2026-05-17-initial-main-menu-dialog-0xe2-design.md](2026-05-17-initial-main-menu-dialog-0xe2-design.md)

---

## Grounding Summary

**Research docs (R1):** Primary reports are [MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md), [MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md), and [MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md). They report **High** confidence for dialog `0xE2`, the `0x71A` RA2TS child, Bink-before-VQA selection, 34 ms poll vs 15 fps playback timing, LANGUAGE.MIX over LANGMD.MIX for RA2TS duplicates, and owner-draw button PCX family selection. No TS-legacy warning applies to this standard YR shell path.

**Ghidra verification (R2):** Live Ghidra decompile reconfirmed:
- `FUN_00531CC0 @ 0x00531CC0`: creates main-menu dialog, moves child `0x71A`, sends `0x4E3`, chooses `Ra2ts_s` when `g_ScreenWidth == 640`, otherwise `Ra2ts_l`, then sends `0x4E4`.
- `OwnerDraw_Static_006153E0 @ 0x006153E0`: `0x4E3` stores loop flag, `0x4E4` destroys previous movie, creates movie handle, resizes child to movie dimensions, and starts timer id `0x65` at interval `0x22`; timer update loops with vtable `+0x1C(1)` when loop flag is set.
- `OwnerDraw_Button_00612B70 @ 0x00612B70`: owner-draw buttons use `b%c%c_li%d.pcx`, `b%c%c_mi%d.pcx`, `b%c%c_ri%d.pcx`; down state shifts text by 2 px; disabled path alpha-blends at `0x80`; click/down transitions call `VocClass__PlayAtPos`.
- `CDFileClass__Constructor @ 0x005C0640` and `VQMovieHandle__Constructor @ 0x005C07D0`: movie base names try `.BIK` before `.VQA`, and `.bik` enters the Bink object path.

**Repo patterns (R3):** Mirror [src/ui/skirmish_shell/layout.rs](../../src/ui/skirmish_shell/layout.rs) for DLU conversion, `RectPx`, and layout tests; [src/ui/skirmish_shell/state.rs](../../src/ui/skirmish_shell/state.rs) for pressed-button hit testing and action mapping; [src/render/skirmish_shell_chrome.rs](../../src/render/skirmish_shell_chrome.rs) for PCX loading and atlas packing; [src/app_skirmish_shell_render.rs](../../src/app_skirmish_shell_render.rs) for app-level shell sprite construction; [src/bin/bik_player_playback.rs](../../src/bin/bik_player_playback.rs) for Bink pacing and YUV-to-RGBA conversion; [src/render/batch.rs](../../src/render/batch.rs) for `create_updatable_texture`.

**Current git-state adjustment:** Commit `82f2115 render/bit_font: scaffold types + module registration` landed after the design was written, and current [src/render/shell_text.rs](../../src/render/shell_text.rs) now provides `draw_in_rect` on top of `AppState.bit_font`. This plan uses that current `BitFont` + `render::shell_text` path directly for main-menu labels.

**INI keys (R4):** `[AudioVisual] GUIMainButtonSound=MenuClick` exists in both [ini/rules.ini](../../ini/rules.ini) and [ini/rulesmd.ini](../../ini/rulesmd.ini). `MenuClick` is present in [ini/sound.ini](../../ini/sound.ini) and [ini/soundmd.ini](../../ini/soundmd.ini). The plan adds parsing for `GUIMainButtonSound` instead of hardcoding `MenuClick`.

**Still unknown after grounding:** Exact first visible RA2TS frame and retail global shell clipping still need screenshot/capture verification. `BinkGoto(1)` is treated as Rust decoder index `0`; a loop unit test verifies this intended mapping in the Rust abstraction.

## Key Technical Decisions

- **Native `0xE2` is the default main-menu path** with egui only as asset/load failure fallback. **Confidence:** high. **Source:** design doc + verified `FUN_00531CC0`.
- **Do not add a generic Win32 dialog framework.** Encode only the verified `0xE2` layout/control identities. **Confidence:** high. **Source:** design doc, small fixed control surface.
- **Use existing `AssetManager` first-match lookup for RA2TS.** Do not bypass archive priority or scan physical archives for RA2TS. **Confidence:** high. **Source:** [MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md); current `AssetManager` order already has `language.mix` before `langmd.mix`.
- **Add `GUIMainButtonSound` parsing to rules data.** UI click sound comes from INI, not a hardcoded string. **Confidence:** high. **Source:** `RULESCLASS_FIELDS.csv` lists `[AudioVisual] GUIMainButtonSound`; INI grep confirms value.
- **Bink movie surface lives in `render/`.** It owns a GPU texture and depends only on `assets` + render GPU/batch primitives, not app or UI. **Confidence:** high. **Source:** architecture boundary in design and `BatchRenderer::create_updatable_texture`.
- **Playback advances by Bink fps, not timer interval.** Use real-time accumulator and catch-up. **Confidence:** high. **Source:** `OwnerDraw_Static_006153E0` timer plus Bink vtable delay report.
- **Main-menu click sound plays directly on valid owner-draw button mouse-down.** Do not push into `SoundEventQueue`, because that queue is drained only during in-game runtime. **Confidence:** high. **Source:** [src/app_sim_tick.rs](../../src/app_sim_tick.rs) calls `drain_sound_events` only in `advance_in_game_runtime`.
- **Shell text dependency is isolated.** Main-menu rendering has one `push_shell_label_text` wrapper backed by current `render::shell_text::draw_in_rect` and `AppState.bit_font`. **Confidence:** high. **Source:** current `src/render/shell_text.rs` and `src/render/bit_font.rs`.

## Open Questions

### Resolved During Planning

- **Does RA2TS need custom archive lookup?** No. Current `AssetManager` first-match rule matches verified LANGUAGE.MIX over LANGMD.MIX for RA2TS.
- **Can UI sound use the existing sound event queue?** No. Main menu is outside `advance_in_game_runtime`, so valid owner-draw button mouse-down should call `SfxPlayer::play_sound` directly.
- **Can the Bink movie texture update without recreating bind groups?** Yes. `BatchRenderer::create_updatable_texture` returns raw `wgpu::Texture` plus `BatchTexture` with `COPY_DST`.
- **Is `render::shell_text` ready?** Yes. Current code provides `draw_in_rect`, `ShellAlign`, and `TextRect`; main-menu rendering should use those instead of any older sidebar-text adapter.

### Deferred to Implementation

- **Exact first visible RA2TS frame:** depends on observing the first decoded/uploaded frame in runtime and comparing to retail capture. Implemented behavior starts from decoder frame index `0`, matching `BinkGoto(1)` assumption.
- **Exact first-pixel text comparison:** use the current `BitFont` + `render::shell_text` path, then verify button/title label pixels against capture as part of visual QA.
- **Downstream dialogs:** button actions are preserved, but Single Player/Network/Options/etc. shell trees remain separate research/implementation work.

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `src/ui/main_menu_shell/mod.rs` | Export initial main menu layout/state/action modules. |
| Create | `src/ui/main_menu_shell/layout.rs` | Verified `0xE2` DLU geometry, RA2TS child position, movie base choice, layout tests. |
| Create | `src/ui/main_menu_shell/state.rs` | Control IDs, button action mapping, pressed-button hit testing, tooltip/CSF keys. |
| Modify | `src/ui/mod.rs` | Export `main_menu_shell`. |
| Create | `src/render/main_menu_shell_chrome.rs` | Load six mandatory owner-draw button PCX pieces into a small atlas. |
| Create | `src/render/bink_movie.rs` | Runtime Bink movie surface, YUV-to-RGBA conversion, frame pacing, loop handling. |
| Modify | `src/render/mod.rs` | Export `main_menu_shell_chrome` and `bink_movie`. |
| Modify | `src/rules/ruleset.rs` | Parse `[AudioVisual] GUIMainButtonSound` into rules data. |
| Modify | `src/app_transitions.rs` | Make sound registry/audio index loaders reusable from startup. |
| Modify | `src/app_init.rs` | Expose CSF-loading helper for startup shell labels. |
| Create | `src/app_main_menu_shell_render.rs` | Compose RA2TS movie quad, shell buttons, and labels into render passes. |
| Modify | `src/lib.rs` | Export `app_main_menu_shell_render`. |
| Modify | `src/app.rs` | Add startup shell resources, route MainMenu rendering/input/actions, and load startup assets. |
| Keep | `src/ui/main_menu.rs` | Retain egui skirmish setup as fallback/downstream temporary flow. |

## Interface Changes

- New public `ui::main_menu_shell::{compute_layout, MainMenuShellLayout, MainMenuShellState, MainMenuShellAction, MainMenuControlId, mouse_down, mouse_up, hit_test_owner_draw_button, csf_key_for_control, return_code_for_action}`.
- New public `render::main_menu_shell_chrome::{MainMenuShellChromeAtlas, MainMenuShellChromeEntry, build_main_menu_shell_chrome_atlas}`.
- New public `render::bink_movie::{BinkMovieSurface, BinkMovieStep, frame_to_rgba}`.
- New `rules::ruleset::GeneralRules::gui_main_button_sound: Option<String>`.
- `app_transitions::load_sound_registry`, `load_audio_indices`, and `load_eva_registry` become `pub(crate)` so startup can initialize menu audio before map load.
- `app_init::load_csf` becomes `pub(crate)` so startup can load CSF labels before map load.
- New app-level renderer module `app_main_menu_shell_render`.

## Sim Checklist

**Not applicable.** This plan touches `ui/`, `render/`, `rules/`, `assets` consumption, and app orchestration only. No `sim/` files change, no deterministic tick state changes, no fixed-point requirements, and no state-hash impact.

## Risk Areas

- **Startup asset load cost:** native main menu needs `AssetManager`, CSF, GAME.FNT, sound registry, audio indices, button PCXs, and RA2TS Bink before map load. Mitigation: load once from `GameConfig` at startup and keep egui fallback if load fails.
- **Bink decode/upload stutter:** decoding multiple catch-up frames on a slow frame can stall UI. Mitigation: cap catch-up to a small bound per step and log when capped; RA2TS is only 632 x 570 at 15 fps.
- **Text parity dependency:** shell text rendering exists, but main-menu label placement still needs visual verification. Mitigation: isolate label drawing behind one helper and list visual text comparison in verification.
- **Audio initialization timing:** current sound registry/audio indices are loaded after map transition. Mitigation: expose and call those helpers during startup when startup `AssetManager` exists.
- **Dev Skirmish shell branch:** keep `dev_skirmish_shell_enabled` behavior intact so existing Skirmish shell research path remains usable.
- **Resize movie swap:** crossing the 640-width boundary must rebuild `ra2ts_s`/`ra2ts_l` cleanly without retaining stale texture dimensions.

## Parity-Critical Items

| Task # | Item | Why it matters | Verification |
|--------|------|----------------|--------------|
| Task 2 | Dialog `0xE2` DLU rects and movie child offset | Main menu must put RA2TS and buttons where YR does; visible on every launch | Unit tests for 640, 800, 1024, 1280 widths; screenshot comparison |
| Task 3 | Button control IDs and return codes | Click routing must preserve original shell action identity for downstream screens | Unit tests assert `0x683 -> 1`, `0x684 -> 2`, `0x578 -> 3`, `0x686 -> 4`, `0x55C -> 5`, `0x3EE -> 6` |
| Task 5 | PCX owner-draw button pieces | Wrong caps/middle/down-state makes every button visibly wrong | Asset classification tests; runtime screenshot of up/down buttons |
| Task 7 | RA2TS `.bik` selection and archive source | Wrong movie or wrong duplicate source changes the first screen | Runtime log asserts `ra2ts_l.bik` source is `language.mix` when duplicate exists |
| Task 8 | Bink 15 fps pacing and loop | Movie animation cadence is visible immediately and continuously | Step tests; runtime observation that it is not 34 ms/frame or monitor-refresh speed |
| Task 10 | Draw order: movie, button chrome, text | Incorrect z/order hides labels or overlays buttons incorrectly | Render screenshot at multiple resolutions |
| Task 11 | Pressed button down art and +2 px content offset | Click feel and button pixels visibly differ on every press | Mouse press visual check and button segment tests |
| Task 1 | `GUIMainButtonSound` from INI | Click sound must follow rulesmd.ini, not hardcoded value | Rules parser test and runtime click audio |
| Task 17 | Main-menu mouse down/up activation semantics | Original buttons activate on matching release, not accidental drag-off | Hit-test tests and manual drag-off check |
| Task 18 | 640-width `Ra2ts_s` vs other-width `Ra2ts_l` rebuild | 640 shell has different movie dimensions; visible on resolution changes | Resize test/manual capture |

---

## Tasks

### Task 1: Parse `GUIMainButtonSound` from rules INI

**Why:** Main-menu click sound must come from `[AudioVisual]`, and this needs to exist before app input wiring plays the sound.

**Files:**
- Modify: `src/rules/ruleset.rs`

**Pattern:** Follow existing optional sound parsing for `building_garrisoned_sound` in `RuleSet::from_ini`.

**Step 1: Add a field to the general rules struct.**

```rust
pub struct GeneralRules {
    // existing fields...
    /// Sound event for shell main-menu buttons from [AudioVisual] GUIMainButtonSound.
    pub gui_main_button_sound: Option<String>,
}
```

**Step 2: Initialize default to `None`.**

```rust
gui_main_button_sound: None,
```

**Step 3: Parse the key from `audio_visual`.**

```rust
gui_main_button_sound: audio_visual
    .and_then(|s| s.get("GUIMainButtonSound"))
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .map(str::to_string),
```

**Step 4: Add a parser test.**

```rust
#[test]
fn test_gui_main_button_sound_parsed() {
    let ini = IniFile::from_str("[General]\n[AudioVisual]\nGUIMainButtonSound=MenuClick\n").unwrap();
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(rules.general.gui_main_button_sound.as_deref(), Some("MenuClick"));
}
```

**Step 5: Verify.**
Run: `cargo test test_gui_main_button_sound_parsed -- --nocapture`
Expected: PASS.

**Step 6: Commit.**

### Task 2: Add `ui/main_menu_shell` layout module

**Why:** Geometry is pure, testable, and should be defined before render/input code consumes it.

**Files:**
- Create: `src/ui/main_menu_shell/mod.rs`
- Create: `src/ui/main_menu_shell/layout.rs`
- Modify: `src/ui/mod.rs`

**Pattern:** Mirror `src/ui/skirmish_shell/layout.rs` for `RectPx`, `mul_div_round`, `dlu_rect`, and tests.

**Step 1: Create module export.**

```rust
// src/ui/main_menu_shell/mod.rs
//! Initial main-menu shell dialog 0xE2 layout and input state.

mod layout;
mod state;

pub use layout::{MainMenuButtonRect, MainMenuMovieBase, MainMenuShellLayout, RectPx, compute_layout};
pub use state::{
    MainMenuControlId, MainMenuShellAction, MainMenuShellState, action_for_control,
    csf_key_for_control, hit_test_owner_draw_button, mouse_down, mouse_up,
    return_code_for_action,
};
```

**Step 2: Add `pub mod main_menu_shell;` to `src/ui/mod.rs`.**

```rust
pub mod main_menu_shell;
```

**Step 3: Define layout constants and structs.**

```rust
use super::state::MainMenuControlId;

pub const SHELL_BASE_W: i32 = 800;
pub const SHELL_BASE_H: i32 = 600;
const BASE_X: i32 = 6;
const BASE_Y: i32 = 13;
pub const RA2TS_L_W: i32 = 632;
pub const RA2TS_L_H: i32 = 570;
pub const RA2TS_S_W: i32 = 472;
pub const RA2TS_S_H: i32 = 450;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RectPx {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self { Self { x, y, w, h } }
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuMovieBase {
    Ra2tsS,
    Ra2tsL,
}

impl MainMenuMovieBase {
    pub const fn asset_name(self) -> &'static str {
        match self {
            Self::Ra2tsS => "ra2ts_s.bik",
            Self::Ra2tsL => "ra2ts_l.bik",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainMenuButtonRect {
    pub id: MainMenuControlId,
    pub rect: RectPx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainMenuShellLayout {
    pub screen: RectPx,
    pub movie_base: MainMenuMovieBase,
    pub movie: RectPx,
    pub title: RectPx,
    pub website_static: RectPx,
    pub buttons: [MainMenuButtonRect; 6],
}
```

**Step 4: Add DLU conversion and movie offset.**

```rust
fn mul_div_round(n: i32, numer: i32, denom: i32) -> i32 {
    let value = n * numer;
    if value >= 0 { (value + denom / 2) / denom } else { (value - denom / 2) / denom }
}

fn dlu_rect(x: i32, y: i32, w: i32, h: i32) -> RectPx {
    RectPx::new(
        mul_div_round(x, BASE_X, 4),
        mul_div_round(y, BASE_Y, 8),
        mul_div_round(w, BASE_X, 4),
        mul_div_round(h, BASE_Y, 8),
    )
}

fn movie_origin(screen_w: i32, screen_h: i32) -> (i32, i32) {
    let x = if screen_w <= SHELL_BASE_W { 0 } else { (screen_w - SHELL_BASE_W) / 2 };
    let y = if screen_h <= SHELL_BASE_H { 0 } else { (screen_h - SHELL_BASE_H) / 2 };
    (x, y)
}
```

**Step 5: Define `compute_layout`.**

```rust
pub fn compute_layout(screen_w: u32, screen_h: u32) -> MainMenuShellLayout {
    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;
    let movie_base = if screen_w == 640 { MainMenuMovieBase::Ra2tsS } else { MainMenuMovieBase::Ra2tsL };
    let (movie_x, movie_y) = movie_origin(screen_w, screen_h);
    let (movie_w, movie_h) = match movie_base {
        MainMenuMovieBase::Ra2tsS => (RA2TS_S_W, RA2TS_S_H),
        MainMenuMovieBase::Ra2tsL => (RA2TS_L_W, RA2TS_L_H),
    };
    MainMenuShellLayout {
        screen: RectPx::new(0, 0, screen_w, screen_h),
        movie_base,
        movie: RectPx::new(movie_x, movie_y, movie_w, movie_h),
        title: dlu_rect(425, 1, 108, 10),
        website_static: dlu_rect(447, 29, 61, 33),
        buttons: [
            MainMenuButtonRect { id: MainMenuControlId::SinglePlayer0x683, rect: dlu_rect(425, 125, 108, 23) },
            MainMenuButtonRect { id: MainMenuControlId::WwOnline0x684, rect: dlu_rect(425, 152, 108, 23) },
            MainMenuButtonRect { id: MainMenuControlId::Network0x578, rect: dlu_rect(425, 179, 108, 23) },
            MainMenuButtonRect { id: MainMenuControlId::MoviesAndCredits0x686, rect: dlu_rect(425, 206, 108, 23) },
            MainMenuButtonRect { id: MainMenuControlId::Options0x55c, rect: dlu_rect(425, 233, 108, 23) },
            MainMenuButtonRect { id: MainMenuControlId::ExitGame0x3ee, rect: dlu_rect(425, 330, 108, 23) },
        ],
    }
}
```

**Step 6: Add tests for layout.**

```rust
#[test]
fn key_rects_match_800x600() {
    let layout = compute_layout(800, 600);
    assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsL);
    assert_eq!(layout.movie, RectPx::new(0, 0, 632, 570));
    assert_eq!(layout.buttons[0].rect, RectPx::new(638, 203, 162, 37));
    assert_eq!(layout.buttons[5].rect, RectPx::new(638, 536, 162, 37));
}

#[test]
fn key_rects_match_640x480_movie_choice() {
    let layout = compute_layout(640, 480);
    assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsS);
    assert_eq!(layout.movie, RectPx::new(0, 0, 472, 450));
}

#[test]
fn movie_centers_against_800x600_shell_on_large_screens() {
    let layout = compute_layout(1024, 768);
    assert_eq!(layout.movie, RectPx::new(112, 84, 632, 570));
}
```

**Step 7: Verify.**
Run: `cargo test main_menu_shell::layout -- --nocapture`
Expected: PASS.

**Step 8: Commit.**

### Task 3: Add main-menu shell state and hit testing

**Why:** Input/action contracts must exist before app event routing uses them.

**Files:**
- Create: `src/ui/main_menu_shell/state.rs`
- Modify: `src/ui/main_menu_shell/mod.rs`

**Pattern:** Mirror `src/ui/skirmish_shell/state.rs` owner-draw hit testing and action mapping.

**Step 1: Define control and action enums.**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuControlId {
    SinglePlayer0x683,
    WwOnline0x684,
    Network0x578,
    MoviesAndCredits0x686,
    Options0x55c,
    ExitGame0x3ee,
    Title0x694,
    Movie0x71a,
    Website0x71c,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuShellAction {
    None,
    SinglePlayer,
    WwOnline,
    Network,
    MoviesAndCredits,
    Options,
    ExitGame,
    YuriWebsite,
}
```

**Step 2: Define state.**

```rust
#[derive(Debug, Clone, Default)]
pub struct MainMenuShellState {
    pub pressed_button: Option<MainMenuControlId>,
}
```

**Step 3: Add mappings.**

```rust
pub fn action_for_control(id: MainMenuControlId) -> MainMenuShellAction {
    match id {
        MainMenuControlId::SinglePlayer0x683 => MainMenuShellAction::SinglePlayer,
        MainMenuControlId::WwOnline0x684 => MainMenuShellAction::WwOnline,
        MainMenuControlId::Network0x578 => MainMenuShellAction::Network,
        MainMenuControlId::MoviesAndCredits0x686 => MainMenuShellAction::MoviesAndCredits,
        MainMenuControlId::Options0x55c => MainMenuShellAction::Options,
        MainMenuControlId::ExitGame0x3ee => MainMenuShellAction::ExitGame,
        MainMenuControlId::Website0x71c => MainMenuShellAction::YuriWebsite,
        MainMenuControlId::Title0x694 | MainMenuControlId::Movie0x71a => MainMenuShellAction::None,
    }
}

pub fn return_code_for_action(action: MainMenuShellAction) -> Option<i32> {
    match action {
        MainMenuShellAction::SinglePlayer => Some(1),
        MainMenuShellAction::WwOnline => Some(2),
        MainMenuShellAction::Network => Some(3),
        MainMenuShellAction::MoviesAndCredits => Some(4),
        MainMenuShellAction::Options => Some(5),
        MainMenuShellAction::ExitGame => Some(6),
        MainMenuShellAction::None | MainMenuShellAction::YuriWebsite => None,
    }
}
```

**Step 4: Add label and tooltip keys.**

```rust
pub fn csf_key_for_control(id: MainMenuControlId) -> Option<&'static str> {
    match id {
        MainMenuControlId::SinglePlayer0x683 => Some("GUI:SinglePlayer"),
        MainMenuControlId::WwOnline0x684 => Some("GUI:WWOnline"),
        MainMenuControlId::Network0x578 => Some("GUI:Network"),
        MainMenuControlId::MoviesAndCredits0x686 => Some("GUI:MoviesAndCredits"),
        MainMenuControlId::Options0x55c => Some("GUI:Options"),
        MainMenuControlId::ExitGame0x3ee => Some("GUI:ExitGame"),
        MainMenuControlId::Title0x694 => Some("GUI:MainMenu"),
        _ => None,
    }
}
```

**Step 5: Add hit-test helpers.**

```rust
pub fn hit_test_owner_draw_button(layout: &MainMenuShellLayout, x: i32, y: i32) -> Option<MainMenuControlId> {
    layout.buttons.iter().find(|button| button.rect.contains(x, y)).map(|button| button.id)
}

pub fn mouse_down(state: &mut MainMenuShellState, layout: &MainMenuShellLayout, x: i32, y: i32) {
    state.pressed_button = hit_test_owner_draw_button(layout, x, y);
}

pub fn mouse_up(state: &mut MainMenuShellState, layout: &MainMenuShellLayout, x: i32, y: i32) -> MainMenuShellAction {
    let pressed = state.pressed_button.take();
    let released = hit_test_owner_draw_button(layout, x, y);
    match (pressed, released) {
        (Some(a), Some(b)) if a == b => action_for_control(a),
        _ => MainMenuShellAction::None,
    }
}
```

**Step 6: Add tests.**

```rust
#[test]
fn actions_preserve_original_return_codes() {
    assert_eq!(return_code_for_action(MainMenuShellAction::SinglePlayer), Some(1));
    assert_eq!(return_code_for_action(MainMenuShellAction::WwOnline), Some(2));
    assert_eq!(return_code_for_action(MainMenuShellAction::Network), Some(3));
    assert_eq!(return_code_for_action(MainMenuShellAction::MoviesAndCredits), Some(4));
    assert_eq!(return_code_for_action(MainMenuShellAction::Options), Some(5));
    assert_eq!(return_code_for_action(MainMenuShellAction::ExitGame), Some(6));
}

#[test]
fn release_must_match_pressed_button() {
    let layout = compute_layout(800, 600);
    let mut state = MainMenuShellState::default();
    mouse_down(&mut state, &layout, 639, 204);
    let action = mouse_up(&mut state, &layout, 639, 248);
    assert_eq!(action, MainMenuShellAction::None);
}
```

**Step 7: Verify.**
Run: `cargo test main_menu_shell::state -- --nocapture`
Expected: PASS.

**Step 8: Commit.**

### Task 4: Add `render/main_menu_shell_chrome.rs` atlas types

**Why:** Asset interfaces come before renderer composition and resource loading.

**Files:**
- Create: `src/render/main_menu_shell_chrome.rs`
- Modify: `src/render/mod.rs`

**Pattern:** Mirror the `SkirmishShellChromeEntry`/atlas shape in `src/render/skirmish_shell_chrome.rs`.

**Step 1: Add module export.**

```rust
pub mod main_menu_shell_chrome;
```

**Step 2: Add entry and atlas types.**

```rust
//! Initial main-menu shell chrome atlas for dialog 0xE2 owner-draw buttons.

use crate::assets::asset_manager::AssetManager;
use crate::assets::pcx_file::PcxFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;

const ATLAS_PADDING: u32 = 2;

#[derive(Debug, Clone, Copy)]
pub struct MainMenuShellChromeEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

pub struct MainMenuShellChromeAtlas {
    pub texture: BatchTexture,
    pub button_up_left_30: MainMenuShellChromeEntry,
    pub button_up_mid_30: MainMenuShellChromeEntry,
    pub button_up_right_30: MainMenuShellChromeEntry,
    pub button_down_left_30: MainMenuShellChromeEntry,
    pub button_down_mid_30: MainMenuShellChromeEntry,
    pub button_down_right_30: MainMenuShellChromeEntry,
}
```

**Step 3: Add asset name table.**

```rust
const BUTTON_PCX_NAMES: [&str; 6] = [
    "bue_li30.pcx",
    "bue_mi30.pcx",
    "bue_ri30.pcx",
    "bde_li30.pcx",
    "bde_mi30.pcx",
    "bde_ri30.pcx",
];
```

**Step 4: Add a classification test.**

```rust
#[test]
fn mandatory_owner_draw_assets_match_dialog_0xe2() {
    assert_eq!(BUTTON_PCX_NAMES, [
        "bue_li30.pcx",
        "bue_mi30.pcx",
        "bue_ri30.pcx",
        "bde_li30.pcx",
        "bde_mi30.pcx",
        "bde_ri30.pcx",
    ]);
}
```

**Step 5: Verify.**
Run: `cargo test mandatory_owner_draw_assets_match_dialog_0xe2 -- --nocapture`
Expected: PASS.

**Step 6: Commit.**

### Task 5: Implement main-menu shell PCX atlas loading

**Why:** The renderer needs a complete atlas before it can draw button chrome.

**Files:**
- Modify: `src/render/main_menu_shell_chrome.rs`

**Pattern:** Reuse the PCX path from `render/skirmish_shell_chrome.rs`, but make all six assets mandatory for this atlas.

**Step 1: Add a rendered entry type.**

```rust
struct RenderedChromeEntry {
    label: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
```

**Step 2: Add PCX loading helper.**

```rust
fn render_pcx_entry(assets: &AssetManager, name: &str) -> Option<RenderedChromeEntry> {
    let bytes = assets.get_ref(name)?;
    let pcx = PcxFile::from_bytes(bytes).map_err(|err| log::warn!("Failed to parse {name}: {err}")).ok()?;
    let rgba = pcx.to_rgba(Some(0));
    Some(RenderedChromeEntry {
        label: name.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba,
    })
}
```

**Step 3: Add atlas packer using `ATLAS_PADDING`.**

Use the same shelf packing logic as `skirmish_shell_chrome::pack_entries`: place entries left-to-right, wrap when exceeding 1024 px, copy RGBA pixels into one atlas, and return `(BatchTexture, Vec<MainMenuShellChromeEntry>)`.

**Step 4: Add builder.**

```rust
pub fn build_main_menu_shell_chrome_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
) -> Option<MainMenuShellChromeAtlas> {
    let mut rendered = Vec::new();
    for name in BUTTON_PCX_NAMES {
        let entry = render_pcx_entry(assets, name).or_else(|| {
            log::warn!("Missing mandatory main-menu shell asset {name}");
            None
        })?;
        rendered.push(entry);
    }
    let (texture, packed) = pack_entries(gpu, batch, &rendered)?;
    Some(MainMenuShellChromeAtlas {
        texture,
        button_up_left_30: packed[0],
        button_up_mid_30: packed[1],
        button_up_right_30: packed[2],
        button_down_left_30: packed[3],
        button_down_mid_30: packed[4],
        button_down_right_30: packed[5],
    })
}
```

**Step 5: Verify.**
Run: `cargo test main_menu_shell_chrome -- --nocapture`
Expected: PASS.

**Step 6: Commit.**

### Task 6: Add reusable Bink RGBA conversion module

**Why:** Movie texture upload needs RGBA frames, and this pure conversion can be tested before GPU integration.

**Files:**
- Create: `src/render/bink_movie.rs`
- Modify: `src/render/mod.rs`

**Pattern:** Move `frame_to_rgba`, `yuv_to_rgb_mpeg`, and `yuv_to_rgb_jpeg` from `src/bin/bik_player_playback.rs` into a library module.

**Step 1: Export module.**

```rust
pub mod bink_movie;
```

**Step 2: Add conversion function.**

```rust
use crate::assets::bink_decode::{BinkFrame, ColorRange};

pub fn frame_to_rgba(frame: &BinkFrame) -> Vec<u8> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let yv = frame.y[y * frame.stride_y + x] as i32;
            let uv_off = (y / 2) * frame.stride_uv + (x / 2);
            let u = frame.u[uv_off] as i32;
            let v = frame.v[uv_off] as i32;
            let (r, g, b) = match frame.color_range {
                ColorRange::Mpeg => yuv_to_rgb_mpeg(yv, u, v),
                ColorRange::Jpeg => yuv_to_rgb_jpeg(yv, u, v),
            };
            let base = (y * w + x) * 4;
            out[base] = r;
            out[base + 1] = g;
            out[base + 2] = b;
            out[base + 3] = 255;
        }
    }
    out
}
```

**Step 3: Keep existing conversion tests.**

Move the `mpeg_black_and_white`, `jpeg_black_and_white`, and `jpeg_mid_grey` tests from the bin helper to `render/bink_movie.rs`.

**Step 4: Update the Bink player bin to import from the library.**

```rust
use vera20k::render::bink_movie::frame_to_rgba;
```

Remove the duplicate conversion functions from `src/bin/bik_player_playback.rs`.

**Step 5: Verify.**
Run: `cargo test bink_movie -- --nocapture`
Expected: PASS.

**Step 6: Commit.**

### Task 7: Implement `BinkMovieSurface` construction

**Why:** Startup shell resources need to load RA2TS, decode the first frame, and create an updatable GPU texture.

**Files:**
- Modify: `src/render/bink_movie.rs`

**Pattern:** Reuse `BinkFile::parse`, `BinkDecoder::new`, and `BatchRenderer::create_updatable_texture`.

**Step 1: Add types.**

```rust
use std::sync::Arc;
use crate::assets::bink_decode::BinkDecoder;
use crate::assets::bink_file::BinkFile;
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;

pub enum BinkMovieStep {
    Unchanged,
    FrameUploaded,
    Ended,
}

pub struct BinkMovieSurface {
    file: BinkFile,
    decoder: BinkDecoder,
    current_frame: usize,
    accumulator_secs: f64,
    texture: wgpu::Texture,
    batch_texture: BatchTexture,
    rgba: Vec<u8>,
    looping: bool,
    source_archive: String,
}
```

**Step 2: Add constructor.**

```rust
impl BinkMovieSurface {
    pub fn from_bytes(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        bytes: Arc<[u8]>,
        source_archive: String,
        looping: bool,
    ) -> Result<Self, crate::assets::error::AssetError> {
        let file = BinkFile::parse(bytes)?;
        let mut decoder = BinkDecoder::new(&file.header)?;
        let first_packet = file.video_packet(0)?;
        let frame = decoder.decode_frame(first_packet)?;
        let rgba = frame_to_rgba(frame);
        let (texture, batch_texture) = batch.create_updatable_texture(
            gpu,
            &rgba,
            file.header.width,
            file.header.height,
        );
        Ok(Self {
            file,
            decoder,
            current_frame: 1,
            accumulator_secs: 0.0,
            texture,
            batch_texture,
            rgba,
            looping,
            source_archive,
        })
    }
}
```

**Step 3: Add accessors.**

```rust
pub fn batch_texture(&self) -> &BatchTexture { &self.batch_texture }
pub fn width(&self) -> u32 { self.file.header.width }
pub fn height(&self) -> u32 { self.file.header.height }
pub fn fps(&self) -> f64 { self.file.header.fps() }
pub fn frame_count(&self) -> usize { self.file.frame_index.len() }
pub fn source_archive(&self) -> &str { &self.source_archive }
```

**Step 4: Verify.**
Run: `cargo check`
Expected: no errors from `render::bink_movie`.

**Step 5: Commit.**

### Task 8: Implement Bink frame stepping and loop reset

**Why:** RA2TS timing is the main continuously visible behavior on the screen.

**Files:**
- Modify: `src/render/bink_movie.rs`

**Pattern:** Follow `Playback::step` in `src/bin/bik_player_playback.rs`, but upload changed RGBA data to the existing texture.

**Step 1: Add an upload helper.**

```rust
fn upload_rgba(&self, gpu: &GpuContext) {
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &self.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &self.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(self.width() * 4),
            rows_per_image: Some(self.height()),
        },
        wgpu::Extent3d {
            width: self.width(),
            height: self.height(),
            depth_or_array_layers: 1,
        },
    );
}
```

**Step 2: Add loop reset.**

```rust
fn restart_at_original_frame_one(&mut self) -> Result<(), crate::assets::error::AssetError> {
    self.decoder.flush();
    let pkt = self.file.video_packet(0)?;
    let frame = self.decoder.decode_frame(pkt)?;
    self.rgba = frame_to_rgba(frame);
    self.current_frame = 1;
    self.accumulator_secs = 0.0;
    Ok(())
}
```

**Step 3: Add `step`.**

```rust
pub fn step(&mut self, gpu: &GpuContext, elapsed_secs: f64) -> Result<BinkMovieStep, crate::assets::error::AssetError> {
    let fps = self.fps();
    if fps <= 0.0 {
        return Ok(BinkMovieStep::Unchanged);
    }
    self.accumulator_secs += elapsed_secs.max(0.0);
    let frame_dt = 1.0 / fps;
    let mut changed = false;
    let mut decoded_this_step = 0usize;
    while self.accumulator_secs >= frame_dt && decoded_this_step < 4 {
        self.accumulator_secs -= frame_dt;
        if self.current_frame >= self.frame_count() {
            if self.looping {
                self.restart_at_original_frame_one()?;
                changed = true;
                break;
            }
            return Ok(BinkMovieStep::Ended);
        }
        let pkt = self.file.video_packet(self.current_frame)?;
        let frame = self.decoder.decode_frame(pkt)?;
        self.rgba = frame_to_rgba(frame);
        self.current_frame += 1;
        changed = true;
        decoded_this_step += 1;
    }
    if changed {
        self.upload_rgba(gpu);
        Ok(BinkMovieStep::FrameUploaded)
    } else {
        Ok(BinkMovieStep::Unchanged)
    }
}
```

**Step 4: Add a pure clock test if GPU-free tests are needed.**

If `BinkMovieSurface` cannot be instantiated in unit tests without GPU, add a private `fn frames_due(accumulator: &mut f64, elapsed: f64, fps: f64, max: usize) -> usize` and test:

```rust
#[test]
fn frame_clock_uses_fps_not_timer_interval() {
    let mut acc = 0.0;
    assert_eq!(frames_due(&mut acc, 0.034, 15.0, 4), 0);
    assert_eq!(frames_due(&mut acc, 0.033, 15.0, 4), 1);
}
```

**Step 5: Verify.**
Run: `cargo test bink_movie -- --nocapture`
Expected: PASS.

**Step 6: Commit.**

### Task 9: Add main-menu shell render helper scaffolding

**Why:** App-level rendering needs isolated helpers before wiring into `App::render_frame`.

**Files:**
- Create: `src/app_main_menu_shell_render.rs`
- Modify: `src/lib.rs`

**Pattern:** Mirror file organization from `src/app_skirmish_shell_render.rs`.

**Step 1: Export module.**

```rust
pub mod app_main_menu_shell_render;
```

**Step 2: Add constants and imports.**

```rust
//! Initial main-menu shell render glue for dialog 0xE2.

use anyhow::Result;

use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::{MainMenuShellChromeAtlas, MainMenuShellChromeEntry};
use crate::ui::main_menu_shell::{MainMenuControlId, MainMenuShellLayout, RectPx, compute_layout};

const MOVIE_DEPTH: f32 = 0.00095;
const BUTTON_DEPTH: f32 = 0.00080;
const TEXT_DEPTH: f32 = 0.00070;
const PRESSED_BUTTON_CONTENT_OFFSET_Y: i32 = 2;
const SHELL_BUTTON_TEXT_RGB_00000C05: [f32; 3] = [0.0, 12.0 / 255.0, 5.0 / 255.0];
```

**Step 3: Add `push_entry_sized` helper.**

```rust
fn push_entry_sized(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
) {
    out.push(SpriteInstance {
        position: [x, y],
        size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}
```

**Step 4: Verify.**
Run: `cargo check`
Expected: module compiles.

**Step 5: Commit.**

### Task 10: Implement owner-draw button segment composition for main menu

**Why:** The six right buttons need the verified cap/middle/cap PCX tiling and pressed art.

**Files:**
- Modify: `src/app_main_menu_shell_render.rs`

**Pattern:** Copy the current `build_button_segments`/`push_button_30` logic from `app_skirmish_shell_render.rs` first; consolidate later only after both call sites are stable.

**Step 1: Add segment types.**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonPiece { Left, Middle, Right }

#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonSegment {
    piece: ButtonPiece,
    x: f32,
    width: f32,
    uv_width_ratio: f32,
}
```

**Step 2: Add button entry selector.**

```rust
fn button_entries(
    atlas: &MainMenuShellChromeAtlas,
    pressed: bool,
) -> (
    MainMenuShellChromeEntry,
    MainMenuShellChromeEntry,
    MainMenuShellChromeEntry,
) {
    if pressed {
        (atlas.button_down_left_30, atlas.button_down_mid_30, atlas.button_down_right_30)
    } else {
        (atlas.button_up_left_30, atlas.button_up_mid_30, atlas.button_up_right_30)
    }
}
```

**Step 3: Add segment builder and `push_button_30`.**

Use the same `build_button_segments(rect, left_w, mid_w, right_w)` implementation from Skirmish shell. For partial middle segments, shrink `entry.uv_size[0]` by `segment.uv_width_ratio` before pushing.

**Step 4: Add tests.**

```rust
#[test]
fn button_segments_tile_middle_and_keep_caps() {
    let rect = RectPx::new(638, 203, 162, 37);
    let segments = build_button_segments(rect, 10.0, 8.0, 10.0);
    assert_eq!(segments.first().unwrap().piece, ButtonPiece::Left);
    assert_eq!(segments.last().unwrap().piece, ButtonPiece::Right);
    let total: f32 = segments.iter().map(|s| s.width).sum();
    assert_eq!(total.round() as i32, rect.w);
}
```

**Step 5: Verify.**
Run: `cargo test app_main_menu_shell_render -- --nocapture`
Expected: PASS.

**Step 6: Commit.**

### Task 11: Implement main-menu label drawing wrapper

**Why:** Text rendering is a known dependency risk; this helper isolates it from geometry and button chrome.

**Files:**
- Modify: `src/app_main_menu_shell_render.rs`

**Pattern:** Mirror `push_button_label_draw` in `app_skirmish_shell_render.rs`: use `render::shell_text::draw_in_rect` with `state.bit_font`, collect `ShellTextDraw` records, then draw them with `state.bit_font.atlas()` and each draw's scissor rect.

**Step 1: Add CSF label resolver.**

```rust
fn resolve_csf<'a>(state: &'a AppState, key: &'static str) -> &'a str {
    state.csf.as_ref().and_then(|csf| csf.get(key)).unwrap_or(key)
}
```

**Step 2: Add centered shell text helper.**

```rust
fn push_centered_label(
    out: &mut Vec<crate::render::shell_text::ShellTextDraw>,
    state: &AppState,
    text: &str,
    rect: RectPx,
    pressed: bool,
) {
    use crate::render::shell_text::{ShellAlign, TextRect};

    let y_offset = if pressed { PRESSED_BUTTON_CONTENT_OFFSET_Y } else { 0 };
    let text_rect = TextRect {
        x: rect.x,
        y: rect.y + y_offset,
        w: rect.w.max(0) as u32,
        h: rect.h.max(0) as u32,
    };
    out.push(crate::render::shell_text::draw_in_rect(
        &state.bit_font,
        text,
        text_rect,
        SHELL_BUTTON_TEXT_RGB_00000C05,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        [0.0, 0.0],
        TEXT_DEPTH,
    ));
}
```

**Step 3: Add a comment at the helper boundary only.**

```rust
// This wrapper is the only main-menu label path; keep placement fixes here.
```

**Step 4: Verify.**
Run: `cargo check`
Expected: no new errors.

**Step 5: Commit.**

### Task 12: Implement main-menu render helper builders

**Why:** This task builds all pure sprite-construction helpers before `AppState` gets the persistent movie/chrome fields in later tasks.

**Files:**
- Modify: `src/app_main_menu_shell_render.rs`

**Pattern:** Mirror `app_skirmish_shell_render.rs` helper structure, but do not add the final `render_main_menu_shell` entry point yet. That entry point needs `AppState.main_menu_movie`, `main_menu_movie_last_step`, and `ensure_movie_for_current_layout`, which are introduced in Tasks 14 and 15.

**Step 1: Add `build_button_instances`.**

```rust
fn build_button_instances(
    atlas: &MainMenuShellChromeAtlas,
    layout: &MainMenuShellLayout,
    pressed_button: Option<MainMenuControlId>,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    for button in &layout.buttons {
        push_button_30(
            &mut out,
            atlas,
            button.rect,
            pressed_button == Some(button.id),
            BUTTON_DEPTH,
        );
    }
    out
}
```

**Step 2: Add `build_text_draws`.**

For each button, resolve `csf_key_for_control(button.id)`, call `push_centered_label`, and pass pressed state. Also draw title `GUI:MainMenu` centered in `layout.title`. Return `Vec<ShellTextDraw>`, not raw `SpriteInstance`, because shell text carries a per-rect scissor.

**Step 3: Add movie sprite.**

```rust
fn movie_instance(layout: &MainMenuShellLayout) -> SpriteInstance {
    SpriteInstance {
        position: [layout.movie.x as f32, layout.movie.y as f32],
        size: [layout.movie.w as f32, layout.movie.h as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: MOVIE_DEPTH,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }
}
```

**Step 4: Add a `build_movie_instances` helper that takes layout only.**

```rust
fn build_movie_instances(layout: &MainMenuShellLayout) -> Vec<SpriteInstance> {
    vec![movie_instance(layout)]
}
```

Do not call `ensure_movie_for_current_layout`, `state.main_menu_movie`, or `state.main_menu_movie_last_step` in Task 12.

**Step 5: Verify.**
Run: `cargo check`
Expected: render helper module compiles without requiring any main-menu `AppState` fields.

**Step 6: Commit.**

### Task 13: Add startup asset/audio helpers

**Why:** Native main menu needs assets, CSF, GAME.FNT, sound registry, and audio bags before map load.

**Files:**
- Modify: `src/app_transitions.rs`
- Modify: `src/app_init.rs`
- Modify: `src/app.rs`

**Pattern:** Reuse existing map-load helpers instead of duplicating sound/CSF parsing.

**Step 1: Make these helpers `pub(crate)` in `app_transitions.rs`.**

```rust
pub(crate) fn load_sound_registry(assets: &AssetManager) -> SoundRegistry { ... }
pub(crate) fn load_audio_indices(assets: &AssetManager) -> Vec<AudioIndex> { ... }
pub(crate) fn load_eva_registry(assets: &AssetManager) -> EvaRegistry { ... }
```

Import concrete types at the top of `app_transitions.rs` rather than spelling full paths repeatedly.

**Step 2: Make `app_init::load_csf` reusable.**

```rust
pub(crate) fn load_csf(asset_manager: &AssetManager) -> Option<crate::assets::csf_file::CsfFile> { ... }
```

**Step 3: Add an `App::build_startup_asset_manager` helper in `app.rs`.**

```rust
fn build_startup_asset_manager(game_config: Option<&GameConfig>) -> Option<AssetManager> {
    game_config.and_then(|cfg| match AssetManager::new(&cfg.paths.ra2_dir) {
        Ok(manager) => Some(manager),
        Err(err) => {
            log::warn!("Could not load startup shell assets: {err:#}");
            None
        }
    })
}
```

**Step 4: In `initialize`, replace the dev-only `startup_asset_manager` construction with this helper.**

It should run for normal startup, not only `RA2_DEV_SKIRMISH_SHELL`.

**Step 5: Use the startup asset manager to set initial `rules`, `csf`, `sound_registry`, `audio_indices`, and FNT-backed `bit_font`.**

```rust
let startup_rules = startup_asset_manager.as_ref().and_then(crate::app_init_helpers::load_rules_ini);
let startup_csf = startup_asset_manager.as_ref().and_then(crate::app_init::load_csf);
let startup_sound_registry = startup_asset_manager
    .as_ref()
    .map(crate::app_transitions::load_sound_registry)
    .unwrap_or_default();
let startup_audio_indices = startup_asset_manager
    .as_ref()
    .map(crate::app_transitions::load_audio_indices)
    .unwrap_or_default();
let bit_font = startup_asset_manager
    .as_ref()
    .and_then(|assets| assets.get_ref("GAME.FNT"))
    .and_then(|data| crate::assets::fnt_file::FntFile::from_bytes(data).ok())
    .map(|fnt| crate::render::bit_font::BitFont::from_fnt(&gpu, &batch_renderer, &fnt))
    .unwrap_or_else(|| crate::render::bit_font::BitFont::fallback_5x7(&gpu, &batch_renderer));
```

This replaces the current early fallback-only `let bit_font = BitFont::fallback_5x7(...)` initialization. Keep the existing fallback behavior when `GAME.FNT` is missing or fails to parse.

**Step 6: Store the startup asset manager in `AppState`.**

After all startup `as_ref()` loading is complete, move the manager into the `AppState` initializer:

```rust
asset_manager: startup_asset_manager,
rules: startup_rules,
csf: startup_csf,
sound_registry: startup_sound_registry,
audio_indices: startup_audio_indices,
```

This must replace the current `asset_manager: None`, `rules: None`, `csf: None`, `sound_registry: SoundRegistry::default()`, and `audio_indices: Vec::new()` startup values. Map loading may still replace these fields later in `transition_to_in_game`.

**Step 7: Verify.**
Run: `cargo check`
Expected: no visibility or borrow errors.

**Step 8: Commit.**

### Task 14: Add main-menu shell resources to `AppState`

**Why:** Render/input logic needs persistent chrome, movie, movie base, and shell state.

**Files:**
- Modify: `src/app.rs`

**Pattern:** Follow existing `skirmish_shell_state` and `skirmish_shell_chrome` fields.

**Step 1: Add fields to `AppState`.**

```rust
pub(crate) main_menu_shell_state: crate::ui::main_menu_shell::MainMenuShellState,
pub(crate) main_menu_shell_chrome: Option<crate::render::main_menu_shell_chrome::MainMenuShellChromeAtlas>,
pub(crate) main_menu_movie: Option<crate::render::bink_movie::BinkMovieSurface>,
pub(crate) main_menu_movie_base: Option<crate::ui::main_menu_shell::MainMenuMovieBase>,
pub(crate) main_menu_movie_last_step: Instant,
pub(crate) main_menu_shell_failed: bool,
pub(crate) main_menu_show_skirmish_setup: bool,
```

**Step 2: Initialize fields in `AppState` construction.**

```rust
let main_menu_shell_chrome = startup_asset_manager.as_ref().and_then(|assets| {
    crate::render::main_menu_shell_chrome::build_main_menu_shell_chrome_atlas(&gpu, &batch_renderer, assets)
});
let main_menu_shell_failed = startup_asset_manager.is_none() || main_menu_shell_chrome.is_none();
```

Set `main_menu_movie` and `main_menu_movie_base` to `None`; a later task lazily loads the correct RA2TS variant after layout is known.

Set `main_menu_show_skirmish_setup` to `false`. This flag is the explicit interim route from the native initial shell's Single Player button into the existing egui skirmish setup flow; it is separate from `main_menu_shell_failed`, which is only for asset/load fallback.

**Step 3: Preserve dev Skirmish shell behavior.**

Keep existing `dev_skirmish_shell_enabled` field and loading branch unchanged except that it can reuse the same `startup_asset_manager`.

**Step 4: Verify.**
Run: `cargo check`
Expected: no missing field errors.

**Step 5: Commit.**

### Task 15: Implement RA2TS movie resource selection

**Why:** The main menu must load `ra2ts_s.bik` only at 640-wide, otherwise `ra2ts_l.bik`, and log source archive.

**Files:**
- Modify: `src/app_main_menu_shell_render.rs`
- Modify: `src/app.rs`

**Pattern:** Use layout's `MainMenuMovieBase::asset_name()` and `AssetManager::get_with_source_ref`. Main-menu shell layout uses swapchain/window pixels (`state.gpu.config.width` / `height`), not `state.render_width()` / `render_height()`, because the menu is drawn directly to the surface even when the in-game upscale pass is enabled.

**Step 1: Add helper in `app_main_menu_shell_render.rs`.**

```rust
pub(crate) fn ensure_movie_for_current_layout(state: &mut AppState) -> Result<()> {
    let layout = compute_layout(state.gpu.config.width, state.gpu.config.height);
    if state.main_menu_movie_base == Some(layout.movie_base) && state.main_menu_movie.is_some() {
        return Ok(());
    }
    let Some(assets) = state.asset_manager.as_ref() else {
        state.main_menu_shell_failed = true;
        return Ok(());
    };
    let asset_name = layout.movie_base.asset_name();
    let Some((bytes, source)) = assets.get_with_source_ref(asset_name) else {
        log::warn!("Missing main-menu RA2TS movie asset {asset_name}");
        state.main_menu_shell_failed = true;
        return Ok(());
    };
    let movie = match crate::render::bink_movie::BinkMovieSurface::from_bytes(
        &state.gpu,
        &state.batch_renderer,
        std::sync::Arc::<[u8]>::from(bytes),
        source.to_string(),
        true,
    ) {
        Ok(movie) => movie,
        Err(err) => {
            log::warn!("Failed to load main-menu RA2TS movie {asset_name} from {source}: {err:#}");
            state.main_menu_shell_failed = true;
            return Ok(());
        }
    };
    log::info!("Loaded {asset_name} for main menu from {}", movie.source_archive());
    state.main_menu_movie = Some(movie);
    state.main_menu_movie_base = Some(layout.movie_base);
    state.main_menu_movie_last_step = Instant::now();
    Ok(())
}
```

**Step 2: Ensure helper is called before native render uses the movie.**

If it sets `main_menu_shell_failed`, return an explicit fallback result before clearing the swapchain target. The caller must draw the existing egui fallback in the same frame so a missing/corrupt RA2TS asset never produces a blank frame.

**Step 3: Add the final `render_main_menu_shell` entry point now that movie state exists.**

```rust
pub(crate) enum MainMenuShellRenderResult {
    Rendered,
    Fallback,
}

pub(crate) fn render_main_menu_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
) -> Result<MainMenuShellRenderResult> {
    ensure_movie_for_current_layout(state)?;
    if state.main_menu_shell_failed {
        return Ok(MainMenuShellRenderResult::Fallback);
    }

    crate::app_transitions::clear_screen(encoder, target);

    if let Some(movie) = state.main_menu_movie.as_mut() {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(state.main_menu_movie_last_step).as_secs_f64();
        state.main_menu_movie_last_step = now;
        if let Err(err) = movie.step(&state.gpu, elapsed) {
            log::warn!("Failed to step main-menu RA2TS movie: {err:#}");
            state.main_menu_shell_failed = true;
            return Ok(MainMenuShellRenderResult::Fallback);
        }
    }

    let layout = compute_layout(state.gpu.config.width, state.gpu.config.height);
    // Build and draw batches in order: movie, buttons, text.
    Ok(MainMenuShellRenderResult::Rendered)
}
```

Use `BatchRenderer::create_instance_buffer` and `draw_with_buffer_passthrough` for each texture group:
- movie uses `movie.batch_texture()`
- buttons use `chrome.texture`
- text uses `state.bit_font.atlas()` and each `ShellTextDraw.scissor` from `render::shell_text::draw_in_rect`

**Step 4: Add an archive priority runtime check log.**

When `asset_name == "ra2ts_l.bik"` and `source` is not `language.mix`, log:

```rust
log::warn!("ra2ts_l.bik resolved from {source}; retail duplicate priority expected language.mix when both language.mix and langmd.mix contain the file");
```

**Step 5: Verify.**
Run: `cargo check`
Expected: no lifetime issues around `bytes`.

**Step 6: Commit.**

### Task 16: Wire native render branch into `GameScreen::MainMenu`

**Why:** This switches the default first screen from egui to native `0xE2`.

**Files:**
- Modify: `src/app.rs`

**Pattern:** Keep existing `dev_skirmish_shell_enabled` branch first, then native `0xE2`, then egui fallback.

**Step 1: Update `GameScreen::MainMenu` render branch.**

First extract the existing egui setup/fallback branch body into a small local helper, for example `Self::render_egui_main_menu_fallback(...)`, so the explicit movie-load fallback and the normal fallback path render identical UI. The helper must either accept `event_loop` and call `Self::handle_main_menu_action` internally, or return the `main_menu::MenuAction` for the caller to dispatch; do not drop fallback menu actions.

```rust
GameScreen::MainMenu => {
    if state.dev_skirmish_shell_enabled {
        // existing Skirmish shell branch unchanged
    } else if !state.main_menu_shell_failed && !state.main_menu_show_skirmish_setup {
        match crate::app_main_menu_shell_render::render_main_menu_shell(state, &mut encoder, &view)? {
            crate::app_main_menu_shell_render::MainMenuShellRenderResult::Rendered => {
                state.egui.begin_frame(&state.window);
                state.egui.end_frame_and_render(
                    &state.gpu,
                    &mut encoder,
                    &view,
                    &state.window,
                    state.use_software_cursor(),
                );
            }
            crate::app_main_menu_shell_render::MainMenuShellRenderResult::Fallback => {
                Self::render_egui_main_menu_fallback(state, &mut encoder, &view, event_loop)?;
            }
        }
    } else {
        Self::render_egui_main_menu_fallback(state, &mut encoder, &view, event_loop)?;
    }
}
```

**Step 2: Do not draw the dev shell toggle over native `0xE2`.**

The dev toggle stays available in the egui fallback branch and via env-enabled dev shell path. Native parity screen should not have in-app dev UI over it.

**Step 3: Verify.**
Run: `cargo check`
Expected: no borrow errors in render branch.

**Step 4: Commit.**

### Task 17: Wire native main-menu mouse input and action handling

**Why:** Buttons must visually press, activate only on matching release, play sound, and dispatch original actions.

**Files:**
- Modify: `src/app.rs`

**Pattern:** Mirror existing Skirmish shell mouse handlers.

**Step 1: Keep main-menu cursor coordinates in swapchain/window pixels.**

Current `CursorMoved` remaps coordinates whenever `upscale_pass` is active. Change that remap so it applies only to screens rendered in the in-game source coordinate space:

```rust
let use_render_source_coords = state.upscale_pass.is_some()
    && (state.screen == GameScreen::InGame || state.screen == GameScreen::SpawnPick);
let (sx, sy) = if use_render_source_coords {
    (
        state.render_width() as f32 / state.gpu.config.width as f32,
        state.render_height() as f32 / state.gpu.config.height as f32,
    )
} else {
    (1.0, 1.0)
};
```

Native main-menu hit testing must use the same swapchain/window pixel space used by its render pass.

**Step 2: Add mouse handlers.**

```rust
fn handle_main_menu_shell_mouse_down(state: &mut AppState) {
    let layout = crate::ui::main_menu_shell::compute_layout(state.gpu.config.width, state.gpu.config.height);
    crate::ui::main_menu_shell::mouse_down(
        &mut state.main_menu_shell_state,
        &layout,
        state.cursor_x.round() as i32,
        state.cursor_y.round() as i32,
    );
    if crate::ui::main_menu_shell::hit_test_owner_draw_button(
        &layout,
        state.cursor_x.round() as i32,
        state.cursor_y.round() as i32,
    )
    .is_some()
    {
        Self::play_main_menu_button_sound(state);
    }
}

fn handle_main_menu_shell_mouse_up(state: &mut AppState, event_loop: &ActiveEventLoop) {
    let layout = crate::ui::main_menu_shell::compute_layout(state.gpu.config.width, state.gpu.config.height);
    let action = crate::ui::main_menu_shell::mouse_up(
        &mut state.main_menu_shell_state,
        &layout,
        state.cursor_x.round() as i32,
        state.cursor_y.round() as i32,
    );
    Self::handle_main_menu_shell_action(state, action, event_loop);
}
```

**Step 3: Add direct UI sound helper.**

```rust
fn play_main_menu_button_sound(state: &mut AppState) {
    let Some(sound_id) = state.rules.as_ref().and_then(|r| r.general.gui_main_button_sound.as_deref()) else {
        return;
    };
    let (Some(sfx), Some(assets)) = (&mut state.sfx_player, &state.asset_manager) else {
        return;
    };
    sfx.play_sound(sound_id, &state.sound_registry, assets, &state.audio_indices);
}
```

**Step 4: Add action handler.**

```rust
fn handle_main_menu_shell_action(
    state: &mut AppState,
    action: crate::ui::main_menu_shell::MainMenuShellAction,
    event_loop: &ActiveEventLoop,
) {
    use crate::ui::main_menu_shell::MainMenuShellAction;
    match action {
        MainMenuShellAction::None => {}
        MainMenuShellAction::ExitGame => event_loop.exit(),
        MainMenuShellAction::SinglePlayer => {
            // Interim route until downstream Single Player shell is implemented.
            // Preserve action identity, then show the existing skirmish setup flow.
            state.main_menu_show_skirmish_setup = true;
        }
        MainMenuShellAction::WwOnline
        | MainMenuShellAction::Network
        | MainMenuShellAction::MoviesAndCredits
        | MainMenuShellAction::Options
        | MainMenuShellAction::YuriWebsite => {
            log::info!("Main-menu shell action {:?} is preserved but downstream dialog is not implemented yet", action);
        }
    }
}
```

**Step 5: Route mouse input.**

In `WindowEvent::MouseInput`, before the dev Skirmish shell branch or as its normal-main-menu sibling:

```rust
if state.screen == GameScreen::MainMenu
    && !state.dev_skirmish_shell_enabled
    && !state.main_menu_shell_failed
    && !state.main_menu_show_skirmish_setup
    && !egui_consumed
{
    if button == MouseButton::Left {
        if btn_state.is_pressed() {
            Self::handle_main_menu_shell_mouse_down(state);
        } else {
            Self::handle_main_menu_shell_mouse_up(state, event_loop);
        }
    }
}
```

**Step 6: Verify.**
Run: `cargo check`
Expected: no borrow/action routing errors.

**Step 7: Commit.**

### Task 18: Handle resize boundary and movie rebuild

**Why:** `g_ScreenWidth == 640` uses `Ra2ts_s`; other widths use `Ra2ts_l`, so resizing across 640 must rebuild the movie surface.

**Files:**
- Modify: `src/app.rs`
- Modify: `src/app_main_menu_shell_render.rs`

**Pattern:** Use existing resize branch in `WindowEvent::Resized`.

**Step 1: Add a resize helper.**

```rust
fn invalidate_main_menu_movie_if_base_changed(state: &mut AppState) {
    let layout = crate::ui::main_menu_shell::compute_layout(state.gpu.config.width, state.gpu.config.height);
    if state.main_menu_movie_base.is_some_and(|base| base != layout.movie_base) {
        state.main_menu_movie = None;
        state.main_menu_movie_base = None;
    }
}
```

**Step 2: Call it after GPU resize and UI scale update.**

```rust
Self::invalidate_main_menu_movie_if_base_changed(state);
```

**Step 3: Ensure render lazily reloads after invalidation.**

`ensure_movie_for_current_layout` from Task 15 already reloads when `main_menu_movie` is `None`.

**Step 4: Verify.**
Run: `cargo check`
Expected: no errors.

**Step 5: Commit.**

### Task 19: Add compile and targeted unit verification batch

**Why:** Catch missing exports, stale imports, and pure parity logic regressions before runtime testing.

**Files:**
- No code changes unless tests fail.

**Pattern:** Same verification style as previous shell plans.

**Step 1: Run targeted tests.**

```powershell
cargo test main_menu_shell -- --nocapture
cargo test main_menu_shell_chrome -- --nocapture
cargo test bink_movie -- --nocapture
cargo test test_gui_main_button_sound_parsed -- --nocapture
```

Expected: all PASS.

**Step 2: Run compile check.**

```powershell
cargo check
```

Expected: PASS or only unrelated pre-existing failures. If unrelated failures exist, capture exact file/test names in the implementation summary and continue only if main-menu modules compile far enough to validate their code paths.

**Step 3: Search for forbidden hardcoded sound value in new main-menu code.**

```powershell
rg -n "\"MenuClick\"|GUIMainButtonSound" src/app.rs src/app_main_menu_shell_render.rs src/ui/main_menu_shell src/render/main_menu_shell_chrome.rs src/render/bink_movie.rs
```

Expected: `GUIMainButtonSound` appears in parser/tests; `"MenuClick"` appears only in tests or INI, not in app click playback logic.

**Step 4: Commit any test-only fixes.**

### Task 20: Runtime verification at 800x600 and 1024x768

**Why:** Rendering and audio are player-visible and cannot be fully proven by unit tests.

**Files:**
- No code changes unless runtime checks fail.

**Pattern:** Manual visual verification, same standard as other shell parity work.

**Step 1: Run the client with retail assets.**

```powershell
cargo run --bin ra2-rust-game
```

Expected at startup: native RA2TS movie panel and six owner-draw right-column buttons; no egui menu card on the normal path.

**Step 2: Verify log messages.**

Expected:
- main-menu button chrome atlas loaded
- `ra2ts_l.bik` loaded for non-640 width
- source archive logged; on retail duplicate install, `language.mix` is expected

**Step 3: Verify movie cadence.**

Watch RA2TS for at least 10 seconds. Expected: smooth 15 fps animation, not one frame per 34 ms timer and not 60 fps speed.

**Step 4: Verify press behavior.**

Press and hold each button. Expected:
- down PCX pieces appear
- text/content shifts down by 2 px
- releasing off the button does not activate
- releasing on the same button activates once

**Step 5: Verify audio.**

Mouse-down on a non-exit button. Expected: one `GUIMainButtonSound` sound immediately on valid button press, if sound assets/audio device are available. Dragging off before release must prevent activation but must not require a second sound.

**Step 6: Commit any runtime fixes.**

### Task 21: Runtime verification at 640-wide and resize boundary

**Why:** 640-wide mode uses `Ra2ts_s`, which has different dimensions and is selected by an exact width check.

**Files:**
- No code changes unless runtime checks fail.

**Pattern:** Manual resolution verification.

**Step 1: Launch or resize to 640-wide.**

Use the app/window configuration available in the repo. If there is no config knob for startup width, temporarily resize the window manually to 640 px width.

Expected: `ra2ts_s.bik` loads, movie rect is 472 x 450 at x=0/y=0.

**Step 2: Resize away from 640-wide.**

Expected: `ra2ts_l.bik` reloads and movie rect becomes 632 x 570 with centered offset for large window sizes.

**Step 3: Verify no stale texture dimensions.**

Expected: movie quad size matches decoded movie dimensions after each swap; no stretched old texture, no crash.

**Step 4: Commit any resize fixes.**

### Task 22: Final regression and residual parity notes

**Why:** Close the implementation with explicit known gaps and no hidden failures.

**Files:**
- Update implementation summary only; no code unless previous verification found issues.

**Pattern:** Same closeout style as prior VERA20k shell work.

**Step 1: Run final checks.**

```powershell
cargo test main_menu_shell main_menu_shell_chrome bink_movie test_gui_main_button_sound_parsed -- --nocapture
cargo check
```

Expected: PASS or documented unrelated failures.

**Step 2: Run placeholder scan for new plan-driven modules.**

```powershell
rg -n "placeholder|unimplemented!|panic!" src/ui/main_menu_shell src/render/main_menu_shell_chrome.rs src/render/bink_movie.rs src/app_main_menu_shell_render.rs
```

Expected:
- no placeholder text
- no panic-style stubs on external assets/INI/files
- any `expect()` only documents internal invariants

**Step 3: Document residuals in implementation summary.**

Mention:
- whether label rendering used completed `render::shell_text` or the isolated current-font adapter
- whether `ra2ts_l.bik` resolved from `language.mix`
- whether sound played successfully
- whether exact first-frame retail screenshot comparison remains open

**Step 4: Commit final verification fixes if needed.**

## Sources & References

- **Design doc:** [docs/plans/2026-05-17-initial-main-menu-dialog-0xe2-design.md](2026-05-17-initial-main-menu-dialog-0xe2-design.md)
- **Ghidra reports:**
  - [docs/research/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md)
  - [docs/research/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md)
  - [docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md)
  - `docs/research/SKIRMISH_OWNERDRAW_CALLBACKS_GHIDRA_REPORT.md`
  - `docs/research/SKIRMISH_OWNERDRAW_CALLBACKS_FOLLOWUP_GHIDRA_REPORT.md`
  - [docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BITFONT_SHELL_TEXT_GHIDRA_REPORT.md)
- **Live Ghidra addresses verified during planning:**
  - `FUN_00531CC0 @ 0x00531CC0` main-menu dialog creation and RA2TS child message sequence
  - `OwnerDraw_Static_006153E0 @ 0x006153E0` custom static movie messages/timer/loop
  - `OwnerDraw_Button_00612B70 @ 0x00612B70` owner-draw button PCX/text/click behavior
  - `CDFileClass__Constructor @ 0x005C0640` `.BIK` before `.VQA`
  - `VQMovieHandle__Constructor @ 0x005C07D0` Bink vs VQA branch
- **INI keys:**
  - [ini/rulesmd.ini](../../ini/rulesmd.ini) `[AudioVisual] GUIMainButtonSound=MenuClick`
  - [ini/rules.ini](../../ini/rules.ini) `[AudioVisual] GUIMainButtonSound=MenuClick`
  - [ini/soundmd.ini](../../ini/soundmd.ini) `[MenuClick]`
  - [ini/sound.ini](../../ini/sound.ini) `[MenuClick]`
- **Related code:**
  - [src/ui/skirmish_shell/layout.rs](../../src/ui/skirmish_shell/layout.rs)
  - [src/ui/skirmish_shell/state.rs](../../src/ui/skirmish_shell/state.rs)
  - [src/app_skirmish_shell_render.rs](../../src/app_skirmish_shell_render.rs)
  - [src/render/skirmish_shell_chrome.rs](../../src/render/skirmish_shell_chrome.rs)
  - [src/bin/bik_player_playback.rs](../../src/bin/bik_player_playback.rs)
  - [src/render/batch.rs](../../src/render/batch.rs)
  - [src/render/bit_font.rs](../../src/render/bit_font.rs)
  - [src/render/shell_text.rs](../../src/render/shell_text.rs)
  - [src/assets/asset_manager.rs](../../src/assets/asset_manager.rs)
- **Current git-state note:** `82f2115 render/bit_font: scaffold types + module registration` has since been extended; current `src/render/shell_text.rs` provides `draw_in_rect`, so this plan uses `AppState.bit_font` and `render::shell_text` directly.
