//! Per-player power state tracking and low-power effects.
//!
//! RA2's power system sums each building's `Power=` value per player:
//! positive values generate power (scaled by building health), negative
//! values consume power (always at full rated value). When drain exceeds
//! output the player enters "low power", disabling `Powered=yes` buildings
//! and slowing production.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ (ObjectType, GeneralRules) and sim/ (EntityStore).
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::util::native_x87::MaskedX87Value;

/// Building `+0x544`, the actual health `BuildingClass::AI` last sampled
/// for its House's power. The constructor (`0x0043B78F`) and a successful
/// ordinary Unlimbo (`0x00440D2D`) reset it to zero ([`Self::reset`]). The
/// sample (`0x00440042..0x00440074`, [`Self::sample`]) stores a changed
/// health and has the owning House reassess its power. A repair changes
/// actual health without invalidating the House; the next sample does, and
/// any other native dirty writer can expose that live health through an
/// earlier House assessment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct PowerHealthSample(i32);

impl PowerHealthSample {
    /// The sampled health.
    pub(crate) const fn get(self) -> i32 {
        self.0
    }

    /// Sample `current`: answers whether it changed, which invalidates the
    /// owning House's power.
    pub(crate) fn sample(&mut self, current: i32) -> bool {
        if self.0 == current {
            return false;
        }
        self.0 = current;
        true
    }

    /// The constructor's and Unlimbo's reset to zero.
    pub(crate) fn reset(&mut self) {
        self.0 = 0;
    }
}

/// Per-House power totals retained until native House+5778 is invalidated.
/// Original gamemd.exe exposes these same retained
/// values through its IHouse interface, rather than reassessing on a query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PowerState {
    /// Derived debug override: real totals and outage timers remain untouched.
    #[cfg(feature = "debug-tools")]
    #[serde(skip)]
    debug_full_power: bool,
    #[cfg(feature = "debug-tools")]
    #[serde(skip)]
    debug_power_changed: bool,
    /// Signed retained generation at House+53A4. `Power_Output4F6A00`
    /// returns interface+5380, where that IHouse receiver is House+24.
    /// The query does not clamp; assessment and the later House clamp own it.
    total_output: i32,
    /// Signed retained drain at House+53A8. `Power_Drain4F6A10` returns
    /// interface+5384 unchanged, including before the House4F87FF clamp.
    total_drain: i32,
    /// House+577B: an admitted positive-output Building has DrainingMe.
    #[serde(default)]
    has_drained_power_source: bool,
    /// Native signed House power ratio is below one.
    is_low_power: bool,
    /// House+2A4/+2AC, read through the shared native countdown owner.
    blackout_timer: crate::sim::timer::CdTimer,
    /// House ctor4F5C5F/4F5C66 initializes both dirty bytes to one.
    power_dirty: bool,
    radar_dirty: bool,
    /// House+2B0/+2B8, the radar outage `CreateRadarOutage @ 0x0050BCD0`
    /// starts (the Lightning Storm; the trigger action RadarBlackout
    /// `0x006E3B56` is not ported). The constructor seeds it like the
    /// blackout timer (`0x004F5843..0x004F584F`).
    radar_outage_timer: crate::sim::timer::CdTimer,
    /// VERA-derived per-owner projection of508DF0's client-local radar
    /// result. Native stores its one local result at Tactical+14D8, not in
    /// House. Inputs are the radar outage, Scenario FreeRadar, cached power
    /// and owned buildings; the projection updates only at the reached
    /// House+5779 receiver.
    radar_available: bool,
}

impl Default for PowerState {
    fn default() -> Self {
        Self {
            #[cfg(feature = "debug-tools")]
            debug_full_power: false,
            #[cfg(feature = "debug-tools")]
            debug_power_changed: false,
            total_output: 0,
            total_drain: 0,
            has_drained_power_source: false,
            is_low_power: false,
            //HouseCtor4F583D seeds the native current frame. Scenario Houses
            //are constructed at0; the empty retained timer is not stopped−1.
            blackout_timer: crate::sim::timer::CdTimer::started(0, 0),
            power_dirty: true,
            radar_dirty: true,
            radar_outage_timer: crate::sim::timer::CdTimer::started(0, 0),
            radar_available: false,
        }
    }
}

impl PowerState {
    /// The retained generation (House+53A4), without the trainer's override
    /// ([`Self::effective_totals`]).
    pub fn total_output(&self) -> i32 {
        self.total_output
    }

    /// The retained drain (House+53A8).
    pub fn total_drain(&self) -> i32 {
        self.total_drain
    }

    /// House::Update_Power_And_EVA5018DD..5018E1 adds retained drain
    /// immediately; it neither clamps nor sets the assessment dirty byte.
    pub(crate) fn add_building_drain(&mut self, drain: i32) {
        self.total_drain = self.total_drain.wrapping_add(drain);
        self.is_low_power = !self.retained_full_power();
    }

    /// House+577B.
    pub fn has_drained_power_source(&self) -> bool {
        self.has_drained_power_source
    }

