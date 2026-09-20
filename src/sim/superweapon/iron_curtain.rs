//! IronCurtain superweapon launch handler.
//!
//! Applies timed invulnerability to all techno entities in a 3×3 cell grid
//! centered on the target cell. Infantry receive forced authored-Strength damage
//! through the InfantryClass::IronCurtain override.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/superweapon/{invulnerability,cell_grid},
//!   sim/game_entity, sim/components, sim/world.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::superweapon::cell_grid::{live_successor, native_cells_3x3, selected_cell_list};
use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
use crate::sim::world::{SimSoundEvent, Simulation};

/// Launch IronCurtain at (target_rx, target_ry). Applies invulnerability or
/// forced authored-Strength damage to infantry in the target’s 3×3 cell grid.
pub fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    sw_type: InternedId,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let duration = rules.general.iron_curtain_duration;
    let anim_name = rules.general.iron_curtain_invoke_anim.clone();
    let current_frame = sim.session.binary_frame;

    // 1. Spawn invoke animation at target.
    super::spawn_cell_anim(sim, rules, &anim_name, target_rx, target_ry, true);

    // SuperClass::Launch case 1 (0x006CCF39..0x006CD035) selects a live
    // CellClass list, invokes +0x154, then reads that object's +0x30 AFTER the
    // call. Never snapshot recipients or infer membership from coordinates.
    // Native also skips external chrono-warp latch +0x27C on Technos. Its
    // ChronoWarp/action-128 producers have no current Rust implementation; do
    // not substitute ordinary teleport_state, whose native path leaves it clear.
    let mut target_count = 0;
    for (x, y) in native_cells_3x3(target_rx, target_ry) {
        let Some(((rx, ry), layer)) = selected_cell_list(sim, x, y) else {
            continue;
        };
        let mut next = sim
            .substrate
            .occupancy
            .get(rx, ry)
            .and_then(|cell| cell.first_on_layer(layer));
        while let Some(id) = next {
            if let Some(entity) = sim.substrate.entities.get(id) {
                let category = entity.category;
                let type_ref = entity.type_ref();
                if category == EntityCategory::Infantry {
                    // InfantryClass::IronCurtain 0x00522600..0x0052263B:
                    // full authored Strength, distance 0, C4Warhead, null
                    // attacker, ignoreDefenses=1, arg6=0, launching sourceHouse.
                    // The shared receiver owns fatal effects and announcements.
                    if let Some(object) = rules.object(sim.interner.resolve(type_ref)) {
                        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                            id,
                            object.strength,
                            0,
                            crate::sim::combat::RAD_NO_ATTACKER,
                            Some(owner),
                            sim.interner.intern(&rules.bridge_warheads.c4_name),
                            crate::sim::combat::ReceiverCallFlags {
                                ignore_defenses: true,
                                arg6: false,
                            },
                        );
                        sim.commit_direct_damage_receiver(rules, overlay_registry, event);
                    }
                } else if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    // TechnoClass::IronCurtain 0x0070E2B0 has no health gate.
                    apply_invulnerability(entity, current_frame, duration, InvulnKind::IronCurtain);
                }
                target_count += 1;
            }
            next = live_successor(sim, id, (rx, ry), layer);
        }
    }

    // 4. Sound event.
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: target_rx,
        ry: target_ry,
    });

    log::info!(
        "IronCurtain launched at ({}, {}) by '{}', {} targets affected",
        target_rx,
        target_ry,
        sim.interner.resolve(owner),
        target_count
    );

    true
}

