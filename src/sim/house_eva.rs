//! `HouseClass::Update @ 0x004F8440` EVA advice block
//! (`0x004F8B08..0x004F8DAB`): the "Insufficient funds" nag and the
//! "Low power" one-shot, with their native predicates and cadence.
//!
//! Native runs the block only for `this == PlayerPtr` (`0x004F8B08
//! CMP [0x00A83D4C],ESI`). The sim does not know the local player, so every
//! human-controlled house evaluates it and the app keeps the local-owner
//! filter; the timer and guard are per-house state for the same reason.
//! Running the funds timer and low-power guard for the non-local human
//! houses (hashed, deterministic) is VERA-internal — native has no such
//! state for them, so a replay/snapshot carries state gamemd never holds;
//! gamemd equivalent UNCHECKED for anything but the local player's lines.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and sim/ only.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::FactoryType;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_options::GameOptions;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::world::{SimSoundEvent, Simulation};

/// `evamd.ini` section played at `0x004F8BA0`.
pub const EVA_INSUFFICIENT_FUNDS: &str = "EVA_InsufficientFunds";
/// `evamd.ini` section played at `0x004F8D14`.
pub const EVA_LOW_POWER: &str = "EVA_LowPower";

/// `0x004F8B6F CMP EAX,0x64`: the nag runs while available money is below
/// this.
const FUNDS_NAG_CREDITS: i32 = 100;
/// `0x007E27F8`, the double both timer re-arms multiply `SpeakDelay` by
/// (`0x004F8BBA`, `0x004F8C2E`, `0x004F8D77`): frames per minute at the
/// unnormalised 15 fps base rate.
const FRAMES_PER_MINUTE: f64 = 900.0;

/// `SpeedNormalize(ftol(SpeakDelay * 900.0))` — the re-arm value shared by
/// the funds nag (`0x004F8BAF..0x004F8BCB`), the silo re-arm
/// (`0x004F8C23..0x004F8C3F`) and the low-power speak timer
/// (`0x004F8D6B..0x004F8D88`). `Math::ftol @ 0x007C5F00` truncates.
pub fn speak_delay_frames(rules: &RuleSet, options: &GameOptions) -> i32 {
    let frames = (rules.general.speak_delay_minutes * FRAMES_PER_MINUTE) as i32;
    options.speed_normalize(frames)
}

/// `HouseClass::GetFactoryCount @ 0x00500940` sum used at
/// `0x004F8B74..0x004F8B94`: `+0x537C` (infantry) + `+0x5380` (vehicle) +
/// `+0x5384` (building) + `+0x5388` (naval); `+0x5378` (aircraft) is not read.
/// The native counters follow building Unlimbo/Limbo, so a factory still
/// building up already counts, while a Dying corpse or a limbo object does
/// not.
pub fn funds_nag_factory_count(
    entities: &EntityStore,
    rules: &RuleSet,
    owner: InternedId,
    interner: &StringInterner,
) -> u32 {
    entities
        .values()
        .filter(|e| {
            !e.dying
                && !e.lifecycle.in_limbo
                && e.owner() == owner
                && e.category == EntityCategory::Structure
                && rules
                    .object(interner.resolve(e.type_ref()))
                    .and_then(|obj| obj.factory)
                    .is_some_and(|factory| {
                        // Naval yards are `Factory=UnitType` + `Naval=yes`
                        // (`GetFactoryCount` case 1/0x28 splits them on the
                        // naval flag); both counters are summed here.
                        matches!(
                            factory,
                            FactoryType::InfantryType
                                | FactoryType::UnitType
                                | FactoryType::BuildingType
                        )
                    })
        })
        .count() as u32
}

/// `0x004F8C0B..0x004F8C21`: with the funds timer expired and no nag due,
/// a nearly full silo bank (`capacity - stored < 30 && capacity > 50`,
/// `HouseClass+0x310` vs `StorageClass::GetTotal(+0x2FC)` through `ftol`)
/// re-arms the same timer without any line, pushing the next funds nag out by
/// one SpeakDelay. There is no `EVA_SilosNeeded` in YR.
pub const fn silo_nearly_full(capacity: i32, stored: i32) -> bool {
    capacity - stored < 30 && capacity > 50
}

