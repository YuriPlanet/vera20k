//! The family pages' shared shell: the right-panel pages (main menu `0xE2`,
//! Single Player `0x100`, Movies & Credits `0x101`, campaign `0x94`, Load
//! Saved Game `0xB7`) paint their panel, monitor, button column, captions,
//! heading and status line through [`encode_family_page`], each with what it
//! draws under and over them. [`FamilyPage`] names the pages whose buttons
//! are `vera20k-gui` nodes (`ui::shell::menu_page`).

use crate::util::time::Instant;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::{Ra2tsDialogOwner, shell_reveal_path_a};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::app::frontend::shell_transition::{ColumnDraw, ShellSlideKind};
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{
    self, ArtFit, ButtonPolicy, CURSOR_DEPTH, PaintButton, PaintLabel, SHELL_TEXT_RGB_DISABLED,
    SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text::{ShellAlign, ShellTextDraw};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW};
use crate::ui::shell::menu_page::{self, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::static_reveal::Kind1RevealWindow;

/// Family pages paint the 156x42 SDBTNANM frame filling its cell,
/// right-anchored and vertically centred — the native placement at the
/// authored cell size, and a proportional stretch of it on a stretched layout.
/// Mouse hover updates static 0x695 but does not select frame 3. Press selects
/// frame 4 without moving the art; a runtime-disabled button keeps its art and
/// shows its caption in the disabled colour (owner-draw type 1, `0x006135F3`).
pub(crate) const MENU_PAGE_BUTTON_POLICY: ButtonPolicy = ButtonPolicy {
    art_fit: ArtFit::FitRightAnchored {
        panel_w: SDBTNANM_CELL_W_NARROW as f32,
        tile_h: SDBTNANM_CELL_H as f32,
    },
    hover_flash: false,
    art_sink_y: 0.0,
};

pub(crate) enum MenuPageRenderResult {
    Rendered,
    Fallback,
}

/// A family page whose buttons are `vera20k-gui` nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FamilyPage {
    MainMenu,
    SinglePlayer,
    MoviesAndCredits,
    Campaign,
}

impl FamilyPage {
    /// Which RA2TS menu page owns the `MainMenu` screen, if any.
    pub(crate) fn menu_page(state: &AppState) -> Option<Self> {
        if state.frontend.screen != crate::ui::game_screen::GameScreen::MainMenu {
            return None;
        }
        // Play_Movie and Show_Credits run after the page is destroyed.
        if state.frontend.fullscreen_movie.is_some() || state.frontend.credits_roll.is_some() {
            return None;
        }
        let route = state.frontend.shell_route;
        if route.single_player() {
            Some(Self::SinglePlayer)
        } else if route.movies_and_credits() {
            Some(Self::MoviesAndCredits)
        } else {
            None
        }
    }

