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
use crate::rules::ruleset::RuleSet;
use crate::sim::game_options::GameOptions;
use crate::sim::intern::InternedId;
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

/// The house has one of the first three `[AI] BuildConst=` types on the map:
/// `0x004F8C97..0x004F8CFC` reads `Rules+0x8B0[0..3]` (the BuildConst list,
/// ReadAI `0x00672BB9`) and tests each type's `+0x5550` count above zero
/// (`CounterClass::GetItemCount @ 0x0049FAE0`). It reads three entries with
/// no bound; a shorter list is read past its end natively and ends here.
fn owns_construction_yard(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    rules.build_const_types.iter().take(3).any(|name| {
        sim.interner.get(name).is_some_and(|type_id| {
            house
                .tracking()
                .active_count(EntityCategory::Structure, type_id)
                > 0
        })
    })
}

/// Advice inside the reached House body, after team creation and before
/// defeat. The deterministic per-human projection is documented above.
pub(crate) fn update_house_eva(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    let now = sim.session.binary_frame() as i32;
    let game_mode_nonzero = sim.session.game_mode_nonzero();
    let delay = speak_delay_frames(rules, sim.session.game_options());
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if !house.is_controlled_by_human(game_mode_nonzero) {
        return;
    }
    let available = house.economy().available_money();
    let factories = house.tracking().funds_nag_factories();
    let mut timer = *house.eva_funds_timer();
    let mut guard = house.eva_low_power_guard();

    // --- Insufficient funds, `0x004F8B3C..0x004F8BE1` ---
    // Available money (`IHouse::Available_Money`, House vtable `0x7EA834`
    // slot `+0x18` = `0x004F6990`, called at `0x004F8B6C`) below 100 and
    // the infantry, vehicle, building and naval factory counters summing
    // above zero → the line, a sidebar redraw request and a re-arm.
    // Original Tab6D0EC0 sets dirty+5545 and starts+5538 for7 frames;
    // TabAI6D068B..6D06BA requests another redraw with1 frame remaining.
    // CreditsDraw4A2370's reached player branch still uses the same RGB
    // source B0FA1C and flags4108 (4A2538..4A25DB): no color/visibility
    // flash. VERA paints the sidebar every frame, so this invalidation-only
    // timer needs no second state owner. Raw native save bytes are separate.
    if timer.expired(now) && available < FUNDS_NAG_CREDITS && factories > 0 {
        sim.sound_events.push(SimSoundEvent::HouseEva {
            owner,
            event: EVA_INSUFFICIENT_FUNDS,
        });
        timer.start(now, delay);
    }
    // --- Silo re-arm, `0x004F8BE4..0x004F8C53` --- With the timer expired,
    // a nearly full silo bank (`0x004F8C0B..0x004F8C21`: capacity `+0x310`
    // minus `ftol(+0x2FC)` below 30, capacity above 50) re-arms it without a
    // line. The store stays empty in YR (`crate::sim::economy`), so capacity
    // would have to be both below 30 and above 50: it never re-arms.

    // --- Low power, `0x004F8C56..0x004F8DAB` ---
    // Short = `PowerOutput < PowerDrain && PowerDrain != 0 && (Output == 0
    // || Output / Drain < 1.0)` (`0x004F8C62..0x004F8C91`); otherwise the
    // guard clears (`0x004F8DAB`). Short without a construction yard leaves
    // the guard untouched (`0x004F8CFC JLE` straight out).
    let short = sim
        .power_states
        .get(&owner)
        .is_some_and(|power| power.low_power());
    if !short {
        guard = false;
    } else if owns_construction_yard(sim, rules, owner) && !guard {
        sim.sound_events.push(SimSoundEvent::HouseEva {
            owner,
            event: EVA_LOW_POWER,
        });
        guard = true;
    }
    // `0x004F8D6B..0x004F8DA6` re-arms `House+0x57BC` here; that
    // timer has no reader (`search_instructions "0x57bc]"`: the
    // constructor write and this write only), so it is not modelled.

    if let Some(house) = sim.houses.get_mut(&owner) {
        house.retain_eva_nag_state(timer, guard);
    }
}