/// The house owns a live instance of one of the first three
/// `[AI] BuildPower=` types (`0x004F8C97..0x004F8CFC` reads exactly
/// `Rules+0x8B0[0..3]` through `CountOwnedInstances @ 0x0049FAE0` on the
/// `House+0x5550` per-type counter, which Unlimbo/Limbo maintain).
pub fn owns_build_power_plant(
    entities: &EntityStore,
    rules: &RuleSet,
    owner: InternedId,
    interner: &StringInterner,
) -> bool {
    let build_power: Vec<&str> = rules
        .build_power_types
        .iter()
        .take(3)
        .map(String::as_str)
        .collect();
    if build_power.is_empty() {
        return false;
    }
    entities.values().any(|e| {
        !e.dying
            && !e.lifecycle.in_limbo
            && e.owner() == owner
            && e.category == EntityCategory::Structure
            && build_power
                .iter()
                .any(|name| name.eq_ignore_ascii_case(interner.resolve(e.type_ref())))
    })
}

/// Run the advice block for every human-controlled house in
/// `ScenarioSession::house_order`, in that order.
pub fn tick_house_eva(sim: &mut Simulation, rules: &RuleSet) {
    let now = i64::from(sim.session.binary_frame);
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    let delay = speak_delay_frames(rules, &sim.session.game_options);
    let owners: Vec<InternedId> = sim.session.house_order.clone();
    for owner in owners {
        let Some(house) = sim.houses.get(&owner) else {
            continue;
        };
        if !house.is_controlled_by_human(game_mode_nonzero) {
            continue;
        }
        let credits = house.economy.credits;
        let mut timer = house.eva_funds_timer;
        let mut guard = house.eva_low_power_guard;

        // --- Insufficient funds, `0x004F8B3C..0x004F8BE1` ---
        // Available money (`IHouse::Available_Money`, House vtable `0x7EA834`
        // slot `+0x18` = `0x004F6990`: credits plus stored ore) below 100 and
        // any infantry/vehicle/building/naval factory owned → the line, the
        // sidebar credits flash and a re-arm. VERA banks ore straight into
        // `credits`, so the wallet is the available money.
        if timer.expired(now)
            && credits < FUNDS_NAG_CREDITS
            && funds_nag_factory_count(&sim.substrate.entities, rules, owner, &sim.interner) > 0
        {
            sim.sound_events.push(SimSoundEvent::HouseEva {
                owner,
                event: EVA_INSUFFICIENT_FUNDS,
            });
            timer.arm(now, delay);
        }
        // --- Silo re-arm, `0x004F8BE4..0x004F8C53` --- (timer re-read after
        // the nag's own re-arm). VERA has no ore storage authority, so
        // `stored` is 0 and the branch is unreachable on any real capacity.
        if timer.expired(now) {
            let capacity: i32 = sim
                .substrate
                .entities
                .values()
                .filter(|e| {
                    !e.dying
                        && !e.lifecycle.in_limbo
                        && e.owner() == owner
                        && e.category == EntityCategory::Structure
                })
                .filter_map(|e| rules.object(sim.interner.resolve(e.type_ref())))
                .map(|obj| obj.storage)
                .fold(0i32, i32::saturating_add);
            if silo_nearly_full(capacity, 0) {
                timer.arm(now, delay);
            }
        }

        // --- Low power, `0x004F8C56..0x004F8DAB` ---
        // Short = `PowerOutput < PowerDrain && PowerDrain != 0 && (Output == 0
        // || Output / Drain < 1.0)` (`0x004F8C62..0x004F8C91`); otherwise the
        // guard clears (`0x004F8DAB`). Short without a `BuildPower=` plant
        // leaves the guard untouched (`0x004F8CFC JLE` straight out).
        let short = sim
            .power_states
            .get(&owner)
            .is_some_and(|power| power.is_low_power);
        if !short {
            guard = false;
        } else if owns_build_power_plant(&sim.substrate.entities, rules, owner, &sim.interner) {
            if !guard {
                sim.sound_events.push(SimSoundEvent::HouseEva {
                    owner,
                    event: EVA_LOW_POWER,
                });
                guard = true;
            }
            // `0x004F8D6B..0x004F8DA6` re-arms `House+0x57BC` here; that
            // timer has no reader (`search_instructions "0x57bc]"`: the
            // constructor write and this write only), so it is not modelled.
        }

        if let Some(house) = sim.houses.get_mut(&owner) {
            house.eva_funds_timer = timer;
            house.eva_low_power_guard = guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::{HouseFrameTimer, HouseState};

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\nSpeakDelayIsInAudioVisual=yes\n\n\
             [AudioVisual]\nSpeakDelay=2\n\n\
             [AI]\nBuildPower=NAPOWR,GAPOWR,YAPOWR\n\n\
             [BuildingTypes]\n0=GAPOWR\n1=GAPILE\n2=GAREFN\n3=GASILO\n\n\
             [GAPOWR]\nStrength=750\nPower=200\n\n\
             [GAPILE]\nStrength=500\nPower=-10\nFactory=InfantryType\n\n\
             [GAREFN]\nStrength=900\nPower=-50\nStorage=2000\n\n\
             [GASILO]\nStrength=300\nStorage=2000\n",
        ))
        .expect("rules parse")
    }

    fn structure(sim: &mut Simulation, id: u64, type_id: &str, owner: &str, cx: u16) {
        let mut e = GameEntity::test_default(id, type_id, owner, cx, 5);
        e.type_ref = sim.interner.intern(type_id);
        e.owner = sim.interner.intern(owner);
        e.category = EntityCategory::Structure;
        e.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(e);
    }

    fn sim_with_house(credits: i32, human: bool) -> (Simulation, InternedId) {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            HouseState::new(owner, 0, Some(owner), human, credits, 10),
        );
        sim.session.house_order.push(owner);
        (sim, owner)
    }

    fn house_lines(sim: &mut Simulation) -> Vec<&'static str> {
        sim.sound_events
            .drain(..)
            .filter_map(|event| match event {
                SimSoundEvent::HouseEva { event, .. } => Some(event),
                _ => None,
            })
            .collect()
    }

    fn run_frames(sim: &mut Simulation, rules: &RuleSet, frames: u32) -> Vec<(u32, &'static str)> {
        let mut lines = Vec::new();
        for _ in 0..frames {
            crate::sim::power_system::tick_power_states(
                &mut sim.power_states,
                &mut sim.substrate.entities,
                rules,
                &sim.interner,
            );
            tick_house_eva(sim, rules);
            let frame = sim.session.binary_frame;
            for line in house_lines(sim) {
                lines.push((frame, line));
            }
            sim.session.binary_frame += 1;
        }
        lines
    }

    /// `SpeakDelay=2` → `ftol(2 * 900) = 1800` → `SpeedNormalize`: stored
    /// speed 1 (retail skirmish) gives `(1800 << 3) / 2 = 7200`; speed 4 gives
    /// `14400 / 5 = 2880`; speed 0 gives `14400`.
    #[test]
    fn speak_delay_frames_follows_the_native_speed_normaliser() {
        let rules = rules();
        let mut options = GameOptions::default();
        assert_eq!(options.game_speed, 1);
        assert_eq!(speak_delay_frames(&rules, &options), 7200);
        options.game_speed = 4;
        assert_eq!(speak_delay_frames(&rules, &options), 2880);
        options.game_speed = 0;
        assert_eq!(speak_delay_frames(&rules, &options), 14400);
        options.game_speed = 7;
        assert_eq!(speak_delay_frames(&rules, &options), 1800);
    }

    #[test]
    fn frame_timer_expiry_matches_the_native_timer_struct_test() {
        let armed = HouseFrameTimer {
            start_frame: 10,
            duration: 5,
        };
        assert!(!armed.expired(14));
        assert!(armed.expired(15));
        let never = HouseFrameTimer {
            start_frame: -1,
            duration: 5,
        };
        assert!(!never.expired(1_000));
        let never_zero = HouseFrameTimer {
            start_frame: -1,
            duration: 0,
        };
        assert!(never_zero.expired(0));
        assert!(HouseFrameTimer::at_construction(0).expired(1));
        assert!(!HouseFrameTimer::at_construction(0).expired(0));
    }

    /// A broke human house with an idle barracks hears the nag once per
    /// normalised SpeakDelay, no build stalled or queued.
    #[test]
    fn funds_nag_repeats_every_speak_delay_while_broke_with_a_factory() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(50, true);
        sim.session.game_options.game_speed = 4;
        structure(&mut sim, 1, "GAPILE", "Americans", 3);
        let lines = run_frames(&mut sim, &rules, 2880 * 2 + 2);
        let nags: Vec<u32> = lines
            .iter()
            .filter(|(_, line)| *line == EVA_INSUFFICIENT_FUNDS)
            .map(|(frame, _)| *frame)
            .collect();
        // Constructor timer (`Duration = 1`) expires at frame 1.
        assert_eq!(nags, vec![1, 1 + 2880, 1 + 2880 * 2]);
        assert_eq!(
            sim.houses[&owner].eva_funds_timer,
            HouseFrameTimer {
                start_frame: 1 + 2880 * 2,
                duration: 2880,
            }
        );
    }

    #[test]
    fn funds_nag_is_silent_at_one_hundred_credits_or_without_a_factory() {
        let rules = rules();
        // Exactly 100 credits: `CMP EAX,0x64 ; JGE` skips.
        let (mut sim, _) = sim_with_house(100, true);
        structure(&mut sim, 1, "GAPILE", "Americans", 3);
        assert!(run_frames(&mut sim, &rules, 40).is_empty());
        // Broke, but only a refinery (no factory counter).
        let (mut sim, _) = sim_with_house(0, true);
        structure(&mut sim, 1, "GAREFN", "Americans", 3);
        assert!(run_frames(&mut sim, &rules, 40).is_empty());
        // Broke with a factory, but an AI house never reaches the block.
        let (mut sim, _) = sim_with_house(0, false);
        structure(&mut sim, 1, "GAPILE", "Americans", 3);
        assert!(run_frames(&mut sim, &rules, 40).is_empty());
    }

    #[test]
    fn funds_nag_factory_count_excludes_dying_and_limbo_and_non_factories() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(0, true);
        structure(&mut sim, 1, "GAPILE", "Americans", 3);
        structure(&mut sim, 2, "GAPILE", "Americans", 4);
        structure(&mut sim, 3, "GAREFN", "Americans", 5);
        sim.substrate.entities.get_mut(2).unwrap().dying = true;
        assert_eq!(
            funds_nag_factory_count(&sim.substrate.entities, &rules, owner, &sim.interner),
            1
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .lifecycle
            .in_limbo = true;
        assert_eq!(
            funds_nag_factory_count(&sim.substrate.entities, &rules, owner, &sim.interner),
            0
        );
    }

    #[test]
    fn silo_nearly_full_predicate_is_exact() {
        assert!(silo_nearly_full(100, 80));
        assert!(!silo_nearly_full(100, 70));
        assert!(!silo_nearly_full(50, 40));
        assert!(silo_nearly_full(51, 30));
    }

    /// Barracks and refinery before any power plant: short on power, but no
    /// `BuildPower=` type owned → silence, and the guard stays clear.
    #[test]
    fn low_power_is_silent_without_a_build_power_plant() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(5_000, true);
        structure(&mut sim, 1, "GAPILE", "Americans", 3);
        structure(&mut sim, 2, "GAREFN", "Americans", 5);
        let lines = run_frames(&mut sim, &rules, 10);
        assert!(lines.is_empty(), "{lines:?}");
        assert!(sim.power_states[&owner].is_low_power);
        assert!(!sim.houses[&owner].eva_low_power_guard);
    }

    /// With a plant the line plays once, stays silent while short, clears on
    /// recovery and re-announces on the next shortfall.
    #[test]
    fn low_power_announces_once_per_shortfall_with_a_build_power_plant() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(5_000, true);
        structure(&mut sim, 1, "GAPOWR", "Americans", 3);
        structure(&mut sim, 2, "GAPILE", "Americans", 4);
        // Drain 10 vs output 200: fine.
        assert!(run_frames(&mut sim, &rules, 3).is_empty());
        // Damage the plant to 1/750 → output 0 < drain 10.
        sim.substrate.entities.get_mut(1).unwrap().health.current = 1;
        let lines = run_frames(&mut sim, &rules, 5);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, EVA_LOW_POWER);
        assert!(sim.houses[&owner].eva_low_power_guard);
        // Repair: guard clears.
        sim.substrate.entities.get_mut(1).unwrap().health.current = 750;
        assert!(run_frames(&mut sim, &rules, 2).is_empty());
        assert!(!sim.houses[&owner].eva_low_power_guard);
        // Short again: one more line.
        sim.substrate.entities.get_mut(1).unwrap().health.current = 1;
        let lines = run_frames(&mut sim, &rules, 5);
        assert_eq!(lines.len(), 1);
    }

    /// Short with the guard already set and the plant sold: the guard is not
    /// cleared by the no-plant exit, so rebuilding the plant while still
    /// short does not replay the line.
    #[test]
    fn low_power_guard_survives_the_no_plant_exit() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(5_000, true);
        structure(&mut sim, 1, "GAPOWR", "Americans", 3);
        structure(&mut sim, 2, "GAPILE", "Americans", 4);
        sim.substrate.entities.get_mut(1).unwrap().health.current = 1;
        assert_eq!(run_frames(&mut sim, &rules, 2).len(), 1);
        sim.substrate.entities.get_mut(1).unwrap().dying = true;
        assert!(run_frames(&mut sim, &rules, 2).is_empty());
        assert!(sim.houses[&owner].eva_low_power_guard);
        sim.substrate.entities.get_mut(1).unwrap().dying = false;
        assert!(run_frames(&mut sim, &rules, 2).is_empty());
    }
}