    /// The retained low-power flag, without the trainer's override
    /// ([`Self::low_power`]).
    pub fn is_low_power(&self) -> bool {
        self.is_low_power
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_has_drained_power_source_for_test(&mut self, value: bool) {
        self.has_drained_power_source = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_is_low_power_for_test(&mut self, value: bool) {
        self.is_low_power = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_total_drain_for_test(&mut self, value: i32) {
        self.total_drain = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_total_output_for_test(&mut self, value: i32) {
        self.total_output = value;
    }

    pub(crate) fn invalidate(&mut self, radar: bool) {
        self.power_dirty = true;
        self.radar_dirty |= radar;
    }

    pub fn blackout_remaining(&self, binary_frame: u32) -> i32 {
        self.blackout_timer.remaining(binary_frame as i32)
    }

    /// House50BC90, also called by ForceShield6CD18B: replace the timer,
    /// including a shorter or zero duration, and dirty power only. Original
    /// setter/overlap controls are preserved in house_power_consumers.
    pub(crate) fn start_blackout(&mut self, binary_frame: u32, duration: i32) {
        self.blackout_timer.start(binary_frame as i32, duration);
        self.invalidate(false);
    }

    /// `HouseClass::CreateRadarOutage @ 0x0050BCD0`: restart the outage
    /// timer at `binary_frame` for `duration` frames, a shorter one too, and
    /// recheck radar (House+5779).
    pub(crate) fn start_radar_outage(&mut self, binary_frame: u32, duration: i32) {
        self.radar_outage_timer.start(binary_frame as i32, duration);
        self.radar_dirty = true;
    }

    /// House+5779 (RecheckRadar): the next House update reassesses radar.
    /// `LightningStorm::Start` sets it for the player (`0x0053A002`).
    pub(crate) fn recheck_radar(&mut self) {
        self.radar_dirty = true;
    }

    /// House4F84F1 admits both radar and SpySat from this same byte. Retain
    /// its entry value before the radar receiver clears it (508DF0).
    pub(crate) fn radar_recheck_pending(&self) -> bool {
        self.radar_dirty
    }

    /// `RadarClass::SetTacticalMapAvailable(1)` from the current house's
    /// defeat (`0x004FC21B`): the radar comes on whatever the house owns.
    pub(crate) fn set_tactical_map_available(&mut self) {
        self.radar_available = true;
    }

    /// House+5779 cleared without a radar recheck: once the current house is
    /// defeated `HouseClass::Update` skips `0x00508DF0` (`0x004F84FB`), and
    /// `HouseClass::Update_SpySat @ 0x00508F60` still clears the byte.
    pub(crate) fn skip_radar_recheck(&mut self) {
        self.radar_dirty = false;
    }

    /// A state its House update already assessed: nothing to recheck.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn settled_for_test() -> Self {
        Self {
            power_dirty: false,
            radar_dirty: false,
            ..Self::default()
        }
    }

    /// House+5779: the next House update reassesses radar.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn radar_recheck_for_test(&self) -> bool {
        self.radar_dirty
    }

    /// House+5778: the next House update reassesses power.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn power_recheck_for_test(&self) -> bool {
        self.power_dirty
    }

    /// House+2A4/+2AC.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn blackout_timer_for_test(&self) -> crate::sim::timer::CdTimer {
        self.blackout_timer
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_radar_outage_for_test(&mut self, timer: crate::sim::timer::CdTimer) {
        self.radar_outage_timer = timer;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn radar_outage_for_test(&self) -> crate::sim::timer::CdTimer {
        self.radar_outage_timer
    }

    pub(crate) fn hash_assessment_state(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.blackout_timer.hash(hasher);
        self.power_dirty.hash(hasher);
        self.radar_dirty.hash(hasher);
        self.radar_available.hash(hasher);
        // A house no outage ever reached keeps its established stream.
        if self.radar_outage_timer != crate::sim::timer::CdTimer::started(0, 0) {
            b"radar-outage-v1".hash(hasher);
            self.radar_outage_timer.hash(hasher);
        }
    }

    /// House4FCE30 and508D99..DC9: produced>=drained or drained==0 gives1;
    /// otherwise signed produced/drained is compared with1. With finite i32
    /// operands, a negative denominator reverses the strict less-than result.
    pub(crate) fn has_full_power(&self) -> bool {
        #[cfg(feature = "debug-tools")]
        if self.debug_full_power {
            return true;
        }
        self.retained_full_power()
    }

    fn retained_full_power(&self) -> bool {
        self.total_output >= self.total_drain || self.total_drain <= 0
    }

    /// Gameplay-facing reading; retained native totals stay available for
    /// persistence and diagnostics even while the trainer supplies power.
    pub fn effective_totals(&self) -> (i32, i32) {
        #[cfg(feature = "debug-tools")]
        if self.debug_full_power {
            return (self.total_output.max(self.total_drain), self.total_drain);
        }
        (self.total_output, self.total_drain)
    }

    pub fn low_power(&self) -> bool {
        #[cfg(feature = "debug-tools")]
        if self.debug_full_power {
            return false;
        }
        self.is_low_power
    }

    #[cfg(feature = "debug-tools")]
    pub(crate) fn set_debug_full_power(&mut self, enabled: bool) -> bool {
        if self.debug_full_power == enabled {
            return false;
        }
        self.debug_full_power = enabled;
        self.debug_power_changed = true;
        self.invalidate(true);
        true
    }

    #[cfg(feature = "debug-tools")]
    pub(crate) fn take_debug_power_changed(&mut self) -> bool {
        std::mem::take(&mut self.debug_power_changed)
    }

    /// House4F87FF..4F8825 clamps signed totals after its early power/radar
    /// receivers, before team creation and EVA. Factory's rate rewrite has
    /// already consumed the assessment's un-clamped totals at this point.
    pub(crate) fn clamp_negative_totals(&mut self) {
        self.total_output = self.total_output.max(0);
        self.total_drain = self.total_drain.max(0);
        self.is_low_power = !self.retained_full_power();
    }
}

/// Every House's power state. Native HouseClass holds these fields inline, so
/// a House's entry comes into being with its first power event and is never
/// removed; a House without one reads as a fresh `PowerState`. Entries change
/// only through `PowerState`'s operations.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct PowerStates {
    by_house: BTreeMap<InternedId, PowerState>,
}

impl PowerStates {
    pub(crate) fn get(&self, house: &InternedId) -> Option<&PowerState> {
        self.by_house.get(house)
    }

