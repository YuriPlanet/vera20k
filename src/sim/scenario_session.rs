//! Scenario session substrate — the launch descriptor the app layer feeds the
//! sim exactly once at construction.
//!
//! Mirrors the original engine fixing one per-match RNG seed before any
//! setup-phase draw, then seeding the scenario and main streams identically.
//! Data flows one-way app→sim; this module depends only on sim/ siblings.

use std::collections::BTreeMap;
use std::hash::Hash;

use crate::sim::game_options::GameOptions;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;

/// NukeLevel (`ScenarioClass+0x3574`): Set_Defaults stores 100 (`0x00683920`,
/// the binary's only store to it) and no INI key reads it.
pub const NUKE_LEVEL_UNITS: i32 = 100;

/// One map-authored global lighting profile in the native integer scales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScenarioLightProfileUnits {
    /// Ambient/R/G/B are percentages scaled by 100 at parse time.
    pub ambient_percent: i32,
    pub red_percent: i32,
    pub green_percent: i32,
    pub blue_percent: i32,
    /// Ground/Level use ScenarioClass's 1000-based INI conversion scale.
    /// Native reset values remain 50/8 (006838F7/00683901), not .20/.032.
    pub ground_units: i32,
    pub level_units: i32,
}

impl ScenarioLightProfileUnits {
    pub const fn normal_default() -> Self {
        Self {
            ambient_percent: 100,
            red_percent: 100,
            green_percent: 100,
            blue_percent: 100,
            ground_units: 50,
            level_units: 8,
        }
    }

    pub const fn ion_default() -> Self {
        Self {
            ambient_percent: 87,
            red_percent: 30,
            green_percent: 40,
            blue_percent: 75,
            ground_units: 0,
            level_units: 0,
        }
    }

    /// `ScenarioClass::Set_Defaults @ 0x00683610` (`0x00683991..
    /// 0x006839B7`).
    pub const fn dominator_default() -> Self {
        Self {
            ambient_percent: 150,
            red_percent: 85,
            green_percent: 20,
            blue_percent: 30,
            ground_units: 0,
            level_units: 0,
        }
    }

    /// The RGB `ScenarioClass::UpdateLighting @ 0x0053C280` hands
    /// RecalcLighting for an alternate profile: each percentage times 10.
    pub fn alternate_rgb(&self) -> [i32; 3] {
        [self.red_percent, self.green_percent, self.blue_percent].map(|v| v.wrapping_mul(10))
    }
}

impl From<crate::map::lighting::LightingProfileUnits> for ScenarioLightProfileUnits {
    fn from(units: crate::map::lighting::LightingProfileUnits) -> Self {
        Self {
            ambient_percent: units.ambient_percent,
            red_percent: units.red_percent,
            green_percent: units.green_percent,
            blue_percent: units.blue_percent,
            ground_units: units.ground_units,
            level_units: units.level_units,
        }
    }
}

impl Default for ScenarioLightProfileUnits {
    fn default() -> Self {
        Self::normal_default()
    }
}

/// Which map-authored RGB/Ground/Level tuple supplies the current global view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ScenarioLightingProfile {
    Normal,
    Ion,
    Dominator,
}

/// Persistent ScenarioClass-style authority for global lighting transitions.
///
/// The per-cell grid remains an app-derived cache. Only the profile inputs,
/// mutable scalar/target, selection, and signed frame timer belong to lockstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScenarioLightingState {
    pub normal: ScenarioLightProfileUnits,
    pub ion: ScenarioLightProfileUnits,
    /// `Dominator*=` (`ScenarioClass+0x357C..+0x3590`).
    pub dominator: ScenarioLightProfileUnits,
    /// `DominatorAmbientChangeRate=` (`+0x3594`): the fade interval while
    /// the Psychic Dominator is active.
    pub dominator_change_rate: i32,
    pub current_ambient: i32,
    pub target_ambient: i32,
    /// The profile the last `UpdateLighting` selected: its ambient is the
    /// target and its RGB the alternate tint.
    pub selected_profile: ScenarioLightingProfile,
    pub transition_timer: CdTimer,
}

impl ScenarioLightingState {
    /// Construct a tick-zero scenario from its map-authored profiles.
    pub const fn new(
        normal: ScenarioLightProfileUnits,
        ion: ScenarioLightProfileUnits,
        dominator: ScenarioLightProfileUnits,
        dominator_change_rate: i32,
    ) -> Self {
        Self {
            current_ambient: normal.ambient_percent,
            target_ambient: normal.ambient_percent,
            normal,
            ion,
            dominator,
            dominator_change_rate,
            selected_profile: ScenarioLightingProfile::Normal,
            // Scenario construction starts this zero-duration timer at frame 0;
            // it is immediately due without using CdTimer's paused sentinel.
            transition_timer: CdTimer::started(0, 0),
        }
    }

