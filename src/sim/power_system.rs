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
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;

/// Per-player power state, updated each simulation tick.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PowerState {
    /// Sum of health-scaled positive `Power=` values for all owned buildings.
    pub total_output: i32,
    /// Sum of absolute negative `Power=` values (always full rated, regardless of health).
    pub total_drain: i32,
    /// House+577B: an admitted positive-output Building has DrainingMe.
    #[serde(default)]
    pub has_drained_power_source: bool,
    /// Native signed House power ratio is below one.
    pub is_low_power: bool,
    /// Remaining power-blackout frames. While > 0, `total_output` is forced to 0.
    /// Set by spy infiltration of power plants AND by ForceShield superweapon launch.
    #[serde(rename = "spy_blackout_remaining")]
    pub power_blackout_remaining: u32,
    /// Whether the player was in low-power state on the previous tick.
    /// Used to detect transitions for EVA voice events.
    pub was_low_power: bool,
    /// Sum of absolute `|Power=|` values from TypeClass for ALL owned buildings,
    /// regardless of health, construction state, or online status. Used by the
    /// sidebar power bar fill curve (asymptotic: `400 / (total + 400)`).
    pub theoretical_total_power: i32,
}

impl PowerState {
    /// House4FCE30 and508D99..DC9: produced>=drained or drained==0 gives1;
    /// otherwise signed produced/drained is compared with1. With finite i32
    /// operands, a negative denominator reverses the strict less-than result.
    pub(crate) fn has_full_power(&self) -> bool {
        self.total_output >= self.total_drain || self.total_drain <= 0
    }
}

/// BuildingType46108A stores NEG(Power) as a signed dword for negative
/// authored values. MIN remains negative and must not pass the positive-drain gate.
pub(crate) fn native_building_power_drain(power: i32) -> i32 {
    if power < 0 { power.wrapping_neg() } else { 0 }
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

/// Events emitted when a player's power state transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerEvent {
    /// Player entered low-power state (drain now exceeds output).
    EnteredLowPower { owner: InternedId },
    /// Player's power was restored after a deficit.
    PowerRestored { owner: InternedId },
}

/// House508CB0/508CB8 tests limbo(+81) and cell-marked(+74), not ObjectAlive
/// (+90) or actual HP. Rust tombstones remain excluded until physical removal.
fn participates_in_power_accounting(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Structure
        && !entity.dying
        && !entity.lifecycle.in_limbo
        && entity.lifecycle.cell_marked
}