    pub(crate) fn get_mut(&mut self, house: &InternedId) -> Option<&mut PowerState> {
        self.by_house.get_mut(house)
    }

    /// The House's power state, created fresh on its first use.
    pub(crate) fn house_mut(&mut self, house: InternedId) -> &mut PowerState {
        self.by_house.entry(house).or_default()
    }

    pub(crate) fn iter(&self) -> std::collections::btree_map::Iter<'_, InternedId, PowerState> {
        self.by_house.iter()
    }

    #[cfg(feature = "debug-tools")]
    pub(crate) fn iter_mut(
        &mut self,
    ) -> std::collections::btree_map::IterMut<'_, InternedId, PowerState> {
        self.by_house.iter_mut()
    }

    /// Read view for the queries that take the House-keyed map.
    pub(crate) fn as_map(&self) -> &BTreeMap<InternedId, PowerState> {
        &self.by_house
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn map_mut_for_test(&mut self) -> &mut BTreeMap<InternedId, PowerState> {
        &mut self.by_house
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_map_for_test(by_house: BTreeMap<InternedId, PowerState>) -> Self {
        Self { by_house }
    }
}

impl std::ops::Index<&InternedId> for PowerStates {
    type Output = PowerState;

    fn index(&self, house: &InternedId) -> &PowerState {
        &self.by_house[house]
    }
}

/// `HouseClass @ 0x004FCE30`: the house's power ratio, left on the x87 stack.
/// Output at least drain, or no drain, gives 1.0; no output gives 0.0;
/// otherwise output / drain (FILD / FIDIV).
/// Original4FCE33/39 reads the same retained House+53A4/+53A8 values as
/// the IHouse getters above. The guarded signed-i32 divide uses the shared
/// PC53/chop owner; neither the query nor its boolean reduction reassesses
/// or clamps totals. `Is_Powered4F6910` is a separate signed-comparison query,
/// not the ratio's `not<1` reduction in `PowerState::has_full_power`.
pub(crate) fn native_power_ratio(output: i32, drain: i32) -> MaskedX87Value {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};

    if output >= drain || drain == 0 {
        X87::load_f64(NativeF64Bits::ONE)
    } else if output == 0 {
        X87::load_f64(NativeF64Bits::POSITIVE_ZERO)
    } else {
        X87::div(X87::load_i32(output), X87::load_i32(drain))
    }
}

/// BuildingType46108A stores NEG(Power) as a signed dword for negative
/// authored values. MIN remains negative and must not pass the positive-drain gate.
pub(crate) fn native_building_power_drain(power: i32) -> i32 {
    if power < 0 { power.wrapping_neg() } else { 0 }
}

/// BuildingClass::GetPowerDrain44E880: both immediate House registration
/// and the later assessment read this same live contribution. Health does
/// not scale drain; a nonzero upgrade count admits all three type slots.
pub(crate) fn building_power_drain(
    entity: &GameEntity,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> i32 {
    if entity.is_warped_out() || !entity.building_online() {
        return 0;
    }
    let Some(object) = interner.object_type(entity.type_ref(), rules) else {
        return 0;
    };
    let mut drain = native_building_power_drain(object.power);
    if entity.gap_generator.supercharged() {
        drain = drain.wrapping_add(object.extra_power.drain());
    }
    let upgrades = entity.building_upgrades();
    if upgrades.count() != 0 {
        for type_ref in upgrades.types().iter().flatten() {
            if let Some(upgrade) = interner.object_type(*type_ref, rules) {
                drain = drain.wrapping_add(native_building_power_drain(upgrade.power));
            }
        }
    }
    drain
}

/// The fields `BuildingClass::Is_Operational_For_Output` (vt+0x350,
/// `0x004555D0`) reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationalFacts {
    /// Building `+0x660` online latch and `+0x67C` Tesla charger count.
    pub online: bool,
    pub tesla_chargers: i32,
    /// `+0x504` EMP time and `+0x6C` Health.
    pub emp_remaining: i32,
    pub health: i32,
    /// Type `+0x1573` Powered and `+0xEE4` drain (the negated negative
    /// `Power=`, [`native_building_power_drain`]).
    pub powered: bool,
    pub power_drain: i32,
    /// Type `+0x1574` PoweredSpecial; the owner's blackout timer
    /// (`House+0x2A4/+0x2AC`) has time left or `House+0x577B` is set.
    pub powered_special: bool,
    pub owner_outage: bool,
    /// Type `+0x1552` NeedsEngineer, Building `+0x6CC` HasEngineer.
    pub needs_engineer: bool,
    pub has_engineer: bool,
    /// vt+0x184: current mission, or queued when current is -1.
    pub effective_mission: i32,
}