    /// The scenario's lighting as the map's `[Lighting]` section sets it.
    pub fn from_map(profiles: &crate::map::lighting::ParsedLightingProfiles) -> Self {
        Self::new(
            profiles.normal.into(),
            profiles.ion.into(),
            profiles.dominator.into(),
            profiles.dominator_change_rate,
        )
    }

    pub fn profile(&self, profile: ScenarioLightingProfile) -> ScenarioLightProfileUnits {
        match profile {
            ScenarioLightingProfile::Normal => self.normal,
            ScenarioLightingProfile::Ion => self.ion,
            ScenarioLightingProfile::Dominator => self.dominator,
        }
    }

    /// The Level a full cell relight's top scalar multiplies by the cell's
    /// height under `profile` (`CellClass::ProcessColourComponents @
    /// 0x00484180`): the profile's own, but [`NUKE_LEVEL_UNITS`] in the
    /// Dominator arm (`0x004844DB`), whose bottom reads `DominatorLevel=`.
    pub fn relight_top_level(&self, profile: ScenarioLightingProfile) -> i32 {
        match profile {
            ScenarioLightingProfile::Dominator => NUKE_LEVEL_UNITS,
            other => self.profile(other).level_units,
        }
    }

    /// `ScenarioClass::UpdateLighting @ 0x0053C280`'s writes for `profile`:
    /// the ambient target (`+0x3530`) becomes its ambient. RecalcLighting's
    /// RGB is [`Self::alternate_rgb`].
    #[inline]
    pub fn select(&mut self, profile: ScenarioLightingProfile) {
        self.selected_profile = profile;
        self.target_ambient = self.profile(profile).ambient_percent;
    }

    /// RecalcLighting's RGB for the selected profile (`0x0053AD00`): the
    /// alternate profile's percentages times 10, none for the ordinary one
    /// (`-1, -1, -1`).
    pub fn alternate_rgb(&self) -> Option<[i32; 3]> {
        (self.selected_profile != ScenarioLightingProfile::Normal)
            .then(|| self.profile(self.selected_profile).alternate_rgb())
    }

    /// Restart the ambient fade timer (`ScenarioClass+0x1248`), as
    /// `PsyDom::Start` does (`0x0053AEFB..0x0053AF24`).
    pub fn restart_transition_timer(&mut self, frame: i32, duration: i32) {
        self.transition_timer.start(frame, duration);
    }

    /// Run the one native pre-ore transition rung for `binary_frame`.
    ///
    /// Returns true when the due rung restarted its timer. Target selection is
    /// deliberately separate, so a Lightning Storm activated later in the same
    /// master frame cannot move the scalar until the following eligible frame.
    pub fn advance_transition_if_due(
        &mut self,
        binary_frame: u32,
        rate_nonzero: bool,
        interval_frames: i32,
        ambient_step: i32,
    ) -> bool {
        if self.current_ambient == self.target_ambient || !rate_nonzero {
            return false;
        }

        let frame = binary_frame as i32;
        if !self.transition_timer.expired(frame) {
            return false;
        }
        self.transition_timer.start(frame, interval_frames);

        // `0x0055B42E..0x0055B43C`: the target is clamped at zero and stored.
        let target = self.target_ambient.max(0);
        self.target_ambient = target;
        if self.current_ambient < target {
            let advanced = self.current_ambient.wrapping_add(ambient_step);
            self.current_ambient = advanced.min(target);
        } else if self.current_ambient > target {
            let advanced = self.current_ambient.wrapping_sub(ambient_step);
            self.current_ambient = advanced.max(target);
        }
        true
    }
}

impl Default for ScenarioLightingState {
    fn default() -> Self {
        Self::new(
            ScenarioLightProfileUnits::normal_default(),
            ScenarioLightProfileUnits::ion_default(),
            ScenarioLightProfileUnits::dominator_default(),
            1,
        )
    }
}

