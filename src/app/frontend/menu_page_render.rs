//! The family pages' shared shell: the right-panel pages (main menu `0xE2`,
//! Single Player `0x100`, Movies & Credits `0x101`, the movie list `0x129`,
//! Mods, campaign `0x94`, Load Saved Game `0xB7` and the score pages) paint
//! their panel, monitor, heading and status line, and their button column's
//! `vera20k-gui` nodes through `gui_paint`, in [`encode_family_page`], each
//! with what it draws under and over them. [`FamilyPage`] names the pages
//! whose buttons are `ui::shell::menu_page` specs.

use crate::app::shell_route::ShellRoute;
use crate::util::time::Instant;

use anyhow::Result;

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::{
    Ra2tsDialogOwner, push_family_list_backing, shell_parent_background_instances,
    shell_reveal_path_a,
};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, software_cursor,
};
use crate::app::frontend::shell_transition::{ColumnDraw, ShellSlideKind};
use crate::app::frontend::skirmish_shell_render::gui_paint::{
    ButtonArt, GuiPaint, GuiStyle, paint_gui_ids,
};
use crate::render::shell_paint::{self, CURSOR_DEPTH, PaintLabel, SHELL_TEXT_RGB_ENABLED};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::menu_page::{self, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::static_reveal::Kind1RevealWindow;

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
        match state.frontend.shell_route {
            ShellRoute::SinglePlayer => Some(Self::SinglePlayer),
            ShellRoute::MoviesAndCredits => Some(Self::MoviesAndCredits),
            _ => None,
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

    pub(crate) fn dialog(self) -> DialogResourceId {
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

    /// The page's button tree ([`page_tree`]).
    pub(crate) fn tree(self, state: &mut AppState) -> Option<(Gui, NodeId)> {
        let layout = self.layout(state);
        let campaign = (self == Self::Campaign).then(|| shell_layout::campaign_layout(state));
        let rect_of = |id| match &campaign {
            Some(campaign) => campaign.control_rect(id),
            None => layout.button_rect(id),
        };
        let enabled = self.enabled(state);
        page_tree(
            state,
            self.dialog(),
            || self.nodes(),
            rect_of,
            enabled.as_slice(),
        )
    }
}

/// A page's button tree: `nodes` kept on `rect_of` with the `enabled`
/// overrides under `dialog`'s live instance (`None`), or, while it has none
/// (before its first pointer message, or under a box that took the stack),
/// a scratch tree of them, which paints released.
pub(crate) fn page_tree(
    state: &mut AppState,
    dialog: DialogResourceId,
    nodes: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
    enabled: &[(u16, bool)],
) -> Option<(Gui, NodeId)> {
    use crate::ui::shell::page;
    let controller = &mut state.frontend.shell_controller;
    if controller.root_in(dialog).is_some() {
        menu_page::sync_nodes(controller, dialog, nodes, rect_of, enabled);
        return None;
    }
    let mut gui = Gui::new(64);
    let root = gui.create(NodeKind::Dialog, None).ok()?;
    page::ensure_built(&mut gui, root, nodes);
    page::sync_rects(&mut gui, root, rect_of);
    for &(id, on) in enabled {
        page::set(&mut gui, root, id, Prop::Enabled, Value::Bool(on));
    }
    Some((gui, root))
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

/// The CSF text of a status help key; empty off every control.
pub(crate) fn help_text(state: &AppState, key: Option<&str>) -> String {
    key.map(|key| state.csf_text(key)).unwrap_or_default()
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
        ShellSlideKind::LoadSavedGame => state.frontend.load_saved_game.as_ref().map_or(
            crate::ui::shell::saved_games::LOAD_SAVED_GAME_PAGE.title_key,
            |browser| browser.game_title_label().0,
        ),
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
    state.csf_text(key)
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
    if state.frontend.main_menu_shell_error.is_some() || !family_art_ready(state) {
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
    let tree = page.tree(state);
    let layout = page.layout(state);
    let panel = page_panel_state(
        state,
        page.slide_kind(),
        true,
        |state| page.status_key(state),
        layout.status_help,
    );
    let title = page.spec().title_key;
    let mut frame = FamilyFrame::new("Menu Page Shell", &layout, panel, title);
    // Menu pages have no parent background under the movie; while a slide
    // runs the RA2TS area shows the shell background instead.
    frame.backdrop = sliding;
    frame
        .under
        .extend(movie::movie_draw(state, layout.movie).filter(|_| !sliding));
    let frame = frame.on(state, page.dialog(), &tree);
    encode_family_page(state, encoder, destination, frame);
    Ok(MenuPageRenderResult::Rendered)
}

/// Whether the family shell's art is ready: the right panel's, and the
/// Skirmish family's that the pages' buttons and lists paint with.
pub(crate) fn family_art_ready(state: &mut AppState) -> bool {
    state.frontend.main_menu_shell_chrome.is_some()
        && crate::app::App::ensure_skirmish_shell_chrome(state)
}

/// One family page frame for [`encode_family_page`].
pub(crate) struct FamilyFrame<'a> {
    pub(crate) label: &'static str,
    pub(crate) layout: &'a MenuPageLayout,
    pub(crate) panel: PanelState,
    /// Heading `0x694`'s CSF key, shown while `panel.title` reveals it.
    pub(crate) title_key: &'static str,
    /// The tree whose nodes `ids` the column's buttons are (its root): the
    /// owner-draw type-1 buttons with their captions, in `style`.
    pub(crate) buttons: Option<(&'a Gui, NodeId)>,
    pub(crate) ids: Vec<u16>,
    pub(crate) style: GuiStyle<'a>,
    /// The family parent background ([`shell_parent_background_instances`])
    /// goes under everything.
    pub(crate) backdrop: bool,
    /// Family list boxes over that background, each its window and, when it
    /// shows one, its scrollbar ([`push_family_list_backing`]).
    pub(crate) list_boxes: Vec<(RectPx, Option<RectPx>)>,
    /// The page's own art under the right panel, in draw order.
    pub(crate) under: Vec<TexturedDraw<'a>>,
    /// Other art over the panel, under the buttons (a score page's own).
    pub(crate) over_panel_art: Vec<TexturedDraw<'a>>,
    /// The page's own text, drawn before the heading and status line.
    pub(crate) labels: Vec<PaintLabel<'a>>,
    /// Skirmish-atlas sprites over the buttons (a list, a prompt), with
    /// their text after all other text.
    pub(crate) over: GuiPaint,
    /// Whether this pass draws the cursor (a box painted over the page next
    /// draws it instead).
    pub(crate) cursor: bool,
}

impl<'a> FamilyFrame<'a> {
    pub(crate) fn new(
        label: &'static str,
        layout: &'a MenuPageLayout,
        panel: PanelState,
        title_key: &'static str,
    ) -> Self {
        Self {
            label,
            layout,
            panel,
            title_key,
            buttons: None,
            ids: layout.buttons.iter().map(|button| button.id).collect(),
            style: GuiStyle::default(),
            backdrop: false,
            list_boxes: Vec::new(),
            under: Vec::new(),
            over_panel_art: Vec::new(),
            labels: Vec::new(),
            over: GuiPaint::default(),
            cursor: true,
        }
    }

    /// The column's buttons are `dialog`'s nodes on the shell controller,
    /// or `scratch`'s ([`page_tree`]).
    pub(crate) fn on(
        self,
        state: &'a AppState,
        dialog: DialogResourceId,
        scratch: &'a Option<(Gui, NodeId)>,
    ) -> Self {
        let controller = &state.frontend.shell_controller;
        let buttons = match scratch {
            Some((gui, root)) => Some((gui, *root)),
            None => controller
                .root_in(dialog)
                .map(|root| (controller.gui(), root)),
        };
        Self { buttons, ..self }
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
    let (Some(chrome), Some(atlas)) = (
        state.frontend.main_menu_shell_chrome.as_ref(),
        state.frontend.skirmish_shell_chrome.as_ref(),
    ) else {
        return;
    };
    let FamilyFrame {
        label,
        layout,
        panel,
        title_key,
        buttons,
        ids,
        style,
        backdrop,
        list_boxes,
        under,
        over_panel_art,
        mut labels,
        over,
        cursor,
    } = frame;
    let (screen, scale) = (layout.screen, layout.scale);
    let mut draws = Vec::new();
    if backdrop {
        draws.push(TexturedDraw {
            texture: &chrome.texture,
            instances: shell_parent_background_instances(chrome, screen.w, screen.h, scale),
        });
        let mut backings = Vec::new();
        for (window, bar) in list_boxes {
            let size = (screen.w, screen.h);
            push_family_list_backing(&mut backings, chrome, size, scale, window, bar);
        }
        draws.push(TexturedDraw {
            texture: &chrome.texture,
            instances: backings,
        });
    }
    draws.extend(under);
    let mut panel_sprites = shell_paint::paint_chrome(
        chrome,
        layout.right_panel,
        Some(layout.lower_strip),
        screen.w,
    );
    panel_sprites.extend(panel.monitor.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.warning_monitor, frame, scale)
    }));
    let mut widgets = match (&panel.column, buttons) {
        (Some(draws), _) => {
            let column = shell_paint::paint_slide_column(chrome, layout.right_panel, draws);
            panel_sprites.extend(column);
            GuiPaint::default()
        }
        (None, Some((gui, root))) => {
            let style = GuiStyle {
                buttons: ButtonArt::Column,
                ..style
            };
            paint_gui_ids(state, gui, root, ids, scale, style)
        }
        (None, None) => GuiPaint::default(),
    };
    if let Some(window) = panel.title {
        labels.push(PaintLabel {
            text: state.csf_text(title_key).into(),
            rect: layout.right_panel.family_title_caption(layout.title),
            align: ShellAlign::ART_CAPTION,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    labels.extend(panel.status);
    let mut text =
        shell_paint::paint_labels_scaled(&state.renderer.bit_font, &labels, scale.factor());
    widgets.extend(over);
    text.extend(widgets.texts);
    draws.push(TexturedDraw {
        texture: &chrome.texture,
        instances: panel_sprites,
    });
    draws.extend(over_panel_art);
    draws.push(TexturedDraw {
        texture: &chrome.texture,
        instances: widgets.column,
    });
    draws.push(TexturedDraw {
        texture: &atlas.texture,
        instances: widgets.controls,
    });
    encode_shell_pass(
        state,
        encoder,
        destination,
        label,
        ShellComposition {
            draws: &draws,
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
    fn status_help_includes_disabled_load() {
        let layout = crate::ui::single_player_shell::compute_layout(800, 600);
        let dialog = SINGLE_PLAYER_PAGE.dialog;
        let mut c = crate::ui::shell::controller::DialogController::default();
        c.ensure_active(dialog);
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
