//! Action42 on the authored ALL06/SOV03 records through the live dispatcher.
//! The authored records and Weapon ordinals are native goldens
//! (trigger_action_values --do-explosion); the body's calls follow
//! DoExplosionAt6E2390..6E251B.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::sim::combat::CombatLightRequest;
use crate::sim::projectile::ProjectileCoord;
use serde_json::{Value, json};

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_do_explosion.json")
}

#[test]
fn authored_waypoint_explosions_detonate_the_selected_weapon_at_the_waypoint() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    for row in native()["maps"].as_array().unwrap() {
        let map_name = row["map"].as_str().unwrap();
        let map = MapFile::from_bytes(assets.get_ref(map_name).unwrap()).unwrap();
        let named_ini = format!("{}.INI", map_name.strip_suffix(".MAP").unwrap());
        let named = assets.resolve_ref(&named_ini).map(|_| {
            crate::rules::retail_sources::select_ini(&assets, &named_ini)
                .unwrap()
                .ini
        });
        let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let (mut rules, _, _, _) = rules_owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(named.as_ref()),
                &map.ini,
            )
            .unwrap()
            .into_parts();
        // The production loader's ART and explosion closure; both maps are
        // NEWURBAN.
        let art = crate::rules::retail_sources::select_ini(&assets, "ARTMD.INI").unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art.ini));
        let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, "UBN", "NEWURBAN");

        let entry = map.actions[row["trigger_id"].as_str().unwrap()]
            .entries()
            .iter()
            .find(|entry| entry.kind == 42)
            .unwrap()
            .clone();
        let index = row["selected_weapon_index"].as_i64().unwrap() as i32;
        assert_eq!(entry.literal_value(), Some(index), "{map_name}");
        let waypoint = entry.waypoint_index.unwrap() as i32;

        let mut sim = Simulation::with_seed(0);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session
            .set_map_width_for_test(map.header.width.try_into().unwrap());
        sim.session
            .set_map_height_for_test(map.header.height.try_into().unwrap());
        // Declared actor-free prior: flat clear cells, no overlays, terrain
        // objects or receivers. Not ALL06/SOV03 topography or casualties.
        sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
            map.header.width.try_into().unwrap(),
            map.header.height.try_into().unwrap(),
            crate::map::resolved_terrain::test_flat_cell,
        ));
        crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "trigger-explosion");
        sim.session.install_waypoints(&map.waypoints);
        let rng_before = [
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ];
        let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
        let context = TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default(),
        };
        let (returned, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
            sim.apply_live_trigger_action(
                &action,
                context,
                &mut Vec::new(),
                Some(&rules),
                None,
                FrameEffects::default(),
            )
        });
        assert!(returned, "{map_name}: 6E2513 returns true");

        // 6E2431..6E2449: the live WeaponType array slot's Damage and
        // Warhead feed every consumer; no House or attacker modifiers.
        let weapon = rules.weapon_by_array_index(index).unwrap();
        let warhead = rules.warhead(weapon.warhead.as_deref().unwrap()).unwrap();
        let [x, y, z] = sim.trigger_waypoint_coord(waypoint).unwrap();
        let coord = ProjectileCoord::new(x, y, z);

        // 6E2468..6E24B8: SelectAnim's type is constructed immediately with
        // delay0, loop1, flags 2600h and 48ACE0's -15 zAdjust.
        // 6E246F skips construction when SelectAnim returns null, as for
        // ALL06's OilExplosionWH with no AnimList.
        let anims: Vec<_> = sim.anims().map(|(_, anim)| anim).collect();
        assert_eq!(
            anims.len(),
            usize::from(!warhead.anim_list.is_empty()),
            "{map_name} constructed Anim"
        );
        if let Some(&anim) = anims.first() {
            assert!(
                warhead
                    .anim_list
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(sim.interner.resolve(anim.type_id()))),
                "{map_name}: {} from {:?}",
                sim.interner.resolve(anim.type_id()),
                warhead.anim_list
            );
            assert_eq!(
                (
                    [
                        anim.world_coord().x,
                        anim.world_coord().y,
                        anim.world_coord().z
                    ],
                    anim.draw_flags(),
                    anim.z_adjust(),
                ),
                ([x, y, z], 0x2600, -15),
                "{map_name} Anim constructor"
            );
        }

        // 6E24EA Flash48A620(damage, WH, coord, 0, 0).
        assert_eq!(
            sim.combat_light_requests,
            [CombatLightRequest::Impact {
                target_id: None,
                damage: weapon.damage,
                warhead_ref: sim.interner.get(&warhead.id).unwrap(),
                coord,
                force_create: false,
                flags: 0,
            }],
            "{map_name} Flash"
        );
        // 6E250B AreaDamage489280(&coord, damage, null source, WH, 1, null House).
        assert_eq!(visits.len(), 1, "{map_name} AreaDamage visits");
        assert_eq!(
            json!([
                visits[0]["coord"],
                visits[0]["damage"],
                visits[0]["warhead"],
                visits[0]["no_source"],
                visits[0]["no_source_house"],
            ]),
            json!([[x, y, z], weapon.damage, warhead.id, true, true]),
            "{map_name} AreaDamage"
        );

        // An empty crop and a non-EMEffect AnimList draw nothing.
        assert_eq!(
            [
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex(),
            ],
            rng_before,
            "{map_name} RNG streams"
        );
    }
}
