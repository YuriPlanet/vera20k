//! Authored map inputs through retail startup, a normal Grizzly Attack, live
//! Bullet AI and the area-damage receiver. Native Bullet AI467C70..467CA4
//! decrements Health only on a continuing flight, and only above five.

use crate::rules::{ini_parser::IniFile, retail_ini_fixture};
use crate::sim::command::{Command, CommandEnvelope};

#[test]
fn map_degenerates_reduces_live_bullet_damage_before_impact() {
    for (enabled, damage) in [(false, 65), (true, 65), (true, 7)] {
        let patch = IniFile::from_str(&format!(
            "[Cannon]\nDegenerates={}\n[105mm]\nDamage={damage}\n",
            if enabled { "yes" } else { "no" },
        ));
        let Some(fixture) = retail_ini_fixture::retail_battle_rules_with_map_patch(&patch) else {
            return;
        };
        let rules = fixture.rules;
        assert_eq!(rules.projectile("Cannon").unwrap().degenerates, enabled);
        assert_eq!(rules.warhead("AP").unwrap().verses_f64[5], 1.0);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let source = sim
            .spawn_object("MTNK", "Americans", 8, 12, 64, &rules)
            .unwrap();
        let target = sim
            .spawn_object("MTNK", "Russians", 12, 12, 0, &rules)
            .unwrap();
        let owner = sim.interner.get("Americans").unwrap();
        let command = CommandEnvelope::new(
            owner,
            1,
            Command::Attack {
                attacker_id: source,
                target_id: target,
            },
        );
        let mut bullet_id = None;
        let mut expected = damage;
        let mut flight_visits = 0;
        let mut hit = false;
        for frame in 0..120 {
            let commands = if frame == 0 {
                std::slice::from_ref(&command)
            } else {
                &[]
            };
            sim.advance_tick(commands, Some(&rules), None, None, 67);
            let bullet = match bullet_id {
                Some(id) => sim.projectiles.get(id),
                None => sim
                    .projectiles
                    .iter()
                    .find_map(|(_, bullet)| (bullet.source_id() == source).then_some(bullet)),
            }
            .filter(|bullet| !sim.substrate.pending_delete.contains_object(&bullet.id()));
            if let Some(bullet) = bullet {
                if bullet_id.is_none() {
                    bullet_id = Some(bullet.id());
                }
                assert_eq!(
                    bullet.id(),
                    bullet_id.unwrap(),
                    "observe the first shot only"
                );
                flight_visits += 1;
                if enabled && expected > 5 {
                    expected -= 1;
                }
                assert_eq!(
                    bullet.payload().base_damage,
                    expected,
                    "flight visit {flight_visits}, enabled={enabled}"
                );
                if enabled && flight_visits == 2 {
                    let bytes = crate::util::save_codec::serialize(&sim.projectiles).unwrap();
                    let restored: crate::sim::projectile::ProjectileStore =
                        crate::util::save_codec::deserialize(&bytes).unwrap();
                    assert_eq!(
                        restored.get(bullet.id()).unwrap().payload().base_damage,
                        expected,
                        "a mid-flight save retains the current damage, not weapon Damage"
                    );
                }
            } else if bullet_id.is_some() {
                // Retail AP's heavy verse is exactly 100%; the target has no
                // armor modifiers. Observe the real receiver, not the fixture
                // trace for external (non-Bullet) area-damage callers.
                assert_eq!(
                    sim.substrate.entities.get(target).unwrap().health.current(),
                    300 - expected,
                    "impact does not take a further decrement"
                );
                hit = true;
                break;
            }
        }
        assert!(
            flight_visits >= 3,
            "the ordinary shot must have a continuing flight"
        );
        assert!(hit, "the shot must reach the real damage receiver");
        if damage == 7 {
            assert_eq!(expected, 5, "damage stops at five");
        }
    }
}
