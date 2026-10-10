//! Per-player game state — identity + economy.
//!
//! Split from the monolithic HouseClass into purpose-specific systems. This module
//! holds the lightweight core: identity, economy scalars, and defeat/victory flags.
//!
//! Stored in `Simulation.houses: BTreeMap<InternedId, HouseState>` keyed by
//! interned owner name for deterministic iteration (BTreeMap + InternedId give
//! sorted order natively; all peers intern in the same order).

use std::collections::BTreeMap;

use crate::map::playfield::local_to_packed_cell;
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::economy::Economy;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// Native per-house AI difficulty index stored by `HouseClass`.
///
/// The discriminants are part of the gameplay contract: stock
/// `AIVirtualPurifiers=4,2,0` is indexed directly in Hard/Normal/Easy order.
#[repr(i32)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum HouseDifficulty {
    Hard = 0,
    Normal = 1,
    Easy = 2,
}

impl Default for HouseDifficulty {
    fn default() -> Self {
        Self::Normal
    }
}

impl HouseDifficulty {
    /// Convert a native HouseClass difficulty value without accepting drifted
    /// or out-of-range values.
    pub const fn from_native(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Hard),
            1 => Some(Self::Normal),
            2 => Some(Self::Easy),
            _ => None,
        }
    }

    /// Exact index into native hardest-first difficulty-control tables.
    pub const fn table_index(self) -> usize {
        self as usize
    }
}

/// `HouseClass+0x1A8`, the house's ROF multiplier (a double); the
/// constructor stores 1.0 (`0x004F567E`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct HouseRofBias(NativeF64Bits);

impl Default for HouseRofBias {
    fn default() -> Self {
        Self(NativeF64Bits::ONE)
    }
}

/// The `Cost*Mult=` floats of the House's type (`HouseTypeClass+0x114..+0x124`,
/// in [`crate::rules::object_type::ObjectType::factor_slot`] order), which
/// `Cost_Of` multiplies in (`0x0050BDF0`). The type never changes, so the
/// House keeps the rules' values from its creation
/// ([`HouseState::project_country_mults`]) and prices objects without a
/// rules lookup, as the lifecycle's value totals must (`house_tracking`).
/// `Simulation::cost_of` checks the copy against the rules in debug builds.
/// The HouseType constructor stores 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CountryCostMults(pub [NativeF32Bits; 5]);

/// The `Speed*Mult=` floats of the House's type (Infantry, Units, Aircraft at
/// `HouseTypeClass+0x128/+0x12C/+0x130`). The HouseType constructor stores 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CountrySpeedMults([NativeF32Bits; 3]);

impl Default for CountrySpeedMults {
    fn default() -> Self {
        Self([NativeF32Bits::ONE; 3])
    }
}

impl Default for CountryCostMults {
    fn default() -> Self {
        Self([NativeF32Bits::ONE; 5])
    }
}

/// Accepted native HouseClass match result whose SavourDelay still owns the
/// scenario's deterministic frame lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HouseOutcomeKind {
    Victory,
    Defeat,
}

/// Persistent HouseClass result transition.
///
/// The absolute target keeps the remaining SavourDelay frame count stable
/// across save/load. The wall-clock Vox drain that follows `exit_ready` belongs
/// to the app and is deliberately not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseOutcomeState {
    pub kind: HouseOutcomeKind,
    pub savour_until_tick: u64,
    pub exit_ready: bool,
}

/// Persistent HouseClass strategy-emergency state.
///
/// Native provenance:
/// - `House+0x250`: signed emergency mode, constructor zero;
/// - `House+0x249`: persistent All-To-Hunt candidate-bias latch;
/// - `House+0x54D8`: signed frame of the last Building damage admission.
///
/// The emergency block of `HouseClass::AI_Building_Strategy @ 0x004FD7A0`
/// (`sim::house_strategy`) steps the mode; All_To_Hunt sets the latch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseStrategyEmergencyState {
    pub(crate) mode: i32,
    pub(crate) all_to_hunt_bias: bool,
    pub(crate) last_building_attack_frame: i32,
    #[serde(default = "last_attacker_house_index_default")]
    pub(crate) last_attacker_house_index: i32,
}

const fn last_attacker_house_index_default() -> i32 {
    -1
}

/// [`HouseState::strategy_timer`]'s constructor value: started at frame 0
/// with no delay, so expired.
pub(crate) const fn strategy_timer_at_construction() -> CdTimer {
    CdTimer::started(0, 0)
}

/// [`HouseState::eva_funds_timer`]'s constructor value
/// (`HouseClass::Constructor 0x004F5D2F`, the duration from `0x004F5CD0 MOV
/// EAX,1`): started at the construction frame, 0, for one frame.
pub(crate) const fn eva_funds_timer_at_construction() -> CdTimer {
    CdTimer::started(0, 1)
}

impl Default for HouseStrategyEmergencyState {
    fn default() -> Self {
        Self {
            mode: 0,
            all_to_hunt_bias: false,
            last_building_attack_frame: 0,
            last_attacker_house_index: last_attacker_house_index_default(),
        }
    }
}

impl HouseStrategyEmergencyState {
    #[cfg(test)]
    pub(crate) const fn mode(&self) -> i32 {
        self.mode
    }

    #[cfg(test)]
    pub(crate) const fn all_to_hunt_bias(&self) -> bool {
        self.all_to_hunt_bias
    }

    #[cfg(test)]
    pub(crate) const fn last_building_attack_frame(&self) -> i32 {
        self.last_building_attack_frame
    }

    #[cfg(test)]
    pub(crate) const fn last_attacker_house_index(&self) -> i32 {
        self.last_attacker_house_index
    }

    /// Trigger action 9 and Team script opcode 30 write state four directly.
    #[cfg(test)]
    pub(crate) fn set_state_four(&mut self) {
        self.mode = 4;
    }

    /// Called only after the exact All-To-Hunt reverse scan completes.
    pub(crate) fn set_all_to_hunt_bias(&mut self) {
        self.all_to_hunt_bias = true;
    }

    /// Native Building damage admission writes the current signed frame.
    pub(crate) fn note_building_attack(&mut self, current_frame: i32) {
        self.last_building_attack_frame = current_frame;
    }

    /// Native Building damage admission stores the attacker's raw House-array
    /// index alongside the current attack frame before shared Techno damage.
    pub(crate) fn note_building_attacker(&mut self, attacker_house_index: i32) {
        self.last_attacker_house_index = attacker_house_index;
    }
}

/// Writer-owned `BaseClass` state embedded in native `HouseClass`.
///
/// `BuildingClass::MarkBaseReservation @ 0x00455F10` updates the four bounds on
/// every normal and repair-only writer call. Normal writers and
/// `BuildingClass::ClearBaseReservationAndRepairNeighbors @ 0x004561F0` also
/// maintain the ordered packed-cell perimeter vector.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BaseReservationState {
    pub(crate) min_x: i32,
    pub(crate) min_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) perimeter_cells: Vec<u32>,
}

impl BaseReservationState {
    fn update_axis(minimum: &mut i32, span: &mut i32, incoming_start: i32, incoming_span: i32) {
        // Literal native order. A legitimate zero minimum is treated as
        // uninitialized again, and the prior span is deliberately retained.
        if *minimum == 0 {
            *minimum = incoming_start;
        }
        if incoming_start < *minimum {
            *span = span.wrapping_add(minimum.wrapping_sub(incoming_start));
            *minimum = incoming_start;
        }
        if incoming_start.wrapping_add(incoming_span) > minimum.wrapping_add(*span) {
            *span = incoming_start
                .wrapping_sub(*minimum)
                .wrapping_add(incoming_span);
        }
    }

    pub(crate) fn update_bounds(&mut self, start_x: i32, start_y: i32, width: i32, height: i32) {
        Self::update_axis(&mut self.min_x, &mut self.width, start_x, width);
        Self::update_axis(&mut self.min_y, &mut self.height, start_y, height);
    }

    pub(crate) fn append_perimeter_cell_if_absent(&mut self, packed_cell: u32) {
        if !self.perimeter_cells.contains(&packed_cell) {
            self.perimeter_cells.push(packed_cell);
        }
    }

    pub(crate) fn remove_perimeter_cell(&mut self, packed_cell: u32) {
        if let Some(index) = self
            .perimeter_cells
            .iter()
            .position(|candidate| *candidate == packed_cell)
        {
            // Vec::remove performs the native stable shift-left removal.
            self.perimeter_cells.remove(index);
        }
    }