/// `BuildingClass::Is_Operational_For_Output @ 0x004555D0`. The house power
/// ratio (`0x004FCE30`, below 1.0) is read only for a Powered building with a
/// positive drain.
pub(crate) fn is_operational_for_output(
    facts: &OperationalFacts,
    power_below_full: impl FnOnce() -> bool,
) -> bool {
    if !facts.online && facts.tesla_chargers < 2 {
        return false;
    }
    if facts.emp_remaining > 0 || facts.health == 0 {
        return false;
    }
    if facts.powered && facts.power_drain > 0 && power_below_full() && facts.tesla_chargers < 2 {
        return false;
    }
    if facts.powered_special && facts.owner_outage {
        return false;
    }
    (!facts.needs_engineer || facts.has_engineer)
        && facts.effective_mission != 0x12
        && facts.effective_mission != 0x13
}

/// Recalculate power totals for a single owner from their buildings.
///
/// Building44E85F..44E86A multiplies native signed-health/live-Strength x87
/// ratio by the signed output contribution and consumes _ftol's low EAX.
/// Ratio-first PC53/chop arithmetic intentionally differs from integer division.
/// Drain is always the full rated `|Power|` regardless of health, plus a
/// super-charged gap generator's negated `ExtraPower=`.
///
/// `UnitAbsorb`/`InfantryAbsorb` buildings (e.g., Yuri Bio-Reactor) add
/// `ExtraPower × OccupantCount` to their pre-scaled output when at least
/// one passenger is garrisoned and `ExtraPower > 0`. The bonus is HP-scaled
/// alongside the base power.
///
/// If spy blackout is active, output is forced to 0 after summation.
fn recalculate_power_for_owner(
    state: &mut PowerState,
    entities: &EntityStore,
    building_order: &[u64],
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
    require_discovery: bool,
) {
    let mut produced: i32 = 0;
    let mut drained: i32 = 0;
    let mut has_drained_power_source = false;
    // House508C8B..508D3E scans the retained House+68 roster, not all stored
    // entities. Destroy/expiry4FBC1C removes a building before an Explodes
    // zero-fuse body necessarily leaves the map. It skips NULL, limbo(+81),
    // unmarked(+74), and (campaign-only508CC7) undiscovered(+41B) entries;
    // health, Alive and death do not gate it.
    for &id in building_order {
        let Some(entity) = entities.get(id) else {
            continue;
        };
        if entity.lifecycle.in_limbo()
            || !entity.lifecycle.cell_marked()
            || (require_discovery && !entity.discovery.discovered_by_current_house())
        {
            continue;
        }
        let Some(obj) = interner.object_type(entity.type_ref(), rules) else {
            continue;
        };

        // A building still building up counts as well. Native
        // AI_AssessPower iterates every live owned building and lets
        // GetPowerOutput/GetPowerDrain decide its contribution; it has no
        // construction-state exclusion.

        // A building being warped out gives and takes nothing
        // (GetPowerOutput `0x0044E7C7`, GetPowerDrain `0x0044E885`), nor
        // does an offline one (Building+660): GetPowerDrain returns 0 before
        // any addend (`0x0044E88F`), GetPowerOutput after summing a positive
        // total (`0x0044E855`; a non-positive total is 0 anyway), and
        // House508D1E's drained-source test repeats that zero output.
        if entity.is_warped_out() || !entity.building_online() {
            continue;
        }

        // Numeric comparison corpus: tools/spatial_oracle/power_health.json.
        // House campaign registration remains a separate lifecycle input.
        // The independent Tesla weapon latch (+661) is not the power-output
        // multiplier at +668.
        // Producer branch: base = max(Power, 0), plus ExtraPower × occupants
        // for InfantryAbsorb/UnitAbsorb buildings (gate is strict on all
        // three conditions, matching gamemd's GetPowerOutput).
        let mut output_contribution: i32 = obj.power.max(0);
        let extra_output = obj.extra_power.output();
        if obj.absorbs_passengers() && extra_output > 0 {
            let occupants = entity.passenger_role.cargo().map_or(0, |c| c.count()) as i32;
            if occupants > 0 {
                output_contribution =
                    output_contribution.wrapping_add(extra_output.wrapping_mul(occupants));
            }
        }
        // Original44E826..44E84D and44E8B5..44E8DA (Steam gamemd):
        // a nonzero count admits all three installed Type slots, including
        // sparse slots beyond the count. Sum before scaling the host's HP;
        // installed upgrades do not own independent power contributors.
        // Native controls: spatial_oracle/building_upgrades.json `power`.
        let upgrades = entity.building_upgrades();
        if upgrades.count() != 0 {
            for type_ref in upgrades.types().iter().flatten() {
                let Some(upgrade) = interner.object_type(*type_ref, rules) else {
                    continue;
                };
                output_contribution = output_contribution.wrapping_add(upgrade.power.max(0));
            }
        }
        if output_contribution > 0 {
            // Original44E85F calls5F5C60, then FIMUL and _ftol; House508CFF
            // adds the signed returned dword with wrapping ADD. No max(1),
            // health clamp or integer product precedes the native division.
            use crate::util::native_x87::MaskedX87Chop53 as X87;
            let output = X87::ftol_i32_low_masked(X87::mul(
                entity.health.ratio(obj.strength),
                X87::load_i32(output_contribution),
            ));
            produced = produced.wrapping_add(output);
            //508D1E calls70FEC0 (DrainingMe != null), then repeats the
            // live GetPowerOutput getter and tests its signed result >0.
            has_drained_power_source |= entity.drain.draining_me().is_some() && output > 0;
        }

        // Drain branch: always full rated value regardless of health.
        // Parser46108A NEG and House508D16 ADD both consume dwords.
        drained = drained.wrapping_add(building_power_drain(entity, rules, interner));
    }

    //508D4A stores the assessment byte;508D79 forces output to zero for
    // either the outage timer or a drained positive-output contributor.
    state.has_drained_power_source = has_drained_power_source;
    if state.blackout_remaining(binary_frame) > 0 || has_drained_power_source {
        produced = 0;
    }

    state.total_output = produced;
    state.total_drain = drained;
    state.is_low_power = !state.retained_full_power();
}