/// Everything the app layer decides about a session before the sim exists.
/// Built from the lobby/launch flow and the selected map file — never
/// hardcoded inside sim/.
#[derive(Debug, Clone, Default)]
pub struct ScenarioDescriptor {
    /// The negotiated per-match seed. 32 bits wide because the original's
    /// negotiated seed is 32 bits and the RNG seeder consumes exactly 32; SP
    /// entropy, future MP handshake, and replay headers all funnel through
    /// this one field.
    pub seed: u32,
    /// Scenario identity: the selected map file name (lobby record / loading
    /// request), with the map's `[Basic]` Name as a human-facing fallback.
    pub map_name: String,
    /// Theater name from the map header (e.g. "TEMPERATE").
    pub theater: String,
    /// Native GameMode classification used by EventClass receiver gates:
    /// false is mode 0; true represents every nonzero mode.
    pub game_mode_nonzero: bool,
    /// Native `ScenarioClass` flags bit `0x20`. The shared direct and area
    /// damage entries return before any receiver or terrain mutation while set.
    pub no_damage: bool,
    /// Map `[Basic] FreeRadar`, native Scenario+34A4. Fresh reset at 0068383C
    /// is false; House 00508DF0 bypasses ordinary power/providers when set.
    pub free_radar: bool,
    /// Map `[Basic] IgnoreGlobalAITriggers`, native Scenario+34B4.
    pub ignore_global_ai_triggers: bool,
    /// Native `ScenarioClass` flags bit `0x40` (`[SpecialFlags] TiberiumGrows`,
    /// bit layout from the writer `0x006B8B30`). Every skirmish/multiplayer
    /// start forces it on (`OR 0xC0` at `0x005E74CD` on the ordinary skirmish
    /// path, `0x005B6ACF`/`0x005B7326`/`0x005BB209` on the network/modem
    /// paths; `Full_Init @ 0x00687C23` copies the session dword into the
    /// scenario when GameMode != 0). Its only reader is the growth driver's
    /// timer reload (`0x00722CA4`: `Growth * 0.3` when set, `* 1.0` when not).
    /// Campaign (GameMode 0) reads the map key instead (`0x006B8CA0`).
    pub tiberium_grows_flag: bool,
    /// Native `ScenarioClass` flags bit `0x80` (`TiberiumSpreads`), forced on
    /// by the same skirmish/multiplayer sites. Gates
    /// `CellClass::CanSpreadTiberium @ 0x00483690`.
    pub tiberium_spreads_flag: bool,
    /// Map-authored global-light profiles plus their tick-zero mutable state.
    pub lighting: ScenarioLightingState,
    /// Shared gameplay conversion profile; independent of every client's window.
    pub pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds,
    /// Authoritative map bounds in the CANONICAL CELL-ARRAY frame (max cell
    /// rx/ry + 1 — the frame entities, waypoints, and vision index), NOT the
    /// raw `[Map] Size=` values: sim cell coordinates span the iso diamond,
    /// whose array extent is ~(SizeW+SizeH). The raw Size= width lives on
    /// `Simulation.playfield_bounds` for the diamond test.
    pub map_width: u16,
    pub map_height: u16,
    /// Playable-area `LocalSize=` rect, stored verbatim.
    pub local_left: u16,
    pub local_top: u16,
    pub local_width: u16,
    pub local_height: u16,
    /// MP start waypoints (index -> cell) from the map `[Waypoints]` list.
    /// BTreeMap for deterministic iteration; sized by content, never by a
    /// player-count assumption.
    pub mp_start_waypoints: BTreeMap<u32, (u16, u16)>,
}

impl ScenarioDescriptor {
    /// Seed normal scenario initialization from a native recording header.
    ///
    /// Map bounds, theater, objects, and active registration order are rebuilt
    /// by the ordinary map loader; a recording is never a snapshot restore.
    #[cfg(test)]
    pub fn from_native_replay_header(header: &crate::sim::replay::NativeReplayHeader) -> Self {
        Self {
            seed: header.seed,
            map_name: header.scenario_name(),
            no_damage: header.special_flags & 0x20 != 0,
            tiberium_grows_flag: header.special_flags & 0x40 != 0,
            tiberium_spreads_flag: header.special_flags & 0x80 != 0,
            ..Self::default()
        }
    }
}

