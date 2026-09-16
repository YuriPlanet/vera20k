# Initial Main Menu Dialog 0xE2 Design

## Goal

Replace the current egui main-menu surface with a faithful native render path for the Yuri's Revenge initial shell dialog `0xE2`: looping RA2TS Bink playback on the left, shell owner-draw buttons on the right, localized CSF button text, main-button click sound, and hit-test behavior that preserves the original button return codes.

This design is limited to the initial main menu. It does not attempt to implement the downstream Single Player, WWOnline, Network, Movies/Credits, or Options dialog trees in the same change.

## Architecture Context

### Current Rust state

- [src/ui/main_menu.rs](../../src/ui/main_menu.rs) is an egui skirmish setup placeholder. It owns `MenuAction`, `SkirmishSettings`, and the temporary map/credits controls.
- [src/app.rs](../../src/app.rs) renders `GameScreen::MainMenu` through the egui path by default, with an opt-in Skirmish shell dev toggle.
- [src/ui/skirmish_shell](../../src/ui/skirmish_shell) already has the correct split for shell work: pure layout/state plus app-level render glue.
- [src/app_skirmish_shell_render.rs](../../src/app_skirmish_shell_render.rs) already implements the owner-draw button composition pattern from `bue_*30.pcx` and `bde_*30.pcx`, including tiled middle pieces and a pressed content offset.
- [src/render/skirmish_shell_chrome.rs](../../src/render/skirmish_shell_chrome.rs) already loads the same button PCX family, but it is scoped to Skirmish dialog `0x102`.
- [src/bin/bik-player.rs](../../src/bin/bik-player.rs) and [src/bin/bik_player_playback.rs](../../src/bin/bik_player_playback.rs) prove the RA2/YR Bink parser and decoder can demux, pace, decode, and convert Bink frames to RGBA.
- [src/render/batch.rs](../../src/render/batch.rs) already provides `create_updatable_texture`, which is the right primitive for a dynamic movie texture updated with `queue.write_texture`.
- [src/assets/asset_manager.rs](../../src/assets/asset_manager.rs) currently loads `language.mix` before `langmd.mix` and uses first-match lookup. That matches the verified RA2TS duplicate priority for this case.

### Source-of-truth research

- [ra2-rust-game-docs/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_SIDEBAR_GHIDRA_REPORT.md)
- [ra2-rust-game-docs/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_VISUAL_ASSETS_GHIDRA_REPORT.md)
- [ra2-rust-game-docs/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_MENU_RA2TS_PLAYBACK_ARCHIVE_PRIORITY_GHIDRA_REPORT.md)
- [docs/plans/2026-05-17-main-menu-sidebar-investigation-plan.md](2026-05-17-main-menu-sidebar-investigation-plan.md)

Verified binary facts carried into this design:

- Initial main menu is Win32 dialog resource `0xE2`, not the in-game `SidebarClass`.
- Dialog proc creates the RA2TS movie child at control `0x71A` and subclasses right-column buttons through the shell owner-draw button path.
- `0x71A` receives:
  - `0x4E3` with loop flag `1`
  - `0x4E4` with movie base `Ra2ts_s` on 640-wide screens, else `Ra2ts_l`
  - `0x4F0` on parent paint for explicit copy/draw
- RA2TS opens `.BIK` before `.VQA`.
- Retail RA2TS files:
  - `ra2ts_l.bik`: 632 x 570, 431 frames, 15 fps, no audio
  - `ra2ts_s.bik`: 472 x 450, 431 frames, 15 fps, no audio
- Static movie timer polls every `0x22` ms, but Bink playback advances by the movie's own 15 fps cadence.
- The static loops by seeking/goto frame `1` when the end/wrap test trips.
- For duplicate `ra2ts_l.bik`, `LANGUAGE.MIX` wins over `LANGMD.MIX`.
- Right buttons use `bue_li30.pcx`, `bue_mi30.pcx`, `bue_ri30.pcx` for up and `bde_li30.pcx`, `bde_mi30.pcx`, `bde_ri30.pcx` for down.
- Pressed buttons offset content downward by 2 px and play `[AudioVisual] GUIMainButtonSound=MenuClick`.

## Impact Analysis

### Files expected to change in implementation

