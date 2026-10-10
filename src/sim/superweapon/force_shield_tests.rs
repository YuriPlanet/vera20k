//! The Force Shield (`force_shield`, the countdown in
//! `super::SuperWeaponInstance`): its native comparisons against
//! `tools/superweapon_oracle.json` (`force_shield_launch`, `super_fade`;
//! `--check` regenerates them), and launches on retail rules through
//! production frames.

use super::{launch, walk};
use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::sim::intern::InternedId;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::superweapon::chronosphere_tests::{
    charge_super, click, retail_rules_binding, step, world_with,
};
use crate::sim::superweapon::invulnerability::InvulnKind;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::Value;

const FORCE_SHIELD: &str = "ForceShieldSpecial";

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coord(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| int(&value[axis]))
}

/// The row's events named `name`, in order.
fn events<'a>(row: &'a Value, name: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(move |event| event[0] == name)
}

/// House `k` (named `H<k>`) lists house `j` when bit `j` of its ally bits
/// (`+0x5788`) is set; IsAlliedWith answers a house's own bit first.
fn alliances(row: &Value) -> HouseAllianceMap {
    let mut map = HouseAllianceMap::new();
    for (asker, bits) in rows(row, "allies").iter().enumerate() {
        let bits = bits.as_u64().unwrap();
        for other in (0..32).filter(|&other| other != asker && bits & (1 << other) != 0) {
            map.entry(format!("H{asker}"))
                .or_default()
                .insert(format!("H{other}"));
        }
    }
    map
}

/// Case 10's walk: the buildings the native walk shields, last first, from
/// each row's buildings (owner and GetCoords) and its centre, with the
/// owner's view of the launching house (`H0`) from the production alliance
/// test.
#[test]
fn the_walk_matches_native() {
    let oracle = oracle();
    for row in rows(&oracle, "force_shield_launch") {
        let native: Vec<usize> = events(row, "curtain")
            .map(|event| event[1].as_u64().unwrap() as usize)
            .collect();
        if !row["charged"].as_bool().unwrap() {
            // ClickFire admits only a charged Super (`fire::click_fire`).
            assert!(native.is_empty());
            continue;
        }
        let alliances = alliances(row);
        let buildings: Vec<(bool, [i32; 3])> = rows(row, "buildings")
            .iter()
            .map(|building| {
                let owner = format!("H{}", building[0]);
                (
                    is_allied_with(&alliances, &owner, "H0"),
                    coord(&building[1]),
                )
            })
            .collect();
        let centre = coord(&row["cell_coords"]);
        assert_eq!(
            walk(centre, int(&row["radius"]), &buildings),
            native,
            "{row}"
        );
        // Each call: ForceShieldDuration=, the launching house, the Force
        // Shield byte.
        for event in events(row, "curtain") {
            assert_eq!(
                (int(&event[2]), int(&event[3]), int(&event[4])),
                (int(&row["duration"]), 0, 1)
            );
        }
    }
}

/// SuperClass::AI's head called once a frame: the countdown after each call,
/// and the call on which `SpecialSound=` plays at the stored coordinate.
#[test]
fn the_countdown_matches_native() {
    let oracle = oracle();
    let stored = [10368, 10368, 416];
    for row in rows(&oracle, "super_fade") {
        let id = InternedId::from_index(1);
        let mut instance = SuperWeaponInstance::new(id, id, 0);
        instance.arm_fade(int(&row["start"]), stored);
        let mut values = Vec::new();
        let mut plays = Vec::new();
        for call in 1..=int(&row["calls"]) {
            if let Some(at) = instance.step_fade() {
                plays.push((call, at));
            }
            values.push(instance.fade().0);
        }
        let native_values: Vec<i32> = rows(row, "values").iter().map(int).collect();
        let native_plays: Vec<(i32, [i32; 3])> = rows(row, "plays")
            .iter()
            .map(|play| (int(&play[0]), coord(&play[2])))
            .collect();
        assert_eq!(values, native_values, "{row}");
        assert_eq!(plays, native_plays, "{row}");
    }
}