/// The sim-resident session aggregate. Owns session identity, the seed,
/// authoritative map bounds, the MP start table, the per-match options, and
/// the frame clock. Constructed once from the descriptor; serialized and
/// hashed (lockstep state, set before tick 0).
///
/// Bounds note: `Simulation.playfield_bounds` (the FNPC diamond lens over
/// `LocalSize`) keeps its own verbatim copy; consolidating the two is a
/// follow-up once the diamond consumers read through the session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScenarioSession {
    /// Construction seed — the negotiated per-match value; the replay header
    /// records it. Stored widened (the negotiated value is 32-bit).
    pub seed: u64,
    /// Scenario identity: map file name (with `[Basic]` Name fallback).
    pub map_name: String,
    /// Theater name from the map header.
    pub theater: String,
    /// Persisted native zero/nonzero GameMode classification.
    #[serde(default)]
    pub game_mode_nonzero: bool,
    /// Persisted native `ScenarioClass` flags bit `0x20`.
    #[serde(default)]
    pub no_damage: bool,
    /// Persistent native Scenario+34A4, included by original CRC at 0068BC16.
    #[serde(default)]
    pub free_radar: bool,
    /// Persistent native Scenario+34B4 (`[Basic] IgnoreGlobalAITriggers`).
    #[serde(default)]
    ignore_global_ai_triggers: bool,
    /// Persisted native `ScenarioClass` flags bit `0x40` (`TiberiumGrows`);
    /// see the descriptor field of the same name.
    #[serde(default)]
    pub tiberium_grows_flag: bool,
    /// Persisted native `ScenarioClass` flags bit `0x80` (`TiberiumSpreads`).
    #[serde(default)]
    pub tiberium_spreads_flag: bool,
    /// Persistent fixed-integer global-light configuration and transition state.
    pub lighting: ScenarioLightingState,
    /// Saved match authority for Building and Tile animation coordinates.
    pub pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds,
    /// Authoritative map bounds in the canonical cell-array frame (max cell
    /// rx/ry + 1); see the descriptor field of the same name. Seeds the fog
    /// grid dimensions at construction.
    pub map_width: u16,
    pub map_height: u16,
    /// Playable-area `LocalSize=` rect, stored verbatim.
    pub local_left: u16,
    pub local_top: u16,
    pub local_width: u16,
    pub local_height: u16,
    /// MP start waypoints (index -> cell) from the map `[Waypoints]` list.
    pub mp_start_waypoints: BTreeMap<u32, (u16, u16)>,
    /// Start waypoint index -> owning house, filled during launch application
    /// (after the random-assignment draws), before tick 0.
    pub start_slot_houses: BTreeMap<u32, InternedId>,
    /// HouseClass array registration order. Native house ids are indices into
    /// this vector; map ordering by name/id is not an equivalent substitute.
    pub house_order: Vec<InternedId>,
    /// Native A83D4C: this process's current HouseClass identity. Campaign
    /// Basic.Player and multiplayer participant index0 choose it at creation;
    /// 67F802/67F9F3 save/load the pointer and 67FA0E swizzles it. It is not
    /// the presentation viewer and cannot be rebound from selection each frame.
    /// Evidence: docs/research/PHASE3_CURRENT_HOUSE_IDENTITY_GHIDRA_REPORT.md.
    pub current_house: Option<InternedId>,
    /// Per-match game settings (the lobby options card). Set once at game
    /// start, read-only during gameplay.
    pub game_options: GameOptions,
    /// Monotonic Rust-side simulation ordinal. This is kept separately from
    /// the wrapping native frame so long-running diagnostics and command
    /// bookkeeping do not lose their ordering at the 32-bit wrap.
    pub tick: u64,
    /// Diagnostic accumulation of the nominal milliseconds supplied by the
    /// host for each admitted frame. Gameplay must not derive timer state,
    /// cadence, or the native frame from this value.
    pub total_sim_ms: u64,
    /// Wrapping native gameplay-frame counter. One admitted `advance_tick`
    /// executes entirely under frame N, then commits N+1 at the late tail.
    ///
    /// The field keeps its historical name until all persistence and app
    /// surfaces can be renamed together; it is not a synthetic 15 Hz clock.
    pub binary_frame: u32,
}