    pub(crate) fn spec(self) -> &'static MenuPageSpec {
        match self {
            Self::MainMenu => &crate::ui::main_menu_shell::MAIN_MENU_PAGE,
            Self::SinglePlayer => &crate::ui::single_player_shell::SINGLE_PLAYER_PAGE,
            Self::MoviesAndCredits => &crate::ui::movies_credits_shell::MOVIES_CREDITS_PAGE,
            Self::Campaign => &crate::ui::campaign_shell::CAMPAIGN_PAGE,
        }
    }

    pub(crate) fn dialog(self) -> DialogId {
        self.spec().dialog
    }

    fn slide_kind(self) -> ShellSlideKind {
        match self {
            Self::MainMenu => ShellSlideKind::MainMenu,
            Self::SinglePlayer => ShellSlideKind::SinglePlayer,
            Self::MoviesAndCredits => ShellSlideKind::MoviesAndCredits,
            Self::Campaign => ShellSlideKind::Campaign,
        }
    }

    /// The dialog owning the RA2TS static `0x71A`.
    fn movie_owner(self) -> Ra2tsDialogOwner {
        match self {
            Self::SinglePlayer => Ra2tsDialogOwner::SinglePlayer0x100,
            Self::MoviesAndCredits => Ra2tsDialogOwner::MoviesAndCredits0x101,
            Self::MainMenu | Self::Campaign => Ra2tsDialogOwner::MainMenu0xE2,
        }
    }

    pub(crate) fn nodes(self) -> vera20k_gui::NodeSpec {
        match self {
            Self::Campaign => crate::ui::campaign_shell::nodes(),
            _ => self.spec().nodes(),
        }
    }

    pub(crate) fn layout(self, state: &AppState) -> MenuPageLayout {
        match self {
            Self::MainMenu => shell_layout::main_menu_layout(state).page(),
            Self::Campaign => shell_layout::campaign_layout(state).page,
            _ => shell_layout::menu_page_layout(state, self.spec()),
        }
    }

    /// Controls the dialog proc runtime-disables: Single Player disables Load
    /// Saved Game on `0x497` when no loadable save exists.
    pub(crate) fn enabled(self, state: &AppState) -> Option<(u16, bool)> {
        (self == Self::SinglePlayer).then(|| {
            let shell = &state.frontend.single_player_shell_state;
            (0x0689, shell.load_saved_game_enabled)
        })
    }

    /// Whether the page reads its press and hover now: `0xE2` keeps painting
    /// its own under a message box; the other pages read theirs only on top.
    fn reads(self, controller: &crate::ui::shell::controller::DialogController) -> bool {
        self == Self::MainMenu || controller.top_id() == Some(self.dialog())
    }

    /// Status help of the hovered control. Hover is enable-unfiltered, so a
    /// disabled button still writes its help.
    pub(crate) fn status_key(self, state: &AppState) -> Option<String> {
        let controller = &state.frontend.shell_controller;
        self.reads(controller)
            .then(|| menu_page::hovered_help(controller, self.dialog()))
            .flatten()
    }

    /// The steady buttons with the live instance's press and hover.
    pub(crate) fn buttons(self, state: &AppState, layout: &MenuPageLayout) -> Vec<PageButton> {
        let controller = &state.frontend.shell_controller;
        let reads = self.reads(controller);
        let dialog = self.dialog();
        page_buttons(
            layout,
            reads
                .then(|| menu_page::pressed_id(controller, dialog))
                .flatten(),
            reads
                .then(|| menu_page::hovered_id(controller, dialog))
                .flatten(),
            self.enabled(state).filter(|(_, on)| !on).map(|(id, _)| id),
        )
    }
}

/// One right-panel button as it paints.
#[derive(Clone, Copy)]
pub(crate) struct PageButton {
    pub(crate) id: u16,
    pub(crate) paint: PaintButton,
}

/// `layout`'s buttons with the `pressed` and `hovered` control and the
/// `disabled` one. A disabled control never paints pressed or hovered.
pub(crate) fn page_buttons(
    layout: &MenuPageLayout,
    pressed: Option<u16>,
    hovered: Option<u16>,
    disabled: Option<u16>,
) -> Vec<PageButton> {
    layout
        .buttons
        .iter()
        .map(|b| {
            let enabled = disabled != Some(b.id);
            PageButton {
                id: b.id,
                paint: PaintButton {
                    rect: b.rect,
                    pressed: enabled && pressed == Some(b.id),
                    hovered: enabled && hovered == Some(b.id),
                    enabled,
                },
            }
        })
        .collect()
}

/// A family page frame's slide and statics.
#[derive(Default)]
pub(crate) struct PanelState {
    /// The page's teardown slide-out runs.
    pub(crate) leaving: bool,
    /// The slide engine's (or a shuttered backdrop's) tile column, drawn in
    /// place of the buttons and their captions (`0x006071E0`).
    pub(crate) column: Option<Vec<ColumnDraw>>,
    /// The `0x71C` monitor frame.
    pub(crate) monitor: Option<usize>,
    /// Heading `0x694`'s reveal.
    pub(crate) title: Option<Kind1RevealWindow>,
    /// Status line `0x695`.
    pub(crate) status: Option<PaintLabel<'static>>,
}

/// Status help of `spec`'s hovered button, for a page still on fed buttons.
pub(crate) fn status_csf_key(spec: &MenuPageSpec, hovered: Option<u16>) -> Option<&'static str> {
    spec.button(hovered?).map(|button| button.tooltip_key)
}

/// The CSF text of a status help key; empty off every control.
pub(crate) fn help_text(state: &AppState, key: Option<&str>) -> String {
    key.map(|key| resolve_csf(state, key).into_owned())
        .unwrap_or_default()
}