/// Fixture-only assessment of supplied buildings. Production visits each
/// House through Simulation's native House receiver, using its own dirty
/// bytes and ordered building list. Kernel fixtures explicitly invalidate
/// every assessment because they mutate EntityStore without lifecycle calls.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn tick_power_states(
    power_states: &mut PowerStates,
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
) {
    // House state outlives its last contributing building. Revisit retained
    // owners too: an empty contribution set must clear totals and keep the
    // existing blackout/transition clock advancing after Unmark or removal.
    let mut owners: Vec<InternedId> = power_states.as_map().keys().copied().collect();
    owners.extend(
        entities
            .values()
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(GameEntity::owner),
    );
    owners.sort_unstable();
    owners.dedup();

    for &owner_id in &owners {
        let state = power_states.house_mut(owner_id);
        state.invalidate(true);
        let buildings: Vec<u64> = entities
            .values()
            .filter(|entity| {
                entity.owner() == owner_id && entity.category() == EntityCategory::Structure
            })
            .map(|entity| entity.stable_id())
            .collect();
        // This raw-store fixture has no House/session discovery policy.
        assess_house_power(
            state,
            entities,
            &buildings,
            rules,
            interner,
            binary_frame,
            false,
        );
        assess_house_radar_projection(
            state,
            entities,
            &buildings,
            rules,
            owner_id,
            interner,
            false,
            binary_frame,
            false,
        );
    }
}

/// Original House4F844B..4F84EA. A remaining value of exactly one resets
/// the retained timer and invalidates power; zero does neither. The radar
/// outage's timer then does the same and rechecks radar
/// (`0x004F8490..0x004F84D2`). A clean House keeps its previous totals even
/// if an Engineer changed live health. The host runs Factory4CA6E0 only when
/// this reports an assessment.
pub(crate) fn assess_house_power(
    state: &mut PowerState,
    entities: &EntityStore,
    building_order: &[u64],
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
    require_discovery: bool,
) -> bool {
    if state.blackout_remaining(binary_frame) == 1 {
        state.blackout_timer.start(binary_frame as i32, 0);
        state.invalidate(false);
    }
    if state.radar_outage_timer.remaining(binary_frame as i32) == 1 {
        state.radar_outage_timer.start(binary_frame as i32, 0);
        state.radar_dirty = true;
    }
    if !state.power_dirty {
        return false;
    }
    //508C79 clears the byte before scanning, not at a health writer.
    state.power_dirty = false;
    recalculate_power_for_owner(
        state,
        entities,
        building_order,
        rules,
        interner,
        binary_frame,
        require_discovery,
    );
    state.radar_dirty = true;
    true
}

/// House508DF0's pure availability scan. Rust exposes the same inputs for
/// each viewer owner; native invokes it only for PlayerPtr and writes a
/// client-global flag. The power-blackout timer is not the radar outage
/// [`assess_house_radar_projection`] tests first.
fn radar_provider_available(
    entities: &EntityStore,
    building_order: &[u64],
    state: &PowerState,
    rules: &RuleSet,
    owner: InternedId,
    interner: &crate::sim::intern::StringInterner,
    require_discovery: bool,
) -> bool {
    if state.low_power() {
        return false;
    }
    // Native House+68 order chooses the first otherwise eligible provider;
    // its EMP/warp result terminates, even when a later provider is healthy.
    for &id in building_order {
        let Some(entity) = entities.get(id) else {
            continue;
        };
        if entity.owner() != owner
            || entity.lifecycle.in_limbo()
            || !entity.lifecycle.cell_marked()
            || entity.death.is_dying()
            || !entity.building_online()
            || (require_discovery && !entity.discovery.discovered_by_current_house())
            || entity.mission.current().known() == Some(crate::sim::mission::MissionType::Selling)
            || entity.mission.queued().known() == Some(crate::sim::mission::MissionType::Selling)
            || !interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|obj| obj.radar)
        {
            continue;
        }
        // Building+504 EMP starts zero. VERA has no EMP writer yet; its
        // online latch has independent map/trigger and Temporal writers.
        // EMP and the independent HasPower transitions remain open.
        return !entity.is_warped_out();
    }
    false
}

/// House508DF0: a running radar outage (`0x00508E07..0x00508E31`, any
/// remaining time) denies radar before Scenario FreeRadar (`+0x34A4`), power
/// and the providers are asked. Returns whether this visit assessed radar,
/// so the local House can reach the process setter at the same boundary.
pub(crate) fn assess_house_radar_projection(
    state: &mut PowerState,
    entities: &EntityStore,
    building_order: &[u64],
    rules: &RuleSet,
    owner: InternedId,
    interner: &crate::sim::intern::StringInterner,
    free_radar: bool,
    binary_frame: u32,
    require_discovery: bool,
) -> bool {
    if state.radar_dirty {
        state.radar_dirty = false;
        state.radar_available = state.radar_outage_timer.remaining(binary_frame as i32) == 0
            && (free_radar
                || radar_provider_available(
                    entities,
                    building_order,
                    state,
                    rules,
                    owner,
                    interner,
                    require_discovery,
                ));
        return true;
    }
    false
}