impl ScenarioSession {
    /// Session identity/bounds/waypoints — appended AFTER the legacy folds so
    /// the pre-session hash prefix order is preserved (SC-2). The clock and
    /// game options keep their original fold positions above; this fold adds
    /// only the fields new to the session aggregate. Order is part of the
    /// hash contract and must never change.
    pub(crate) fn fold_identity(&self, hasher: &mut impl std::hash::Hasher) {
        let s = self;
        s.seed.hash(hasher);
        s.map_name.hash(hasher);
        s.theater.hash(hasher);
        s.game_mode_nonzero.hash(hasher);
        // current_house is persisted process input, deliberately absent from
        // the shared peer hash: native64DAB0 does not fold A83D4C, and different
        // participants legitimately select different current houses. Gameplay
        // mutations made by current-house callbacks retain their own hash owners.
        // This is not a claim that Rust's stronger hash equals native's CRC.
        // Preserve the legacy default-false hash stream while still making
        // the native ScenarioFlags 0x20 state lockstep-visible.
        if s.no_damage {
            b"scenario-no-damage-v1".hash(hasher);
        }
        // Keep the historical absent/false stream while distinguishing the
        // map-owned radar availability flag in lockstep state.
        if s.free_radar {
            b"scenario-free-radar-v1".hash(hasher);
        }
        if s.ignore_global_ai_triggers {
            b"scenario-ignore-global-ai-triggers-v1".hash(hasher);
        }
        // Same legacy-preserving shape for the `0x40`/`0x80` tiberium bits.
        if s.tiberium_grows_flag {
            b"scenario-tiberium-grows-v1".hash(hasher);
        }
        if s.tiberium_spreads_flag {
            b"scenario-tiberium-spreads-v1".hash(hasher);
        }
        (s.map_width, s.map_height).hash(hasher);
        (s.local_left, s.local_top, s.local_width, s.local_height).hash(hasher);
        s.mp_start_waypoints.len().hash(hasher);
        for (idx, cell) in &s.mp_start_waypoints {
            idx.hash(hasher);
            cell.hash(hasher);
        }
        s.start_slot_houses.len().hash(hasher);
        for (idx, owner) in &s.start_slot_houses {
            idx.hash(hasher);
            owner.hash(hasher);
        }
        s.house_order.hash(hasher);

        let lighting = &s.lighting;
        lighting.normal.ambient_percent.hash(hasher);
        lighting.normal.red_percent.hash(hasher);
        lighting.normal.green_percent.hash(hasher);
        lighting.normal.blue_percent.hash(hasher);
        lighting.normal.ground_units.hash(hasher);
        lighting.normal.level_units.hash(hasher);
        lighting.ion.ambient_percent.hash(hasher);
        lighting.ion.red_percent.hash(hasher);
        lighting.ion.green_percent.hash(hasher);
        lighting.ion.blue_percent.hash(hasher);
        lighting.ion.ground_units.hash(hasher);
        lighting.ion.level_units.hash(hasher);
        lighting.current_ambient.hash(hasher);
        lighting.target_ambient.hash(hasher);
        let profile_tag = |profile| match profile {
            ScenarioLightingProfile::Normal => 0u8,
            ScenarioLightingProfile::Ion => 1u8,
            ScenarioLightingProfile::Dominator => 2u8,
        };
        profile_tag(lighting.selected_profile).hash(hasher);
        lighting.transition_timer.start_frame().hash(hasher);
        lighting.transition_timer.duration().hash(hasher);
        // Appended after the original fields, and only for values other
        // than the Set_Defaults ones, so a map without `Dominator*=` keys
        // keeps its earlier hash stream.
        if lighting.dominator != ScenarioLightProfileUnits::dominator_default()
            || lighting.dominator_change_rate != 1
        {
            0x357cu16.hash(hasher);
            lighting.dominator.hash(hasher);
            lighting.dominator_change_rate.hash(hasher);
        }
    }

    /// Hash per-match game options for lockstep verification.
    pub(crate) fn fold_game_options(&self, hasher: &mut impl std::hash::Hasher) {
        let opts = &self.game_options;
        opts.short_game.hash(hasher);
        opts.bases.hash(hasher);
        opts.bridges_destroyable.hash(hasher);
        opts.super_weapons.hash(hasher);
        opts.build_off_ally.hash(hasher);
        opts.crates.hash(hasher);
        opts.mcv_redeploy.hash(hasher);
        opts.fog_of_war.hash(hasher);
        opts.shroud.hash(hasher);
        opts.tiberium_grows.hash(hasher);
        opts.multi_engineer.hash(hasher);
        opts.harvester_truce.hash(hasher);
        opts.ally_change_allowed.hash(hasher);
        opts.starting_credits.hash(hasher);
        opts.unit_count.hash(hasher);
        opts.tech_level.hash(hasher);
        opts.game_speed.hash(hasher);
        opts.ai_difficulty.hash(hasher);
        opts.ai_players.hash(hasher);
    }

    /// Scenario+34B4 (`[Basic] IgnoreGlobalAITriggers=`): the computer's
    /// team creation skips the global (`AIMD.INI`) AI triggers.
    pub(crate) const fn ignore_global_ai_triggers(&self) -> bool {
        self.ignore_global_ai_triggers
    }

