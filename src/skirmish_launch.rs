//! App-level skirmish launch contract.
//!
//! This module is intentionally data-only: it packages the lobby state the app
//! needs to create stock-offline houses and initial spawns without making `sim/`
//! depend on UI, rendering, audio, or networking modules.

use crate::sim::game_options::GameOptions;
use crate::sim::rng::SimRng;
use crate::skirmish_modes::SkirmishGameMode;

pub const SKIRMISH_PLAYER_SLOT_COUNT: usize = 8;
pub const SKIRMISH_AI_SLOT_COUNT: usize = SKIRMISH_PLAYER_SLOT_COUNT - 1;
pub const HOUSE_COLOR_COUNT: usize = 8;

/// One human node in the native pre-Fill House-construction roster.
///
/// Nodes are stably sorted by the signed priority byte. Observers still
/// construct a House (and therefore consume the constructor timer draw), but
/// do not increase Gather's required-start count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreFillHumanHouse {
    pub priority: i8,
    pub source_order: u8,
    pub observer: bool,
}

/// One native AI session slot before active opponents are compacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreFillAiHouseSlot {
    pub slot_index: u8,
    pub valid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreFillFixedHouse {
    Neutral,
    Special,
}

/// Immutable House roster consumed identically by both noncampaign pre-Fill
/// construction passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreFillHouseRoster {
    human_nodes: Vec<PreFillHumanHouse>,
    ai_slots: Vec<PreFillAiHouseSlot>,
    fixed_tail: [PreFillFixedHouse; 2],
}

impl PreFillHouseRoster {
    pub fn new(
        mut human_nodes: Vec<PreFillHumanHouse>,
        mut ai_slots: Vec<PreFillAiHouseSlot>,
    ) -> Self {
        // Native sorts by the signed priority byte and preserves source-list
        // order for ties. `source_order` makes that ordering independent of
        // whichever app adapter assembled this normalized value.
        human_nodes.sort_by_key(|node| (node.priority, node.source_order));
        ai_slots.sort_by_key(|slot| slot.slot_index);
        Self {
            human_nodes,
            ai_slots,
            fixed_tail: [PreFillFixedHouse::Neutral, PreFillFixedHouse::Special],
        }
    }

    /// Compatibility constructor for callers that already hold the compact
    /// ordinary-skirmish session. Production shell packing retains all seven
    /// raw AI slots and does not use this shortcut.
    pub fn from_compact_skirmish(active_ai_count: usize) -> Self {
        let ai_slots = (0..SKIRMISH_AI_SLOT_COUNT)
            .map(|slot_index| PreFillAiHouseSlot {
                slot_index: slot_index as u8,
                valid: slot_index < active_ai_count,
            })
            .collect();
        Self::new(
            vec![PreFillHumanHouse {
                priority: 0,
                source_order: 0,
                observer: false,
            }],
            ai_slots,
        )
    }

    pub fn human_nodes(&self) -> &[PreFillHumanHouse] {
        &self.human_nodes
    }

    pub fn ai_slots(&self) -> &[PreFillAiHouseSlot] {
        &self.ai_slots
    }

    pub fn fixed_tail(&self) -> &[PreFillFixedHouse; 2] {
        &self.fixed_tail
    }

    pub fn created_house_count(&self) -> usize {
        self.human_nodes.len()
            + self.ai_slots.iter().filter(|slot| slot.valid).count()
            + self.fixed_tail.len()
    }

    pub fn required_start_count(&self) -> usize {
        self.nonobserver_human_count() + self.ai_slots.iter().filter(|slot| slot.valid).count()
    }