    /// `HouseClass+0x5754..+0x5760`: the left, top, width and height of the
    /// reserved cells.
    pub(crate) fn bounds(&self) -> (i32, i32, i32, i32) {
        (self.min_x, self.min_y, self.width, self.height)
    }

    #[cfg(test)]
    pub(crate) fn perimeter_cells(&self) -> &[u32] {
        &self.perimeter_cells
    }
}

/// Independent persistent House AI-activation latches. Successful AI base-unit
/// deployment co-enables three of them; House update owns the separate
/// AutocreateAllowed writer and its three-store activation transaction.
///
/// gamemd-derived: `HouseClass__Constructor` clears the corresponding bytes at
/// `0x004F56F1`, `0x004F56F7`, `0x004F570A`, and `0x004F5710`;
/// `HouseClass__Save @ 0x00504080` and `HouseClass__Load @ 0x00503040`
/// persist the raw House block.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HouseAiActivationLatches {
    pub production: bool,
    pub autocreate_allowed: bool,
    pub ai_triggers_active: bool,
    pub auto_base_building: bool,
}

/// Per-player game state.
///
/// Created once per player at game start, lives for the duration of the match.
/// Heavy subsystems (power, fog, production queues, AI) remain in their own
/// containers — HouseState holds the lightweight scalars.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HouseState {
    /// Owner name as interned ID (resolve via interner for display).
    pub name: InternedId,
    /// Stable rules-owned side index. Stock YR uses 0=Allied, 1=Soviet,
    /// 2=Yuri, 3=Civilian, and 4=Mutant.
    pub side_index: u8,
    /// Country interned ID from map INI `Country=` key (e.g., "Americans", "Russians").
    pub country: Option<InternedId>,
    /// Native HouseClass `IsHuman` byte.
    pub is_human: bool,
    /// Native HouseClass `PlayerControl` byte. It differs from `IsHuman` in
    /// scenario modes and participates independently in EventClass admission.
    #[serde(default)]
    pub player_control: bool,
    /// Per-house native difficulty. Human houses retain Normal unless a map or
    /// game-mode initializer explicitly assigns another native value.
    #[serde(default)]
    pub difficulty: HouseDifficulty,
    /// The house's ROF multiplier, `HouseClass+0x1A8` (a double; the
    /// constructor stores 1.0 at `0x004F567E`). Only
    /// [`HouseState::set_difficulty`] writes it after construction; GetROF
    /// scales every full reload by it (`0x006FD0C5`).
    #[serde(default)]
    rof_bias: HouseRofBias,
    /// `MultiplayPassive=` from this house's country/house-type rules.
    ///
    /// gamemd keeps this on the house type and reads it back out of the house
    /// during defeat evaluation: a passive house is never tested for defeat and
    /// never counted in the "everyone still alive is allied" game-over scan.
    /// Stock `Neutral` (Civilian) and `Special` (JP) both set it, and they exist
    /// in every skirmish, so without it the alive set can never shrink to one.
    ///
    /// Stamped once at house creation, while a `RuleSet` is still in hand, and
    /// then read straight off the house — the house rung takes
    /// `rules: Option<&RuleSet>` and never has to resolve the country itself. A
    /// house built with no rules available is stamped `false`, the INI default
    /// for `MultiplayPassive=`; there is no runtime fallback, because gamemd has
    /// none.
    ///
    /// Persisted state, and versioned as such: it is an authoritative input to
    /// the win/loss outcome, so a save that dropped it would reload as an
    /// ordinary house and lose the match forever. It is nonetheless left out of
    /// the state hash and the retail multiplayer checksum, which fold the
    /// mutable `HouseClass` bytes — gamemd keeps this one on the house type.
    #[serde(default)]
    pub multiplay_passive: bool,
    /// The HouseType's `Cost*Mult=`, projected from the rules at creation.
    /// Like the flag above it is the type's, so it is left out of the state
    /// hash.
    #[serde(default)]
    pub country_cost_mults: CountryCostMults,
    /// The HouseType's `Speed*Mult=`, projected with the cost factors and
    /// likewise left out of the state hash. Read through [`Self::speed_bonus`].
    #[serde(default)]
    country_speed_mults: CountrySpeedMults,
    /// Whether this player has been eliminated.
    pub is_defeated: bool,
    /// Victory flag.
    pub has_won: bool,
    /// Defeat flag. Note: Flag_To_Lose clears HasWon first.
    pub has_lost: bool,
    /// Accepted win/loss transition plus the deterministic SavourDelay target.
    /// App-owned wall waits and audio teardown are intentionally excluded.
    #[serde(default)]
    pub outcome_state: Option<HouseOutcomeState>,
    /// HouseClass map-clear byte folded by the retail multiplayer checksum.
    ///
    /// Defeat/reveal paths set this independently of the win/loss flags, and
    /// shroud restoration can clear it again.
    #[serde(default)]
    pub map_is_clear: bool,
    /// Aggregate active SpySat state for this house.
    ///
    /// This is the edge-trigger authority for whole-map reveal/restoration.
    /// Individual uplinks do not own the transition. The first marked,
    /// non-limbo, non-selling SpySat in house building order decides it:
    /// warp-out clears the latch; otherwise that provider sets it.
    #[serde(default)]
    pub spy_sat_active: bool,
    /// The object counts the defeat gate reads (`house_tracking`), written by
    /// the lifecycle authority at construction, deletion, Limbo, Unlimbo and
    /// owner change.
    #[serde(default)]
    pub(crate) tracking: crate::sim::house_tracking::HouseTracking,
    /// Tech Hospital self-heal capacity — `HouseClass+0x164`.
    ///
    /// `BuildingClass::OnConstructionComplete` adds the built type's
    /// `InfantryGainSelfHeal` here (`0x0044638E..0x00446398`), Limbo and
    /// ChangeOwner subtract it again. `TechnoClass::AI_Update`'s infantry pulse
    /// runs while it is above zero (`0x0050D9C0`) and heals
    /// `SelfHealInfantryAmount × this` (`0x0050D9E0`); `0x0070A534` reads the
    /// same predicate for the status pip. Stock source: `[CATHOSP]`.
    ///
    /// Private to this owner: every read is [`Self::self_heal_infantry`] and
    /// every write is [`Self::grant_self_heal`] / [`Self::revoke_self_heal`].
    #[serde(default)]
    self_heal_infantry: i32,
    /// Tech Machine Shop self-heal capacity — `HouseClass+0x168`. The unit
    /// counterpart of [`Self::self_heal_infantry`], fed by
    /// `UnitsGainSelfHeal` and read by `0x0050D9D0`/`0x0050D9F0`. Stock source:
    /// `[CAMACH]`. Private, as above.
    #[serde(default)]
    self_heal_units: i32,
    /// Historical House4FD150 primary base cell; updates at native building
    /// lifecycle boundaries rather than when a consumer requests a destination.
    /// The existing House base owner publishes this with its private radius;
    /// launch/deploy's explicit House50E000 writes retain that radius.
    pub base_center: Option<(u16, u16)>,
    #[serde(default)]
    pub(crate) base_projection: crate::sim::world::HouseBaseState,
    /// Alternate base-placement cell written by trigger actions 137/138.
    ///
    /// This is the packed-zero `HouseClass+0x5494` authority. It is distinct
    /// from the launch/primary `base_center` (`HouseClass+0x5490`) and defaults
    /// to the native invalid sentinel `(0, 0)`.
    #[serde(default)]
    pub alternate_base_center: (u16, u16),
    /// Stable IDs in native HouseClass's owned `[AI] BuildConst=` vector order
    /// (`RulesClass__ReadAI @ 0x00672AE0`, binding
    /// `0x00672B14..0x00672C01`). Successful Unlimbo/re-entry and capture
    /// append at the tail; Limbo and old-owner transfer stable-remove in place.
    #[serde(default)]
    pub build_const_order: Vec<u64>,
    /// Ordered native `BaseClass` plan authority. Scenario nodes are installed
    /// before map-object Unlimbo; later ordinary planning remains disconnected.
    #[serde(default)]
    pub base_plan: crate::sim::base_plan::BasePlanState,
    /// Packed-zero-default center owned by native `BaseClass` at
    /// `HouseClass+0x5750`. A successful non-controlled ConstructionYard
    /// deploy writes this after anchoring BasePlan node zero; it is distinct
    /// from the launch/primary `base_center` at `HouseClass+0x5490`.
    #[serde(default)]
    pub base_plan_center: (u16, u16),
    /// Native `HouseClass+0x5700` BaseClass reservation writer outputs.
    #[serde(default)]
    pub base_reservation: BaseReservationState,
    /// Max tech level for this player. From game options at match start.
    pub tech_level: i32,
    /// Live HouseClass CurrentIQ (+0x24C), used by AI behavior thresholds.
    ///
    /// Named scenario houses read their own `IQ=`. Generated skirmish computer
    /// houses are stamped from `[IQ] MaxIQLevels`; generated human and special
    /// houses retain the native constructor value zero.
    pub current_iq: i32,
    /// HouseClass `+0x1D0`: the map section's `IQ=`, which only
    /// `HouseClass::Read_Scenario_INI` writes (`0x00500DBA`, with CurrentIQ).
    /// `ScenarioClass::Full_Init` reads the map's houses only in a campaign
    /// (`0x006877B9`), so every skirmish house keeps the constructor's zero
    /// (`0x004F56BD`). Its one gameplay reader is the computer's low-credit
    /// sale (`[IQ] SellBack`, `0x004507A4`); the House CRC does not fold it.
    #[serde(default)]
    pub authored_iq: i32,
    /// Native `AngerStruct` scores keyed by the other house's stable identity.
    ///
    /// gamemd stores an O(N^2) vector in global HouseClass creation order. The
    /// Rust house registry already owns that exact order in
    /// `ScenarioSession::house_order`, so only touched scores live here; enemy
    /// selection still scans session order, never this map's key order.
    #[serde(default)]
    pub grudge_scores: BTreeMap<InternedId, i32>,
    /// House selected by `HouseClass::UpdateAngerNodes`, or native `-1` as None.
    #[serde(default)]
    pub enemy_house: Option<InternedId>,
    /// Edge of the playfield where this house spawns paradrop carriers.
    /// Encoding: 0=N, 1=E, 2=S, 3=W. Launch setup seeds it from the assigned
    /// start anchor; committed structures refresh it through lifecycle authority.
    pub waypoint_edge: u8,
    /// End-of-match score-screen statistics (Kills / Losses / Built columns).
    ///
    /// gamemd keeps the same three quantities on the house: per-house
    /// `UnitsKilled`/`BuildingsKilled` tables that the score screen sums, a
    /// `UnitsLost`/`BuildingsLost` pair, and four "quantity built" counters that
    /// its `Record_Last_Built` step increments once per finished factory item.
    /// Only the totals are player-visible, so the Rust model keeps totals.
    ///
    /// House Save504080/Load503040 preserve the native inline statistics and
    /// all four built Counters (naval_house_stats native execution). Keep the
    /// existing totals through loading: an active sinking hull has already
    /// booked its first loss and records another at terminal depth. These
    /// live values also feed TerminalScoreSnapshot's Scenario RNG bound and
    /// participate in the deterministic hash from schema227.
    pub stats: MatchStatistics,
    /// House+244: Engineer PerCell519F4F records that this House has had a
    /// building captured. Constructor4F577D clears it, and the original
    /// TEvent Evaluate71F0E1 reads it for event kind3 (tables71F328/71F350).
    /// There is no clearing writer. Live Tag/TEvent polling remains a separate
    /// trigger mechanism; retaining this prerequisite does not implement it.
    #[serde(default)]
    building_capture_notified: bool,
    /// House+1F4: first discovery of this House's foreign object by PlayerPtr.
    /// Constructor4F5716 clears it; TechnoDiscovered6F4A25 sets it and no
    /// clearing writer exists. TEvent71F0FB reads it for event kind5, after
    /// resolving the House by its CountryType index through502D30. Live
    /// Tag/TEvent evaluation remains a separate trigger mechanism.
    /// Native House CRC502D60 omits1F4. Like Techno41A/B/C this is a
    /// client-relative retained observation and is excluded from peer hashing.
    #[serde(default)]
    discovered_by_current_house: bool,
    /// Sole credit balance and economy statistics; serialized and hashed.
    pub economy: Economy,
    /// Snapshot/hash authority for the Strategy emergency-state block.
    #[serde(default)]
    pub strategy_emergency: HouseStrategyEmergencyState,
    /// `HouseClass+0x5634`/`+0x563C`, the timer that runs Strategy
    /// (`sim::house_strategy::update_strategy`). The constructor starts it at
    /// the construction frame, which is frame 0, with no delay
    /// (`0x004F5B9D..0x004F5BA8`). Persisted and hashed (schema v234).
    #[serde(default = "strategy_timer_at_construction")]
    pub(crate) strategy_timer: CdTimer,
    /// The team timer and `RatioAITriggerTeam=` (`HouseClass+0x5798`/`+0x57A0`,
    /// `+0x565C`), owned by `sim::ai_team_creation`. Persisted and hashed
    /// (schema v238).
    #[serde(default)]
    pub(crate) team_creation: crate::sim::ai_team_creation::HouseTeamCreation,
    /// Native House bytes `+0x1EE`, `+0x1EF`, `+0x1F2`, and `+0x1F3`. All four
    /// persist, while Production, AutocreateAllowed, and AITriggersActive
    /// directly enter House CRC.
    #[serde(default)]
    pub ai_activation: HouseAiActivationLatches,
    /// The computer's production mode, building choice and naval latch
    /// (`HouseClass+0x1E4`, `+0x564C`, `+0x1F0`), owned by
    /// `sim::ai_base_building`. Persisted and hashed (schema v232).
    #[serde(default)]
    pub(crate) ai_production: crate::sim::ai_base_building::HouseAiProduction,
    /// The computer's Unit, Infantry and Aircraft choices (`HouseClass+0x5650`,
    /// `+0x5654`, `+0x5658`), owned by `sim::ai_unit_choice`. Persisted and
    /// hashed (schema v238).
    #[serde(default)]
    pub(crate) ai_unit_choices: crate::sim::ai_unit_choice::HouseAiUnitChoices,
    /// Native `HouseClass+0x242`: "a harvester of this house found no ore".
    ///
    /// Exhaustive instruction census (`search_instructions` operand `+0x242]`,
    /// 1,164,209 instructions, 4 hits, 2026-09-05):
    /// - `HouseClass__Constructor` `0x004F5771` writes BL (= 0);
    /// - `UnitClass::Mission_Harvest @ 0x0073E5E0` writes 1 at `0x0073E911`
    ///   on the state-0 scan-miss path (no ore in `TiberiumLongScan`, no
    ///   destination, no archive) when the type is `Harvester=yes`;
    /// - `UnitClass::Mission_Guard @ 0x00740810` reads it at `0x00740922` —
    ///   the AI-house arm re-queues Harvest only while it is clear;
    /// - `HouseClass__AI_Choose_Unit` reads it at `0x004FEB7B` (AI harvester
    ///   production bias).
    ///
    /// No clearing writer exists anywhere: once set it stays set for the rest
    /// of the match (save/load carries the raw House block). The
    /// `Mission_Guard` arm (ii) reader is implemented
    /// (`techno_ai/mission_handlers.rs`, AI houses only); the
    /// `AI_Choose_Unit` reader belongs to the deferred AI lane, which is why
    /// AI-house miners take a VERA-internal re-scan bridge instead of the
    /// native Guard park (`miner_system::handle_going_to_idle`).
    /// Persisted and hashed (schema v132).
    #[serde(default)]
    pub harvester_no_ore: bool,
    /// Native `HouseClass+0x57D4`: the `EVA_InsufficientFunds` nag timer
    /// (`HouseClass::Update 0x004F8B3C..0x004F8C53`: the expiry test
    /// `0x004F8B4E..0x004F8B63`, the restart `0x004F8BD0..0x004F8BE1`). Only
    /// the local player's house reaches that block natively; here every human
    /// house runs it and the app keeps the local filter. Persisted and hashed
    /// (schema v133).
    #[serde(default = "eva_funds_timer_at_construction")]
    pub eva_funds_timer: CdTimer,
    /// Native `[0x00A8F040]`, the `EVA_LowPower` one-shot guard
    /// (`0x004F8D02` test, `0x004F8D61` set, `0x004F8DAB` clear). It is a
    /// process global gated behind `this == PlayerPtr`, so one flag per human
    /// house is the same state. Persisted and hashed (schema v133).
    #[serde(default)]
    pub eva_low_power_guard: bool,
    /// Native `HouseClass+0x1C0`, RepairDelay: the house's difficulty row's
    /// `RepairDelay=`, which [`HouseState::set_difficulty`] copies (the
    /// constructor's 0.0 until then). The computer's auto-repair start draws
    /// its latch time from it (`production::update_repair_and_power`).
    /// Persisted and hashed (schema v216).
    #[serde(default)]
    pub(crate) repair_delay: f64,
    /// Native `HouseClass+0x245`: a building's auto-repair start sets it
    /// (`0x004506FF`), and while set no other building of the house starts
    /// one. [`HouseState::release_repair_latch`] clears it. Persisted and
    /// hashed (schema v216).
    #[serde(default)]
    pub(crate) repair_start_latch: bool,
    /// Native `HouseClass+0x280` timer: the auto-repair start of a house no
    /// human controls arms it (`0x00450764..0x00450779`), and the latch holds
    /// until it expires. The constructor starts it at the construction frame
    /// with no time left. Persisted and hashed (schema v216).
    pub(crate) repair_latch_timer: CdTimer,
    /// House+57E4..+15FF3: native retained 130x130 padded spatial threat map.
    /// Constructor4F61EF/4F6312 clears it. Save504093 and Load5031C9 retain
    /// it through the raw House stream; rebuilding from current objects loses
    /// incremental rounding/clamping and diplomacy history.
    #[serde(default)]
    spatial_threat: crate::sim::house_threat::HouseSpatialThreat,
    #[serde(default)]
    super_weapon_cells: HouseSuperWeaponCells,
}

