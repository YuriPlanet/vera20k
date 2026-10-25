//! The Psychic Reveal (`psychic_reveal`): its native comparison against
//! `tools/superweapon_oracle.json` `psychic_launch` (`--check` regenerates
//! it), and a production launch against the cells RevealArea maps in
//! `tools/spatial_oracle/reveal_area.json`.

use super::{OBSERVED, Observed, launch};
use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::superweapon::chronosphere_tests::{
    charge_super, click, retail_rules_binding, world_with,
};
use crate::sim::world::SimSoundEvent;
use serde_json::Value;
use std::collections::BTreeSet;

const REVEAL: &str = "PsychicRevealSpecial";

/// The keys case 11 reads, with `PsychicRevealRadius=` at `radius`.
fn rules(radius: i32) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\n[CombatDamage]\nPsychicRevealRadius={radius}\n\
         [AudioVisual]\nPsychicRevealActivateSound=PsychicRevealActivate\n\
         [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [SuperWeaponTypes]\n0=PsychicRevealSpecial\n\
         [PsychicRevealSpecial]\nType=PsychicReveal\n"
    )))
    .expect("psychic reveal fixture rules")
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coord(value: &Value) -> [i32; 3] {
    [int(&value[0]), int(&value[1]), int(&value[2])]
}

/// Case 11 against `psychic_launch`, through the production launch on a
/// flat 64-square map whose target cell carries the row's level and bridge
/// bit. Compared:
/// - the charge gate;
/// - RevealArea's coordinate (the cell's GetCoords, no deck over a bridge)
///   and radius (as read), for the pair of calls native makes with the
///   Super's house, final 0 then 1 and no other argument;
/// - the launch event at PlayAtCoord's coordinate, its height included, the
///   app's cue (`sound_dispatch::launch_lines_match_native`).
///
/// The player's tail is the app's (`super_selection`, `sound_dispatch`).
#[test]
fn the_launch_matches_native() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let rows = oracle["psychic_launch"].as_array().unwrap();
    for row in rows {
        let target = (int(&row["target"][0]) as u16, int(&row["target"][1]) as u16);
        let level = int(&row["level"]) as u8;
        let (rules, mut sim, owner) =
            world_with(rules(int(&row["radius"])), 64, &[(target, level)]);
        if row["bridge"] == true {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            let index = terrain.index(target.0, target.1).unwrap();
            terrain.cells_mut_for_test()[index].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        }
        let sw_type = charge_super(&mut sim, owner, REVEAL);
        let charged = row["charged"] == true;
        sim.super_weapons
            .map_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .get_mut(&sw_type)
            .unwrap()
            .is_ready = charged;
        OBSERVED.set(Some(Vec::new()));
        let launched = launch(&mut sim, &rules, owner, target.0, target.1, sw_type);
        let observed = OBSERVED.take().unwrap();
        assert_eq!(launched, charged, "{row}");

        let events = row["events"].as_array().unwrap();
        let calls: Vec<&Value> = events.iter().filter(|e| e[0] == "reveal_area").collect();
        let native: Vec<Observed> = calls
            .first()
            .map(|call| {
                // On the Map, with the Super's house, outline, unreveal and
                // line of sight 0, final 0 then 1, the same coordinate and
                // radius twice.
                assert_eq!(calls.len(), 2, "{row}");
                for (call, last) in calls.iter().zip([0, 1]) {
                    assert_eq!(call[1], true, "{row}");
                    assert_eq!(call[2], calls[0][2], "{row}");
                    assert_eq!(call[3], calls[0][3], "{row}");
                    assert_eq!(call[4], true, "{row}");
                    assert_eq!(call[5], serde_json::json!([0, 0, 0, 0]), "{row}");
                    assert_eq!(int(&call[6]), last, "{row}");
                }
                Observed::RevealArea(coord(&call[2]), int(&call[3]) as u16)
            })
            .into_iter()
            .collect();
        assert_eq!(observed, native, "{row}");

        let sound: Vec<[i32; 3]> = events
            .iter()
            .filter(|e| e[0] == "play_at")
            .map(|e| coord(&e[2]))
            .collect();
        let pushed: Vec<[i32; 3]> = sim
            .sound_events
            .iter()
            .filter_map(|event| match *event {
                SimSoundEvent::SuperWeaponLaunched {
                    owner: by,
                    sw_type: kind,
                    world,
                } if by == owner && kind == sw_type => Some([world.x, world.y, world.z]),
                _ => None,
            })
            .collect();
        assert_eq!(pushed, sound, "{row}");
    }
    assert_eq!(rows.len(), 11);
}

/// Case 11 through a production click on retail rules
/// (`PsychicRevealRadius=15`, `AllyReveal=yes`) on a map whose `Size=` is
/// the corpus's 50x50 diamond, at a cell raised four levels: the launcher's
/// map, and the map of the house it names its ally, gain exactly the cells
/// RevealArea hands its leaf in `tools/spatial_oracle/reveal_area.json`'s
/// row at (75, 50), z 416, radius 10 (the walk clamps 15 to 10:
/// `reveal_area_tests`), its centre lifted two cells. A house that names the
/// launcher its ally, unanswered, gains nothing.
#[test]
fn a_launch_maps_the_native_cells_for_its_house_and_allies() {
    let Some(rules) = retail_rules_binding(&[]) else {
        return;
    };
    assert_eq!(rules.general.psychic_reveal_radius, 15);
    let target = (75, 50);
    let (rules, mut sim, americans) = world_with(rules, 128, &[(target, 4)]);
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 50,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    }));
    sim.set_playfield_size_height_for_test(Some(50));
    let russians = sim.interner.intern("Russians");
    let yuri = sim.interner.intern("YuriCountry");
    *sim.house_alliances_mut_for_test() = [
        ("AMERICANS".to_string(), ["RUSSIANS".to_string()].into()),
        ("YURICOUNTRY".to_string(), ["AMERICANS".to_string()].into()),
    ]
    .into();

    charge_super(&mut sim, americans, REVEAL);
    click(&mut sim, &rules, americans, REVEAL, target);

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/reveal_area.json",
    ))
    .unwrap();
    let row = corpus["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["function"] == "area"
                && row["house"] == "player"
                && row["center"] == serde_json::json!([75, 50])
                && row["z"] == 416
                && row["radius"] == 10
                && row["levels"] == ""
        })
        .expect("the corpus row");
    let native: BTreeSet<(u16, u16)> = row["leaf"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .map(|pair| {
            let (x, y) = pair.split_once(',').unwrap();
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect();
    assert_eq!(native.len(), 309);
    let revealed = |house| -> BTreeSet<(u16, u16)> {
        (0..128)
            .flat_map(|y| (0..128).map(move |x| (x, y)))
            .filter(|&(x, y)| sim.fog.is_cell_revealed(house, x, y))
            .collect()
    };
    assert_eq!(revealed(americans), native);
    assert_eq!(revealed(russians), native);
    assert!(revealed(yuri).is_empty());
}