/// A row's launch on retail rules carrying its `[General]` values, a
/// 64-square flat map with the row's level and bridge bit at its cell, and
/// no buildings: the invoke anim's coordinate and row, the countdown and its
/// coordinate, and the blackout. The fixture-only row whose cell answers the
/// zero coordinate is left out.
#[test]
fn the_launch_matches_native() {
    let Some(mut rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    let oracle = oracle();
    let mut compared = 0;
    for row in rows(&oracle, "force_shield_launch") {
        let cell = (int(&row["cell"][0]) as u16, int(&row["cell"][1]) as u16);
        let level = int(&row["level"]);
        let centre = [
            i32::from(cell.0) * 256 + 128,
            i32::from(cell.1) * 256 + 128,
            level * 104,
        ];
        if !row["charged"].as_bool().unwrap() || coord(&row["cell_coords"]) != centre {
            continue;
        }
        rules.general.force_shield_radius = int(&row["radius"]);
        rules.general.force_shield_duration = int(&row["duration"]);
        rules.general.force_shield_blackout_duration = int(&row["blackout"]);
        rules.general.force_shield_fade_sound_time = int(&row["fade"]);
        let (world_rules, mut sim, _) = world_with(rules, 64, &[(cell, level as u8)]);
        rules = world_rules;
        if row["bridge"].as_bool().unwrap()
            && let Some(terrain) = sim.resolved_terrain.as_mut()
        {
            let index = terrain.index(cell.0, cell.1).unwrap();
            terrain.cells[index].bridge_facts.raw_flags =
                crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
        let russians = sim.interner.intern("Russians");
        let sw_type = charge_super(&mut sim, russians, FORCE_SHIELD);
        let frame = sim.session.binary_frame;
        assert!(launch(&mut sim, &rules, russians, cell.0, cell.1, sw_type));

        let native_anim = events(row, "anim").next().unwrap();
        let invoke = sim.interner.intern("FORCSHLD");
        let anims: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| anim)
            .filter(|anim| anim.type_id == invoke)
            .collect();
        assert_eq!(anims.len(), 1, "{row}");
        let world = anims[0].world_coord;
        assert_eq!([world.x, world.y, world.z], coord(&native_anim[2]), "{row}");
        // (delay, loopCount, drawFlags, zAdjust, reverse) = (0, 1, 0x600, 0, 0).
        assert_eq!(native_anim[3], serde_json::json!([0, 1, 0x600, 0, 0]));
        assert_eq!(
            (anims[0].draw_flags, anims[0].z_adjust),
            (0x600, 0),
            "{row}"
        );

        let instance = &sim.super_weapons[&russians][&sw_type];
        assert_eq!(
            instance.fade(),
            (int(&row["fade_countdown"]), coord(&row["fade_coords"])),
            "{row}"
        );
        let native_blackout = events(row, "blackout").next().unwrap();
        assert_eq!(
            sim.power_states[&russians].blackout_remaining(frame),
            int(&native_blackout[2]),
            "{row}"
        );
        compared += 1;
    }
    assert_eq!(compared, 21);
}

/// Retail rules (`ForceShieldRadius=4`, `ForceShieldDuration=500`,
/// `ForceShieldPlayFadeSoundTime=75`) through production frames: the click's
/// launch shields the Russian buildings within four cells and the American
/// one only while the Americans count the Russians as allies, and
/// `ForceShieldFading` plays at the cell on the 425th frame after the launch
/// frame, once.
#[test]
fn retail_force_shield_shields_allied_buildings_and_fades() {
    let Some(mut rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    for americans_list_russians in [false, true] {
        let (world_rules, mut sim, _) = world_with(rules, 64, &[]);
        rules = world_rules;
        let russians = sim.interner.intern("Russians");
        if americans_list_russians {
            sim.house_alliances
                .entry("AMERICANS".to_string())
                .or_default()
                .insert("RUSSIANS".to_string());
        } else {
            // The Russians' own list does not reach the Americans' buildings.
            sim.house_alliances
                .entry("RUSSIANS".to_string())
                .or_default()
                .insert("AMERICANS".to_string());
        }
        let spawn = |sim: &mut Simulation, kind: &str, owner: &str, at: (u16, u16)| {
            sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, 0, &rules)
                .unwrap()
        };
        // The battle lab provides the Super, far from the cell.
        spawn(&mut sim, "NATECH", "Russians", (10, 10));
        // GetCoords is the 2x2 foundation's centre, so from cell (40, 40)'s
        // centre: 652, 905, 905 and 1159 leptons.
        let west = spawn(&mut sim, "GAPOWR", "Russians", (37, 40));
        let north = spawn(&mut sim, "GAPOWR", "Russians", (40, 36));
        let american = spawn(&mut sim, "GAPOWR", "Americans", (40, 43));
        let far = spawn(&mut sim, "GAPOWR", "Russians", (44, 40));
        charge_super(&mut sim, russians, FORCE_SHIELD);
        let launch_frame = sim.session.binary_frame;
        click(&mut sim, &rules, russians, FORCE_SHIELD, (40, 40));

        let shield = |sim: &Simulation, id: u64| {
            sim.substrate
                .entities
                .get(id)
                .and_then(|entity| entity.invulnerability.as_ref())
                .map(|state| (state.kind, state.timer.duration()))
        };
        let shielded = Some((InvulnKind::ForceShield, 500));
        assert_eq!(shield(&sim, west), shielded);
        assert_eq!(shield(&sim, north), shielded);
        assert_eq!(
            shield(&sim, american),
            shielded.filter(|_| americans_list_russians)
        );
        assert_eq!(shield(&sim, far), None);

        let fading = |sim: &Simulation| {
            sim.sound_events
                .iter()
                .filter(|event| {
                    matches!(event, SimSoundEvent::VocAt { sound_id, audible_to: None, rx: 40, ry: 40, world_z_leptons: 0, .. }
                        if sound_id == "ForceShieldFading")
                })
                .count()
        };
        let mut heard = Vec::new();
        while sim.session.binary_frame < launch_frame + 430 {
            sim.sound_events.clear();
            let frame = sim.session.binary_frame;
            step(&mut sim, &rules);
            if fading(&sim) > 0 {
                heard.push((frame - launch_frame, fading(&sim)));
            }
        }
        assert_eq!(heard, [(425, 1)]);
    }
}

/// Over a bridge the walk still measures from the cell's ground centre while
/// the countdown keeps the deck (`force_shield_launch` row 7 shields the
/// ground building 1000 leptons out, not the one 416 leptons above it): a
/// click on bridge cell (40, 40) shields the Russian GAPOWR 974 leptons from
/// its ground centre, 1059 from its deck.
#[test]
fn a_bridge_cell_walks_from_the_ground_centre() {
    let Some(rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    let terrain = sim.resolved_terrain.as_mut().expect("the world has a map");
    let index = terrain.index(40, 40).unwrap();
    terrain.cells[index].bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    let russians = sim.interner.intern("Russians");
    sim.spawn_object_at_height("NATECH", "Russians", 10, 10, 0, 0, &rules)
        .unwrap();
    // The 2x2 foundation's centre (11264, 10752).
    let plant = sim
        .spawn_object_at_height("GAPOWR", "Russians", 43, 41, 0, 0, &rules)
        .unwrap();
    let sw_type = charge_super(&mut sim, russians, FORCE_SHIELD);
    click(&mut sim, &rules, russians, FORCE_SHIELD, (40, 40));

    assert_eq!(
        sim.substrate
            .entities
            .get(plant)
            .and_then(|entity| entity.invulnerability.as_ref())
            .map(|state| state.kind),
        Some(InvulnKind::ForceShield)
    );
    assert_eq!(
        sim.super_weapons[&russians][&sw_type].fade().1,
        [10368, 10368, 416]
    );
}

/// SuperClass::Grant leaves the countdown and its coordinate and releases
/// the held anim (`0x006CB6B4..0x006CB6D2`): a Force Shield whose provider
/// was lost and rebuilt still fades where it was launched.
#[test]
fn a_grant_keeps_the_countdown_and_releases_the_anim() {
    let Some(rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    let russians = sim.interner.intern("Russians");
    let sw_type = sim.interner.intern(FORCE_SHIELD);
    let anim = super::super::spawn_super_anim(&mut sim, &rules, "FORCSHLD", [10368, 10368, 5])
        .expect("FORCSHLD constructs");
    let mut instance = SuperWeaponInstance::new(sw_type, russians, 0);
    instance.arm_fade(100, [10368, 10368, 416]);
    instance.placement_anim = Some(anim);
    sim.super_weapons
        .entry(russians)
        .or_default()
        .insert(sw_type, instance);
    sim.spawn_object_at_height("NATECH", "Russians", 20, 20, 0, 0, &rules)
        .unwrap();

    super::super::refresh_super_weapons_for_owner(&mut sim, &rules, russians);

    let instance = &sim.super_weapons[&russians][&sw_type];
    assert!(instance.is_active);
    assert_eq!(instance.fade(), (100, [10368, 10368, 416]));
    assert_eq!(instance.placement_anim(), None);
    assert_eq!(
        sim.substrate
            .anims
            .get(anim)
            .unwrap()
            .runtime
            .loop_remaining,
        0
    );
}