/// The super weapon cells a House keeps: where its nuclear missile flies
/// (`HouseClass+0x5784`) and its last super weapon alert (`+0x54F4` cell,
/// `+0x54FC` frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct HouseSuperWeaponCells {
    /// `+0x5784` NukeTarget: `SuperClass::Launch`'s MultiMissile arm stores
    /// the target cell (`0x006CDDC4`) and the silo's Mission_Missile reads
    /// it for the warning anim and the missile (`0x0044C9DC`, `0x0044CAA9`).
    /// Constructor (0, 0) (`0x004F5C8D`).
    nuke_target: (u16, u16),
    /// `+0x54F4` PreferredDefensiveCell: the base the launch alert
    /// (`0x004FB0BD`) asks the computer to shield. `HouseClass::AI_TryFireSW
    /// @ 0x005098F0` aims its ForceShield there (`0x00509A6D..0x00509A9B`)
    /// while the alert is younger than `[General] AISuperDefenseFrames=`
    /// (`superweapon::ai_fire`). Constructor (0, 0) (`0x004F5A8F`); trigger
    /// actions also write it (`0x0050DA20`, `0x0050DA50`), which VERA does not
    /// run.
    defense_cell: (u16, u16),
    /// `+0x54FC`: the frame of that alert (`0x004FB0C9`); constructor -100
    /// (`0x004F5AB1`).
    defense_frame: i32,
}

