//! The two production Firepower consumers must read the one House bias owner.
//! Arithmetic expected values come from the existing original fire/estimator
//! corpora; these checks cover the binding of that owner to both call sites.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::{CountryDifficultyBiases, RuleSet};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::world::Simulation;
use crate::util::native_x87::NativeF64Bits;

fn scene(damage: i32, firepower: u64) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nVeteranCombat=1\nVeteranArmor=1\n\
         [CombatDamage]\nMaxDamage=2147483647\n\
         [Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=MTNK\n[InfantryTypes]\n0=E1\n\
         [MTNK]\nStrength=100\nPrimary=Gun\nVeteranAbilities=FIREPOWER\n\
         [E1]\nStrength=100\nArmor=plate\nVeteranAbilities=STRONGER\n\
         [Gun]\nDamage={damage}\nWarhead=WH\nProjectile=P\n\
         [P]\nInviso=yes\n\
         [WH]\nCellSpread=0\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ))).unwrap();
    let mut sim = Simulation::new();
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let owner = sim.interner.intern(name);
        let mut house = HouseState::new(owner, 0, Some(owner), index == 0, 0, 10);
        let mut general = rules.general.clone();
        general.difficulty_rows[1].firepower = f64::from_bits(if index == 0 {
            firepower
        } else {
            17f64.to_bits()
        });
        house.set_difficulty(
            HouseDifficulty::Normal,
            &general,
            CountryDifficultyBiases::default(),
            false,
            index as i32,
            0,
        );
        sim.houses.insert(owner, house);
    }
    for (id, kind, owner, rx) in [(1, "MTNK", "Americans", 2), (2, "E1", "Russians", 3)] {
        let mut entity = GameEntity::test_default(id, kind, owner, rx, 2);
        entity.owner = sim.interner.get(owner).unwrap();
        entity.type_ref = sim.interner.intern(kind);
        // The estimator fixture's retained raw rank is2.0 (40000000).
        entity.set_veterancy_rank(200);
        entity.firepower_multiplier = NativeF64Bits::from_bits(if id == 1 {
            1f64.to_bits()
        } else {
            19f64.to_bits()
        });
        entity.armor_multiplier = NativeF64Bits::from_bits(if id == 1 {
            1f64.to_bits()
        } else {
            23f64.to_bits()
        });
        sim.substrate.entities.insert(entity);
    }
    (sim, rules)
}

#[test]
fn fire_at_uses_house_firepower_before_the_existing_native_fold() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/damage_build.json",
    ))
    .unwrap();
    let rows: Vec<_> = native["fire"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            let input = &row["input"];
            input["house_firepower"].as_u64() == Some(1.5f64.to_bits())
                && input["unit_firepower"].as_u64() == Some(1f64.to_bits())
                && input["damage"].as_i64().unwrap() > 0
                && [
                    "sonic",
                    "fire_particles",
                    "occupied",
                    "bunker_link",
                    "open_topped",
                    "veteran_firepower",
                    "elite_firepower",
                ]
                .iter()
                .all(|key| input[*key].as_u64() == Some(0))
        })
        .collect();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let (sim, rules) = scene(
            row["input"]["damage"].as_i64().unwrap() as i32,
            row["input"]["house_firepower"].as_u64().unwrap(),
        );
        let mut snap = super::super::build_attacker_snapshot(
            sim.substrate.entities.get(1).unwrap(),
            super::super::TargetKind::Entity(2),
            None,
        );
        snap.veterancy = 0;
        let actual = super::fireat_damage(
            &sim,
            &rules,
            &snap,
            rules.object("MTNK").unwrap(),
            rules.weapon("Gun").unwrap(),
            false,
        );
        assert_eq!(actual, row["damage"].as_i64().unwrap() as i32);
    }
}

#[test]
fn threat_estimate_uses_the_attacking_house_firepower() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/estimated_damage.json",
    ))
    .unwrap();
    let row = native["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "finite64_attacker_house_firepower_3fe0000000000000")
        .unwrap();
    let bits = u64::from_str_radix(
        row["input"]["attacker_house_firepower"].as_str().unwrap(),
        16,
    )
    .unwrap();
    let (sim, rules) = scene(row["input"]["weapon_damage"].as_i64().unwrap() as i32, bits);
    assert_eq!(
        super::super::estimated_damage_on(&sim, &rules, 1, 2, rules.weapon("Gun").unwrap()),
        row["result_i32"].as_i64().unwrap() as i32
    );
}