| File | Change |
|---|---|
| [src/ui/main_menu_shell/mod.rs](../../src/ui/main_menu_shell/mod.rs) | New module exporting layout/state/action types for dialog `0xE2`. |
| [src/ui/main_menu_shell/layout.rs](../../src/ui/main_menu_shell/layout.rs) | New DLU-to-pixel layout for the verified `0xE2` controls and RA2TS child positioning. |
| [src/ui/main_menu_shell/state.rs](../../src/ui/main_menu_shell/state.rs) | New hit testing, pressed-button tracking, return-code mapping, tooltip keys, and command actions. |
| [src/render/main_menu_shell_chrome.rs](../../src/render/main_menu_shell_chrome.rs) | New small atlas for owner-draw PCX button pieces used by the initial main menu. |
| [src/render/bink_movie.rs](../../src/render/bink_movie.rs) | New reusable runtime Bink playback surface built from the existing tool playback logic. |
| [src/app_main_menu_shell_render.rs](../../src/app_main_menu_shell_render.rs) | New app-level render glue for movie quad, owner-draw buttons, and button text. |
| [src/app.rs](../../src/app.rs) | Add main-menu shell state/resources, initialize assets, route mouse down/up, tick movie, and replace the default `GameScreen::MainMenu` egui branch. |
| [src/render/mod.rs](../../src/render/mod.rs) | Export `main_menu_shell_chrome` and `bink_movie`. |
| [src/ui/mod.rs](../../src/ui/mod.rs) | Export `main_menu_shell`. |
| [src/lib.rs](../../src/lib.rs) | Export `app_main_menu_shell_render`. |
| [src/ui/main_menu.rs](../../src/ui/main_menu.rs) | Keep skirmish setup data for downstream temporary flow; stop using it as the first screen. |

### Risk areas

- **Bink timing drift:** the original polls at 34 ms but only advances when Bink is ready. The Rust path must use the Bink header fps and catch-up accumulator, not one frame per app frame or one frame per 34 ms poll.
- **Frame numbering:** gamemd calls Bink goto frame `1` on loop. The Rust decoder indexes frames zero-based, so implementation must explicitly map original Bink frame 1 to decoder frame index `0` unless testing proves the library's semantic frame number differs.
- **Archive priority:** do not special-case RA2TS search. The existing `AssetManager` first-match rule already matches the verified `language.mix` over `langmd.mix` relation; adding direct archive bypasses would risk regressing parity.
- **Button text fidelity:** the current Skirmish shell still uses `SidebarTextRenderer`. If the shell-text parity plan has not landed first, main-menu text can use the existing renderer as an interim, but the final target is the verified shell text path.
- **Downstream actions:** clicking the main-menu buttons is player-visible. The initial implementation should preserve return-code/action identity even where the target dialog is not implemented yet, so later shell screens can attach without changing `0xE2`.
- **App render ownership:** Bink movie state touches GPU textures and real-time playback. It belongs above `sim/`; no simulation module should depend on or know about it.

## Chosen Approach

Build a dedicated native main-menu shell path, following the Skirmish shell architecture but not merging the two screens.

The split is:

- `ui/main_menu_shell`: pure recovered dialog layout, control IDs, hit testing, and button state.
- `render/main_menu_shell_chrome`: static owner-draw PCX assets.
- `render/bink_movie`: reusable decoded Bink playback surface and updatable GPU texture.
- `app_main_menu_shell_render`: app-owned composition of the movie, buttons, text, sound trigger surface, and render pass.
- `app.rs`: screen orchestration, resource lifetime, input dispatch, and transition actions.

This keeps the parity logic out of egui, keeps dynamic movie playback out of pure UI layout code, and keeps all render/audio work above `sim/`.

### Why this over alternatives

- A generic Win32 dialog interpreter is too broad for the current target. Dialog `0xE2` has a small verified surface and can be represented directly without inventing a partial dialog engine.
- Extending the Skirmish shell renderer would couple two different dialogs. They share owner-draw button primitives, but layout, movie playback, and action routing are different.
- Skinning the egui menu would miss the RA2TS movie child, shell owner-draw button behavior, and original hit-test/action identity.
- Implementing the full shell tree in this change would multiply scope. The right move is to make `0xE2` faithful and preserve action boundaries for later downstream dialogs.

## Tiny-Detail Ledger

Each item below is a carry-forward constraint for `/write-plan` and implementation.

### Dialog and controls

- Dialog resource: `0xE2`.
- Dialog template: `DIALOGEX`, style `0x40000040`, rect `0,0,533,369`, font `MS Sans Serif` 8.
- DLU base used by prior shell work: x base `6`, y base `13`, with `MulDiv` rounding.
- Verified controls:

| Control | Kind | DLU rect | Text/key | Action |
|---|---|---:|---|---|
| `0x683` | Button | `425,125,108,23` | `GUI:SinglePlayer` | return `1` |
| `0x684` | Button | `425,152,108,23` | `GUI:WWOnline` | return `2` |
| `0x578` | Button | `425,179,108,23` | `GUI:Network` | return `3` |
| `0x686` | Button | `425,206,108,23` | `GUI:MoviesAndCredits` | return `4` |
| `0x55C` | Button | `425,233,108,23` | `GUI:Options` | return `5` |
| `0x3EE` | Button | `425,330,108,23` | `GUI:ExitGame` | return `6` |
| `0x694` | Static | `425,1,108,10` | `GUI:MainMenu` | title/static |
| `0x695` | Static | `2,355,303,12` | `GUI:Blank` | blank/static |
| `0x71A` | Static | `0,0,304,266` | none | RA2TS movie child |
| `0x71C` | Static | `447,29,61,33` | none | Yuri website static |
| `0x71D` | Static | `425,357,108,10` | `GUI:Blank` | blank/static |

- Main button tooltip keys:
  - `STT:MainButtonSinglePlayer`
  - `STT:MainButtonWWOnline`
  - `STT:MainButtonNetwork`
  - `STT:MainButtonMovies`
  - `STT:MainButtonOptions`
  - `STT:MainButtonExitGamemd`
  - `STT:MainButtonYuriWebSite`

### RA2TS movie

- Movie control ID is `0x71A`.
- Use `Ra2ts_s` only when screen width is exactly the original 640-wide path; otherwise use `Ra2ts_l`.
- Try `.bik` before `.vqa`. Implementation can initially support Bink only for RA2TS because retail YR ships these assets as Bink; VQA fallback is a deferred compatibility path if the Bink asset is missing.
- For standard retail YR:
  - `ra2ts_s.bik` dimensions are 472 x 450.
  - `ra2ts_l.bik` dimensions are 632 x 570.
  - Both are 431 frames at 15 fps and have no audio.
- The child static is moved to:
  - x = `0` if screen width <= 800, else `(screen_width - 800) / 2`
  - y = `0` if screen height <= 600, else `(screen_height - 600) / 2`
- The static is resized to movie dimensions after movie construction.
- Playback cadence:
  - maintain a real-time accumulator using `BinkHeader::fps()`
  - advance frames while accumulator >= frame duration
  - update the updatable texture only when a frame decodes
  - invalidate/request redraw when a frame changes
- Looping:
  - loop flag is true for the main menu
  - at end/wrap, seek/goto original frame `1`
  - in Rust, this should restart from decoder frame index `0` unless a targeted playback test proves a different mapping
- Audio:
  - RA2TS retail assets have no audio tracks
  - `render/bink_movie` should not require an audio sink for this use case
  - future cutscene playback can extend the same module with audio after separate research

### Owner-draw buttons

- Use PCX pieces:
  - up: `bue_li30.pcx`, `bue_mi30.pcx`, `bue_ri30.pcx`
  - down: `bde_li30.pcx`, `bde_mi30.pcx`, `bde_ri30.pcx`
- Button family suffix is `30`, derived from the verified 108 x 23 DLU main-menu buttons.
- Compose each button as left cap, tiled middle, right cap.
- Pressed state uses down PCX pieces and shifts text/content by `+2` px in y.
- Disabled alpha is `0x80`, but all six main menu buttons are enabled for the normal initial shell.
- Button click sound is `[AudioVisual] GUIMainButtonSound`, which resolves to `MenuClick` in both `rules.ini` and `rulesmd.ini`.
- Sound should fire on a successful click/release activation, not merely hover.

### Text

- Use CSF keys for visible labels, not hardcoded English strings:
  - `GUI:MainMenu`
  - `GUI:SinglePlayer`
  - `GUI:WWOnline`
  - `GUI:Network`
  - `GUI:MoviesAndCredits`
  - `GUI:Options`
  - `GUI:ExitGame`
- Final target for rendering is the verified shell text path from the shell text parity design.
- If the shell-text plan has not landed before implementation, use the current bitmap text renderer only as a temporary adapter and keep the main-menu API shaped around shell text rect drawing.

## Design

### `ui/main_menu_shell/layout.rs`

Owns only deterministic geometry:

```rust
pub const SHELL_BASE_W: i32 = 800;
pub const SHELL_BASE_H: i32 = 600;

pub struct MainMenuShellLayout {
    pub screen: RectPx,
    pub movie: RectPx,
    pub title: RectPx,
    pub website_static: RectPx,
    pub buttons: [MainMenuButtonRect; 6],
}

pub struct MainMenuButtonRect {
    pub id: MainMenuControlId,
    pub rect: RectPx,
}
```

Responsibilities:

- Convert the verified DLU rectangles to pixels with the existing Skirmish shell DLU formula.
- Apply the verified movie child offset and resize-to-movie dimensions.
- Preserve exclusive bottom/right hit-test edges, matching the existing `RectPx::contains` convention.
- Provide tests for 640 x 480, 800 x 600, 1024 x 768, and 1280 x 960.

The layout module should not load assets, decode movies, localize strings, or dispatch actions.

### `ui/main_menu_shell/state.rs`

Owns button identity and input state:

```rust
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

pub struct MainMenuShellState {
    pub pressed_button: Option<MainMenuControlId>,
}
```

Responsibilities:

- Hit-test owner-draw buttons.
- Track mouse-down button identity and only activate on mouse-up over the same button.
- Map controls to original return codes for tests and future shell integration:
  - Single Player `1`
  - WW Online `2`
  - Network `3`
  - Movies/Credits `4`
  - Options `5`
  - Exit `6`
- Expose tooltip CSF keys separately from action mapping.

### `render/bink_movie.rs`

Reusable runtime playback surface:

```rust
pub struct BinkMovieSurface {
    file: BinkFile,
    decoder: BinkDecoder,
    current_frame: usize,
    accumulator_secs: f64,
    texture: wgpu::Texture,
    batch_texture: BatchTexture,
    rgba: Vec<u8>,
    looping: bool,
}

pub enum BinkMovieStep {
    Unchanged,
    FrameUploaded,
    Ended,
}
```

Responsibilities:

- Load from bytes returned by `AssetManager::get_with_source_ref`.
- Initialize the updatable texture from the first decoded frame.
- Convert YUV to RGBA using logic moved from `src/bin/bik_player_playback.rs`.
- Step by elapsed wall-clock seconds and Bink fps.
- Upload changed frames to the existing `COPY_DST` texture.
- Seek/restart cleanly at loop.
- Expose `batch_texture()`, `width()`, `height()`, `fps()`, `frame_count()`, and `source_archive()`.

This module is in `render/` because it owns GPU texture lifetime. It depends on `assets` and `render/gpu`, not on `ui`, `app`, or `sim`.

### `render/main_menu_shell_chrome.rs`

Small atlas for main menu static button assets:

```rust
pub struct MainMenuShellChromeAtlas {
    pub texture: BatchTexture,
    pub button_up_left_30: ShellChromeEntry,
    pub button_up_mid_30: ShellChromeEntry,
    pub button_up_right_30: ShellChromeEntry,
    pub button_down_left_30: ShellChromeEntry,
    pub button_down_mid_30: ShellChromeEntry,
    pub button_down_right_30: ShellChromeEntry,
}
```

Responsibilities:

- Load only the six verified PCX pieces needed by `0xE2`.
- Use embedded PCX palette and transparent index `0`, consistent with the Skirmish shell atlas.
- Fail the native shell path if any of the six pieces are missing; a partial owner-draw button set is worse than a visible development fallback.
- Include tests classifying these six files as mandatory `0xE2` assets.

### `app_main_menu_shell_render.rs`

App-level compositor equivalent to the Skirmish shell render glue:

```rust
pub(crate) fn render_main_menu_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
) -> Result<MainMenuShellAction>;
```

Responsibilities:

- Clear the frame.
- Compute layout from current render width/height and selected movie dimensions.
- Step/upload Bink movie before drawing.
- Build one full-movie `SpriteInstance`.
- Build button instances from the six PCX pieces and pressed-button state.
- Build shell text instances for title and buttons.
- Draw movie first, then button chrome, then text.
- Return `MainMenuShellAction::None`; input actions are handled by `app.rs` mouse up so render stays side-effect-light.

The existing `build_button_segments` logic from [src/app_skirmish_shell_render.rs](../../src/app_skirmish_shell_render.rs) should be moved to a shared small helper only if that can be done without broad churn. Otherwise duplicate the few lines in the first implementation and consolidate later after both call sites are stable.

### `app.rs` integration

New `AppState` fields:

```rust
pub(crate) main_menu_shell_state: ui::main_menu_shell::MainMenuShellState,
pub(crate) main_menu_shell_chrome: Option<render::main_menu_shell_chrome::MainMenuShellChromeAtlas>,
pub(crate) main_menu_movie: Option<render::bink_movie::BinkMovieSurface>,
pub(crate) main_menu_shell_failed: bool,
```

Resource loading:

- Build chrome at startup when asset manager is available.
- Load `ra2ts_s.bik` or `ra2ts_l.bik` based on current screen width.
- On resize crossing the 640-wide boundary, rebuild the movie surface with the other base name.
- Log source archive for RA2TS at load time. This is important because archive priority is a parity-sensitive finding.

Main menu render branch:

- Default path is native `0xE2` shell.
- If required assets fail to load, fall back to the current egui menu and log a clear warning. This fallback is for development robustness, not the parity target.
- Keep the existing Skirmish shell dev toggle behind its current feature/env behavior until it is removed or folded into downstream shell work.

Input:

- On left mouse down in `GameScreen::MainMenu`, hit-test native main-menu shell buttons and store `pressed_button`.
- On left mouse up, activate only if the release is over the same button.
- On activation:
  - play `GUIMainButtonSound`
  - dispatch action
  - clear `pressed_button`
- If the native shell is in asset-failure fallback mode, keep using the existing egui action path.

Action routing:

- `ExitGame`: call `event_loop.exit()`.
- `SinglePlayer`: route to the current skirmish setup/load flow as an interim only if no faithful Single Player shell exists yet. Keep the action enum named `SinglePlayer`, not `StartSelected`, so later downstream shell work can replace the target without changing `0xE2`.
- `Network`, `WwOnline`, `MoviesAndCredits`, `Options`: preserve action identity and show the existing placeholder/unsupported behavior until those shell dialogs are researched and implemented.
- `YuriWebsite`: preserve hit-test identity if implemented; opening an external browser is not required for initial parity.

## Verification Plan

Unit tests:

- `main_menu_shell::layout`:
  - DLU button rects convert to expected pixel rects at 800 x 600.
  - RA2TS child uses `ra2ts_s` dimensions at 640-wide and `ra2ts_l` otherwise.
  - Movie offset matches centered 800 x 600 shell behavior on larger screens.
  - Hit-test excludes bottom/right edge.
- `main_menu_shell::state`:
  - each button maps to the verified return code
  - mouse-down/up activates only when released over the same button
  - tooltip keys match verified strings
- `render/bink_movie`:
  - initializes RA2TS dimensions and fps from real assets
  - stepping by less than one frame duration does not advance
  - stepping by one frame duration advances once
  - large elapsed time catches up more than one frame without skipping texture state invariants
  - loop from end restarts at decoder frame index `0`
- `render/main_menu_shell_chrome`:
  - all six PCX pieces are mandatory and classified as `0xE2` owner-draw assets

Runtime checks:

- Start at 800 x 600 and confirm the first screen is native RA2TS + right-column buttons, not the egui setup page.
- Confirm RA2TS animates at 15 fps, not at monitor refresh and not at 34 ms per frame.
- Confirm `ra2ts_l.bik` source archive logs as `language.mix` when the duplicate exists.
- Press/release each button and confirm only matching release activates.
- Pressed state changes to down PCX and text/content shifts by 2 px.
- Confirm `MenuClick` plays once per activated main button.

Visual capture:

- Capture 640 x 480, 800 x 600, 1024 x 768, and 1280 x 960.
- Compare button positions and RA2TS placement against the verified DLU geometry and retail screenshots/captures when available.
- Confirm the movie is not scaled; it is drawn at decoded pixel dimensions.

## Deferred Work

- Full downstream Single Player shell dialogs.
- WWOnline and Network shell flows.
- Movies/Credits shell behavior and cutscene list playback.
- Options shell dialog.
- VQA fallback for RA2TS if a non-retail asset set lacks Bink.
- Exact retail screenshot comparison for first visible RA2TS frame and global shell clipping.
- Consolidating Skirmish and main-menu button chrome into a shared shell button module, after both call sites are stable.

## Open Assumptions

- The next implementation should make native `0xE2` the normal `GameScreen::MainMenu` path, with egui only as an asset-failure fallback.
- The implementation may temporarily route `SinglePlayer` into the current skirmish setup flow, but the action boundary must stay faithful to the original return code.
- `BinkGoto(1)` in gamemd maps to Rust decoder frame index `0`; this should be verified with a targeted playback test during implementation.
- Shell text parity work may land before this plan is implemented. If it does, main-menu text should use that renderer directly.