impl Default for HouseSuperWeaponCells {
    fn default() -> Self {
        Self {
            nuke_target: (0, 0),
            defense_cell: (0, 0),
            defense_frame: -100,
        }
    }
}

impl HouseState {
    /// The infantry self-heal count — `HouseClass+0x164`, the value
    /// `HasInfSelfHeal @ 0x0050D9C0` tests and `0x0070A534` draws the status
    /// pip from.
    pub(crate) fn self_heal_infantry(&self) -> i32 {
        self.self_heal_infantry
    }

    /// The unit self-heal count — `HouseClass+0x168`, the value
    /// `HasUnitSelfHeal @ 0x0050D9D0` tests and `0x0070A5A0` draws the status
    /// pip from.
    pub(crate) fn self_heal_units(&self) -> i32 {
        self.self_heal_units
    }

    /// `BuildingClass::OnConstructionComplete`'s grant
    /// (`0x00446382..0x004463B4`) and the arrival half of `ChangeOwner`
    /// (`0x00448B0A..`): a plain 32-bit add of the built type's
    /// `InfantryGainSelfHeal` / `UnitsGainSelfHeal`, with no clamp.
    pub(crate) fn grant_self_heal(&mut self, infantry: i32, units: i32) {
        self.self_heal_infantry = self.self_heal_infantry.wrapping_add(infantry);
        self.self_heal_units = self.self_heal_units.wrapping_add(units);
    }

    /// `BuildingClass::Limbo`'s share (`0x004459AE..0x004459CA`, unit arm at
    /// `0x004459E0`) and the departure half of `ChangeOwner`
    /// (`0x00448AFC..0x00448B04`, `0x00448B3E..0x00448B46`): subtract, then
    /// raise a negative count to zero.
    pub(crate) fn revoke_self_heal(&mut self, infantry: i32, units: i32) {
        self.self_heal_infantry = self.self_heal_infantry.wrapping_sub(infantry).max(0);
        self.self_heal_units = self.self_heal_units.wrapping_sub(units).max(0);
    }

    /// The cell the house's nuclear missile flies to (`HouseClass+0x5784`).
    pub(crate) fn nuke_target(&self) -> (u16, u16) {
        self.super_weapon_cells.nuke_target
    }

    /// `SuperClass::Launch 0x006CDDC4`: the MultiMissile arm's target.
    pub(crate) fn set_nuke_target(&mut self, cell: (u16, u16)) {
        self.super_weapon_cells.nuke_target = cell;
    }

    /// The last super weapon alert's defence cell and frame (`+0x54F4`,
    /// `+0x54FC`).
    pub(crate) fn super_weapon_defense(&self) -> ((u16, u16), i32) {
        (
            self.super_weapon_cells.defense_cell,
            self.super_weapon_cells.defense_frame,
        )
    }

    /// The launch alert's stores (`0x004FB0BD`, `0x004FB0C9`).
    pub(crate) fn alert_super_weapon_defense(&mut self, cell: (u16, u16), frame: i32) {
        self.super_weapon_cells.defense_cell = cell;
        self.super_weapon_cells.defense_frame = frame;
    }

    /// Folded only off the constructor values, so houses that never saw a
    /// super weapon hash as earlier schemas did.
    pub(crate) fn hash_super_weapon_cells(&self, hasher: &mut impl std::hash::Hasher) {
        if self.super_weapon_cells != HouseSuperWeaponCells::default() {
            std::hash::Hash::hash(b"house-super-weapon-cells-v1", hasher);
            std::hash::Hash::hash(&self.super_weapon_cells, hasher);
        }
    }
    /// The old House's notification before the arrival's Building ChangeOwner.
    pub(crate) fn notify_building_capture(&mut self) {
        self.building_capture_notified = true;
    }

    /// Retained input to original TEvent kind3, not current building ownership.
    pub(crate) fn building_capture_notified(&self) -> bool {
        self.building_capture_notified
    }

    pub(crate) fn notify_discovered_by_current_house(&mut self) {
        self.discovered_by_current_house = true;
    }

    pub(crate) fn discovered_by_current_house(&self) -> bool {
        self.discovered_by_current_house
    }

    pub(crate) fn hash_event_notifications(&self, hasher: &mut impl std::hash::Hasher) {
        if self.building_capture_notified {
            std::hash::Hash::hash(b"house-building-capture-notification-v1", hasher);
            std::hash::Hash::hash(&self.building_capture_notified, hasher);
        }
    }

    pub(crate) fn spatial_threat_values(&self) -> &[i32] {
        self.spatial_threat.values()
    }

    pub(crate) fn spatial_threat_at(&self, cell: (i16, i16)) -> Result<i32, String> {
        self.spatial_threat.at(cell)
    }

    /// EstimateZoneThreat585F40 level1 reads House+57E4 by ZoneRecord+20.
    pub(crate) fn spatial_threat_at_padded_index(&self, index: i32) -> Result<i32, String> {
        self.spatial_threat.at_padded_index(index)
    }

    pub(crate) fn adjust_spatial_threat(
        &mut self,
        cell: (i16, i16),
        delta: i32,
    ) -> Result<(), String> {
        self.spatial_threat.adjust(cell, delta)
    }

    pub(crate) fn clear_spatial_threat(&mut self) {
        self.spatial_threat.clear();
    }
    /// The HouseType this House was made from (HouseClass `+0x34`): its
    /// `Country=`, else its own name, which a map House shares with its type.
    pub(crate) fn house_type_id(&self) -> InternedId {
        self.country.unwrap_or(self.name)
    }

    /// Take the `Cost*Mult=` and `Speed*Mult=` of the House's type from
    /// `rules` ([`CountryCostMults`]); the scenario's House creation calls it
    /// once.
    pub(crate) fn project_country_mults(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        interner: &crate::sim::intern::StringInterner,
    ) {
        let house_type = interner.resolve(self.house_type_id());
        self.country_cost_mults = CountryCostMults(rules.country_cost_mults(house_type));
        self.country_speed_mults = CountrySpeedMults(rules.country_speed_mults(house_type));
    }