/// Recalculate power totals for a single owner from their buildings.
///
/// Building44E85F..44E86A multiplies native signed-health/live-Strength x87
/// ratio by the signed output contribution and consumes _ftol's low EAX.
/// Ratio-first PC53/chop arithmetic intentionally differs from integer division.
/// Drain is always the full rated `|Power|` regardless of health.
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
    rules: &RuleSet,
    owner_id: InternedId,
    interner: &crate::sim::intern::StringInterner,
) {
    let mut produced: i32 = 0;
    let mut drained: i32 = 0;
    let mut has_drained_power_source = false;
    // Theoretical total: sum of |Power=| from TypeClass for ALL buildings,
    // including those under construction. Used by the power bar fill curve.
    // Does NOT include the ExtraPower garrison bonus.
    let mut theoretical: i32 = 0;

    for entity in entities.values() {
        if !participates_in_power_accounting(entity) || entity.owner() != owner_id {
            continue;
        }
        let Some(obj) = rules.object(interner.resolve(entity.type_ref())) else {
            continue;
        };

        // Theoretical total includes ALL buildings regardless of state.
        // This derived sidebar capacity retains its dword storage contract.
        theoretical = theoretical.wrapping_add(obj.power.wrapping_abs());

        // BuildingUp is presentation state, not lifecycle authority. Native
        // AI_AssessPower iterates every live owned building and lets
        // GetPowerOutput/GetPowerDrain decide its contribution; it has no
        // construction-state exclusion.

        // A building being warped out gives and takes nothing
        // (GetPowerOutput `0x0044E7C7`, GetPowerDrain `0x0044E885`). The
        // drain's online-latch test (`0x0044E88F`) adds nothing while the
        // warp is the latch's only represented writer.
        if entity.is_warped_out() {
            continue;
        }

        // Numeric comparison corpus: tools/spatial_oracle/power_health.json.
        // Still-open native lifecycle inputs: overpowered, attached upgrade
        // slots and House campaign registration.
        // Producer branch: base = max(Power, 0), plus ExtraPower × occupants
        // for InfantryAbsorb/UnitAbsorb buildings (gate is strict on all
        // three conditions, matching gamemd's GetPowerOutput).
        let mut output_contribution: i32 = obj.power.max(0);
        if (obj.infantry_absorb || obj.unit_absorb) && obj.extra_power > 0 {
            let occupants = entity.passenger_role.cargo().map_or(0, |c| c.count()) as i32;
            if occupants > 0 {
                output_contribution =
                    output_contribution.wrapping_add(obj.extra_power.wrapping_mul(occupants));
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
            has_drained_power_source |= entity.draining_me.is_some() && output > 0;
        }

        // Drain branch: always full rated value regardless of health.
        // Parser46108A NEG and House508D16 ADD both consume dwords.
        drained = drained.wrapping_add(native_building_power_drain(obj.power));
    }

    //508D4A stores the assessment byte;508D79 forces output to zero for
    // either the outage timer or a drained positive-output contributor.
    state.has_drained_power_source = has_drained_power_source;
    if state.power_blackout_remaining > 0 || has_drained_power_source {
        produced = 0;
    }

    state.total_output = produced;
    state.total_drain = drained;
    state.is_low_power = !state.has_full_power();
    state.theoretical_total_power = theoretical;
}

/// Main per-native-frame power system entry point.
///
/// For each retained power owner or player with contributing structures: recalculates totals, decrements
/// spy blackout timer, and returns transition events for EVA voice lines.
///
/// gamemd has no HP-degradation effect during low power — `Powered=yes`
/// buildings simply become inoperational (`is_building_powered` returns false)
/// without taking damage. The DamageDelay timer fields at HouseClass+0x578C/
/// +0x5794 are written in the constructor and never read.
pub fn tick_power_states(
    power_states: &mut BTreeMap<InternedId, PowerState>,
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Vec<PowerEvent> {
    // House state outlives its last contributing building. Revisit retained
    // owners too: an empty contribution set must clear totals and keep the
    // existing blackout/transition clock advancing after Unmark or removal.
    let mut owners: Vec<InternedId> = power_states.keys().copied().collect();
    owners.extend(
        entities
            .values()
            .filter(|entity| participates_in_power_accounting(entity))
            .map(GameEntity::owner),
    );
    owners.sort_unstable();
    owners.dedup();

    let mut events: Vec<PowerEvent> = Vec::new();

    for &owner_id in &owners {
        let state = power_states.entry(owner_id).or_default();

        // Save previous state for transition detection.
        state.was_low_power = state.is_low_power;

        // Decrement the spy blackout timer once per reached native frame.
        if state.power_blackout_remaining > 0 {
            state.power_blackout_remaining = state.power_blackout_remaining.saturating_sub(1);
        }

        // Recalculate power totals with health scaling.
        recalculate_power_for_owner(state, entities, rules, owner_id, interner);

        // Detect transitions.
        if state.is_low_power && !state.was_low_power {
            events.push(PowerEvent::EnteredLowPower { owner: owner_id });
        } else if !state.is_low_power && state.was_low_power {
            events.push(PowerEvent::PowerRestored { owner: owner_id });
        }
    }

    events
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
    if entity.category != EntityCategory::Structure {
        return true;
    }
    let Some(obj) = rules.object(interner.resolve(entity.type_ref())) else {
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

/// Trigger a spy-infiltration power blackout for the target owner.
///
/// Sets `power_blackout_remaining` to the configured duration from `[General]`.
/// While active, the owner's power output is forced to 0.
#[cfg(test)]
pub fn trigger_spy_blackout(
    power_states: &mut BTreeMap<InternedId, PowerState>,
    owner_id: InternedId,
    duration_frames: u32,
) {
    let state = power_states.entry(owner_id).or_default();
    state.power_blackout_remaining = duration_frames;
}

/// Check if the given owner has at least one active (powered) radar building.
pub fn has_active_radar(
    entities: &EntityStore,
    power_states: &BTreeMap<InternedId, PowerState>,
    rules: &RuleSet,
    owner_id: InternedId,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    // Native UpdateTacticalRadarAvailability gates the house on its aggregate
    // power ratio before scanning Radar= providers. Stock GAAIRC and AMRADR
    // omit Powered=, so this cannot be a per-building Powered gate.
    if power_states
        .get(&owner_id)
        .is_some_and(|state| state.is_low_power)
    {
        return false;
    }

    entities.values().any(|e| {
        !e.dying
            && !e.lifecycle.in_limbo
            // `0x00508EA3`: an offline building (the warp's latch) is no
            // radar provider; the scan moves on to the next.
            && e.building_online()
            && e.category == EntityCategory::Structure
            && e.owner() == owner_id
            && rules
                .object(interner.resolve(e.type_ref()))
                .is_some_and(|obj| obj.radar)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern;

    #[test]
    fn original_signed_operational_and_drained_generator_assessment() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_power_state.json"
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
            entity.health.current = row["current"].as_i64().unwrap() as i32;
            entity.draining_me = row["draining"].as_bool().unwrap().then_some(99);
            entity.lifecycle.in_limbo = row["limbo"].as_bool().unwrap();
            entity.lifecycle.cell_marked = row["marked"].as_bool().unwrap();
            let mut entities = EntityStore::default();
            entities.insert(entity);
            let mut states = BTreeMap::from([(owner, PowerState::default())]);
            tick_power_states(&mut states, &mut entities, &rules, &interner);
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
            entities.get_mut(1).unwrap().draining_me = None;
            tick_power_states(&mut states, &mut entities, &rules, &interner);
            assert!(!states[&owner].has_drained_power_source);
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
        let mut e = GameEntity::test_default(id, type_ref, owner, 10, 10);
        e.category = EntityCategory::Structure;
        e.health = Health { current: hp };
        e.lifecycle.in_limbo = false;
        e.lifecycle.cell_marked = true;
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
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/spatial_oracle/power_health.json"))
                .unwrap();
        assert_eq!(corpus["rows"].as_array().unwrap().len(), 429);
        let mut compared = 0;
        for row in corpus["rows"].as_array().unwrap() {
            let input = &row["input"];
            // These native branches have no equivalent Rust lifecycle producers
            // yet. The native corpus records them but cannot prove them migrated.
            if !input["online"].as_bool().unwrap()
                || input["warped"].as_bool().unwrap()
                || input["overpowered"].as_bool().unwrap()
            {
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
            let occupants = value("occupants") as u32;
            let mut cargo = crate::sim::passenger::PassengerCargo::new(occupants, 0);
            cargo.passengers = (0..occupants).map(|i| 100 + u64::from(i)).collect();
            cargo.passenger_sizes = vec![1; occupants as usize];
            cargo.total_size = occupants;
            building.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
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
            let mut states = BTreeMap::new();
            tick_power_states(&mut states, &mut store, &rules, &interner);
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
        assert_eq!(compared, 422);
    }

    #[test]
    fn native_power_admission_uses_cell_mark_not_object_alive_or_signed_health() {
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/spatial_oracle/power_health.json"))
                .unwrap();
        let rules = test_rules();
        for row in corpus["admission_rows"].as_array().unwrap() {
            let input = &row["input"];
            let mut store = EntityStore::new();
            let mut plant = make_building(1, "GAPOWR", "Allies", 600);
            plant.lifecycle.cell_marked = input["marked"].as_bool().unwrap();
            plant.lifecycle.object_alive = input["alive"].as_bool().unwrap();
            plant.lifecycle.in_limbo = input["limbo"].as_bool().unwrap();
            store.insert(plant);
            let owner = intern::test_intern("Allies");
            let mut state = PowerState::default();
            recalculate_power_for_owner(&mut state, &store, &rules, owner, &test_interner());
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
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/spatial_oracle/power_health.json"))
                .unwrap();
        let rules = test_rules();
        let owner = intern::test_intern("Allies");
        for row in corpus["empty_contribution_rows"].as_array().unwrap() {
            let mut store = EntityStore::new();
            let input = &row["input"];
            match input["member"].as_str().unwrap() {
                "unmarked" | "limbo" => {
                    let mut plant = make_building(1, "GAPOWR", "Allies", 600);
                    plant.lifecycle.in_limbo = input["member"] == "limbo";
                    plant.lifecycle.cell_marked = input["member"] == "limbo";
                    store.insert(plant);
                }
                "empty" | "null" => {}
                other => panic!("unexpected native member fixture {other}"),
            }
            let mut states = BTreeMap::from([(
                owner,
                PowerState {
                    total_output: input["prior_output"].as_i64().unwrap() as i32,
                    total_drain: input["prior_drain"].as_i64().unwrap() as i32,
                    ..Default::default()
                },
            )]);
            tick_power_states(&mut states, &mut store, &rules, &test_interner());
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
    fn power_owner_survives_last_contribution_removal_and_ticks_blackout() {
        let rules = test_rules();
        let plant = make_building(1, "GAPOWR", "Allies", 600);
        let drain = make_building(2, "GAPILE", "Allies", 500);
        let owner = plant.owner();
        let interner = test_interner();
        let mut store = EntityStore::new();
        let mut states = BTreeMap::new();
        store.insert(plant);
        tick_power_states(&mut states, &mut store, &rules, &interner);
        assert_eq!(states[&owner].total_output, 200);
        store.get_mut(1).unwrap().lifecycle.cell_marked = false;
        tick_power_states(&mut states, &mut store, &rules, &interner);
        assert_eq!(states[&owner].total_output, 0);
        assert_eq!(states[&owner].theoretical_total_power, 0);

        store.insert(drain);
        assert_eq!(
            tick_power_states(&mut states, &mut store, &rules, &interner),
            vec![PowerEvent::EnteredLowPower { owner }],
        );
        trigger_spy_blackout(&mut states, owner, 2);
        store.remove(2);
        assert_eq!(
            tick_power_states(&mut states, &mut store, &rules, &interner),
            vec![PowerEvent::PowerRestored { owner }],
        );
        assert_eq!(states[&owner].total_drain, 0);
        assert_eq!(states[&owner].power_blackout_remaining, 1);
        assert!(tick_power_states(&mut states, &mut store, &rules, &interner).is_empty());
        assert_eq!(states[&owner].power_blackout_remaining, 0);
    }

    #[test]
    fn test_health_scaled_output() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        // Power plant at 50% HP should produce 50% output.
        store.insert(make_building(1, "GAPOWR", "Allies", 300));
        // Barracks at any health always drains full amount.
        store.insert(make_building(2, "GAPILE", "Allies", 50));

        let mut state = PowerState::default();
        let interner = test_interner();
        let allies = intern::test_intern("Allies");
        recalculate_power_for_owner(&mut state, &store, &rules, allies, &interner);

        assert_eq!(state.total_output, 100, "200 * 300/600 = 100");
        assert_eq!(state.total_drain, 10, "|-10| = 10");
        assert!(!state.is_low_power, "100 >= 10");
    }

    #[test]
    fn test_full_health_full_output() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        store.insert(make_building(1, "GAPOWR", "Allies", 600));

        let mut state = PowerState::default();
        let interner = test_interner();
        let allies = intern::test_intern("Allies");
        recalculate_power_for_owner(&mut state, &store, &rules, allies, &interner);

        assert_eq!(state.total_output, 200);
        assert_eq!(state.total_drain, 0);
        assert!(!state.is_low_power);
    }

    #[test]
    fn test_low_power_detection() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        // Small power plant at low health.
        store.insert(make_building(1, "NAPOWR", "Soviet", 40)); // native ratio-first PC53/chop returns14
        // Tesla Coil drains 75.
        store.insert(make_building(2, "TESLA", "Soviet", 400));

        let mut state = PowerState::default();
        let interner = test_interner();
        let soviet = intern::test_intern("Soviet");
        recalculate_power_for_owner(&mut state, &store, &rules, soviet, &interner);

        assert_eq!(
            state.total_output, 14,
            "native44E866 ratio-first multiply truncates to14"
        );
        assert_eq!(state.total_drain, 75);
        assert!(state.is_low_power, "14 < 75");
    }

    #[test]
    fn test_drain_always_full_regardless_of_health() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        // Tesla Coil at 1 HP still drains full 75.
        store.insert(make_building(1, "TESLA", "Soviet", 1));

        let mut state = PowerState::default();
        let interner = test_interner();
        let soviet = intern::test_intern("Soviet");
        recalculate_power_for_owner(&mut state, &store, &rules, soviet, &interner);

        assert_eq!(state.total_drain, 75, "drain is always full rated value");
    }

    #[test]
    fn test_spy_blackout_forces_zero_output() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        store.insert(make_building(1, "GAPOWR", "Allies", 600));
        store.insert(make_building(2, "GAPILE", "Allies", 500));

        let mut state = PowerState::default();
        state.power_blackout_remaining = 100;
        let interner = test_interner();
        let allies = intern::test_intern("Allies");
        recalculate_power_for_owner(&mut state, &store, &rules, allies, &interner);

        assert_eq!(state.total_output, 0, "blackout forces output to 0");
        assert_eq!(state.total_drain, 10);
        assert!(state.is_low_power, "0 < 10 during blackout");
    }

    #[test]
    fn test_spy_blackout_timer_decrements() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        store.insert(make_building(1, "GAPOWR", "Allies", 600));

        let interner = test_interner();
        let allies = intern::test_intern("Allies");
        let mut states: BTreeMap<InternedId, PowerState> = BTreeMap::new();
        trigger_spy_blackout(&mut states, allies, 5);

        // Tick 5 times — each tick decrements by 1.
        for _ in 0..5 {
            tick_power_states(&mut states, &mut store, &rules, &interner);
        }

        let state = states.get(&allies).expect("state should exist");
        assert_eq!(state.power_blackout_remaining, 0, "timer should reach 0");
        assert!(!state.is_low_power, "power restored after blackout");
    }

    #[test]
    fn test_power_transition_events() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        // Start with just a tesla coil (drain=75, output=0) → immediate low power.
        store.insert(make_building(1, "TESLA", "Soviet", 400));

        // Pre-intern all strings that will be used (including NAPOWR for the second
        // building added later) so the interner clone has everything.
        let soviet = intern::test_intern("Soviet");
        intern::test_intern("NAPOWR");
        let interner = test_interner();
        let mut states: BTreeMap<InternedId, PowerState> = BTreeMap::new();

        let events = tick_power_states(&mut states, &mut store, &rules, &interner);
        assert!(
            events.contains(&PowerEvent::EnteredLowPower { owner: soviet }),
            "should detect entering low power"
        );

        // Add a power plant → should restore power.
        store.insert(make_building(2, "NAPOWR", "Soviet", 400));
        let events = tick_power_states(&mut states, &mut store, &rules, &interner);
        assert!(
            events.contains(&PowerEvent::PowerRestored { owner: soviet }),
            "should detect power restored"
        );
    }

    #[test]
    fn test_is_building_powered_for_generator() {
        let rules = test_rules();
        let allies = intern::test_intern("Allies");

        // Power plant (positive Power=) is never deactivated.
        let plant = make_building(1, "GAPOWR", "Allies", 600);

        // Get interner AFTER all strings are interned (make_building interns type_ref).
        let interner = test_interner();
        let mut states: BTreeMap<InternedId, PowerState> = BTreeMap::new();
        states.insert(
            allies,
            PowerState {
                total_drain: 100,
                is_low_power: true,
                ..PowerState::default()
            },
        );

        assert!(
            is_building_powered(&states, &rules, &plant, &interner),
            "generators are never deactivated"
        );
    }

    #[test]
    fn test_is_building_powered_for_consumer_during_low_power() {
        let rules = test_rules();
        let soviet = intern::test_intern("Soviet");
        let tesla = make_building(1, "TESLA", "Soviet", 400);

        // Get interner AFTER all strings are interned.
        let interner = test_interner();
        let mut states: BTreeMap<InternedId, PowerState> = BTreeMap::new();
        states.insert(
            soviet,
            PowerState {
                total_drain: 100,
                is_low_power: true,
                ..PowerState::default()
            },
        );

        assert!(
            !is_building_powered(&states, &rules, &tesla, &interner),
            "Powered=yes consumer deactivated during low power"
        );
    }

    #[test]
    fn test_is_building_powered_for_consumer_during_surplus() {
        let rules = test_rules();
        let soviet = intern::test_intern("Soviet");
        let tesla = make_building(1, "TESLA", "Soviet", 400);

        // Get interner AFTER all strings are interned.
        let interner = test_interner();
        let mut states: BTreeMap<InternedId, PowerState> = BTreeMap::new();
        states.insert(
            soviet,
            PowerState {
                is_low_power: false,
                ..PowerState::default()
            },
        );

        assert!(
            is_building_powered(&states, &rules, &tesla, &interner),
            "consumer active during power surplus"
        );
    }

    #[test]
    fn test_low_power_does_not_damage_buildings() {
        // gamemd does not apply degradation damage during low power — the
        // DamageDelay timer fields at HouseClass+0x578C/+0x5794 are written
        // in the constructor and never read. This test pins the Rust port
        // to that behavior.
        let rules = test_rules();
        let mut store = EntityStore::new();
        // Tesla coil (Powered=yes, Power=-75) with no power plant → sustained low power.
        store.insert(make_building(1, "TESLA", "Soviet", 100));

        let interner = test_interner();
        let mut states: BTreeMap<InternedId, PowerState> = BTreeMap::new();

        // Tick well past any prior degradation threshold.
        for _ in 0..3750 {
            tick_power_states(&mut states, &mut store, &rules, &interner);
        }

        let entity = store.get(1).expect("entity should exist");
        assert_eq!(
            entity.health.current, 100,
            "low power must not damage buildings (gamemd parity)"
        );
    }

    #[test]
    fn test_live_building_under_construction_contributes_power() {
        let rules = test_rules();
        let mut store = EntityStore::new();
        let mut plant = make_building(1, "GAPOWR", "Allies", 600);
        plant.building_up = Some(crate::sim::components::BuildingUp::completing_in_ticks(
            30, 0,
        ));
        store.insert(plant);

        let mut state = PowerState::default();
        let interner = test_interner();
        let allies = intern::test_intern("Allies");
        recalculate_power_for_owner(&mut state, &store, &rules, allies, &interner);

        assert_eq!(
            state.total_output, 200,
            "native power authority includes a live building during buildup"
        );
    }

    #[test]
    fn test_live_buildup_radar_follows_house_power() {
        let rules = rules_from_ini(
            "\
[BuildingTypes]
0=AMRADR
1=GAPOWR

[AMRADR]
Radar=yes
Power=-50
Strength=600

[GAPOWR]
Power=200
Strength=600

[General]
BuildSpeed=0.02
",
        );
        let mut store = EntityStore::new();
        let mut radar = make_building(1, "AMRADR", "Allies", 600);
        radar.building_up = Some(crate::sim::components::BuildingUp::completing_in_ticks(
            29, 0,
        ));
        store.insert(radar);
        store.insert(make_building(2, "GAPOWR", "Allies", 600));

        let interner = test_interner();
        let allies = intern::test_intern("Allies");
        let mut states: BTreeMap<InternedId, PowerState> = BTreeMap::new();
        tick_power_states(&mut states, &mut store, &rules, &interner);

        assert!(
            has_active_radar(&store, &states, &rules, allies, &interner),
            "a live radar provider remains authoritative during its buildup"
        );

        // Remove the power plant: aggregate low power disables the same live
        // buildup provider.
        store.remove(2);
        tick_power_states(&mut states, &mut store, &rules, &interner);

        assert!(
            !has_active_radar(&store, &states, &rules, allies, &interner),
            "house low power must disable radar during provider buildup"
        );
    }

    // -------- ExtraPower (Yuri Bio-Reactor) bonus tests -----------------

    /// Rules with a YAPOWR-shaped Bio-Reactor: power producer +
    /// InfantryAbsorb=yes + ExtraPower=100. Mirrors stock YR rulesmd.ini.
    fn yapowr_rules() -> RuleSet {
        rules_from_ini(
            "\
[BuildingTypes]
0=YAPOWR
1=GAPOWR

[YAPOWR]
Power=150
Strength=750
Powered=no
InfantryAbsorb=yes
UnitAbsorb=no
ExtraPower=100
Passengers=5

[GAPOWR]
Power=200
Strength=600
Powered=no

[General]
BuildSpeed=0.02
",
        )
    }

    /// YAPOWR test entity with `n` passengers and signed actual HP.
    fn make_yapowr(id: u64, owner: &str, hp: i32, passenger_count: u32) -> GameEntity {
        let mut e = make_building(id, "YAPOWR", owner, hp);
        let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0);
        for i in 0..passenger_count {
            cargo.board_forced(100 + i as u64, 1);
        }
        e.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
        e
    }

    #[test]
    fn test_yapowr_empty_no_bonus() {
        let rules = yapowr_rules();
        let mut store = EntityStore::new();
        store.insert(make_yapowr(1, "Yuri", 750, 0));

        let yuri = intern::test_intern("Yuri");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, yuri, &interner);

        assert_eq!(state.total_output, 150, "empty YAPOWR = base Power only");
        assert_eq!(state.total_drain, 0);
    }

    #[test]
    fn test_yapowr_garrisoned_full_hp() {
        let rules = yapowr_rules();
        let mut store = EntityStore::new();
        store.insert(make_yapowr(1, "Yuri", 750, 5));

        let yuri = intern::test_intern("Yuri");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, yuri, &interner);

        assert_eq!(state.total_output, 650, "150 + 100*5 = 650 at full HP");
    }

    #[test]
    fn test_yapowr_garrisoned_half_hp_scales_bonus() {
        let rules = yapowr_rules();
        let mut store = EntityStore::new();
        store.insert(make_yapowr(1, "Yuri", 375, 5));

        let yuri = intern::test_intern("Yuri");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, yuri, &interner);

        // (150 + 500) * 375 / 750 = 650 * 375 / 750 = 325
        assert_eq!(state.total_output, 325, "bonus scales with HP");
    }

    #[test]
    fn test_no_infantry_absorb_no_bonus() {
        // GAPOWR has no InfantryAbsorb/UnitAbsorb. A stray passenger
        // (which the garrison flow would never produce) must NOT grant
        // a bonus — the gate is on the TypeClass flags.
        let rules = yapowr_rules();
        let mut store = EntityStore::new();
        let mut e = make_building(1, "GAPOWR", "Allies", 600);
        let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0);
        cargo.board_forced(100, 1);
        e.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
        store.insert(e);

        let allies = intern::test_intern("Allies");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, allies, &interner);

        assert_eq!(state.total_output, 200, "no InfantryAbsorb = no bonus");
    }

    #[test]
    fn test_extra_power_zero_no_bonus() {
        let rules = rules_from_ini(
            "\
[BuildingTypes]
0=ZEROEX

[ZEROEX]
Power=150
Strength=750
InfantryAbsorb=yes
ExtraPower=0
Passengers=5

[General]
BuildSpeed=0.02
",
        );
        let mut store = EntityStore::new();
        let mut e = make_building(1, "ZEROEX", "Yuri", 750);
        let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0);
        for i in 0..5 {
            cargo.board_forced(100 + i as u64, 1);
        }
        e.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
        store.insert(e);

        let yuri = intern::test_intern("Yuri");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, yuri, &interner);

        assert_eq!(
            state.total_output, 150,
            "ExtraPower=0 fails strict > 0 gate"
        );
    }

    #[test]
    fn test_extra_power_negative_no_bonus() {
        let rules = rules_from_ini(
            "\
[BuildingTypes]
0=NEGEX

[NEGEX]
Power=150
Strength=750
InfantryAbsorb=yes
ExtraPower=-50
Passengers=5

[General]
BuildSpeed=0.02
",
        );
        let mut store = EntityStore::new();
        let mut e = make_building(1, "NEGEX", "Yuri", 750);
        let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0);
        for i in 0..3 {
            cargo.board_forced(100 + i as u64, 1);
        }
        e.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
        store.insert(e);

        let yuri = intern::test_intern("Yuri");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, yuri, &interner);

        assert_eq!(
            state.total_output, 150,
            "ExtraPower<0 fails strict > 0 gate"
        );
    }

    #[test]
    fn test_unit_absorb_path_also_works() {
        // gamemd gate is (UnitAbsorb || InfantryAbsorb). UnitAbsorb alone
        // (no InfantryAbsorb) still grants the bonus.
        let rules = rules_from_ini(
            "\
[BuildingTypes]
0=UABS

[UABS]
Power=100
Strength=500
InfantryAbsorb=no
UnitAbsorb=yes
ExtraPower=80
Passengers=3

[General]
BuildSpeed=0.02
",
        );
        let mut store = EntityStore::new();
        let mut e = make_building(1, "UABS", "Yuri", 500);
        let mut cargo = crate::sim::passenger::PassengerCargo::new(3, 0);
        for i in 0..2 {
            cargo.board_forced(100 + i as u64, 1);
        }
        e.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
        store.insert(e);

        let yuri = intern::test_intern("Yuri");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, yuri, &interner);

        assert_eq!(
            state.total_output, 260,
            "100 + 80*2 = 260 via UnitAbsorb gate"
        );
    }

    #[test]
    fn test_live_yapowr_under_construction_keeps_native_output_formula() {
        let rules = yapowr_rules();
        let mut store = EntityStore::new();
        let mut e = make_yapowr(1, "Yuri", 750, 5);
        e.building_up = Some(crate::sim::components::BuildingUp::completing_in_ticks(
            30, 0,
        ));
        store.insert(e);

        let yuri = intern::test_intern("Yuri");
        let interner = test_interner();
        let mut state = PowerState::default();
        recalculate_power_for_owner(&mut state, &store, &rules, yuri, &interner);

        assert_eq!(
            state.total_output, 650,
            "building_up does not bypass the native base-plus-occupant formula"
        );
    }
}
