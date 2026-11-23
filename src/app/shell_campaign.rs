//! Campaign selection route: dialog `0x94`, `Main__PrepareSession` state 8
//! (`0x0052DF05`). Single Player's New Campaign (`0x688`) tears `0x100` down
//! and creates `0x94`; Back (`0x686`, result -1) tears `0x94` down and state 1
//! recreates Single Player.

use crate::util::time::Instant;

use super::{App, AppState};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::shell_route::ShellRoute;
use crate::ui::campaign_shell::{
    BACK_BUTTON, CAMPAIGN_DIALOG, CampaignShellState, CampaignSide, DIFFICULTY_SLIDER,
};
use crate::ui::shell::menu_page;
use crate::ui::shell::page::{PageCue, PageEvent};

/// Owner id of the campaign voice handle `0x00A8F300`: a new voice stops the
/// one before it (`0x00405D40` then `0x00750920`).
const CAMPAIGN_VOICE_OWNER: u64 = 0xFFFF_FFFF_0000_0094;

impl App {
    /// State 8: create `0x94` after `0x100` is destroyed. Its proc starts the
    /// slider at OptionsClass `Difficulty` (`0x0052F114`).
    pub(super) fn open_campaign_page(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        state.frontend.campaign = Some(CampaignShellState::open(
            state.persistence.options_profile.difficulty,
        ));
        Self::show_shell_page(state, ShellRoute::Campaign, CAMPAIGN_DIALOG);
    }

    /// Back's teardown has run: the art is freed (`0x0072DAA0`) and state 1
    /// recreates Single Player.
    pub(super) fn commit_campaign_back(state: &mut AppState) {
        state.frontend.campaign = None;
        state.frontend.campaign_art = None;
        Self::open_single_player_shell(state);
    }

