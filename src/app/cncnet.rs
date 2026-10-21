//! Offline CnCNet admission. This adapter never constructs gameplay state:
//! normalized requests enter the same campaign/skirmish owners as the shell.

use anyhow::{Context, Result, ensure};

use super::{App, AppState};
use crate::app::frontend::cncnet_spawn::{SpawnGame, SpawnRequest};
use crate::app::frontend::startup_options::RetailStartupOptions;
use crate::app::match_bootstrap::{MatchSeedClock, MatchSeedSource, OrdinaryMatchSeedClock};

struct SpawnSeedClock {
    seed: Option<u32>,
}

impl MatchSeedClock for SpawnSeedClock {
    fn low_u32(&mut self) -> u32 {
        self.seed
            .unwrap_or_else(|| OrdinaryMatchSeedClock.low_u32())
    }

    fn source(&self) -> MatchSeedSource {
        if self.seed.is_some() {
            MatchSeedSource::Controlled
        } else {
            OrdinaryMatchSeedClock.source()
        }
    }

    fn seed_authority_certifying(&self) -> bool {
        self.seed.is_none() && OrdinaryMatchSeedClock.seed_authority_certifying()
    }
}

impl App {
    /// CnCNet runs the executable with its game directory as cwd and writes
    /// spawn.ini immediately before launch. Read once, before window creation.
    pub fn new_spawn(options: RetailStartupOptions) -> Result<Self> {
        ensure!(
            std::env::var_os("RA2_QUICKPLAY").is_none(),
            "-SPAWN cannot be combined with RA2_QUICKPLAY"
        );
        let request = SpawnRequest::read(std::path::Path::new("spawn.ini"))?;
        let mut app = Self::new(options);
        app.spawn_request = Some(request);
        Ok(app)
    }

    pub(super) fn start_spawn_session(state: &mut AppState, request: SpawnRequest) -> Result<()> {
        ensure!(
            state.frontend.main_menu_shell_error.is_none(),
            "retail assets are unavailable: {}",
            state
                .frontend
                .main_menu_shell_error
                .as_deref()
                .unwrap_or_default()
        );
        state.frontend.external_launch = true;
        state.persistence.options_profile.game_speed = request.game_speed;
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_first_paint_slide = None;
        let mut clock = SpawnSeedClock { seed: request.seed };
        match request.game {
            SpawnGame::Skirmish(session) => {
                log::info!(
                    "CnCNet offline skirmish: scenario={:?}, players={}, seed={:?}",
                    session.selected_map_file,
                    1 + session.opponents.len(),
                    request.seed
                );
                Self::start_skirmish_session(state, *session, &mut clock)?;
            }
            SpawnGame::Campaign(campaign) => {
                state.process_assets.refresh_empty_campaigns_if_available();
                let catalog = state
                    .process_assets
                    .campaigns()
                    .context("retail campaign catalog unavailable")?;
                let side = campaign.side;
                let campaign_id = campaign.campaign_id;
                let startup = campaign.prepare_startup(catalog, request.game_speed, &mut clock)?;
                state.persistence.options_profile.difficulty = startup.difficulty();
                state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
                log::info!(
                    "CnCNet campaign: scenario={}, side={}, difficulty={}, client_campaign_id={}",
                    startup.scenario(),
                    side,
                    startup.difficulty(),
                    campaign_id
                );
                Self::start_campaign_scenario(
                    state,
                    crate::app::loading::pump::LoadingRequest::campaign(startup),
                );
            }
        }
        Ok(())
    }
}