#[cfg(any(test, feature = "test-support"))]
fn tick_house_eva(sim: &mut Simulation, rules: &RuleSet) {
    for owner in sim.houses.order().to_vec() {
        update_house_eva(sim, rules, owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;
    use crate::sim::house_tracking::FactorySlot;
    use crate::sim::timer::CdTimer;

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\nSpeakDelayIsInAudioVisual=yes\n\n\
             [AudioVisual]\nSpeakDelay=2\n\n\
             [AI]\nBuildConst=GACNST,NACNST\nBuildPower=GAPOWR\n\n\
             [BuildingTypes]\n0=GAPOWR\n1=GAPILE\n2=GAREFN\n3=GASILO\n4=GACNST\n5=NACNST\n\
             6=GAYARD\n7=GAAIRC\n\n\
             [GAPOWR]\nStrength=750\nPower=200\n\n\
             [GAPILE]\nStrength=500\nPower=-10\nFactory=InfantryType\n\n\
             [GAREFN]\nStrength=900\nPower=-50\nStorage=2000\n\n\
             [GASILO]\nStrength=300\nStorage=2000\n\n\
             [GACNST]\nStrength=1000\nFactory=BuildingType\n\n\
             [NACNST]\nStrength=1000\nFactory=BuildingType\n\n\
             [GAYARD]\nStrength=1000\nFactory=UnitType\nNaval=yes\n\n\
             [GAAIRC]\nStrength=1000\nFactory=AircraftType\n",
        ))
        .expect("rules parse")
    }

    /// Place a building through BuildingClass::Unlimbo, as production does.
    fn structure(sim: &mut Simulation, rules: &RuleSet, type_id: &str, cx: u16) -> u64 {
        sim.spawn_object(type_id, "Americans", cx, 5, 0, rules)
            .expect("building placed")
    }

    fn set_health(sim: &mut Simulation, id: u64, health: i32) {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(health);
    }

    fn sim_with_house(credits: i32, human: bool) -> (Simulation, InternedId) {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, Some(owner), human, credits, 10));
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
                sim.session.binary_frame(),
            );
            tick_house_eva(sim, rules);
            let frame = sim.session.binary_frame();
            for line in house_lines(sim) {
                lines.push((frame, line));
            }
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame() + 1);
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

    /// A broke human house with an idle barracks hears the nag once per
    /// normalised SpeakDelay, no build stalled or queued.
    #[test]
    fn funds_nag_repeats_every_speak_delay_while_broke_with_a_factory() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(50, true);
        sim.session.game_options_mut_for_test().game_speed = 4;
        structure(&mut sim, &rules, "GAPILE", 3);
        let lines = run_frames(&mut sim, &rules, 2880 * 2 + 2);
        let nags: Vec<u32> = lines
            .iter()
            .filter(|(_, line)| *line == EVA_INSUFFICIENT_FUNDS)
            .map(|(frame, _)| *frame)
            .collect();
        // Constructor timer (`Duration = 1`) expires at frame 1.
        assert_eq!(nags, vec![1, 1 + 2880, 1 + 2880 * 2]);
        assert_eq!(
            *sim.houses[&owner].eva_funds_timer(),
            CdTimer::started(1 + 2880 * 2, 2880)
        );
    }

    #[test]
    fn funds_nag_is_silent_at_one_hundred_credits_or_without_a_factory() {
        let rules = rules();
        // Exactly 100 credits: `CMP EAX,0x64 ; JGE` skips.
        let (mut sim, _) = sim_with_house(100, true);
        structure(&mut sim, &rules, "GAPILE", 3);
        assert!(run_frames(&mut sim, &rules, 40).is_empty());
        // Broke, but only a refinery (no factory counter).
        let (mut sim, _) = sim_with_house(0, true);
        structure(&mut sim, &rules, "GAREFN", 3);
        assert!(run_frames(&mut sim, &rules, 40).is_empty());
        // Broke with an airfield only: the nag does not read `+0x5378`.
        let (mut sim, _) = sim_with_house(0, true);
        structure(&mut sim, &rules, "GAAIRC", 3);
        assert!(run_frames(&mut sim, &rules, 40).is_empty());
        // Broke with a factory, but an AI house never reaches the block.
        let (mut sim, _) = sim_with_house(0, false);
        structure(&mut sim, &rules, "GAPILE", 3);
        assert!(run_frames(&mut sim, &rules, 40).is_empty());
    }

    /// The factory counters follow Unlimbo, ChangeOwner and the death's
    /// Limbo, and the nag reads them: a shipyard counts as naval, not as a
    /// war factory.
    #[test]
    fn factory_counters_follow_placement_capture_and_death() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(0, true);
        let soviet = sim.interner.intern("Russians");
        sim.houses
            .register(HouseState::new(soviet, 1, Some(soviet), false, 0, 10));
        let barracks = structure(&mut sim, &rules, "GAPILE", 3);
        structure(&mut sim, &rules, "GAPILE", 4);
        let yard = structure(&mut sim, &rules, "GAYARD", 6);
        structure(&mut sim, &rules, "GAAIRC", 8);
        structure(&mut sim, &rules, "GAREFN", 10);
        let counts = |sim: &Simulation, house| {
            let tracking = sim.houses[&house].tracking();
            [
                FactorySlot::Aircraft,
                FactorySlot::Infantry,
                FactorySlot::Vehicle,
                FactorySlot::Building,
                FactorySlot::Naval,
            ]
            .map(|slot| tracking.factory_count(slot))
        };
        assert_eq!(counts(&sim, owner), [1, 2, 0, 0, 1]);
        assert_eq!(sim.houses[&owner].tracking().funds_nag_factories(), 3);

        sim.change_owner_with_rules(
            barracks,
            soviet,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(counts(&sim, owner), [1, 1, 0, 0, 1]);
        assert_eq!(counts(&sim, soviet), [0, 1, 0, 0, 0]);

        sim.uninit_with_rules(barracks, &rules);
        assert_eq!(counts(&sim, soviet), [0; 5]);
        assert_eq!(counts(&sim, owner), [1, 1, 0, 0, 1]);

        // Only the first Limbo recounts (`0x004458CE`); ChangeOwner moves a
        // limbo factory's counter all the same.
        sim.techno_limbo_with_rules(yard, &rules, None);
        sim.techno_limbo_with_rules(yard, &rules, None);
        assert_eq!(counts(&sim, owner), [1, 1, 0, 0, 0]);
        sim.change_owner_with_rules(
            yard,
            soviet,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(counts(&sim, owner), [1, 1, 0, 0, -1]);
        assert_eq!(counts(&sim, soviet), [0, 0, 0, 0, 1]);
    }

    /// Short on power without a construction yard: silence, and the guard
    /// stays clear, even with a power plant on the map.
    #[test]
    fn low_power_is_silent_without_a_construction_yard() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(5_000, true);
        let plant = structure(&mut sim, &rules, "GAPOWR", 3);
        structure(&mut sim, &rules, "GAPILE", 4);
        structure(&mut sim, &rules, "GAREFN", 6);
        set_health(&mut sim, plant, 1);
        let lines = run_frames(&mut sim, &rules, 10);
        assert!(lines.is_empty(), "{lines:?}");
        assert!(sim.power_states[&owner].is_low_power());
        assert!(!sim.houses[&owner].eva_low_power_guard());
    }

    /// With a construction yard the line plays once, stays silent while
    /// short, clears on recovery and re-announces on the next shortfall. The
    /// yard need not be the first `BuildConst=` type.
    #[test]
    fn low_power_announces_once_per_shortfall_with_a_construction_yard() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(5_000, true);
        structure(&mut sim, &rules, "NACNST", 3);
        let plant = structure(&mut sim, &rules, "GAPOWR", 6);
        structure(&mut sim, &rules, "GAPILE", 7);
        // Drain 10 vs output 200: fine.
        assert!(run_frames(&mut sim, &rules, 3).is_empty());
        // Damage the plant to 1/750 → output 0 < drain 10.
        set_health(&mut sim, plant, 1);
        let lines = run_frames(&mut sim, &rules, 5);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, EVA_LOW_POWER);
        assert!(sim.houses[&owner].eva_low_power_guard());
        // Repair: guard clears.
        set_health(&mut sim, plant, 750);
        assert!(run_frames(&mut sim, &rules, 2).is_empty());
        assert!(!sim.houses[&owner].eva_low_power_guard());
        // Short again: one more line.
        set_health(&mut sim, plant, 1);
        let lines = run_frames(&mut sim, &rules, 5);
        assert_eq!(lines.len(), 1);
    }

    /// Short with the guard already set and the yard destroyed: the guard is
    /// not cleared by the no-yard exit, so a new yard while still short does
    /// not replay the line.
    #[test]
    fn low_power_guard_survives_the_no_yard_exit() {
        let rules = rules();
        let (mut sim, owner) = sim_with_house(5_000, true);
        let yard = structure(&mut sim, &rules, "GACNST", 3);
        let plant = structure(&mut sim, &rules, "GAPOWR", 6);
        structure(&mut sim, &rules, "GAPILE", 7);
        set_health(&mut sim, plant, 1);
        assert_eq!(run_frames(&mut sim, &rules, 2).len(), 1);
        sim.uninit_with_rules(yard, &rules);
        assert!(run_frames(&mut sim, &rules, 2).is_empty());
        assert!(sim.houses[&owner].eva_low_power_guard());
        structure(&mut sim, &rules, "GACNST", 9);
        assert!(run_frames(&mut sim, &rules, 2).is_empty());
    }
}
