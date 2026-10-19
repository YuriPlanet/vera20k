//! Fresh fixed-tail House cash through a real stock SPY entry. Original
//! CreateHouses687F10's Neutral/Special tail6882C2..68837E constructs these
//! Houses without participant SetCreditsAndColor4FCE00. House ctor4F58D8
//! initializes cash to zero. Binary1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.

use crate::headless_scenario::{SIM_TICK_MS, battle_session, load_session};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::spy_infiltration::InfiltrationEffect;
use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

#[test]
fn neutral_refinery_spy_entry_cannot_steal_the_participants_starting_cash() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let directory = crate::map::source::test_support::TestDirectory::new("neutral-spy-cash");
    let path = directory.path().join("neutral-spy-cash.map");
    let mut map = String::from_utf8(
        assets
            .resolve_ref("XMP03T4.MAP")
            .expect("physical Hills map")
            .bytes
            .to_vec(),
    )
    .expect("retail Hills is a text INI");
    for (section, records) in [
        (
            "Infantry",
            "WalletSpy=<human player>,SPY,256,30,85,0,Guard,0,None,0,0,0,1,1\n",
        ),
        (
            "Structures",
            "WalletRefinery=Neutral,NAREFN,256,31,85,0,None,0,0,1,0,0,0,None,None,None,0\n",
        ),
    ] {
        let marker = format!("[{section}]");
        if let Some(start) = map.find(&marker) {
            let line_end = start + map[start..].find('\n').unwrap() + 1;
            map.insert_str(line_end, records);
        } else {
            map.push_str(&format!("\n{marker}\n{records}"));
        }
    }
    std::fs::write(&path, map).unwrap();
    let name = path.to_str().unwrap();
    let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.starting_credits = 5000;
    launch.options.bases = true;
    launch.options.unit_count = 0;
    launch.options.tech_level = 10;
    launch.options.short_game = false;
    launch.options.crates = false;
    let mut scene = load_session(&root, name, 0x1234_5678, launch)
        .expect("ordinary authored retail Battle startup");
    let sim = scene.sim();
    let player = sim.session.current_house().unwrap();
    let neutral = sim.interner.get("Neutral").unwrap();
    let special = sim.interner.get("Special").unwrap();
    let computer = sim.interner.get("Computer1").unwrap();
    let refinery = sim
        .entities()
        .values()
        .find(|entity| {
            sim.interner.resolve(entity.type_ref()) == "NAREFN"
                && entity.owner() == neutral
                && (entity.position.rx(), entity.position.ry()) == (31, 85)
        })
        .expect("authored Neutral refinery succeeds in the real placement funnel")
        .stable_id();
    let spy = sim
        .entities()
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "SPY" && entity.owner() == player)
        .expect("authored current-human SPY is actually admitted")
        .stable_id();
    let rules = &scene.runtime.resources.rules;
    let target_type = rules.object("NAREFN").unwrap();
    assert!(target_type.spyable && target_type.storage > 0);
    assert!(!target_type.radar && target_type.power <= 0);
    assert!(!rules.is_build_tech("NAREFN") && target_type.super_weapon.is_none());
    assert!(sim.infiltrator_building_action(spy, refinery, rules));
    assert!(sim.houses[&neutral].multiplay_passive && sim.houses[&special].multiplay_passive);
    assert_eq!(sim.houses[&player].economy.credits(), 5000);
    assert_eq!(sim.houses[&computer].economy.credits(), 5000);
    let initial_neutral_cash = sim.houses[&neutral].economy.credits();

    // The consumed whole native4571E0 control uses balance0/percent.5 and
    // transfers zero. Initial target cash0 comes from the constructor/caller
    // proof above, not from this Rust execution or an injected wallet.
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spy_infiltration_oracle.json",
    ))
    .unwrap();
    let zero = native["infiltrate"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "cash, balance 0, SpyMoneyStealPercent .5")
        .unwrap();
    let gain = zero["expected"]["infiltrator"]["balance"].as_i64().unwrap()
        - zero["infiltrator"]["balance"].as_i64().unwrap();
    assert_eq!(gain, 0);
    let command = CommandEnvelope::new(
        player,
        sim.session.tick() + 1,
        Command::CaptureBuilding {
            engineer_id: spy,
            target_building_id: refinery,
        },
    );
    scene.runtime.simulation.queue_command(command);
    let mut effects = Vec::new();
    let mut entered = false;
    for _ in 0..600 {
        let commands = scene.runtime.simulation.take_due_commands();
        let frame = scene
            .runtime
            .advance_frame(
                &commands,
                SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary bound-resource command frame");
        effects.extend(frame.sound_events);
        let sim = scene.sim();
        if sim.entities().get(spy).is_none() {
            entered = true;
            break;
        }
        assert_eq!(
            sim.houses[&neutral].economy.credits(),
            initial_neutral_cash,
            "no intervening Neutral income/spend before the actual spy entry"
        );
    }
    assert!(
        entered,
        "SPY must reach the refinery and complete normal UnInit cleanup"
    );
    let cash_entries = effects.iter().filter(|event| matches!(
        event,
        SimSoundEvent::BuildingInfiltrated { owner, infiltrator, effect: InfiltrationEffect::Cash, .. }
            if *owner == neutral && *infiltrator == player
    )).count();
    assert_eq!(
        cash_entries, 1,
        "one real cash-effect/EVA request, no callback injection"
    );
    let sim = scene.sim();
    assert_eq!(sim.entities().get(refinery).unwrap().owner(), neutral);
    assert_eq!(
        sim.houses[&player].economy.credits(),
        5000 + gain as i32,
        "a Neutral refinery starts with ctor cash0, not participant launch money; initial Neutral={initial_neutral_cash}"
    );
    assert_eq!(sim.houses[&neutral].economy.credits(), 0);
    assert_eq!(sim.houses[&neutral].economy.spent_credits(), 0);
    assert_eq!(sim.houses[&special].economy.credits(), 0);
}