/// Spawn the invoke animation at the target cell.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::superweapon::invulnerability::is_invulnerable;

    fn test_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nStrength=125\nArmor=flak\nSpeed=4\n\
             [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
             [CombatDamage]\nC4Warhead=C4\n[Warheads]\n0=C4\n\
             [C4]\nInfDeath=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        RuleSet::from_ini(&ini).expect("test rules")
    }

    /// `SuperClass::Launch 0x006CCF09`: the invoke animation is a real
    /// `AnimClass` with the row `(type, &coord, 0, 1, 0x600, 0, 0)`, so its art
    /// `Report=` plays from `AnimClass::Start` (retail `[IRONBLST]
    /// Report=IronCurtainBlast`, which no separate launch cue carries).
    #[test]
    fn launch_constructs_the_invoke_anim_and_plays_its_report() {
        let mut rules = test_rules();
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[IRONBLST]
Rate=450
Report=IronCurtainBlast
Translucent=yes
",
        ));
        art.bind_anim_frame_count_for_test("IRONBLST", 12);
        rules.art_registry = art;
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        spawn(&mut sim, 1, "MTNK", 10, 10, EntityCategory::Unit);
        let sw_test = sim.interner.intern("SWTEST");

        assert!(launch(&mut sim, &rules, owner, 10, 10, sw_test, None));

        let invoke = sim.interner.intern("IRONBLST");
        let anims: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| anim)
            .filter(|anim| anim.type_id == invoke)
            .collect();
        assert_eq!(anims.len(), 1);
        let (rx, ry, ..) = anims[0].world_coord.to_cell_sub_z();
        assert_eq!((rx, ry), (10, 10));
        assert_eq!(anims[0].draw_flags, 0x600);
        assert_eq!(anims[0].z_adjust, 0);
        let report = sim.interner.intern("IronCurtainBlast");
        assert!(
            sim.sound_events.iter().any(|event| matches!(
                event,
                SimSoundEvent::AnimationStarted { sound_id, .. } if *sound_id == report
            )),
            "AnimClass::Start plays the art Report="
        );
    }

    fn spawn(sim: &mut Simulation, id: u64, type_ref: &str, rx: u16, ry: u16, cat: EntityCategory) {
        let rules = test_rules();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.playfield_bounds = Some(super::super::cell_receiver_tests::test_playfield_bounds());
        if sim.resolved_terrain.is_none() {
            let cells = (0..20)
                .flat_map(|y| {
                    (0..20).map(move |x| super::super::cell_receiver_tests::test_terrain_cell(x, y))
                })
                .collect();
            sim.resolved_terrain =
                Some(crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(20, 20, cells));
        }
        let actual = sim
            .spawn_object_at_height(type_ref, "Americans", rx, ry, 0, 0, &rules)
            .expect("production constructor and Mark");
        assert_eq!(actual, id);
        assert_eq!(sim.substrate.entities.get(id).unwrap().category, cat);
        assert!(sim.substrate.occupancy.contains_entity(rx, ry, id));
    }

    #[test]
    fn ic_protects_vehicles_in_grid() {
        let rules = test_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        spawn(&mut sim, 1, "MTNK", 10, 10, EntityCategory::Unit);
        let sw_test = sim.interner.intern("SWTEST");
        assert!(launch(&mut sim, &rules, owner, 10, 10, sw_test, None));
        let e = sim.substrate.entities.get(1).expect("tank exists");
        assert!(e.invulnerability.is_some());
        assert!(is_invulnerable(
            e.invulnerability.as_ref(),
            sim.session.binary_frame
        ));
    }

    #[test]
    fn ic_kills_infantry_in_grid() {
        let rules = test_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        spawn(&mut sim, 1, "E1", 10, 10, EntityCategory::Infantry);
        let sw_test = sim.interner.intern("SWTEST");
        assert!(launch(&mut sim, &rules, owner, 10, 10, sw_test, None));
        let e = sim.substrate.entities.get(1).expect("infantry exists");
        assert_eq!(e.health.current, 0);
        assert!(e.dying);
        assert!(e.invulnerability.is_none());
    }

    /// `InfantryClass::IronCurtain @ 0x00522632` kills through `ReceiveDamage`
    /// (`+0x16C`, `C4Warhead=`), so the death reaches `Death_Announcement`
    /// (`+0x3B8`): each human-owned kill publishes the radar type-7 request
    /// (`0x004D98FE`) whose client-side 8-cell dedupe limits "Unit lost".
    #[test]
    fn ic_infantry_kill_publishes_unit_lost_per_human_death() {
        use crate::sim::house_state::HouseState;

        let rules = test_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            HouseState::new(owner, 0, Some(owner), true, 5_000, 10),
        );
        sim.session.house_order.push(owner);
        spawn(&mut sim, 1, "E1", 10, 10, EntityCategory::Infantry);
        spawn(&mut sim, 2, "E1", 11, 11, EntityCategory::Infantry);
        spawn(&mut sim, 3, "MTNK", 9, 9, EntityCategory::Unit);
        let sw_test = sim.interner.intern("SWTEST");
        assert!(launch(&mut sim, &rules, owner, 10, 10, sw_test, None));

        let lost: Vec<InternedId> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::UnitLost { owner, .. } => Some(*owner),
                _ => None,
            })
            .collect();
        assert_eq!(
            lost,
            vec![owner, owner],
            "both infantry kills reach Death_Announcement; the vehicle survives"
        );
        assert!(sim.substrate.entities.get(1).unwrap().dying);
        assert!(sim.substrate.entities.get(2).unwrap().dying);
        assert!(!sim.substrate.entities.get(3).unwrap().dying);
    }

    #[test]
    fn ic_affects_both_diagonals_and_center() {
        let rules = test_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        spawn(&mut sim, 1, "MTNK", 9, 9, EntityCategory::Unit);
        spawn(&mut sim, 2, "MTNK", 10, 10, EntityCategory::Unit);
        spawn(&mut sim, 3, "MTNK", 11, 11, EntityCategory::Unit);
        let sw_test = sim.interner.intern("SWTEST");
        launch(&mut sim, &rules, owner, 10, 10, sw_test, None);
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .invulnerability
                .is_some()
        );
        assert!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .invulnerability
                .is_some()
        );
        assert!(
            sim.substrate
                .entities
                .get(3)
                .unwrap()
                .invulnerability
                .is_some()
        );
    }

    #[test]
    fn ic_ignores_cells_outside_grid() {
        let rules = test_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        spawn(&mut sim, 1, "MTNK", 15, 15, EntityCategory::Unit);
        let sw_test = sim.interner.intern("SWTEST");
        launch(&mut sim, &rules, owner, 10, 10, sw_test, None);
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .invulnerability
                .is_none()
        );
    }
}
