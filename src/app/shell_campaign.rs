//! Campaign selection route: dialog `0x94`, `Main__PrepareSession` state 8
//! (`0x0052DF05`). Single Player's New Campaign (`0x688`) tears `0x100` down
//! and creates `0x94`; Back (`0x686`, result -1) tears `0x94` down and state 1
//! recreates Single Player.

use std::time::Instant;

use super::{App, AppState};
use crate::app::shell_route::ShellRoute;
use crate::ui::campaign_shell::{
    CAMPAIGN_DIALOG, CampaignLayout, CampaignRelease, CampaignShellState, CampaignSide,
    DIFFICULTY_SLIDER, compute_layout,
};
use crate::ui::shell::layout::LaidOutControl;

/// Owner id of the campaign voice handle `0x00A8F300`: a new voice stops the
/// one before it (`0x00405D40` then `0x00750920`).
const CAMPAIGN_VOICE_OWNER: u64 = 0xFFFF_FFFF_0000_0094;

impl App {
    /// State 8: create `0x94` after `0x100` is destroyed. Its proc starts the
    /// slider at OptionsClass `Difficulty` (`0x0052F114`).
    pub(super) fn open_campaign_page(state: &mut AppState) {
        Self::enter_shell_window_mode(state);
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        state.frontend.campaign = Some(CampaignShellState::open(
            state.persistence.options_profile.difficulty,
        ));
        state.frontend.shell_route = ShellRoute::Campaign;
        state
            .frontend
            .shell_controller
            .reset_to(CAMPAIGN_DIALOG, false);
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
        let small = state.renderer.gpu.config.width == 640;
        let slider = Self::campaign_layout(state).slider;
        state.frontend.campaign_art =
            crate::render::main_menu_shell_chrome::build_campaign_shell_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                small,
                (slider.w as u32, slider.h as u32),
            );
        let ready = state.frontend.campaign_art.is_some();
        if !ready {
            log::warn!("Could not prepare campaign art from the registered retail archives");
        }
        ready
    }

    pub(super) fn campaign_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.campaign()
            && state.frontend.campaign.is_some()
    }

    fn campaign_layout(state: &AppState) -> CampaignLayout {
        compute_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        )
    }

    /// Hover targets for the status line: Back, then the emblems and the
    /// slider, which never press through the shared controller.
    fn campaign_feed(state: &mut AppState, layout: &CampaignLayout) -> Vec<LaidOutControl> {
        state
            .frontend
            .shell_controller
            .ensure_active(CAMPAIGN_DIALOG, false);
        let mut feed = Self::menu_page_button_feed(&layout.page);
        for side in CampaignSide::ALL {
            feed.push(LaidOutControl {
                id: side.emblem(),
                rect: layout.emblem(side),
            });
        }
        feed.push(LaidOutControl {
            id: DIFFICULTY_SLIDER,
            rect: layout.slider,
        });
        feed
    }

    fn campaign_buttons(feed: Vec<LaidOutControl>) -> Vec<LaidOutControl> {
        feed.into_iter()
            .filter(|control| control.id == crate::ui::campaign_shell::BACK_BUTTON)
            .collect()
    }

    pub(super) fn handle_campaign_mouse_move(state: &mut AppState) {
        let layout = Self::campaign_layout(state);
        let feed = Self::campaign_feed(state, &layout);
        let (x, y) = state.match_state.input.cursor_px();
        // A slider holding the mouse since a press on it keeps the status
        // line and Back's paint: the dialog's hit test (`0x00622CCB`) sees
        // nothing until the release.
        let slider_held = state
            .frontend
            .campaign
            .as_ref()
            .is_some_and(|campaign| campaign.slider_held());
        if !slider_held {
            state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        }
        let now = Instant::now();
        // A slider or a pressed Back holding the mouse: the dialog sees no
        // hit test (`0x0052ED60`) until the release.
        let back_pressed = state.frontend.shell_controller.pressed().is_some();
        let mut slider_moved = false;
        if let Some(campaign) = state.frontend.campaign.as_mut() {
            if !slider_held && !back_pressed {
                campaign.pointer_moved(layout.emblem_at(x, y), now);
            }
            slider_moved = campaign.slider_drag(x - layout.slider.x, layout.slider.w);
        }
        if slider_moved {
            // 0x0061E6DD: mouse-driven position changes click.
            Self::play_generic_click_sound(state);
        }
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.repaint();
        state.platform.window.request_redraw();
    }

    pub(super) fn handle_campaign_mouse_down(state: &mut AppState) {
        let layout = Self::campaign_layout(state);
        let (x, y) = state.match_state.input.cursor_px();
        let emblem = layout.emblem_at(x, y);
        let captured = state
            .frontend
            .campaign
            .as_mut()
            .is_some_and(|campaign| campaign.pointer_down(emblem));
        if captured {
            // 0x0052EF52: GUIMainButtonSound (Rules +0x188).
            Self::play_main_menu_button_sound(state);
            return;
        }
        if layout.slider.contains(x, y) {
            let moved = state.frontend.campaign.as_mut().is_some_and(|campaign| {
                campaign.slider_press(
                    x - layout.slider.x,
                    y - layout.slider.y,
                    layout.slider.w,
                    layout.slider.h,
                )
            });
            if moved {
                Self::play_generic_click_sound(state);
            }
            return;
        }
        let feed = Self::campaign_buttons(Self::campaign_feed(state, &layout));
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_campaign_mouse_up(state: &mut AppState) {
        let layout = Self::campaign_layout(state);
        let (x, y) = state.match_state.input.cursor_px();
        let back_pressed = state.frontend.shell_controller.pressed().is_some();
        let Some(campaign) = state.frontend.campaign.as_mut() else {
            return;
        };
        let emblem_capture = campaign.pressed().is_some();
        let slider_held = campaign.slider_held();
        campaign.slider_release();
        if slider_held {
            // The release frees the mouse; the pointer's next hit test finds
            // what lies under it.
            Self::handle_campaign_mouse_move(state);
            return;
        }
        // The release reaches the dialog proc (`0x0052F1CF`) when an emblem
        // press holds its capture, or over the background and the emblems
        // (their statics do not take the mouse). Back and the slider are
        // windows of their own and receive it themselves.
        let over_child = layout.slider.contains(x, y)
            || layout
                .page
                .buttons
                .iter()
                .any(|button| button.rect.contains(x, y));
        if emblem_capture || (!back_pressed && !over_child) {
            let release = campaign.pointer_up(layout.emblem_at(x, y), Instant::now());
            if let CampaignRelease::Selected(side) = release {
                Self::select_campaign(state, side);
            }
            return;
        }
        let feed = Self::campaign_buttons(Self::campaign_feed(state, &layout));
        if state.frontend.shell_controller.on_pointer_up(x, y, &feed)
            == Some(crate::ui::campaign_shell::BACK_BUTTON)
        {
            Self::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::CampaignBack,
            );
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
        state.persistence.options_profile.difficulty = difficulty;
        if let Some(voice) = voice {
            Self::play_campaign_voice(state, voice);
        }
        Self::leave_shell_dialog(
            state,
            crate::app::frontend::shell_transition::ShellExitThen::CampaignStart(side),
        );
    }

    pub(super) fn commit_campaign_start(state: &mut AppState, side: CampaignSide) {
        state.process_assets.refresh_empty_campaigns_if_available();
        let mut clock = crate::match_bootstrap::OrdinaryMatchSeedClock;
        let startup = state.process_assets.campaigns().and_then(|campaigns| {
            crate::match_bootstrap::prepare_campaign_startup(
                campaigns,
                side.campaign_name(),
                state.persistence.options_profile.difficulty,
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
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
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
    fn start_campaign_scenario(
        state: &mut AppState,
        request: crate::app::loading::pump::LoadingRequest,
    ) {
        let movie = (|| -> anyhow::Result<Option<String>> {
            let Some(assets) = state.process_assets.manager() else {
                return Ok(None);
            };
            let Some(rules) = state.process_assets.native_rules() else {
                return Ok(None);
            };
            let Some(config) = state.platform.game_config.as_ref() else {
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

    /// The dialog loop's hover voice and the emblem timers. Returns the next
    /// wake deadline.
    pub(super) fn poll_campaign(state: &mut AppState) -> Option<Instant> {
        if !Self::campaign_active(state)
            || crate::app::frontend::shell_transition::shell_slide_running(state)
        {
            return None;
        }
        let now = Instant::now();
        let frame_counts = crate::app::frontend::campaign_shell_render::emblem_frame_counts(state);
        let campaign = state.frontend.campaign.as_mut()?;
        let voice = campaign.take_due_voice(now);
        let changed = campaign.advance_emblems(now, frame_counts);
        let deadline = campaign.next_deadline();
        if let Some(voice) = voice {
            Self::play_campaign_voice(state, voice);
        }
        if changed {
            state.platform.window.request_redraw();
        }
        deadline
    }
}