    pub fn from_descriptor(desc: &ScenarioDescriptor) -> Self {
        Self {
            seed: u64::from(desc.seed),
            map_name: desc.map_name.clone(),
            theater: desc.theater.clone(),
            game_mode_nonzero: desc.game_mode_nonzero,
            no_damage: desc.no_damage,
            free_radar: desc.free_radar,
            ignore_global_ai_triggers: desc.ignore_global_ai_triggers,
            tiberium_grows_flag: desc.tiberium_grows_flag,
            tiberium_spreads_flag: desc.tiberium_spreads_flag,
            lighting: desc.lighting,
            pixel_conversion_bounds: desc.pixel_conversion_bounds,
            map_width: desc.map_width,
            map_height: desc.map_height,
            local_left: desc.local_left,
            local_top: desc.local_top,
            local_width: desc.local_width,
            local_height: desc.local_height,
            mp_start_waypoints: desc.mp_start_waypoints.clone(),
            start_slot_houses: BTreeMap::new(),
            house_order: Vec::new(),
            current_house: None,
            game_options: GameOptions::default(),
            tick: 0,
            total_sim_ms: 0,
            binary_frame: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::Simulation;

    #[test]
    fn from_descriptor_equals_with_seed_widened() {
        let mut a = Simulation::from_descriptor(&ScenarioDescriptor {
            seed: 0xDEAD_BEEF,
            ..Default::default()
        });
        let b = Simulation::with_seed(0xDEAD_BEEF);
        assert_eq!(a.session.seed, 0xDEAD_BEEF_u64);
        assert_eq!(a.scenario_rng.state(), b.scenario_rng.state());
        assert_eq!(a.main_rng.state(), b.main_rng.state());
        assert_eq!(a.mapgen_rng.state(), b.mapgen_rng.state());
        // A production descriptor awaits its native Full_Init prefix; the
        // test/dev constructor explicitly supplies a synthetic cursor. That
        // authoritative difference must remain hashed. Compare the remaining
        // constructor state only after declaring the same fixture boundary.
        assert!(a.native_unique_ids.is_none());
        assert_eq!(b.native_unique_ids.as_ref().unwrap().current_raw(), 0);
        assert_ne!(a.state_hash(), b.state_hash());
        a.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn gsi_04_20_lighting_defaults_and_due_rungs_clamp_both_directions() {
        let mut lighting = ScenarioLightingState::default();
        assert_eq!(lighting.normal, ScenarioLightProfileUnits::normal_default());
        assert_eq!(lighting.ion, ScenarioLightProfileUnits::ion_default());
        assert_eq!(lighting.transition_timer, CdTimer::started(0, 0));

        lighting.select(ScenarioLightingProfile::Ion);
        assert!(lighting.advance_transition_if_due(0, true, 180, 20));
        assert_eq!(lighting.current_ambient, 87);
        assert_eq!(lighting.transition_timer, CdTimer::started(0, 180));

        lighting.select(ScenarioLightingProfile::Normal);
        assert!(!lighting.advance_transition_if_due(179, true, 180, 20));
        assert_eq!(lighting.current_ambient, 87);
        assert!(lighting.advance_transition_if_due(180, true, 180, 20));
        assert_eq!(lighting.current_ambient, 100);

        lighting.select(ScenarioLightingProfile::Ion);
        assert!(!lighting.advance_transition_if_due(360, false, 180, 20));
        assert_eq!(lighting.current_ambient, 100);
        assert_eq!(lighting.transition_timer, CdTimer::started(180, 180));
    }

    #[test]
    fn gsi_04_20_lighting_config_runtime_selection_and_timer_are_hashed() {
        let baseline = Simulation::new().state_hash();
        let assert_differs = |mutate: fn(&mut ScenarioLightingState)| {
            let mut sim = Simulation::new();
            mutate(&mut sim.session.lighting);
            assert_ne!(sim.state_hash(), baseline);
        };

        assert_differs(|lighting| lighting.normal.red_percent += 1);
        assert_differs(|lighting| lighting.ion.blue_percent += 1);
        assert_differs(|lighting| lighting.current_ambient -= 1);
        assert_differs(|lighting| lighting.target_ambient -= 1);
        assert_differs(|lighting| lighting.selected_profile = ScenarioLightingProfile::Ion);
        assert_differs(|lighting| lighting.transition_timer = CdTimer::from_raw(1, 0));
        assert_differs(|lighting| lighting.transition_timer = CdTimer::from_raw(0, 1));
    }

    /// AT-1: two sims constructed from the same descriptor seed stay in
    /// per-stream lockstep across 300 ticks of identical commands; a
    /// different seed diverges. The descriptor seed reaches Scenario/Main;
    /// fresh MapGen remains the same verified Seed(0) object.
    #[test]
    fn mp_sibling_rng_state_matches_after_seed_sync() {
        use crate::map::entities::{EntityCategory, MapEntity};
        use crate::sim::command::{Command, CommandEnvelope};

        fn build(seed: u32) -> Simulation {
            let mut sim = Simulation::from_descriptor(&ScenarioDescriptor {
                seed,
                ..Default::default()
            });
            // This synthetic fixture omits the production native Rules prefix.
            sim.native_unique_ids =
                Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
            let entity = MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 2,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Unit,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
                structure_ai_sellable: false,
                structure_ai_repairable: false,
            };
            sim.spawn_from_map(&[entity], None);
            sim
        }
        fn run_300(sim: &mut Simulation) -> Vec<u64> {
            let owner = sim.interner.get("Americans").expect("owner interned");
            (0..300u64)
                .map(|t| {
                    let cmds = if t == 5 {
                        vec![CommandEnvelope::new(
                            owner,
                            6,
                            Command::Move {
                                entity_id: 1,
                                target_rx: 20,
                                target_ry: 2,
                                queue: false,
                            },
                        )]
                    } else {
                        Vec::new()
                    };
                    sim.advance_tick(&cmds, None, None, None, 67).state_hash
                })
                .collect()
        }

        let (mut a, mut b) = (build(0xA5EED), build(0xA5EED));
        assert_eq!(
            run_300(&mut a),
            run_300(&mut b),
            "same descriptor seed must produce an identical hash timeline"
        );
        assert_eq!(a.scenario_rng.state(), b.scenario_rng.state());
        assert_eq!(a.main_rng.state(), b.main_rng.state());
        assert_eq!(a.mapgen_rng.state(), b.mapgen_rng.state());

        let mut c = build(0xA5EED + 1);
        run_300(&mut c);
        assert_ne!(
            a.state_hash(),
            c.state_hash(),
            "different descriptor seeds must diverge"
        );
        assert_ne!(a.scenario_rng.state(), c.scenario_rng.state());
        assert_ne!(a.main_rng.state(), c.main_rng.state());
        assert_eq!(
            a.mapgen_rng.state(),
            c.mapgen_rng.state(),
            "descriptor seed must not alter fresh MapGen Seed(0) state"
        );
    }

    /// AT-5: authoritative bounds are queryable before any advance_tick — no
    /// zero-dim fog window between construction and the first vision pass.
    #[test]
    fn map_bounds_known_before_first_tick() {
        let desc = ScenarioDescriptor {
            seed: 7,
            map_width: 80,
            map_height: 60,
            ..Default::default()
        };
        let sim = Simulation::from_descriptor(&desc);
        assert_eq!((sim.fog.width, sim.fog.height), (80, 60));
        assert_eq!((sim.session.map_width, sim.session.map_height), (80, 60));
    }

    /// AT-4: scenario identity (map name, theater) is sim-resident and
    /// survives a snapshot round-trip.
    #[test]
    fn scenario_identity_is_sim_resident() {
        let desc = ScenarioDescriptor {
            seed: 9,
            map_name: "tournamentb.map".into(),
            pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds {
                width: 640,
                height: 480,
            },
            theater: "SNOW".into(),
            map_width: 100,
            map_height: 100,
            mp_start_waypoints: [(0u32, (10u16, 12u16)), (1, (88, 90))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let mut sim = Simulation::from_descriptor(&desc);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // descriptor persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 2, "tournamentb.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("snapshot load")
            .sim;
        assert_eq!(restored.session.map_name, "tournamentb.map");
        assert_eq!(restored.session.theater, "SNOW");
        assert_eq!(
            restored.session.pixel_conversion_bounds,
            desc.pixel_conversion_bounds
        );
        assert_eq!(
            restored.session.mp_start_waypoints,
            sim.session.mp_start_waypoints
        );
        assert_eq!(restored.state_hash(), sim.state_hash());
    }

    /// AT-6: the MP start-waypoint table is hashed lockstep state — a one-cell
    /// difference diverges the desync detector — and round-trips save/load.
    #[test]
    fn mp_waypoints_round_trip_and_hash() {
        let mut desc = ScenarioDescriptor {
            seed: 11,
            map_width: 64,
            map_height: 64,
            mp_start_waypoints: [(0u32, (5u16, 5u16)), (1, (50, 50))].into_iter().collect(),
            ..Default::default()
        };
        let mut a = Simulation::from_descriptor(&desc);
        desc.mp_start_waypoints.insert(1, (50, 51)); // one waypoint, one cell off
        let b = Simulation::from_descriptor(&desc);
        assert_ne!(
            a.state_hash(),
            b.state_hash(),
            "a one-cell waypoint difference must be visible to the desync detector"
        );

        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // waypoint persistence after the independent hash-sensitivity check.
        a.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&a, 1, 2, "wp", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("snapshot load")
            .sim;
        assert_eq!(restored.state_hash(), a.state_hash());
    }

    #[test]
    fn native_replay_header_starts_normal_scenario_descriptor() {
        let header = crate::sim::replay::NativeReplayHeader::new(0x1234_5678, "arena.map");
        let descriptor = ScenarioDescriptor::from_native_replay_header(&header);
        assert_eq!(descriptor.seed, 0x1234_5678);
        assert_eq!(descriptor.map_name, "arena.map");
        assert_eq!(descriptor.map_width, 0);
        assert_eq!(descriptor.map_height, 0);
    }

    #[test]
    fn free_radar_is_persistent_hashed_scenario_authority() {
        let desc = ScenarioDescriptor {
            seed: 0,
            free_radar: true,
            ..Default::default()
        };
        let sim = Simulation::from_descriptor(&desc);
        let ordinary = Simulation::from_descriptor(&ScenarioDescriptor {
            free_radar: false,
            ..desc
        });
        assert!(sim.session.free_radar);
        assert_ne!(sim.state_hash(), ordinary.state_hash());
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 2, "radar.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("FreeRadar snapshot")
            .sim;
        assert!(restored.session.free_radar);
        assert_eq!(restored.state_hash(), sim.state_hash());

        // Self-describing formats may omit a newly added field; bincode saves
        // have a separate mandatory version boundary and are not defaulted.
        let mut json = serde_json::to_value(&sim.session).unwrap();
        json.as_object_mut().unwrap().remove("free_radar");
        let missing: ScenarioSession = serde_json::from_value(json).unwrap();
        assert!(!missing.free_radar);
        let header = crate::sim::replay::NativeReplayHeader::new(0, "radar.map");
        assert!(
            !ScenarioDescriptor::from_native_replay_header(&header).free_radar,
            "recording header does not contain this map field"
        );
    }

    #[test]
    fn gsi_04_10_replay_scenario_no_damage_roundtrips_and_changes_hash() {
        let mut header = crate::sim::replay::NativeReplayHeader::new(0x1234_5678, "inert.map");
        header.special_flags = 0x20;
        let descriptor = ScenarioDescriptor::from_native_replay_header(&header);
        assert!(descriptor.no_damage);

        let mut inert = Simulation::from_descriptor(&descriptor);
        assert!(inert.session.no_damage);
        let mut ordinary = Simulation::from_descriptor(&ScenarioDescriptor {
            no_damage: false,
            ..descriptor.clone()
        });
        assert_ne!(inert.state_hash(), ordinary.state_hash());

        ordinary.session.no_damage = true;
        assert_eq!(inert.state_hash(), ordinary.state_hash());
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // no-damage persistence after the independent hash-sensitivity check.
        inert.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&inert, 1, 2, "inert.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("v61 inert snapshot")
            .sim;
        assert!(restored.session.no_damage);
        assert_eq!(restored.state_hash(), inert.state_hash());
    }

    /// The `0x40`/`0x80` `TiberiumGrows`/`TiberiumSpreads` bits reach the
    /// session from the descriptor and the native replay header, are
    /// lockstep-visible, and survive a snapshot.
    #[test]
    fn gsi_09_04_tiberium_flag_bits_roundtrip_and_change_hash() {
        let mut header = crate::sim::replay::NativeReplayHeader::new(0x1234_5678, "skirmish.map");
        header.special_flags = 0xC0;
        let descriptor = ScenarioDescriptor::from_native_replay_header(&header);
        assert!(descriptor.tiberium_grows_flag);
        assert!(descriptor.tiberium_spreads_flag);
        assert!(!descriptor.no_damage);

        let mut forced = Simulation::from_descriptor(&descriptor);
        assert!(forced.session.tiberium_grows_flag && forced.session.tiberium_spreads_flag);
        let clear = Simulation::from_descriptor(&ScenarioDescriptor {
            tiberium_grows_flag: false,
            tiberium_spreads_flag: false,
            ..descriptor.clone()
        });
        assert_ne!(forced.state_hash(), clear.state_hash());
        let grows_only = Simulation::from_descriptor(&ScenarioDescriptor {
            tiberium_spreads_flag: false,
            ..descriptor.clone()
        });
        assert_ne!(forced.state_hash(), grows_only.state_hash());
        assert_ne!(clear.state_hash(), grows_only.state_hash());

        forced.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&forced, 1, 2, "skirmish.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("snapshot load")
            .sim;
        assert!(restored.session.tiberium_grows_flag && restored.session.tiberium_spreads_flag);
        assert_eq!(restored.state_hash(), forced.state_hash());
    }
}