    /// `HouseClass::GetSpeedBonus @ 0x0050C050`: the HouseType's float for
    /// the type's WhatAmI (AircraftType `+0x130`, InfantryType `+0x128`,
    /// UnitType `+0x12C`), 1.0 for any other type.
    pub(crate) fn speed_bonus(
        &self,
        category: crate::rules::object_type::ObjectCategory,
    ) -> NativeF32Bits {
        use crate::rules::object_type::ObjectCategory;
        let [infantry, units, aircraft] = self.country_speed_mults.0;
        match category {
            ObjectCategory::Infantry => infantry,
            ObjectCategory::Vehicle => units,
            ObjectCategory::Aircraft => aircraft,
            ObjectCategory::Building => NativeF32Bits::ONE,
        }
    }

    /// `HouseClass::SetDifficulty @ 0x004F6EC0`, for the fields VERA keeps:
    /// the difficulty index (`+0x184`), the ROF bias (`+0x1A8`), the repair
    /// delay (`+0x1C0`) and the team timer. Outside a campaign the bias is
    /// the difficulty row's `ROF=` times the country's (`FLD; FMUL; FSTP
    /// qword`, `0x004F6F6C..0x004F6F79`); in a campaign it is the row's value
    /// alone (`0x004F7072..0x004F707B`). Both copy the row's `RepairDelay=`
    /// unchanged (`0x004F6FA5`, `0x004F70AC`). The team timer restarts at
    /// `frame` for the difficulty's `TeamDelays=` plus 175 frames for each
    /// house before this one (`array_index`, `+0x30`; wrapping,
    /// `0x004F70F0..0x004F712D`).
    pub(crate) fn set_difficulty(
        &mut self,
        difficulty: HouseDifficulty,
        general: &crate::rules::ruleset::GeneralRules,
        country_rof: f64,
        game_mode_nonzero: bool,
        array_index: i32,
        frame: i32,
    ) {
        use crate::util::native_x87::MaskedX87Chop53 as X;
        self.difficulty = difficulty;
        self.repair_delay = general.difficulty_repair_delay[difficulty.table_index()];
        let row =
            NativeF64Bits::from_bits(general.difficulty_rof[difficulty.table_index()].to_bits());
        self.rof_bias = HouseRofBias(if game_mode_nonzero {
            X::store_f64_masked_chop(X::mul(
                X::load_f64(row),
                X::load_f64(NativeF64Bits::from_bits(country_rof.to_bits())),
            ))
        } else {
            row
        });
        let team_delay = self
            .difficulty_value(&general.team_delays)
            .wrapping_add(array_index.wrapping_mul(175));
        self.team_creation.restart(frame, team_delay);
    }

    /// The house's ROF multiplier (`HouseClass+0x1A8`).
    pub(crate) const fn rof_bias(&self) -> NativeF64Bits {
        self.rof_bias.0
    }

    /// The house's entry of a per-difficulty Rules vector, indexed by
    /// `+0x184` (hardest first). Native reads past a short vector's end;
    /// VERA reads 0 there (retail vectors hold three entries).
    pub(crate) fn difficulty_value(&self, values: &[i32]) -> i32 {
        values
            .get(self.difficulty.table_index())
            .copied()
            .unwrap_or(0)
    }

    /// Active offline EventClass house-scan eligibility.
    #[cfg(test)]
    pub const fn event_dispatch_eligible(&self) -> bool {
        self.is_human || self.player_control
    }

    /// Native mode-aware House-control predicate shared by successful Building
    /// Unlimbo BasePlan satisfaction and the early House-update activation.
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` supplies the former;
    /// `HouseClass__Update @ 0x004F8440` inlines the same branch shape.
    pub(crate) const fn is_controlled_by_human(&self, game_mode_nonzero: bool) -> bool {
        self.is_human || (!game_mode_nonzero && self.player_control)
    }

    /// The cell the house bases itself around: the alternate base centre
    /// (`+0x5494`) unless it is the empty cell `(0, 0)` (`0xA8EF98`, zeroed
    /// by its static initializer `0x004F50A0`), else the primary (`+0x5490`,
    /// `(0, 0)` while unset); `(0, 0)` is none.
    pub(crate) fn base_origin(&self) -> (u16, u16) {
        if self.alternate_base_center != (0, 0) {
            self.alternate_base_center
        } else {
            self.base_center.unwrap_or((0, 0))
        }
    }

    /// `HouseClass::Update 0x004F9302..0x004F9338`: the auto-repair latch
    /// releases once its timer has expired.
    pub(crate) fn release_repair_latch(&mut self, frame: u32) {
        if self.repair_start_latch && self.repair_latch_timer.expired(frame as i32) {
            self.repair_start_latch = false;
        }
    }

    /// Co-enable the three successful AI base-unit deploy latches.
    ///
    /// gamemd-derived: `UnitClass__Deploy @ 0x007393C0` writes Production at
    /// `0x007398FF`, AITriggersActive at `0x0073990C`, then AutoBaseBuilding at
    /// `0x00739919`, with no branch or call between the stores.
    pub(crate) fn enable_ai_deploy_latches(&mut self) {
        self.ai_activation.production = true;
        self.ai_activation.ai_triggers_active = true;
        self.ai_activation.auto_base_building = true;
    }

    /// Run the early `HouseClass__Update` AI-activation transition.
    ///
    /// gamemd-derived: `HouseClass__Update @ 0x004F8440`, block
    /// `0x004F8564..0x004F85B7`, rejects the mode-aware controlled House,
    /// accepts any nonzero AutoBaseBuilding or signed `CurrentIQ >=
    /// Rules+0x143C`, then writes AutoBaseBuilding, Production, and
    /// AutocreateAllowed in that order without touching AITriggersActive.
    pub(crate) fn update_ai_activation(&mut self, game_mode_nonzero: bool, iq_production: i32) {
        if self.is_controlled_by_human(game_mode_nonzero)
            || (!self.ai_activation.auto_base_building && self.current_iq < iq_production)
        {
            return;
        }
        self.ai_activation.auto_base_building = true;
        self.ai_activation.production = true;
        self.ai_activation.autocreate_allowed = true;
    }

    /// Accept a victory and arm its deterministic grace interval.
    ///
    /// gamemd provenance: HouseClass::Flag_To_Win @ `0x004FC9E0` accepts only
    /// while Win/Draw/Lose are clear, sets HasWon, announces victory, and sets
    /// the house timer to `ftol(SavourDelay * 900)`.
    pub(crate) fn flag_to_win(&mut self, current_tick: u64, savour_frames: u64) -> bool {
        if self.has_won || self.has_lost {
            return false;
        }
        self.has_won = true;
        self.outcome_state = Some(HouseOutcomeState {
            kind: HouseOutcomeKind::Victory,
            savour_until_tick: current_tick.saturating_add(savour_frames),
            exit_ready: false,
        });
        true
    }

    /// Accept a defeat, replacing an earlier pending victory when present.
    ///
    /// gamemd provenance: HouseClass::Flag_To_Lose @ `0x004FCBD0` clears
    /// HasWon unconditionally, then (unless already Draw/Lost) sets HasLost,
    /// announces defeat, and re-arms the full SavourDelay interval.
    pub(crate) fn flag_to_lose(&mut self, current_tick: u64, savour_frames: u64) -> bool {
        self.has_won = false;
        if self.has_lost {
            return false;
        }
        self.has_lost = true;
        self.outcome_state = Some(HouseOutcomeState {
            kind: HouseOutcomeKind::Defeat,
            savour_until_tick: current_tick.saturating_add(savour_frames),
            exit_ready: false,
        });
        true
    }

    /// Advance the HouseClass result timer at the late house-update boundary.
    ///
    /// gamemd provenance: HouseClass::Update @ `0x004F8440` keeps the scenario
    /// running until this timer expires, then enters the bounded Vox wait.
    pub(crate) fn advance_outcome_savour(&mut self, current_tick: u64) -> bool {
        let Some(outcome) = self.outcome_state.as_mut() else {
            return false;
        };
        if current_tick >= outcome.savour_until_tick {
            outcome.exit_ready = true;
        }
        outcome.exit_ready
    }