/// The slide and statics of a family page other than `0xE2`: its teardown
/// slide-out or its entry slide, then the monitor (when it has one), heading
/// and status line with `status_key`'s help. The teardown starts with a full
/// dialog repaint (`0x00622C4F`) and pumps no messages until it ends: the
/// statics stay blank and the monitor window shows the right panel's own art.
pub(crate) fn page_panel_state(
    state: &mut AppState,
    kind: ShellSlideKind,
    monitor: bool,
    status_key: impl FnOnce(&AppState) -> Option<String>,
    status_rect: RectPx,
) -> PanelState {
    use crate::app::frontend::shell_transition::shell_exit_wave;
    let exit = shell_exit_wave(state, kind).map(|wave| wave.button_draws());
    if exit.is_some() {
        return PanelState {
            leaving: true,
            column: exit,
            ..PanelState::default()
        };
    }
    let column = state
        .frontend
        .shell_first_paint_slide
        .as_ref()
        .map(|wave| wave.button_draws());
    let monitor = if monitor {
        paint_shell_monitor(state)
    } else {
        None
    };
    let title = state.frontend.shell_page_title.paint(Instant::now());
    let text = help_text(state, status_key(state).as_deref());
    let status = paint_shell_status_line(state, text, status_rect);
    PanelState {
        leaving: false,
        column,
        monitor,
        title,
        status,
    }
}

/// Advance the showing family dialog's `0x71C` for this recomposition; the
/// frame loop commits it after present. No timer reaches the static while the
/// dialog's first-paint slide runs.
pub(crate) fn paint_shell_monitor(state: &mut AppState) -> Option<usize> {
    let frames = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .map_or(0, |chrome| chrome.warning_monitor_frames.len());
    let timers = !crate::app::frontend::shell_transition::shell_slide_running(state);
    state
        .frontend
        .shell_monitor
        .paint(Instant::now(), frames, timers)
}

/// Status line `0x695` for this recomposition: deliver the hover help text
/// (`0x4B2`, empty off every control) and return the label to draw, `None`
/// while the static is hidden or blank. Kind-1 paint passes left alignment
/// only (`0x00615A91`), so the text is top-left in the window.
pub(crate) fn paint_shell_status_line(
    state: &mut AppState,
    text: String,
    window: RectPx,
) -> Option<PaintLabel<'static>> {
    let now = Instant::now();
    let status = &mut state.frontend.shell_status_line;
    status.set_text(&text, now);
    let reveal = status.paint(now)?;
    (!text.is_empty()).then(|| PaintLabel {
        text: text.into(),
        rect: crate::ui::shell::layout::status_text_rect(window),
        align: ShellAlign::NONE,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: Some(shell_reveal_path_a(reveal)),
    })
}

/// Heading text of the menu page a first-paint slide belongs to.
pub(crate) fn active_page_title_text(
    state: &AppState,
    kind: crate::app::frontend::shell_transition::ShellSlideKind,
) -> String {
    use crate::app::frontend::shell_transition::ShellSlideKind;
    let key = match kind {
        ShellSlideKind::SinglePlayer => {
            crate::ui::single_player_shell::SINGLE_PLAYER_PAGE.title_key
        }
        ShellSlideKind::MoviesAndCredits => {
            crate::ui::movies_credits_shell::MOVIES_CREDITS_PAGE.title_key
        }
        ShellSlideKind::MovieList => crate::ui::movies_credits_shell::MOVIE_LIST_PAGE.title_key,
        ShellSlideKind::Mods => crate::ui::mods_shell::PAGE.title_key,
        ShellSlideKind::Campaign => crate::ui::campaign_shell::CAMPAIGN_PAGE.title_key,
        ShellSlideKind::LoadSavedGame => {
            crate::ui::shell::saved_games::LOAD_SAVED_GAME_PAGE.title_key
        }
        ShellSlideKind::WolWelcome => crate::ui::wol_shell::WOL_WELCOME_PAGE.title_key,
        ShellSlideKind::WolLogin => crate::ui::wol_login::PAGE.title_key,
        ShellSlideKind::WolLobby => crate::ui::wol_shell::WOL_LOBBY_TITLE_KEY,
        ShellSlideKind::WolOptions => crate::ui::wol_options::PAGE.title_key,
        ShellSlideKind::WolSoundOptions => crate::ui::shell::sound::SHELL_SOUND_PAGE.title_key,
        ShellSlideKind::WolAutoLoginQuery => crate::ui::wol_persona::AUTO_LOGIN_PAGE.title_key,
        ShellSlideKind::WolLocale => crate::ui::wol_persona::LOCALE_PAGE.title_key,
        ShellSlideKind::WolMyInformation => crate::ui::wol_my_info::PAGE.title_key,
        ShellSlideKind::WolBuddyList => crate::ui::wol_buddy::PAGE.title_key,
        ShellSlideKind::WolQuickMatch => crate::ui::wol_quick_match::PAGE.title_key,
        ShellSlideKind::WolAddBuddyQuery => crate::ui::wol_persona::ADD_BUDDY_PAGE.title_key,
        ShellSlideKind::Keyboard => crate::ui::shell::keyboard::KEYBOARD_PAGE.title_key,
        ShellSlideKind::LanLobby => crate::ui::lan_shell::PAGE.title_key,
        ShellSlideKind::NetworkSetup(dialog) => dialog.page().title_key,
        ShellSlideKind::ChooseMap => crate::ui::skirmish_shell::CHOOSE_MAP_TITLE_KEY,
        ShellSlideKind::RandomMap => crate::ui::skirmish_shell::RANDOM_MAP_TITLE_KEY,
        // The Options heading is the dialog's own resolved label.
        ShellSlideKind::Options => {
            return state
                .frontend
                .options_dialog
                .as_ref()
                .map(|dialog| dialog.shell_title_text().to_owned())
                .unwrap_or_default();
        }
        // These dialogs set their headings themselves.
        ShellSlideKind::MainMenu
        | ShellSlideKind::Skirmish
        | ShellSlideKind::Score
        | ShellSlideKind::CampaignScore
        | ShellSlideKind::CampaignEnd => {
            return String::new();
        }
    };
    resolve_csf(state, key).into_owned()
}