    /// Load the dialog's art (`0x0072D9A0`) for the current screen width.
    pub(crate) fn ensure_campaign_art(state: &mut AppState) -> bool {
        if state.frontend.campaign_art.is_some() {
            return true;
        }
        let Some(assets) = state.process_assets.manager() else {
            log::warn!("Could not prepare campaign art: process asset manager is unavailable");
            return false;
        };
        // The 640 art set belongs to the unscaled 640 dialog; a fitted layout
        // always authors `0x94` at 800x600, so it takes the large set and the
        // trackbar frame of that control size.
        let small = shell_layout::art_key_screen_width(state) == 640;
        let layout = shell_layout::campaign_layout(state);
        let base = layout.scale.base_rect(layout.slider);
        state.frontend.campaign_art =
            crate::render::main_menu_shell_chrome::build_campaign_shell_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                small,
                (base.w as u32, base.h as u32),
            );
        let ready = state.frontend.campaign_art.is_some();
        if !ready {
            log::warn!("Could not prepare campaign art from the registered retail archives");
        }
        ready
    }

    /// One pointer message (`down`: `None` moves) on `0x94`'s nodes: Back,
    /// the emblems and the difficulty slider, on the page's scale with the
    /// slider at the dialog's position. Returns their events after the cues.
    fn campaign_pointer(state: &mut AppState, down: Option<bool>) -> Vec<PageEvent> {
        let layout = shell_layout::campaign_layout(state);
        let difficulty = state.frontend.campaign.as_ref().map(|c| c.difficulty());
        let controller = &mut state.frontend.shell_controller;
        controller.ensure_active(CAMPAIGN_DIALOG);
        controller.set_scale(layout.scale);
        crate::ui::shell::menu_page::sync_nodes(
            controller,
            CAMPAIGN_DIALOG,
            crate::ui::campaign_shell::nodes,
            |id| layout.control_rect(id),
            &[],
        );
        if let (Some(root), Some(difficulty)) = (controller.root_in(CAMPAIGN_DIALOG), difficulty) {
            let pos = vera20k_gui::Value::Int(difficulty);
            let gui = controller.gui_mut();
            crate::ui::shell::page::set(gui, root, DIFFICULTY_SLIDER, vera20k_gui::Prop::Pos, pos);
        }
        let (x, y) = state.match_state.input.cursor_px();
        controller.on_input(crate::app::input::dialog_page::event_at(x, y, down));
        if down.is_none() {
            // Every hover message repaints the status line (0x00615EF7).
            state.frontend.shell_status_line.repaint();
        }
        let mut rest = Vec::new();
        for event in Self::shell_page_events(state, true) {
            match event {
                PageEvent::Slid { value, .. } => {
                    let moved = (state.frontend.campaign.as_mut())
                        .is_some_and(|campaign| campaign.set_difficulty(value));
                    if moved {
                        // 0x0061E6DD: mouse-driven position changes click.
                        Self::play_page_cue(state, PageCue::ListClick);
                    }
                }
                event => rest.push(event),
            }
        }
        rest
    }

    /// The control holding the mouse since a press on it.
    fn campaign_captured(state: &AppState) -> Option<u16> {
        menu_page::pressed_id(&state.frontend.shell_controller, CAMPAIGN_DIALOG)
    }

    /// The emblem under the pointer.
    fn campaign_emblem_hovered(state: &AppState) -> Option<CampaignSide> {
        menu_page::hovered_id(&state.frontend.shell_controller, CAMPAIGN_DIALOG)
            .and_then(CampaignSide::from_emblem)
    }

    /// A move: the slider holding the mouse follows it, and no control sees
    /// it then; with no press held, the dialog's own hit test (`0x0052ED60`)
    /// animates the emblem under it.
    pub(super) fn handle_campaign_mouse_move(state: &mut AppState) {
        Self::campaign_pointer(state, None);
        if Self::campaign_captured(state).is_none() {
            let under = Self::campaign_emblem_hovered(state);
            if let Some(campaign) = state.frontend.campaign.as_mut() {
                campaign.pointer_moved(under, Instant::now());
            }
        }
    }

    /// A press: an emblem captures it with GUIMainButtonSound (`0x0052EF52`),
    /// the slider holds the mouse, Back takes its own.
    pub(super) fn handle_campaign_mouse_down(state: &mut AppState) {
        Self::campaign_pointer(state, Some(true));
    }

    pub(super) fn handle_campaign_mouse_up(state: &mut AppState) {
        let captured = Self::campaign_captured(state);
        let hovered = menu_page::hovered_id(&state.frontend.shell_controller, CAMPAIGN_DIALOG);
        let events = Self::campaign_pointer(state, Some(false));
        if captured == Some(DIFFICULTY_SLIDER) {
            // The release frees the mouse; the pointer's next hit test finds
            // what lies under it.
            Self::handle_campaign_mouse_move(state);
            return;
        }
        for event in events {
            match event {
                PageEvent::Activated(BACK_BUTTON) => {
                    Self::leave_shell_dialog(
                        state,
                        ShellSlideKind::Campaign,
                        App::commit_campaign_back,
                    );
                    return;
                }
                PageEvent::Activated(id) => {
                    if let Some(side) = CampaignSide::from_emblem(id) {
                        if let Some(campaign) = state.frontend.campaign.as_mut() {
                            campaign.select();
                        }
                        Self::select_campaign(state, side);
                        return;
                    }
                }
                _ => {}
            }
        }
        // The release reaches the dialog proc (`0x0052F1CF`) when an emblem
        // press holds its capture, or over the background and the emblems.
        // Back and the slider are windows of their own and receive it
        // themselves.
        let emblem_capture = captured.and_then(CampaignSide::from_emblem).is_some();
        let over_child = matches!(hovered, Some(BACK_BUTTON | DIFFICULTY_SLIDER));
        if emblem_capture || (captured != Some(BACK_BUTTON) && !over_child) {
            let under = hovered.and_then(CampaignSide::from_emblem);
            if let Some(campaign) = state.frontend.campaign.as_mut() {
                campaign.pointer_up(under, Instant::now());
            }
        }
    }

    /// A press is no longer held (the window lost the focus): the slider
    /// lets go of the mouse (`0x0061E3AF`).
    pub(crate) fn release_campaign_slider(state: &mut AppState) {
        if Self::campaign_captured(state) == Some(DIFFICULTY_SLIDER) {
            state.frontend.shell_controller.gui_mut().cancel_pointer();
        }
    }

    /// Released on the pressed emblem: the proc stores the slider in
    /// OptionsClass `Difficulty` and plays the queued voice at once
    /// (`0x0052F2A5..0x0052F347`). PrepareSession state8 tears the dialog
    /// down and waits for that voice before starting the selected campaign.
    fn select_campaign(state: &mut AppState, side: CampaignSide) {
        let Some(campaign) = state.frontend.campaign.as_mut() else {
            return;
        };
        let difficulty = campaign.difficulty();
        let voice = campaign.take_selection_voice();
        // CampaignDialog52F28B rearms the process's initial-save attempt.
        state.persistence.arm_initial_campaign_save();
        state.persistence.options_profile.difficulty = difficulty;
        if let Some(voice) = voice {
            Self::play_campaign_voice(state, voice);
        }
        Self::leave_shell_dialog(state, ShellSlideKind::Campaign, move |state| {
            App::commit_campaign_start(state, side)
        });
    }

    pub(super) fn commit_campaign_start(state: &mut AppState, side: CampaignSide) {
        state.process_assets.refresh_empty_campaigns_if_available();
        let mut clock = crate::app::match_bootstrap::OrdinaryMatchSeedClock;
        let startup = state.process_assets.campaigns().and_then(|campaigns| {
            crate::app::match_bootstrap::prepare_campaign_startup(
                campaigns,
                side.campaign_name(),
                state.persistence.options_profile.difficulty,
                None,
                crate::app::match_bootstrap::CampaignLaunchOptions::for_speed_control(
                    state.frontend.campaign_speed_control,
                    state.persistence.options_profile.game_speed,
                ),
                &mut clock,
            )
        });
        let Some(startup) = startup else {
            // FindIndex46CC90's -1 follows the same result as Back.
            log::warn!("Campaign {} is unavailable", side.campaign_name());
            Self::commit_campaign_back(state);
            return;
        };
        state.frontend.campaign = None;
        state.frontend.campaign_art = None;
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_route = ShellRoute::MainMenu;
        state.frontend.shell_first_paint_slide = None;
        let request = crate::app::loading::pump::LoadingRequest::campaign(startup);
        Self::start_campaign_scenario(state, request);
    }

    /// Start_Scenario683AB0 reads the authored map's Intro/Brief before
    /// LOADING and ReadScenario. Brief is considered only when the Intro
    /// registry index is -1; a valid Intro with missing bytes does not fall back.
    /// Campaign.CD reaches4790B0/4790E0 at683BC9/683BEE; both stock rows require
    /// media2. The portable installed-archive owner supplies their map bytes.
    /// Native Windows drive prompt/remount limits are recorded beside the
    /// original comparison in tools/input_oracle/campaign_start.md.
    pub(super) fn start_campaign_scenario(
        state: &mut AppState,
        request: crate::app::loading::pump::LoadingRequest,
    ) {
        let movie = (|| -> anyhow::Result<Option<String>> {
            let (Some(assets), Some(rules), Some(config)) = (
                state.process_assets.manager(),
                state.process_assets.native_rules(),
                state.platform.game_config.as_ref(),
            ) else {
                return Ok(None);
            };
            let loaded = crate::map::source::load_map_by_name_or_path_with_assets(
                &config.paths.ra2_dir,
                request.selected_map_file(),
                assets,
            )?;
            let Some(basic) = loaded.map.ini.section("Basic") else {
                return Ok(None);
            };
            let movies = rules.movies();
            let intro = basic.read_movie("Intro", -1, movies);
            let selected = if intro == -1 {
                basic.read_movie("Brief", -1, movies)
            } else {
                intro
            };
            Ok(movies.name(selected).map(str::to_owned))
        })()
        .unwrap_or_else(|error| {
            // ReadScenario retains the authoritative load failure. The
            // optional playback lookup must not install a fallback scenario.
            log::warn!("Campaign Intro/Brief lookup failed: {error:#}");
            None
        });
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
        if let Some(movie) = movie {
            // StartScenario683AB0 stops the shell theme before Intro/Brief;
            // PlayMovie's pause/resume therefore cannot revive INTRO here.
            state.audio.stop_theme();
            Self::start_fullscreen_movie(
                state,
                &movie,
                crate::app::frontend::fullscreen_movie::MovieReturn::CampaignStart(Box::new(
                    request,
                )),
            );
        } else {
            crate::app::loading::pump::begin_loading(state, request);
        }
    }

    fn play_campaign_voice(state: &mut AppState, side: CampaignSide) {
        let (Some(sfx), Some(catalog)) = (
            &mut state.audio.sfx_player,
            state.process_assets.audio_catalog(),
        ) else {
            return;
        };
        sfx.play_animation_sound_spatial(
            CAMPAIGN_VOICE_OWNER,
            side.voice(),
            crate::audio::sfx::SpatialGain::CENTRED_FULL,
            catalog.sounds(),
        );
    }

    /// Whether the campaign voice still plays (`0x00406130` on `0x00A8F300`).
    pub(crate) fn campaign_voice_playing(state: &mut AppState) -> bool {
        state
            .audio
            .sfx_player
            .as_mut()
            .is_some_and(|sfx| sfx.owner_sound_live(CAMPAIGN_VOICE_OWNER))
    }

    /// The dialog loop's hover voice and the emblem timers while the page
    /// shows. Returns the next wake deadline.
    pub(super) fn poll_campaign(state: &mut AppState) -> Option<Instant> {
        if crate::app::frontend::shell_transition::shell_slide_running(state) {
            return None;
        }
        let now = Instant::now();
        let frame_counts = crate::app::frontend::campaign_shell_render::emblem_frame_counts(state);
        let campaign = state.frontend.campaign.as_mut()?;
        let voice = campaign.take_due_voice(now);
        campaign.advance_emblems(now, frame_counts);
        let deadline = campaign.next_deadline();
        if let Some(voice) = voice {
            Self::play_campaign_voice(state, voice);
        }
        deadline
    }
}