    pub fn new(
        name: InternedId,
        side_index: u8,
        country: Option<InternedId>,
        is_human: bool,
        credits: i32,
        tech_level: i32,
    ) -> Self {
        Self {
            name,
            side_index,
            country,
            is_human,
            player_control: is_human,
            difficulty: HouseDifficulty::Normal,
            rof_bias: HouseRofBias::default(),
            multiplay_passive: false,
            country_cost_mults: CountryCostMults::default(),
            country_speed_mults: CountrySpeedMults::default(),
            is_defeated: false,
            has_won: false,
            has_lost: false,
            outcome_state: None,
            map_is_clear: false,
            spy_sat_active: false,
            tracking: Default::default(),
            self_heal_infantry: 0,
            self_heal_units: 0,
            base_center: None,
            base_projection: crate::sim::world::HouseBaseState::default(),
            alternate_base_center: (0, 0),
            build_const_order: Vec::new(),
            base_plan: crate::sim::base_plan::BasePlanState::default(),
            base_plan_center: (0, 0),
            base_reservation: BaseReservationState::default(),
            tech_level,
            current_iq: 0,
            authored_iq: 0,
            grudge_scores: BTreeMap::new(),
            enemy_house: None,
            waypoint_edge: 0,
            stats: MatchStatistics::default(),
            building_capture_notified: false,
            discovered_by_current_house: false,
            economy: Economy::new(credits),
            strategy_emergency: HouseStrategyEmergencyState::default(),
            strategy_timer: strategy_timer_at_construction(),
            team_creation: Default::default(),
            ai_activation: HouseAiActivationLatches::default(),
            ai_production: Default::default(),
            ai_unit_choices: Default::default(),
            harvester_no_ore: false,
            eva_funds_timer: eva_funds_timer_at_construction(),
            eva_low_power_guard: false,
            repair_delay: 0.0,
            repair_start_latch: false,
            repair_latch_timer: CdTimer::started(0, 0),
            spatial_threat: Default::default(),
            super_weapon_cells: HouseSuperWeaponCells::default(),
        }
    }
}

/// Post-match statistics accumulated for the end-of-match score screen.
///
/// gamemd sums per-victim-house kill tables into one number for the Kills
/// column, adds its two loss counters for the Losses column, and sums its four
/// per-category built counters for the Built column. Totals are all the screen
/// ever reads, so these are kept as totals. Its Score column reads
/// [`Economy::score`](crate::sim::economy::Economy::score).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct MatchStatistics {
    /// Non-building objects recorded as kills for this house, including allies.
    units_killed: u32,
    /// Buildings recorded as kills for this house, including captures.
    buildings_killed: u32,
    /// Non-building objects of this house that were destroyed.
    units_lost: u32,
    /// Buildings of this house that were destroyed.
    buildings_lost: u32,
    /// Objects of this house that left their factory, `DontScore=` types
    /// excepted: placed buildings and delivered units
    /// (`production::factory_lifecycle::record_last_built`).
    built: u32,
}

#[cfg(test)]
#[path = "house_statistics_tests.rs"]
mod statistics_tests;

impl MatchStatistics {
    pub const fn units_killed(&self) -> u32 {
        self.units_killed
    }

    pub const fn buildings_killed(&self) -> u32 {
        self.buildings_killed
    }

    pub const fn units_lost(&self) -> u32 {
        self.units_lost
    }

    pub const fn buildings_lost(&self) -> u32 {
        self.buildings_lost
    }

    pub const fn built(&self) -> u32 {
        self.built
    }

    /// RecordKill70305C/7031A9 increment one native loss counter. Callers own
    /// DontScore, Insignificant and sale admission; this owner only mutates it.
    pub(crate) fn record_loss(&mut self, category: crate::map::entities::EntityCategory) {
        let counter = if category == crate::map::entities::EntityCategory::Structure {
            &mut self.buildings_lost
        } else {
            &mut self.units_lost
        };
        *counter = counter.wrapping_add(1);
    }

    /// RecordKill7030AC/7031DC and ChangeOwner70164D increment the kill table.
    /// The existing total projection keeps their sum, rather than a second
    /// per-house table. Both callers share its mutation and numeric semantics.
    pub(crate) fn record_kill(&mut self, category: crate::map::entities::EntityCategory) {
        let counter = if category == crate::map::entities::EntityCategory::Structure {
            &mut self.buildings_killed
        } else {
            &mut self.units_killed
        };
        *counter = counter.wrapping_add(1);
    }

    /// The existing Record_Last_Built caller owns its DontScore admission.
    pub(crate) fn record_built(&mut self) {
        self.built = self.built.wrapping_add(1);
    }

    #[cfg(test)]
    pub(crate) const fn from_totals_for_test(
        units_killed: u32,
        buildings_killed: u32,
        units_lost: u32,
        buildings_lost: u32,
        built: u32,
    ) -> Self {
        Self {
            units_killed,
            buildings_killed,
            units_lost,
            buildings_lost,
            built,
        }
    }

    /// Score-screen Kills column: units + buildings destroyed.
    pub const fn kills(&self) -> u32 {
        self.units_killed + self.buildings_killed
    }

    /// Score-screen Losses column: units + buildings lost.
    pub const fn losses(&self) -> u32 {
        self.units_lost + self.buildings_lost
    }
}

/// Look up a HouseState by interned owner ID (O(1) BTreeMap lookup).
pub fn house_state_for_owner_id<'a>(
    houses: &'a std::collections::BTreeMap<InternedId, HouseState>,
    owner_id: InternedId,
) -> Option<&'a HouseState> {
    houses.get(&owner_id)
}

/// Look up a HouseState by owner name string (case-insensitive).
/// Requires the interner to convert the name to an InternedId first.
/// Returns None if the name is not interned or no house matches.
pub fn house_state_for_owner<'a>(
    houses: &'a std::collections::BTreeMap<InternedId, HouseState>,
    owner: &str,
    interner: &crate::sim::intern::StringInterner,
) -> Option<&'a HouseState> {
    let id = interner.get(owner)?;
    houses.get(&id)
}

/// Mutable version of `house_state_for_owner`.
pub fn house_state_for_owner_mut<'a>(
    houses: &'a mut std::collections::BTreeMap<InternedId, HouseState>,
    owner: &str,
    interner: &crate::sim::intern::StringInterner,
) -> Option<&'a mut HouseState> {
    let id = interner.get(owner)?;
    houses.get_mut(&id)
}

/// Resolve an owner's ore-income `IncomeMult` (parts-per-million; 1_000_000 = 1.0×) by
/// routing through its country: `HouseState.country` (InternedId) -> country name ->
/// `RuleSet::country_income_ppm`. An owner with no house, no country, or an unknown
/// country resolves to the neutral 1.0 (no income change) — so stock YR (all countries
/// 1.0, the key commented out) is the identity.
pub fn income_ppm_for_owner(
    houses: &std::collections::BTreeMap<InternedId, HouseState>,
    interner: &crate::sim::intern::StringInterner,
    rules: &crate::rules::ruleset::RuleSet,
    owner: &str,
) -> i64 {
    house_state_for_owner(houses, owner, interner)
        .and_then(|h| h.country)
        .map(|c| rules.country_income_ppm(interner.resolve(c)))
        .unwrap_or(crate::rules::ruleset::INCOME_PPM_SCALE)
}

/// Map side name string to numeric index.
/// "Allies"/"GDI" → 0, "Soviet"/"Nod" → 1, "ThirdSide"/"YuriCountry" → 2.
pub fn side_index_from_name(side: Option<&str>) -> u8 {
    side_index_alias(side).unwrap_or(0)
}

fn side_index_alias(side: Option<&str>) -> Option<u8> {
    match side.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("allied" | "allies" | "gdi") => Some(0),
        Some("soviet" | "nod" | "russia") => Some(1),
        Some("thirdside" | "yuricountry" | "yuri") => Some(2),
        _ => None,
    }
}

/// Resolve the side identity used to construct a house.
///
/// Rules-owned country membership is authoritative. An explicit side name is
/// the next-best source for incomplete scenario data, followed by the legacy
/// stock aliases and finally the caller's bounded fallback.
pub fn resolve_house_side_index(
    rules: &crate::rules::ruleset::RuleSet,
    country: Option<&str>,
    side: Option<&str>,
    fallback: u8,
) -> u8 {
    country
        .and_then(|country| rules.country_side_index(country))
        .or_else(|| side.and_then(|side| rules.side_index(side)))
        .map(|index| index.0)
        .or_else(|| side_index_alias(side))
        .unwrap_or(fallback)
}

/// The house type an absent `Country=` binds to.
///
/// gamemd's `[Houses]` reader asks the INI for `Country=` with a default of -1
/// and maps a -1 result to 0, so a house section with no `Country=` key binds to
/// the first `[Countries]` entry — stock `Americans`, which is not
/// MultiplayPassive. It does NOT fall back to the house's own section name.
const ABSENT_COUNTRY_IDX: crate::rules::ruleset::CountryIdx = crate::rules::ruleset::CountryIdx(0);