/// Paint whichever RA2TS menu page the route selects.
pub(crate) fn render_active_menu_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<MenuPageRenderResult> {
    match FamilyPage::menu_page(state) {
        Some(page) => render_menu_page(state, encoder, destination, page),
        None => Ok(MenuPageRenderResult::Fallback),
    }
}

/// Single Player `0x100` or Movies & Credits `0x101`: the RA2TS movie under
/// the shared family shell.
pub(crate) fn render_menu_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    page: FamilyPage,
) -> Result<MenuPageRenderResult> {
    use crate::app::frontend::main_menu_shell_render as movie;
    movie::ensure_movie_for_current_layout(state, page.movie_owner())?;
    if state.frontend.main_menu_shell_error.is_some()
        || state.frontend.main_menu_shell_chrome.is_none()
    {
        state
            .frontend
            .main_menu_shell_error
            .get_or_insert_with(|| crate::i18n::tr("VERA:MenuResourcesMissing"));
        return Ok(MenuPageRenderResult::Fallback);
    }
    // While either slide runs the RA2TS static shows no movie and gets no
    // timer (`0x006071E0`; the teardown also stops it with 0x4E2), so the
    // shell background shows and the movie clock starts after the slide.
    let sliding = crate::app::frontend::shell_transition::shell_exit_wave(state, page.slide_kind())
        .is_some()
        || state.frontend.shell_first_paint_slide.is_some();
    if !movie::step_ra2ts_movie(state, !sliding) {
        return Ok(MenuPageRenderResult::Fallback);
    }
    let layout = page.layout(state);
    let panel = page_panel_state(
        state,
        page.slide_kind(),
        true,
        |state| page.status_key(state),
        layout.status_help,
    );
    let buttons = page.buttons(state, &layout);
    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    // Menu pages have no parent background under the movie; while a slide
    // runs the RA2TS area shows the shell background instead.
    let under = if sliding {
        vec![TexturedDraw {
            texture: &chrome.texture,
            instances: movie::shell_parent_background_instances(
                chrome,
                layout.screen.w,
                layout.screen.h,
                layout.scale,
            ),
        }]
    } else {
        movie::movie_draw(state, layout.movie).into_iter().collect()
    };
    let mut frame = FamilyFrame::new("Menu Page Shell", page.spec(), &layout, panel, buttons);
    frame.under = under;
    encode_family_page(state, encoder, destination, frame);
    Ok(MenuPageRenderResult::Rendered)
}