/// Check whether a specific building is functionally active (not disabled by low power).
///
/// Returns `false` if the owner is in low power AND the building has `Powered=yes`
/// AND its derived native signed drain is positive. A wrapped MIN drain
/// does not pass this gate, though accounting still adds that signed value.
pub fn is_building_powered(
    power_states: &BTreeMap<InternedId, PowerState>,
    rules: &RuleSet,
    entity: &GameEntity,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    if entity.category() != EntityCategory::Structure {
        return true;
    }
    let Some(obj) = interner.object_type(entity.type_ref(), rules) else {
        return true;
    };
    //4555D0 gates the ratio read on strictly positive native Type+EE4.
    if native_building_power_drain(obj.power) <= 0 {
        return true;
    }
    // Non-Powered buildings are never deactivated.
    if !obj.powered {
        return true;
    }
    power_states
        .get(&entity.owner())
        .is_none_or(PowerState::has_full_power)
}

/// Check if the given owner has at least one active (powered) radar building.
pub fn has_active_radar(
    power_states: &BTreeMap<InternedId, PowerState>,
    owner_id: InternedId,
) -> bool {
    power_states
        .get(&owner_id)
        .is_some_and(|state| state.radar_available)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern;

    #[test]
    fn original_signed_operational_and_drained_generator_assessment() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_power_state.json",
        ))
        .unwrap();
        let rules = rules_from_ini(
            "[BuildingTypes]\n0=B\n1=C\n[B]\nPower=200\nStrength=100\n[C]\nPower=-100\nPowered=yes\nStrength=100\n",
        );
        let consumer = make_building(2, "C", "A", 100);
        let generator = make_building(1, "B", "A", 100);
        let interner = test_interner();
        let owner = generator.owner();
        assert_eq!(corpus["predicate_rows"].as_array().unwrap().len(), 121);
        for row in corpus["predicate_rows"].as_array().unwrap() {
            let state = PowerState {
                total_output: row["output"].as_i64().unwrap() as i32,
                total_drain: row["drain"].as_i64().unwrap() as i32,
                ..Default::default()
            };
            assert_eq!(
                state.has_full_power(),
                row["operational"].as_bool().unwrap(),
                "{row}"
            );
            let states = BTreeMap::from([(owner, state)]);
            assert_eq!(
                is_building_powered(&states, &rules, &consumer, &interner),
                row["operational"].as_bool().unwrap(),
                "{row}"
            );
        }
        assert_eq!(corpus["assessment_rows"].as_array().unwrap().len(), 32);
        for row in corpus["assessment_rows"].as_array().unwrap() {
            let mut entity = generator.clone();
            entity
                .health
                .set_for_test(row["current"].as_i64().unwrap() as i32);
            entity
                .drain
                .set_draining_me_for_test(row["draining"].as_bool().unwrap().then_some(99));
            entity
                .lifecycle
                .set_in_limbo_for_test(row["limbo"].as_bool().unwrap());
            entity
                .lifecycle
                .set_cell_marked_for_test(row["marked"].as_bool().unwrap());
            let mut entities = EntityStore::default();
            entities.insert(entity);
            let mut states =
                PowerStates::from_map_for_test(BTreeMap::from([(owner, PowerState::default())]));
            tick_power_states(&mut states, &mut entities, &rules, &interner, 0);
            assert_eq!(
                states[&owner].total_output,
                row["output"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                states[&owner].has_drained_power_source,
                row["special_outage"].as_bool().unwrap(),
                "{row}"
            );
            // The next real assessment clears the retained flag on detach.
            entities
                .get_mut(1)
                .unwrap()
                .drain
                .set_draining_me_for_test(None);
            tick_power_states(&mut states, &mut entities, &rules, &interner, 0);
            assert!(!states[&owner].has_drained_power_source);
        }
    }

    /// The radar outage (`House+0x2B0`) against
    /// `tools/superweapon_oracle.json` `radar_outage` (66 timer and frame
    /// pairs each): `HouseClass::Update`'s expiry (`0x004F8490..0x004F84D9`)
    /// restarts a timer with exactly one frame left at zero length and
    /// rechecks radar, and `0x00508DF0` denies radar while any time remains,
    /// Scenario FreeRadar set.
    #[test]
    fn radar_outage_matches_native_rows() {
        use crate::sim::timer::CdTimer;
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json"))
                .unwrap();
        let int = |value: &serde_json::Value| i32::try_from(value.as_i64().unwrap()).unwrap();
        let rules = rules_from_ini("[BuildingTypes]\n");
        let mut interner = test_interner();
        let owner = interner.intern("A");
        let entities = EntityStore::default();
        let expiry = oracle["radar_outage"]["expiry"].as_array().unwrap();
        assert_eq!(expiry.len(), 66);
        for row in expiry {
            let mut state = PowerState {
                power_dirty: false,
                radar_dirty: false,
                ..PowerState::default()
            };
            state.set_radar_outage_for_test(CdTimer::from_raw(int(&row[0]), int(&row[1])));
            let frame = int(&row[2]) as u32;
            let _ = assess_house_power(&mut state, &entities, &[], &rules, &interner, frame, false);
            assert_eq!(
                (state.radar_outage_timer, state.radar_dirty),
                (CdTimer::from_raw(int(&row[3]), int(&row[4])), row[5] == 1),
                "{row}"
            );
        }
        let availability = oracle["radar_outage"]["availability"].as_array().unwrap();
        assert_eq!(availability.len(), 66);
        for row in availability {
            let mut state = PowerState::default();
            state.set_radar_outage_for_test(CdTimer::from_raw(int(&row[0]), int(&row[1])));
            let frame = int(&row[2]) as u32;
            assess_house_radar_projection(
                &mut state,
                &entities,
                &[],
                &rules,
                owner,
                &interner,
                true,
                frame,
                false,
            );
            assert_eq!(state.radar_available, row[3] == 1, "{row}");
        }
    }

    fn test_interner() -> intern::StringInterner {
        intern::test_interner()
    }

    /// Build a minimal RuleSet with the given INI text.
    fn rules_from_ini(text: &str) -> RuleSet {
        let ini = IniFile::from_str(text);
        RuleSet::from_ini(&ini).expect("test rules")
    }

    fn make_building(id: u64, type_ref: &str, owner: &str, hp: i32) -> GameEntity {
        let mut e = GameEntity::test_default_of_category(
            id,
            type_ref,
            owner,
            10,
            10,
            EntityCategory::Structure,
        );
        e.health = Health::new(hp);
        e.lifecycle.set_in_limbo_for_test(false);
        e.lifecycle.set_cell_marked_for_test(true);
        e
    }

    fn test_rules() -> RuleSet {
        rules_from_ini(
            "\
[BuildingTypes]
0=GAPOWR
1=NAPOWR
2=TESLA
3=GAPILE

[GAPOWR]
Power=200
Strength=600
Powered=no

[NAPOWR]
Power=150
Strength=400
Powered=no

[TESLA]
Power=-75
Strength=400
Powered=yes

[GAPILE]
Power=-10
Strength=500
Powered=yes

[General]
DamageDelay=1.0
SpyPowerBlackout=1000
MinLowPowerProductionSpeed=0.5
MaxLowPowerProductionSpeed=0.8
LowPowerPenaltyModifier=1.25
BuildSpeed=0.02
",
        )
    }

    #[test]
    fn original_signed_health_power_corpus_through_owner_recalculation() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/power_health.json",
        ))
        .unwrap();
        assert_eq!(corpus["rows"].as_array().unwrap().len(), 429);
        let mut compared = 0;
        for row in corpus["rows"].as_array().unwrap() {
            let input = &row["input"];
            // These native branches have no equivalent Rust lifecycle producers
            // yet. The native corpus records them but cannot prove them migrated.
            if input["warped"].as_bool().unwrap() || input["overpowered"].as_bool().unwrap() {
                continue;
            }
            let value = |key: &str| input[key].as_i64().unwrap() as i32;
            let rules = rules_from_ini(&format!(
                "[BuildingTypes]\n0=TEST\n1=PRIOR\n2=DRAINMAX\n3=DRAINMIN\n\
                 [TEST]\nStrength={}\nPower={}\nExtraPower={}\nUnitAbsorb={}\nInfantryAbsorb={}\n\
                 [PRIOR]\nStrength=1\nPower=1\n[DRAINMAX]\nStrength=1\nPower=-2147483647\n\
                 [DRAINMIN]\nStrength=1\nPower=-2147483648\n",
                value("strength"),
                value("power"),
                value("extra"),
                input["unit_absorb"].as_bool().unwrap(),
                input["infantry_absorb"].as_bool().unwrap()
            ));
            let mut store = EntityStore::new();
            let mut building = make_building(1, "TEST", "Owner", value("actual"));
            building.set_building_online_for_test(input["online"].as_bool().unwrap());
            let occupants = value("occupants") as u32;
            let mut cargo = crate::sim::passenger::PassengerCargo::new(occupants, 0.0);
            cargo.set_passengers_for_test((0..occupants).map(|i| 100 + u64::from(i)).collect());
            cargo.set_passenger_sizes_for_test(vec![1.0; occupants as usize]);
            building.passenger_role = crate::sim::passenger::PassengerRole::transport(cargo);
            store.insert(building);
            // Supply existing aggregate output with a signed-HP Power=1 source.
            store.insert(make_building(2, "PRIOR", "Owner", value("prior_output")));
            match value("prior_drain") {
                0 => {}
                i32::MAX => {
                    store.insert(make_building(3, "DRAINMAX", "Owner", 1));
                }
                i32::MIN => {
                    store.insert(make_building(3, "DRAINMIN", "Owner", 1));
                }
                -1 => {
                    store.insert(make_building(3, "DRAINMAX", "Owner", 1));
                    store.insert(make_building(4, "DRAINMIN", "Owner", 1));
                }
                other => panic!("unrepresented fixture prior drain {other}"),
            }
            let owner = intern::test_intern("Owner");
            let interner = test_interner();
            let mut states = PowerStates::default();
            tick_power_states(&mut states, &mut store, &rules, &interner, 0);
            let state = &states[&owner];
            assert_eq!(
                state.total_output,
                row["output"]["total_output"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                state.total_drain,
                row["output"]["total_drain"].as_i64().unwrap() as i32,
                "{row}"
            );
            compared += 1;
        }
        assert_eq!(compared, 423);
    }

    #[test]
    fn native_installed_upgrade_power_scales_once_with_host_health() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/building_upgrades.json");
        let rows = corpus["power"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let input = &row["inputs"];
            let value = |key: &str| i32::try_from(input[key].as_i64().unwrap()).unwrap();
            // The oracle seeds native +EE0/+EE4 separately. These controls
            // use mutually exclusive output/drain so the production Rules
            // reader can express exactly the same BuildingType with Power=.
            let power = |output: &str, drain: &str| {
                let (output, drain) = (value(output), value(drain));
                assert!(output == 0 || drain == 0);
                if drain == 0 { output } else { -drain }
            };
            let rules = rules_from_ini(&format!(
                "[BuildingTypes]\n0=HOST\n1=UPGRADE\n2=OTHER\n\
                 [HOST]\nStrength={}\nPower={}\nUpgrades=3\n\
                 [UPGRADE]\nPower={}\nPowersUpBuilding=HOST\n\
                 [OTHER]\nPower=70\nPowersUpBuilding=HOST\n",
                value("strength"),
                power("host_power", "host_drain"),
                power("upgrade_power", "upgrade_drain"),
            ));
            let mut host = make_building(1, "HOST", "Owner", value("hp"));
            host.set_building_upgrades_for_test(
                i8::try_from(value("count")).unwrap(),
                [
                    Some(intern::test_intern("UPGRADE")),
                    None,
                    input["other_slot"]
                        .as_bool()
                        .unwrap()
                        .then(|| intern::test_intern("OTHER")),
                ],
            );
            let mut entities = EntityStore::new();
            entities.insert(host);
            let mut state = PowerState::default();
            recalculate_power_for_owner(
                &mut state,
                &entities,
                &[1],
                &rules,
                &test_interner(),
                0,
                false,
            );
            assert_eq!(
                (state.total_output(), state.total_drain()),
                (
                    row["output"].as_i64().unwrap() as i32,
                    row["drain"].as_i64().unwrap() as i32,
                ),
                "{}",
                row["name"]
            );
        }
    }

    #[test]
    fn native_power_admission_uses_cell_mark_not_object_alive_or_signed_health() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/power_health.json",
        ))
        .unwrap();
        let rules = test_rules();
        for row in corpus["admission_rows"].as_array().unwrap() {
            let input = &row["input"];
            let mut store = EntityStore::new();
            let mut plant = make_building(1, "GAPOWR", "Allies", 600);
            plant
                .lifecycle
                .set_cell_marked_for_test(input["marked"].as_bool().unwrap());
            plant
                .lifecycle
                .set_object_alive_for_test(input["alive"].as_bool().unwrap());
            plant
                .lifecycle
                .set_in_limbo_for_test(input["limbo"].as_bool().unwrap());
            store.insert(plant);
            let mut state = PowerState::default();
            recalculate_power_for_owner(
                &mut state,
                &store,
                &[1],
                &rules,
                &test_interner(),
                0,
                false,
            );
            assert_eq!(
                state.total_output,
                if row["admitted"].as_bool().unwrap() {
                    200
                } else {
                    0
                },
                "{row}"
            );
        }
    }

    #[test]
    fn native_empty_house_scan_clears_retained_power_totals() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/power_health.json",
        ))
        .unwrap();
        let rules = test_rules();
        let owner = intern::test_intern("Allies");
        for row in corpus["empty_contribution_rows"].as_array().unwrap() {
            let mut store = EntityStore::new();
            let input = &row["input"];
            match input["member"].as_str().unwrap() {
                "unmarked" | "limbo" => {
                    let mut plant = make_building(1, "GAPOWR", "Allies", 600);
                    plant
                        .lifecycle
                        .set_in_limbo_for_test(input["member"] == "limbo");
                    plant
                        .lifecycle
                        .set_cell_marked_for_test(input["member"] == "limbo");
                    store.insert(plant);
                }
                "empty" | "null" => {}
                other => panic!("unexpected native member fixture {other}"),
            }
            let state = PowerState {
                total_output: input["prior_output"].as_i64().unwrap() as i32,
                total_drain: input["prior_drain"].as_i64().unwrap() as i32,
                ..Default::default()
            };
            let mut states = PowerStates::from_map_for_test(BTreeMap::from([(owner, state)]));
            tick_power_states(&mut states, &mut store, &rules, &test_interner(), 0);
            assert_eq!(
                (states[&owner].total_output, states[&owner].total_drain),
                (
                    row["output"]["total_output"].as_i64().unwrap() as i32,
                    row["output"]["total_drain"].as_i64().unwrap() as i32,
                ),
                "{row}",
            );
        }
    }

    #[test]
    fn test_spy_blackout_timer_resets_at_native_remaining_one() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        store.insert(make_building(1, "GAPOWR", "Allies", 600));

        let interner = test_interner();
        let allies = intern::test_intern("Allies");
        let mut states = PowerStates::default();
        states.house_mut(allies).start_blackout(0, 5);

        // House4F844B..4F84EA resets at exactly one remaining frame.
        // Assessments before that boundary preserve the original start/delay.
        for frame in 0..4 {
            tick_power_states(&mut states, &mut store, &rules, &interner, frame);
            assert_eq!(states[&allies].blackout_remaining(frame), 5 - frame as i32);
            assert_eq!(states[&allies].total_output, 0);
        }

        assert_eq!(states[&allies].blackout_remaining(4), 1);
        tick_power_states(&mut states, &mut store, &rules, &interner, 4);
        let state = states.get(&allies).expect("state should exist");
        assert_eq!(state.blackout_remaining(4), 0, "House resets the timer");
        assert_eq!(
            state.total_output, 200,
            "the reset reassesses native output"
        );
        assert!(!state.is_low_power, "power restored after blackout");
    }

    // -------- ExtraPower (Yuri Bio-Reactor) bonus tests -----------------
}