/// Resolve a house's `MultiplayPassive` fact from its country/house-type rules.
///
/// A house with no `Country=` resolves through [`ABSENT_COUNTRY_IDX`], matching
/// the native reader. Missing rules, an empty `[Countries]` registry, or an
/// unknown country name resolve to `false` — the INI default for
/// `MultiplayPassive=`.
pub fn resolve_multiplay_passive(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    country: Option<&str>,
) -> bool {
    let Some(rules) = rules else {
        return false;
    };
    let key = match country {
        Some(country) => Some(country),
        None => rules.country_name(ABSENT_COUNTRY_IDX),
    };
    key.is_some_and(|key| rules.country_multiplay_passive(key))
}

/// Resolve a house type's `WallOwner=` permission. Missing data keeps the native
/// constructor default of `true`; an absent `Country=` binds to registry entry zero.
pub fn resolve_wall_owner(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    country: Option<&str>,
) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    let key = match country {
        Some(country) => Some(country),
        None => rules.country_name(ABSENT_COUNTRY_IDX),
    };
    key.map_or(true, |key| rules.country_wall_owner(key))
}

/// Cell-space distance through the native Sqrt_Approx/Math::ftol pipeline.
fn native_edge_distance(anchor: (u16, u16), reference: (i32, i32)) -> i32 {
    crate::util::native_x87::sqrt_approx_length([
        i32::from(anchor.0 as i16).wrapping_sub(reference.0),
        i32::from(anchor.1 as i16).wrapping_sub(reference.1),
    ])
}

/// HouseClass-style playfield edge selection for a committed anchor cell.
///
/// The four asymmetric reference points live in the map's LocalSize frame and
/// must be skewed into cell-grid coordinates before comparison. Strictly-better
/// replacement preserves the native N/E/S/W tie order.
pub(crate) fn determine_waypoint_edge(anchor: (u16, u16), bounds: PlayfieldBounds) -> u8 {
    let references = [
        (bounds.off_104 / 2, 1),
        (bounds.off_104, bounds.off_108),
        (bounds.off_104 / 2, bounds.off_108.wrapping_mul(2)),
        (0, bounds.off_108),
    ];
    let mut best_edge = 0u8;
    let mut best_distance = i32::MAX;
    for (edge, local_reference) in references.into_iter().enumerate() {
        let reference = local_to_packed_cell(bounds, local_reference.0, local_reference.1);
        let distance = native_edge_distance(anchor, reference);
        if distance < best_distance {
            best_distance = distance;
            best_edge = edge as u8;
        }
    }
    best_edge
}

#[cfg(test)]
mod ai_activation_latch_tests {
    use super::{HouseAiActivationLatches, HouseDifficulty, HouseState};

    #[test]
    fn house_ai_activation_latches_default_false() {
        let house = HouseState::new(Default::default(), 0, None, false, 0, 10);
        assert_eq!(house.ai_activation, HouseAiActivationLatches::default());
        assert!(!house.ai_activation.production);
        assert!(!house.ai_activation.autocreate_allowed);
        assert!(!house.ai_activation.ai_triggers_active);
        assert!(!house.ai_activation.auto_base_building);
        assert_eq!(house.current_iq, 0);
    }

    #[test]
    fn house_ai_activation_deploy_enable_is_ordered_and_idempotent() {
        let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
        house.ai_activation = HouseAiActivationLatches {
            production: true,
            autocreate_allowed: false,
            ai_triggers_active: false,
            auto_base_building: true,
        };

        house.enable_ai_deploy_latches();
        assert_eq!(
            house.ai_activation,
            HouseAiActivationLatches {
                production: true,
                autocreate_allowed: false,
                ai_triggers_active: true,
                auto_base_building: true,
            }
        );
        let once = house.ai_activation;
        house.enable_ai_deploy_latches();
        assert_eq!(house.ai_activation, once);
    }

    #[test]
    fn house_ai_activation_signed_threshold_and_auto_base_bypass() {
        for (current_iq, threshold, auto_base, expected) in [
            (4, 5, false, false),
            (5, 5, false, true),
            (6, 5, false, true),
            (0, -1, false, true),
            (-100, 5, true, true),
        ] {
            let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
            house.current_iq = current_iq;
            house.ai_activation.ai_triggers_active = true;
            house.ai_activation.auto_base_building = auto_base;

            house.update_ai_activation(true, threshold);

            assert_eq!(house.ai_activation.production, expected);
            assert_eq!(house.ai_activation.autocreate_allowed, expected);
            assert_eq!(house.ai_activation.auto_base_building, expected);
            assert!(
                house.ai_activation.ai_triggers_active,
                "House update never writes AITriggersActive"
            );
        }
    }

    #[test]
    fn house_ai_activation_uses_mode_sensitive_control_predicate() {
        let mut campaign_current = HouseState::new(Default::default(), 0, None, true, 0, 10);
        campaign_current.current_iq = 5;
        campaign_current.update_ai_activation(false, 5);
        assert_eq!(
            campaign_current.ai_activation,
            HouseAiActivationLatches::default()
        );

        let mut campaign_player_control =
            HouseState::new(Default::default(), 0, None, false, 0, 10);
        campaign_player_control.player_control = true;
        campaign_player_control.current_iq = 5;
        campaign_player_control.update_ai_activation(false, 5);
        assert_eq!(
            campaign_player_control.ai_activation,
            HouseAiActivationLatches::default()
        );

        let mut skirmish_player_control = campaign_player_control.clone();
        skirmish_player_control.update_ai_activation(true, 5);
        assert!(skirmish_player_control.ai_activation.production);
        assert!(skirmish_player_control.ai_activation.autocreate_allowed);
        assert!(skirmish_player_control.ai_activation.auto_base_building);

        let mut skirmish_current = HouseState::new(Default::default(), 0, None, true, 0, 10);
        skirmish_current.current_iq = 5;
        skirmish_current.update_ai_activation(true, 5);
        assert_eq!(
            skirmish_current.ai_activation,
            HouseAiActivationLatches::default()
        );
    }

    #[test]
    fn house_ai_activation_preserves_split_states_and_completes_deploy_state() {
        let mut split = HouseState::new(Default::default(), 0, None, false, 0, 10);
        split.current_iq = 4;
        split.ai_activation = HouseAiActivationLatches {
            production: true,
            autocreate_allowed: true,
            ai_triggers_active: false,
            auto_base_building: false,
        };
        let below_threshold = split.ai_activation;
        split.update_ai_activation(true, 5);
        assert_eq!(split.ai_activation, below_threshold);

        split.current_iq = 5;
        split.update_ai_activation(true, 5);
        assert_eq!(
            split.ai_activation,
            HouseAiActivationLatches {
                production: true,
                autocreate_allowed: true,
                ai_triggers_active: false,
                auto_base_building: true,
            }
        );

        let mut deployed = HouseState::new(Default::default(), 0, None, false, 0, 10);
        deployed.current_iq = i32::MIN;
        deployed.enable_ai_deploy_latches();
        assert!(!deployed.ai_activation.autocreate_allowed);
        deployed.update_ai_activation(true, 5);
        assert_eq!(
            deployed.ai_activation,
            HouseAiActivationLatches {
                production: true,
                autocreate_allowed: true,
                ai_triggers_active: true,
                auto_base_building: true,
            }
        );
    }