/// One family page frame for [`encode_family_page`].
pub(crate) struct FamilyFrame<'a> {
    pub(crate) label: &'static str,
    pub(crate) spec: &'static MenuPageSpec,
    pub(crate) layout: &'a MenuPageLayout,
    pub(crate) panel: PanelState,
    pub(crate) buttons: Vec<PageButton>,
    /// The page's own art under the right panel, in draw order.
    pub(crate) under: Vec<TexturedDraw<'a>>,
    /// Chrome-atlas sprites over the panel, under its button column (a list).
    pub(crate) over_panel: Vec<SpriteInstance>,
    /// The page's own text, drawn before the captions and statics.
    pub(crate) labels: Vec<PaintLabel<'a>>,
    /// Drawn over the buttons, with text after all other text (a prompt).
    pub(crate) over: Option<TexturedDraw<'a>>,
    pub(crate) over_text: Vec<ShellTextDraw>,
    /// Whether this pass draws the cursor (a box painted over the page next
    /// draws it instead).
    pub(crate) cursor: bool,
}

impl<'a> FamilyFrame<'a> {
    pub(crate) fn new(
        label: &'static str,
        spec: &'static MenuPageSpec,
        layout: &'a MenuPageLayout,
        panel: PanelState,
        buttons: Vec<PageButton>,
    ) -> Self {
        Self {
            label,
            spec,
            layout,
            panel,
            buttons,
            under: Vec::new(),
            over_panel: Vec::new(),
            labels: Vec::new(),
            over: None,
            over_text: Vec::new(),
            cursor: true,
        }
    }
}