    pub fn nonobserver_human_count(&self) -> usize {
        self.human_nodes
            .iter()
            .filter(|node| !node.observer)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishLaunchMode {
    pub id: i32,
    pub ui_name_key: String,
    pub tooltip_key: String,
    pub override_file: String,
    pub map_filter: String,
    pub random_maps_allowed: bool,
    pub allies_allowed: bool,
    pub must_ally: bool,
}

impl SkirmishLaunchMode {
    pub fn from_game_mode(mode: &SkirmishGameMode) -> Self {
        Self {
            id: mode.id,
            ui_name_key: mode.ui_name_key.clone(),
            tooltip_key: mode.tooltip_key.clone(),
            override_file: mode.override_file.clone(),
            map_filter: mode.map_filter.clone(),
            random_maps_allowed: mode.random_maps_allowed,
            allies_allowed: mode.allies_allowed,
            must_ally: mode.must_ally,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchCountry {
    America,
    Korea,
    France,
    Germany,
    GreatBritain,
    Libya,
    Iraq,
    Cuba,
    Russia,
    Yuri,
}

impl LaunchCountry {
    pub const fn country_name(self) -> &'static str {
        match self {
            Self::America => "Americans",
            Self::Korea => "Alliance",
            Self::France => "French",
            Self::Germany => "Germans",
            Self::GreatBritain => "British",
            Self::Libya => "Africans",
            Self::Iraq => "Arabs",
            Self::Cuba => "Confederation",
            Self::Russia => "Russians",
            Self::Yuri => "YuriCountry",
        }
    }

    pub const fn side_index(self) -> u8 {
        match self {
            Self::America | Self::Korea | Self::France | Self::Germany | Self::GreatBritain => 0,
            Self::Libya | Self::Iraq | Self::Cuba | Self::Russia => 1,
            Self::Yuri => 2,
        }
    }

    /// Map a ranged country index (0..=9) onto a concrete country, in the
    /// country-list order used by the setup UI. Values above the last index
    /// clamp to the final country so the mapping is total.
    pub const fn from_country_index(index: u32) -> Self {
        match index {
            0 => Self::America,
            1 => Self::Korea,
            2 => Self::France,
            3 => Self::Germany,
            4 => Self::GreatBritain,
            5 => Self::Libya,
            6 => Self::Iraq,
            7 => Self::Cuba,
            8 => Self::Russia,
            _ => Self::Yuri,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchStartPosition {
    Auto,
    Position(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchTeam {
    None,
    Team(u8),
}

impl LaunchTeam {
    pub const fn from_shell_value(value: i32) -> Self {
        if value < 0 {
            Self::None
        } else {
            Self::Team(value as u8)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiDifficulty {
    Easy,
    Normal,
    Hard,
}

impl AiDifficulty {
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Hard => 0,
            Self::Normal => 1,
            Self::Easy => 2,
        }
    }
}

impl Default for AiDifficulty {
    fn default() -> Self {
        Self::Easy
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishLaunchOptions {
    pub starting_credits: i32,
    pub unit_count: i32,
    pub tech_level: i32,
    pub game_speed: i32,
    /// Rules-owned default/global difficulty retained from
    /// `[MultiplayerDialogSettings]`. Offline AI houses carry their selected
    /// row difficulty separately; this value is not their shared authority.
    pub default_ai_difficulty: i32,
    pub short_game: bool,
    pub bases: bool,
    pub bridges_destroyable: bool,
    pub super_weapons: bool,
    pub build_off_ally: bool,
    pub crates: bool,
    pub mcv_redeploy: bool,
    pub fog_of_war: bool,
    pub shroud: bool,
    pub tiberium_grows: bool,
    pub multi_engineer: bool,
    pub harvester_truce: bool,
    pub ally_change_allowed: bool,
}

impl Default for SkirmishLaunchOptions {
    fn default() -> Self {
        let defaults = GameOptions::default();
        Self {
            starting_credits: defaults.starting_credits,
            unit_count: defaults.unit_count,
            tech_level: defaults.tech_level,
            game_speed: defaults.game_speed,
            default_ai_difficulty: defaults.ai_difficulty,
            short_game: defaults.short_game,
            bases: defaults.bases,
            bridges_destroyable: defaults.bridges_destroyable,
            super_weapons: defaults.super_weapons,
            build_off_ally: defaults.build_off_ally,
            crates: defaults.crates,
            mcv_redeploy: defaults.mcv_redeploy,
            fog_of_war: defaults.fog_of_war,
            shroud: defaults.shroud,
            tiberium_grows: defaults.tiberium_grows,
            multi_engineer: defaults.multi_engineer,
            harvester_truce: defaults.harvester_truce,
            ally_change_allowed: defaults.ally_change_allowed,
        }
    }
}

impl SkirmishLaunchOptions {
    /// Build the launch base from per-match options parsed once from
    /// `[MultiplayerDialogSettings]`. The setup dialog later overrides the
    /// values it exposes as widgets; the remaining fields — tech level and the
    /// non-widget toggles (bases, shroud, tiberium growth, …) — flow straight
    /// through to the match from this base. The rules-owned default AI
    /// difficulty is retained for the global GameOptions mirror; configured
    /// opponent difficulty is copied separately into each HouseState.
    pub fn from_game_options(options: &GameOptions) -> Self {
        Self {
            starting_credits: options.starting_credits,
            unit_count: options.unit_count,
            tech_level: options.tech_level,
            game_speed: options.game_speed,
            default_ai_difficulty: options.ai_difficulty,
            short_game: options.short_game,
            bases: options.bases,
            bridges_destroyable: options.bridges_destroyable,
            super_weapons: options.super_weapons,
            build_off_ally: options.build_off_ally,
            crates: options.crates,
            mcv_redeploy: options.mcv_redeploy,
            fog_of_war: options.fog_of_war,
            shroud: options.shroud,
            tiberium_grows: options.tiberium_grows,
            multi_engineer: options.multi_engineer,
            harvester_truce: options.harvester_truce,
            ally_change_allowed: options.ally_change_allowed,
        }
    }

    pub fn to_game_options(&self, ai_players: i32) -> GameOptions {
        GameOptions {
            short_game: self.short_game,
            bases: self.bases,
            bridges_destroyable: self.bridges_destroyable,
            super_weapons: self.super_weapons,
            build_off_ally: self.build_off_ally,
            crates: self.crates,
            mcv_redeploy: self.mcv_redeploy,
            fog_of_war: self.fog_of_war,
            shroud: self.shroud,
            tiberium_grows: self.tiberium_grows,
            multi_engineer: self.multi_engineer,
            harvester_truce: self.harvester_truce,
            ally_change_allowed: self.ally_change_allowed,
            starting_credits: self.starting_credits,
            unit_count: self.unit_count,
            tech_level: self.tech_level,
            game_speed: self.game_speed,
            ai_difficulty: self.default_ai_difficulty,
            ai_players,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishLocalSlot {
    pub country: LaunchCountry,
    /// When true, `country` is a placeholder to be replaced by a random draw
    /// on the app-owned shell Scenario cursor before map loading.
    pub country_random: bool,
    pub color_index: u8,
    /// When true, `color_index` is a placeholder to be replaced by a random
    /// collision-free color draw during shell session resolution. The raw UI
    /// selection and persisted snapshot retain the Random sentinel.
    pub color_random: bool,
    pub start_position: LaunchStartPosition,
    pub team: LaunchTeam,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishAiSlot {
    pub country: LaunchCountry,
    /// When true, `country` is a placeholder to be replaced by a random draw
    /// on the app-owned shell Scenario cursor before map loading.
    pub country_random: bool,
    pub color_index: u8,
    /// See [`SkirmishLocalSlot::color_random`].
    pub color_random: bool,
    pub start_position: LaunchStartPosition,
    pub team: LaunchTeam,
    pub difficulty: AiDifficulty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishLaunchSession {
    pub mode: SkirmishLaunchMode,
    pub selected_map_file: Option<String>,
    pub player_name: String,
    pub local: SkirmishLocalSlot,
    pub opponents: Vec<SkirmishAiSlot>,
    /// Native pre-compaction House constructor roster used by both pre-Fill
    /// passes. It deliberately retains inactive AI slots and observer state.
    pub pre_fill_house_roster: PreFillHouseRoster,
    pub options: SkirmishLaunchOptions,
}

/// Staged shell assignment copy used by the app's native-order close
/// transaction. Keeping the phases explicit lets snapshot Slot packing occur
/// after local country but before local colour without mutating raw controls.
pub(crate) struct ShellRandomAssignmentState {
    resolved: SkirmishLaunchSession,
    used_colors: Vec<u8>,
}

impl ShellRandomAssignmentState {
    pub(crate) fn new(session: &SkirmishLaunchSession) -> Self {
        let mut used_colors = Vec::new();
        if !session.local.color_random {
            used_colors.push(session.local.color_index);
        }
        for opponent in &session.opponents {
            if !opponent.color_random {
                used_colors.push(opponent.color_index);
            }
        }
        Self {
            resolved: session.clone(),
            used_colors,
        }
    }

    pub(crate) fn resolve_local_country<E>(
        &mut self,
        rng: &mut SimRng,
        draw_country: &mut impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<(), E> {
        if self.resolved.local.country_random {
            self.resolved.local.country = draw_country(RandomCountryRole::Human, rng)?;
            self.resolved.local.country_random = false;
        }
        Ok(())
    }

    /// Materialize the AI assignment arrays after the local-country phase.
    /// The app close transaction starts with an empty opponent list so this
    /// write occurs at the same native phase as the raw persisted Slot pack.
    pub(crate) fn pack_ai_assignments(&mut self, opponents: &[SkirmishAiSlot]) {
        debug_assert!(self.resolved.opponents.is_empty());
        self.resolved.opponents = opponents.to_vec();
        self.used_colors.extend(
            opponents
                .iter()
                .filter(|opponent| !opponent.color_random)
                .map(|opponent| opponent.color_index),
        );
    }

    pub(crate) fn resolve_local_color(&mut self, rng: &mut SimRng) {
        if self.resolved.local.color_random {
            let color = draw_collision_free_color(rng, &self.used_colors);
            self.resolved.local.color_index = color;
            self.resolved.local.color_random = false;
            self.used_colors.push(color);
        }
    }

    pub(crate) fn resolve_ai<E>(
        &mut self,
        rng: &mut SimRng,
        draw_country: &mut impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<(), E> {
        for (index, opponent) in self.resolved.opponents.iter_mut().enumerate() {
            if opponent.country_random {
                opponent.country = draw_country(RandomCountryRole::Ai { index }, rng)?;
                opponent.country_random = false;
            }
            if opponent.color_random {
                let color = draw_collision_free_color(rng, &self.used_colors);
                opponent.color_index = color;
                opponent.color_random = false;
                self.used_colors.push(color);
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> SkirmishLaunchSession {
        self.resolved
    }
}

impl SkirmishLaunchSession {
    /// Resolve every slot left on "random country"/"random color" using the
    /// verified ordinary selected-mode Scenario-RNG order:
    /// **all humans first, then all AI slots**, and **within each slot country
    /// before color**. Each random country is one inclusive `(0, 9)` draw; each
    /// random color is one inclusive `(0, 7)` draw repeated until it does not
    /// collide with an already-assigned color (every retry is one more draw).
    /// Slots that already hold a concrete value are left untouched and consume
    /// no draw, so the stream only advances for the slots that requested random.
    ///
    /// The supplied RNG must be the app-owned frontend/process Scenario cursor,
    /// never the freshly seeded gameplay cursor.
    pub fn resolve_shell_random_assignments(&self, rng: &mut SimRng) -> Self {
        self.resolve_shell_random_assignments_with(rng, |_, rng| {
            LaunchCountry::from_country_index(rng.next_range_u32_inclusive(0, 9))
        })
    }

    /// Resolve shell assignments while delegating country selection to the
    /// selected MPModes authority. Cooperative uses this hook for eligibility
    /// retries; colours remain here so every rejected country candidate shifts
    /// the following colour draw.
    pub fn resolve_shell_random_assignments_with(
        &self,
        rng: &mut SimRng,
        mut draw_country: impl FnMut(RandomCountryRole, &mut SimRng) -> LaunchCountry,
    ) -> Self {
        match self.try_resolve_shell_random_assignments_with(rng, |role, rng| {
            Ok::<_, std::convert::Infallible>(draw_country(role, rng))
        }) {
            Ok(resolved) => resolved,
            Err(never) => match never {},
        }
    }

    /// Fallible form of shell assignment resolution for selected-mode country
    /// authorities whose data can be malformed. An error stops the transaction
    /// immediately, retaining draws already consumed but making no later
    /// country or colour calls.
    pub fn try_resolve_shell_random_assignments_with<E>(
        &self,
        rng: &mut SimRng,
        mut draw_country: impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<Self, E> {
        let mut state = ShellRandomAssignmentState::new(self);

        // Phase A: humans (here, the single local slot) — COUNTRY then COLOR.
        state.resolve_local_country(rng, &mut draw_country)?;
        state.resolve_local_color(rng);

        // Phase B: AI slots in order — each COUNTRY then COLOR.
        state.resolve_ai(rng, &mut draw_country)?;

        Ok(state.finish())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomCountryRole {
    Human,
    Ai { index: usize },
}

/// Draw an unused shell colour in `0..=7`.
///
/// Each collision retry advances the supplied Scenario stream once. More
/// Random colour slots than free colours intentionally has no exit, matching
/// the native malformed-state behavior.
fn draw_collision_free_color(rng: &mut SimRng, used: &[u8]) -> u8 {
    loop {
        let color = rng.next_range_u32_inclusive(0, 7) as u8;
        if !used.contains(&color) {
            return color;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchValidationError {
    NoSelectedMap,
    NoSelectedMode {
        mode_id: i32,
    },
    NoEnabledOpponent,
    MapCapacityExceeded {
        capacity: i32,
        requested_players: usize,
    },
    SameExplicitTeam {
        team: u8,
    },
    InvalidColorIndex {
        slot: usize,
        color_index: usize,
    },
    InvalidStartPosition {
        slot: usize,
        position: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_options_preserve_build_off_ally_default() {
        let options = SkirmishLaunchOptions::default();
        let game_options = options.to_game_options(3);

        assert!(game_options.build_off_ally);
        assert_eq!(game_options.ai_players, 3);
        assert_eq!(game_options.ai_difficulty, 0);
    }

    #[test]
    fn yuri_country_uses_third_side() {
        assert_eq!(LaunchCountry::Yuri.side_index(), 2);
    }

    #[test]
    fn from_game_options_carries_non_widget_fields_to_the_match() {
        // The launch base is the path by which parsed-INI fields the setup
        // dialog does not expose (tech level, bases, shroud, …) reach the
        // launched match. Build a base from modded options and confirm those
        // fields survive the round-trip through to_game_options.
        let mut parsed = GameOptions::default();
        parsed.tech_level = 3;
        parsed.bases = false;
        parsed.shroud = false;
        parsed.tiberium_grows = false;
        parsed.harvester_truce = true;
        parsed.multi_engineer = true;
        parsed.bridges_destroyable = false;
        parsed.ally_change_allowed = false;
        parsed.fog_of_war = true;
        parsed.ai_difficulty = 2;

        let base = SkirmishLaunchOptions::from_game_options(&parsed);
        let launched = base.to_game_options(2);

        assert_eq!(launched.tech_level, 3);
        assert!(!launched.bases);
        assert!(!launched.shroud);
        assert!(!launched.tiberium_grows);
        assert!(launched.harvester_truce);
        assert!(launched.multi_engineer);
        assert!(!launched.bridges_destroyable);
        assert!(!launched.ally_change_allowed);
        assert!(launched.fog_of_war);
        // The global/default difficulty remains rules-owned; individual AI
        // rows are copied to HouseState during launch population.
        assert_eq!(launched.ai_players, 2);
        assert_eq!(launched.ai_difficulty, 2);
    }

    #[test]
    fn launch_mode_carries_selected_mpmode_data() {
        let mode = SkirmishGameMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
            id: 9,
            ui_name_key: "GUI:TeamGame".to_string(),
            tooltip_key: "STT:ModeTeamGame".to_string(),
            override_file: "MPTeamMD.ini".to_string(),
            map_filter: "teamgame".to_string(),
            random_maps_allowed: false,
            allies_allowed: true,
            must_ally: true,
        };

        let launch_mode = SkirmishLaunchMode::from_game_mode(&mode);

        assert_eq!(launch_mode.id, 9);
        assert_eq!(launch_mode.ui_name_key, "GUI:TeamGame");
        assert_eq!(launch_mode.override_file, "MPTeamMD.ini");
        assert!(launch_mode.allies_allowed);
        assert!(launch_mode.must_ally);
    }

    #[test]
    fn shell_team_values_use_negative_none_and_zero_based_teams() {
        assert_eq!(LaunchTeam::from_shell_value(-2), LaunchTeam::None);
        assert_eq!(LaunchTeam::from_shell_value(-1), LaunchTeam::None);
        assert_eq!(LaunchTeam::from_shell_value(0), LaunchTeam::Team(0));
        assert_eq!(LaunchTeam::from_shell_value(3), LaunchTeam::Team(3));
    }

    #[test]
    fn pre_fill_roster_orders_signed_humans_and_raw_ai_slots() {
        let roster = PreFillHouseRoster::new(
            vec![
                PreFillHumanHouse {
                    priority: 4,
                    source_order: 2,
                    observer: false,
                },
                PreFillHumanHouse {
                    priority: -1,
                    source_order: 1,
                    observer: true,
                },
                PreFillHumanHouse {
                    priority: -1,
                    source_order: 0,
                    observer: false,
                },
            ],
            vec![
                PreFillAiHouseSlot {
                    slot_index: 6,
                    valid: true,
                },
                PreFillAiHouseSlot {
                    slot_index: 1,
                    valid: false,
                },
                PreFillAiHouseSlot {
                    slot_index: 3,
                    valid: true,
                },
            ],
        );

        assert_eq!(
            roster
                .human_nodes()
                .iter()
                .map(|node| (node.priority, node.source_order))
                .collect::<Vec<_>>(),
            vec![(-1, 0), (-1, 1), (4, 2)]
        );
        assert_eq!(
            roster
                .ai_slots()
                .iter()
                .map(|slot| (slot.slot_index, slot.valid))
                .collect::<Vec<_>>(),
            vec![(1, false), (3, true), (6, true)]
        );
        assert_eq!(roster.nonobserver_human_count(), 2);
        assert_eq!(roster.required_start_count(), 4);
        assert_eq!(roster.created_house_count(), 7);
    }

    fn random_test_session() -> SkirmishLaunchSession {
        SkirmishLaunchSession {
            mode: SkirmishLaunchMode {
                id: 1,
                ui_name_key: "GUI:Battle".to_string(),
                tooltip_key: "STT:ModeBattle".to_string(),
                override_file: "MPBattleMD.ini".to_string(),
                map_filter: "standard".to_string(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            selected_map_file: Some("test.mmx".to_string()),
            player_name: "Player".to_string(),
            local: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: true,
                color_index: 0,
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
            },
            opponents: vec![
                SkirmishAiSlot {
                    country: LaunchCountry::Russia,
                    country_random: true,
                    color_index: 1,
                    color_random: false,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                    difficulty: AiDifficulty::Easy,
                },
                SkirmishAiSlot {
                    country: LaunchCountry::Cuba,
                    country_random: false,
                    color_index: 2,
                    color_random: false,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                    difficulty: AiDifficulty::Easy,
                },
            ],
            pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(2),
            options: SkirmishLaunchOptions::default(),
        }
    }

    #[test]
    fn shell_random_assignments_are_deterministic_for_a_seed() {
        let session = random_test_session();
        let mut rng_a = SimRng::new(0xC0FFEE);
        let mut rng_b = SimRng::new(0xC0FFEE);

        let first = session.resolve_shell_random_assignments(&mut rng_a);
        let second = session.resolve_shell_random_assignments(&mut rng_b);

        assert_eq!(first, second, "same seed must yield the same assignment");
        assert!(!first.local.country_random);
        assert!(!first.opponents[0].country_random);
    }

    #[test]
    fn shell_random_assignments_only_draw_for_random_slots_in_order() {
        let session = random_test_session();

        // Two random slots (local + opponent 0); opponent 1 is concrete and must
        // not consume a draw or change. The draw order is local first, then AI,
        // so resolving by hand in that order must reproduce the same result.
        let mut rng = SimRng::new(7);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        let mut expected_rng = SimRng::new(7);
        let expected_local =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));
        let expected_ai0 =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));

        assert_eq!(resolved.local.country, expected_local);
        assert_eq!(resolved.opponents[0].country, expected_ai0);
        // The concrete slot is untouched and the stream is now exhausted of the
        // two expected draws — both RNGs must be in the same state.
        assert_eq!(resolved.opponents[1].country, LaunchCountry::Cuba);
        assert!(!resolved.opponents[1].country_random);
        assert_eq!(rng.state(), expected_rng.state());
    }

    #[test]
    fn shell_random_assignments_leave_concrete_session_untouched() {
        let mut session = random_test_session();
        session.local.country_random = false;
        session.opponents[0].country_random = false;

        let before = SimRng::new(42).state();
        let mut rng = SimRng::new(42);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        assert_eq!(resolved, session, "no random slots means no change");
        assert_eq!(rng.state(), before, "no random slots means no draws");
    }

    #[test]
    fn shell_random_assignments_draw_country_then_color_humans_then_ai() {
        // local + ai0 random country AND random color; ai1 concrete (color 2).
        let mut session = random_test_session();
        session.local.color_random = true;
        session.opponents[0].color_random = true;
        assert!(!session.opponents[1].color_random);

        let mut rng = SimRng::new(7);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        // Replay the Rust compatibility order by hand: ALL humans (country then
        // color), THEN all AI (country then color). The used-color set seeds
        // with concrete colors first (here ai1 = 2), then accumulates as colors
        // are assigned.
        let mut expected_rng = SimRng::new(7);
        let mut used: Vec<u8> = vec![session.opponents[1].color_index];
        let exp_local_country =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));
        let exp_local_color = draw_collision_free_color(&mut expected_rng, &used);
        used.push(exp_local_color);
        let exp_ai0_country =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));
        let exp_ai0_color = draw_collision_free_color(&mut expected_rng, &used);

        assert_eq!(resolved.local.country, exp_local_country);
        assert_eq!(resolved.local.color_index, exp_local_color);
        assert_eq!(resolved.opponents[0].country, exp_ai0_country);
        assert_eq!(resolved.opponents[0].color_index, exp_ai0_color);
        assert_eq!(
            resolved.opponents[1].color_index, 2,
            "concrete AI color untouched"
        );
        assert!(!resolved.local.color_random && !resolved.opponents[0].color_random);
        // Identical stream state proves this Rust compatibility helper used this
        // order and count (country before color, humans before AI, collision
        // retries included).
        assert_eq!(rng.state(), expected_rng.state());
    }

    #[test]
    fn shell_random_assignments_color_collision_forces_redraw() {
        // Pre-occupy the color the seed would draw first, so the resolver must
        // redraw — proving each collision costs an extra scenario-cursor draw.
        let seed = 12345;
        let first_color = {
            let mut r = SimRng::new(seed);
            r.next_range_u32_inclusive(0, 7) as u8
        };

        let mut session = random_test_session();
        session.local.country_random = false;
        session.local.color_random = false;
        session.local.color_index = first_color; // occupy the colliding color
        session.opponents.truncate(1);
        session.opponents[0].country_random = false;
        session.opponents[0].color_random = true;

        let mut rng = SimRng::new(seed);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        assert_ne!(
            resolved.opponents[0].color_index, first_color,
            "assigned color must avoid the occupied one"
        );
        // The colliding first draw plus at least one retry => more than one draw.
        let mut one_draw = SimRng::new(seed);
        one_draw.next_range_u32_inclusive(0, 7);
        assert_ne!(
            rng.state(),
            one_draw.state(),
            "a color collision must cost an extra draw"
        );
    }

    #[test]
    fn fallible_country_authority_stops_before_later_assignment_draws() {
        let mut session = random_test_session();
        session.local.color_random = true;
        session.opponents[0].color_random = true;
        let seed = 0x4567;
        let mut rng = SimRng::new(seed);
        let mut roles = Vec::new();

        let result = session.try_resolve_shell_random_assignments_with(
            &mut rng,
            |role, _| -> Result<LaunchCountry, &'static str> {
                roles.push(role);
                match role {
                    RandomCountryRole::Human => Ok(LaunchCountry::America),
                    RandomCountryRole::Ai { .. } => Err("invalid Cooperative country list"),
                }
            },
        );

        assert_eq!(result, Err("invalid Cooperative country list"));
        assert_eq!(
            roles,
            vec![RandomCountryRole::Human, RandomCountryRole::Ai { index: 0 }]
        );
        let mut expected_rng = SimRng::new(seed);
        let _ = draw_collision_free_color(&mut expected_rng, &[1, 2]);
        assert_eq!(rng.state(), expected_rng.state());
    }
}