    #[test]
    fn house_ai_activation_has_no_defeat_passive_or_difficulty_gate_and_is_idempotent() {
        for difficulty in [HouseDifficulty::Hard, HouseDifficulty::Easy] {
            let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
            house.current_iq = 5;
            house.is_defeated = true;
            house.multiplay_passive = true;
            house.difficulty = difficulty;
            house.economy.add_credits(4321);

            house.update_ai_activation(true, 5);
            let once = house.ai_activation;
            house.update_ai_activation(true, 5);

            assert_eq!(house.ai_activation, once);
            assert_eq!(
                once,
                HouseAiActivationLatches {
                    production: true,
                    autocreate_allowed: true,
                    ai_triggers_active: false,
                    auto_base_building: true,
                }
            );
            assert_eq!(house.current_iq, 5);
            assert_eq!(house.economy.credits(), 4321);
            assert!(house.is_defeated);
            assert!(house.multiplay_passive);
            assert_eq!(house.difficulty, difficulty);
        }
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::{HouseOutcomeKind, HouseState};

    #[test]
    fn gsi_01_04_savour_gates_exact_frame_and_defeat_restarts_pending_victory() {
        let mut house = HouseState::new(Default::default(), 0, None, true, 0, 10);
        assert!(house.flag_to_win(100, 90));
        assert!(!house.advance_outcome_savour(189));
        assert!(house.advance_outcome_savour(190));

        let mut replaced = HouseState::new(Default::default(), 0, None, true, 0, 10);
        assert!(replaced.flag_to_win(100, 90));
        assert!(replaced.flag_to_lose(150, 90));
        assert!(!replaced.has_won);
        assert!(replaced.has_lost);
        assert_eq!(
            replaced.outcome_state.expect("defeat outcome").kind,
            HouseOutcomeKind::Defeat
        );
        assert_eq!(
            replaced
                .outcome_state
                .expect("restarted defeat outcome")
                .savour_until_tick,
            240
        );
        assert!(!replaced.flag_to_win(160, 90));
        assert!(!replaced.flag_to_lose(170, 90));
        assert_eq!(
            replaced
                .outcome_state
                .expect("unchanged defeat outcome")
                .savour_until_tick,
            240
        );
    }
}

#[cfg(test)]
mod difficulty_tests {
    use super::{HouseDifficulty, HouseState};

    #[test]
    fn native_difficulty_values_are_hardest_first() {
        assert_eq!(HouseDifficulty::Hard as i32, 0);
        assert_eq!(HouseDifficulty::Normal as i32, 1);
        assert_eq!(HouseDifficulty::Easy as i32, 2);
        assert_eq!(HouseDifficulty::from_native(0), Some(HouseDifficulty::Hard));
        assert_eq!(
            HouseDifficulty::from_native(1),
            Some(HouseDifficulty::Normal)
        );
        assert_eq!(HouseDifficulty::from_native(2), Some(HouseDifficulty::Easy));
        assert_eq!(HouseDifficulty::from_native(-1), None);
        assert_eq!(HouseDifficulty::from_native(3), None);
    }

    #[test]
    fn new_house_defaults_to_normal_difficulty() {
        let house = HouseState::new(Default::default(), 0, None, false, 0, 10);
        assert_eq!(house.difficulty, HouseDifficulty::Normal);
        assert_eq!(house.current_iq, 0);
        assert_eq!(house.rof_bias(), super::NativeF64Bits::ONE);
    }

    /// `tools/spatial_oracle/house_difficulty.py` runs the original
    /// `HouseClass::SetDifficulty @ 0x004F6EC0` over difficulty, GameMode,
    /// country `ROF=`, row `ROF=` values and house indexes; every row replays
    /// through [`HouseState::set_difficulty`].
    #[test]
    fn set_difficulty_matches_the_original() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_difficulty.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 72);
        let bits = |value: &serde_json::Value| {
            f64::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        };
        for row in &rows {
            let input = &row["input"];
            let row_rof: [f64; 3] = std::array::from_fn(|index| bits(&input["row_rof"][index]));
            let difficulty =
                HouseDifficulty::from_native(input["difficulty"].as_i64().unwrap() as i32).unwrap();
            let general = crate::rules::ruleset::GeneralRules {
                difficulty_rof: row_rof,
                difficulty_repair_delay: [0.02; 3],
                team_delays: vec![11, 22, 33],
                ..Default::default()
            };
            let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
            house.set_difficulty(
                difficulty,
                &general,
                bits(&input["country_rof"]),
                input["mode"].as_i64().unwrap() != 0,
                input["array_index"].as_i64().unwrap() as i32,
                100,
            );
            assert_eq!(
                i64::from(house.difficulty as i32),
                row["difficulty"].as_i64().unwrap(),
                "{input}"
            );
            assert_eq!(
                format!("{:016x}", house.rof_bias().bits()),
                row["rof_bias"].as_str().unwrap(),
                "{input}"
            );
            let team_timer = house.team_creation.timer();
            assert_eq!(
                [team_timer.start_frame(), team_timer.duration()],
                [0, 1].map(|slot| row["team_timer"][slot].as_i64().unwrap() as i32),
                "{input}"
            );
        }
    }
}

#[cfg(test)]
mod base_reservation_tests {
    use super::BaseReservationState;

    #[test]
    fn gsi_04_05_zero_minimum_rebases_and_retains_prior_span() {
        let mut state = BaseReservationState::default();
        state.update_bounds(0, 0, 3, 4);
        assert_eq!(state.bounds(), (0, 0, 3, 4));

        state.update_bounds(10, 20, 3, 5);
        assert_eq!(
            state.bounds(),
            (10, 20, 3, 5),
            "a zero minimum is treated as uninitialized again"
        );

        let mut retained = BaseReservationState {
            min_x: 0,
            width: 20,
            ..BaseReservationState::default()
        };
        retained.update_bounds(10, 1, 3, 1);
        assert_eq!(
            (retained.min_x, retained.width),
            (10, 20),
            "the sentinel assignment does not reset an already larger span"
        );
    }

    #[test]
    fn gsi_04_05_perimeter_vector_append_and_remove_are_stable() {
        let mut state = BaseReservationState::default();
        state.append_perimeter_cell_if_absent(30);
        state.append_perimeter_cell_if_absent(10);
        state.append_perimeter_cell_if_absent(20);
        state.append_perimeter_cell_if_absent(10);
        assert_eq!(state.perimeter_cells(), &[30, 10, 20]);

        state.remove_perimeter_cell(10);
        assert_eq!(state.perimeter_cells(), &[30, 20]);
    }
}

#[cfg(test)]
mod multiplay_passive_tests {
    use super::resolve_multiplay_passive;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    fn rules_with_country_order(first: &str, second: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[Countries]\n0={first}\n1={second}\n\
             [Americans]\nSide=Allies\n\
             [Neutral]\nSide=Civilian\nMultiplayPassive=true\n"
        ));
        RuleSet::from_ini(&ini).expect("country registry parses")
    }

    #[test]
    fn named_country_resolves_its_own_multiplay_passive() {
        let rules = rules_with_country_order("Americans", "Neutral");
        assert!(resolve_multiplay_passive(Some(&rules), Some("Neutral")));
        assert!(!resolve_multiplay_passive(Some(&rules), Some("Americans")));
        // Case-insensitive, like every other country lookup.
        assert!(resolve_multiplay_passive(Some(&rules), Some("neutral")));
    }

    #[test]
    fn absent_country_binds_to_the_first_countries_entry() {
        // The native `[Houses]` reader defaults a missing `Country=` to -1 and
        // maps -1 to 0, so the house takes the FIRST `[Countries]` entry. It
        // does not fall back to the house's own section name — with `Neutral`
        // sitting at entry 1, a section-name fallback would answer `true` here.
        let americans_first = rules_with_country_order("Americans", "Neutral");
        assert!(!resolve_multiplay_passive(Some(&americans_first), None));

        // Flip the registry order and the same absent key now follows entry 0.
        let neutral_first = rules_with_country_order("Neutral", "Americans");
        assert!(resolve_multiplay_passive(Some(&neutral_first), None));
    }

    #[test]
    fn missing_rules_or_unknown_country_is_not_passive() {
        let rules = rules_with_country_order("Americans", "Neutral");
        assert!(!resolve_multiplay_passive(None, Some("Neutral")));
        assert!(!resolve_multiplay_passive(None, None));
        assert!(!resolve_multiplay_passive(
            Some(&rules),
            Some("Nonexistent")
        ));
    }
}

#[cfg(test)]
mod waypoint_edge_tests {
    use super::*;

    fn square_bounds() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 100,
            off_fc: 0,
            off_100: 0,
            off_104: 100,
            off_108: 100,
        }
    }

    #[test]
    fn transformed_reference_points_select_their_corresponding_edges() {
        let bounds = square_bounds();
        assert_eq!(determine_waypoint_edge((51, 50), bounds), 0);
        assert_eq!(determine_waypoint_edge((150, 50), bounds), 1);
        assert_eq!(determine_waypoint_edge((150, 150), bounds), 2);
        assert_eq!(determine_waypoint_edge((50, 150), bounds), 3);
    }

    #[test]
    fn gsi_04_16_dustbowl_local_size_skew_selects_south() {
        let bounds = PlayfieldBounds {
            base: 70,
            off_fc: 2,
            off_100: 8,
            off_104: 65,
            off_108: 62,
        };
        assert_eq!(local_to_packed_cell(bounds, 32, 124), (100, 102));
        assert_eq!(determine_waypoint_edge((69, 115), bounds), 2);
    }
}