/// Compose and present one family page: its art, the right panel and lower
/// strip with the monitor, the button column (owner-draw buttons with their
/// captions, or the slide's tiles), the page's text, heading and status line,
/// what it draws over them, and the cursor.
pub(crate) fn encode_family_page(
    state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    frame: FamilyFrame<'_>,
) {
    let Some(chrome) = state.frontend.main_menu_shell_chrome.as_ref() else {
        return;
    };
    let FamilyFrame {
        label,
        spec,
        layout,
        panel,
        buttons,
        mut under,
        over_panel,
        mut labels,
        over,
        over_text,
        cursor,
    } = frame;
    let mut panel_sprites = shell_paint::paint_chrome(
        chrome,
        layout.right_panel,
        Some(layout.lower_strip),
        layout.screen.w,
    );
    panel_sprites.extend(panel.monitor.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.warning_monitor, frame, layout.scale)
    }));
    panel_sprites.extend(over_panel);
    let buttons = match &panel.column {
        Some(draws) => {
            panel_sprites.extend(shell_paint::paint_slide_column(
                chrome,
                layout.right_panel,
                draws,
            ));
            Vec::new()
        }
        None => buttons,
    };
    for PageButton { id, paint } in &buttons {
        let Some(spec_button) = spec.button(*id) else {
            continue;
        };
        labels.push(PaintLabel {
            text: resolve_csf(state, spec_button.csf_key),
            rect: owner_draw_button_label_rect(paint.rect, paint.pressed),
            align: ShellAlign::BUTTON_CAPTION,
            rgb: if paint.enabled {
                SHELL_TEXT_RGB_ENABLED
            } else {
                SHELL_TEXT_RGB_DISABLED
            },
            path_a_reveal: None,
        });
    }
    if let Some(window) = panel.title {
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.title_key),
            rect: layout.title,
            align: ShellAlign::PANEL_TITLE,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    labels.extend(panel.status);
    let buttons: Vec<PaintButton> = buttons.iter().map(|b| b.paint).collect();
    let button_sprites = shell_paint::paint_buttons(
        chrome,
        &buttons,
        MENU_PAGE_BUTTON_POLICY,
        Instant::now(),
        None,
    );
    let mut text =
        shell_paint::paint_labels_scaled(&state.renderer.bit_font, &labels, layout.scale.factor());
    text.extend(over_text);
    under.push(TexturedDraw {
        texture: &chrome.texture,
        instances: panel_sprites,
    });
    under.push(TexturedDraw {
        texture: &chrome.texture,
        instances: button_sprites,
    });
    under.extend(over);
    encode_shell_pass(
        state,
        encoder,
        destination,
        label,
        ShellComposition {
            draws: &under,
            text: &text,
            cursor: if cursor {
                software_cursor(state, CURSOR_DEPTH)
            } else {
                None
            },
            effects: None,
            smooth_art: false,
        },
    );
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::ui::single_player_shell::SINGLE_PLAYER_PAGE;

    #[test]
    fn menu_page_policy_fills_the_authored_cell_without_mouse_hover_flash() {
        // The fit is keyed on the authored 156x42 cell, so an unstretched
        // layout gets the native placement; a stretched cell stretches the art.
        assert_eq!(
            MENU_PAGE_BUTTON_POLICY.art_fit,
            ArtFit::FitRightAnchored {
                panel_w: 156.0,
                tile_h: 42.0
            }
        );
        const { assert!(!MENU_PAGE_BUTTON_POLICY.hover_flash) };
        assert_eq!(MENU_PAGE_BUTTON_POLICY.art_sink_y, 0.0);
    }

    #[test]
    fn status_help_includes_disabled_load() {
        let layout = crate::ui::single_player_shell::compute_layout(800, 600);
        let dialog = SINGLE_PLAYER_PAGE.dialog;
        let mut c = crate::ui::shell::controller::DialogController::default();
        c.ensure_active(dialog, false);
        let rect_of = |id| layout.button_rect(id);
        menu_page::sync_nodes(
            &mut c,
            dialog,
            || SINGLE_PLAYER_PAGE.nodes(),
            rect_of,
            &[(0x0689, false)],
        );
        assert_eq!(menu_page::hovered_help(&c, dialog), None);
        c.on_input(vera20k_gui::InputEvent::PointerMove { x: 700, y: 250 });
        assert_eq!(
            menu_page::hovered_help(&c, dialog).as_deref(),
            Some("STT:SingleButtonLoadSavedGame")
        );
    }

    #[test]
    fn disabled_page_button_never_paints_pressed_or_hovered() {
        let layout = crate::ui::single_player_shell::compute_layout(800, 600);
        let buttons = page_buttons(&layout, Some(0x0689), Some(0x0689), Some(0x0689));
        let load = buttons
            .iter()
            .find(|button| button.id == 0x0689)
            .map(|button| button.paint)
            .expect("load button");
        assert!(!load.enabled && !load.pressed && !load.hovered);
    }

    #[test]
    fn gsi_13_26_menu_page_steady_frame_uses_rgb565_presenter_after_full_composition() {
        let source = include_str!("menu_page_render.rs");
        let production = source
            .split_once("#[cfg(test)]")
            .expect("test module follows production renderer")
            .0;
        let renderer = &production[production
            .find("pub(crate) fn render_menu_page")
            .expect("production renderer")..];
        assert!(renderer.contains("destination: &wgpu::Texture"));
        let fallback_returns: Vec<_> = renderer
            .match_indices("return Ok(MenuPageRenderResult::Fallback)")
            .map(|(index, _)| index)
            .collect();
        let pass = renderer
            .find("encode_shell_pass(")
            .expect("shared RGB565 composition pass");
        assert_eq!(fallback_returns.len(), 2);
        assert!(fallback_returns.iter().all(|&index| index < pass));

        // The shared pass composes everything, the cursor last, before the
        // RGB565 presentation.
        let shared = include_str!("shell_pass.rs");
        let encoder = &shared[shared
            .find("pub(crate) fn encode_shell_pass")
            .expect("shared encoder")..];
        let source_view = encoder
            .find("shell_surface_presenter.source_render_view()")
            .expect("RGB565 presenter source view");
        let render_pass = encoder
            .find("encoder.begin_render_pass")
            .expect("complete shell render pass");
        let cursor = encoder.find("if let (Some((texture, _))").expect("cursor");
        let retained = encoder
            .find("stage_cursor_free_composition(")
            .expect("cursor-free backing");
        let pass_end = encoder
            .rfind("drop(pass);")
            .expect("cursor render pass end");
        let present = encoder.find("presenter.encode_present(").expect("present");
        assert!(source_view < render_pass);
        assert!(render_pass < cursor);
        assert!(render_pass < retained);
        assert!(retained < cursor);
        assert!(cursor < pass_end);
        assert!(pass_end < present);

        // Steady dispatch presents the page directly to the swapchain.
        let app_source = include_str!("../frame.rs");
        let dispatch = &app_source[app_source
            .find("ScreenPage::SinglePlayer | ScreenPage::MoviesAndCredits =>")
            .expect("menu page steady dispatch")..];
        let shell_call = dispatch
            .find("render_active_menu_page")
            .expect("menu page renderer call");
        let dispatch_end = dispatch[shell_call..]
            .find("MainMenuShellRenderResult")
            .expect("next shell dispatch");
        assert!(dispatch[shell_call..shell_call + dispatch_end].contains("&output.texture"));
    }
}
