//! Phobos `Anim.Update`'s per-frame `UpdateAsFiringAnim` pass
//! (`Ext/Anim/Body.cpp:72..105`, early-AI hook `0x423B95`,
//! `Ext/Anim/Hooks.cpp:714-740`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::FacingClass;
use crate::sim::world::{FrameEffects, Simulation};

/// `GeneralUtils::GetItemForDirection` (`Utilities/GeneralUtils.h:66..93`).
#[cfg(feature = "phobos")]
#[test]
fn item_for_direction_matches_the_native_bit_formula() {
    use crate::sim::unit_simple_deploy::item_for_direction;
    // 8 entries: `(dir8 + 1) & 7`, dir8 = raw >> 13.
    assert_eq!(item_for_direction(8, 0x0000), 1);
    assert_eq!(item_for_direction(8, 0x2000), 2);
    assert_eq!(item_for_direction(8, 0xE000), 0);
    // 16 entries: bitsTo = 4, index = ((raw >> 11) + 1) >> 1 + 2.
    assert_eq!(item_for_direction(16, 0x0000), 2);
    assert_eq!(item_for_direction(16, 0x0800), 3);
    // Fewer than 8 answers the first item whatever the direction.
    for count in [1, 2, 3, 4, 5, 7] {
        assert_eq!(item_for_direction(count, 0x4000), 0, "{count}");
    }
}

/// The owner-relative anchor `GetFLH(0) - GetRenderCoords` the pass writes.
#[cfg(feature = "phobos")]
fn expected_anchor(sim: &Simulation, rules: &RuleSet) -> AnimWorldCoord {
    let entity = sim.substrate.entities.get(1).unwrap();
    let source = crate::sim::combat::fire_coord::FireSource::of_entity(entity);
    let obj = sim.interner.object_type(entity.type_ref(), rules).unwrap();
    let coords = crate::sim::movement::ground_pose::object_get_coords(entity, None);
    let fire = crate::sim::combat::fire_coord::fire_coordinate(
        sim,
        rules,
        &source,
        obj,
        0,
        0,
        crate::rules::flh::Flh::default(),
    );
    AnimWorldCoord {
        x: fire.coord.x.wrapping_sub(coords.x),
        y: fire.coord.y.wrapping_sub(coords.y),
        z: fire.coord.z.wrapping_sub(coords.z),
    }
}

/// One marked muzzle anim: the recorded state starts zero (only the pass
/// writes it), a facing change re-picks the type from the weapon's `Anim=`
/// list, and a facing or owner-coordinate change re-anchors the stored
/// location to `GetFLH(weaponIndex) - GetRenderCoords`. Destroying the anim
/// releases the count; the gate stops the pass when it reaches zero.
#[cfg(feature = "phobos")]
#[test]
fn the_firing_anim_pass_retypes_and_reanchors() {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n\
         [TANK]\nStrength=100\nSpeed=5\nPrimary=GUN\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         Anim=F0,F1,F2,F3,F4,F5,F6,F7\nAnim.Update=yes\n\
         [Warheads]\n0=WH\n[WH]\nCellSpread=0\n\
         [Animations]\n0=F0\n1=F1\n2=F2\n3=F3\n4=F4\n5=F5\n6=F6\n7=F7\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("firing-anim rules");
    // `F3` stays unbound so a retype to it is refused.
    let art = IniFile::from_str(
        "[F0]\nRate=1\n[F1]\nRate=1\n[F2]\nRate=1\n\
         [F4]\nRate=1\n[F5]\nRate=1\n[F6]\nRate=1\n[F7]\nRate=1\n",
    );
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    for name in ["F0", "F1", "F2", "F4", "F5", "F6", "F7"] {
        registry.bind_anim_frame_count_for_test(name, 20);
    }
    rules.replace_art_registry_for_test(registry);

    let mut sim = Simulation::with_seed(3);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner.intern("Neutral");
    let type_ref = sim.interner.intern("TANK");
    let mut entity = GameEntity::test_default(1, "TANK", "Neutral", 10, 10);
    entity.set_owner_for_test(owner);
    entity.set_type_ref_for_test(type_ref);
    entity.health.set_for_test(100);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked_for_test(true);
    entity.body_facing = FacingClass::new(0, 0);
    entity.barrel_facing = Some(FacingClass::new(0, 0));
    sim.substrate.entities.insert(entity);

    let coords = {
        let entity = sim.substrate.entities.get(1).unwrap();
        crate::sim::movement::ground_pose::object_get_coords(entity, None)
    };
    let owner_coords = AnimWorldCoord {
        x: coords.x,
        y: coords.y,
        z: coords.z,
    };
    let type_id = sim.interner.intern("F0");
    let (rx, ry, sub_x, sub_y, z) = owner_coords.to_cell_sub_z();
    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
        owner: Some(1),
        ..crate::sim::components::AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
    };
    let anim = sim
        .spawn_anim_at_world(&rules, descriptor.clone(), owner_coords)
        .expect("muzzle anim");
    let gun = sim.interner.intern("GUN");
    sim.attach_firing_anim(
        anim,
        FiringAnimState {
            weapon: gun,
            weapon_index: 0,
            burst_index: 0,
            last_facing: 0,
            last_coords: AnimWorldCoord { x: 0, y: 0, z: 0 },
        },
    );
    assert_eq!(sim.firing_anim_update_count(), 1);

    // Facing 0 equals the recorded 0, but the recorded coordinate is zero:
    // the first visit only re-anchors and keeps the type.
    sim.visit_anim(anim, &rules, None, FrameEffects::default())
        .expect("the firing anim stays in domain");
    assert_eq!(
        sim.interner.resolve(sim.anim(anim).unwrap().type_id()),
        "F0"
    );
    assert_eq!(
        sim.anim(anim).unwrap().world_coord(),
        expected_anchor(&sim, &rules)
    );

    // Turn the barrel east: dir8 = 1 picks index 2, and the facing change
    // re-anchors too.
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing = Some(FacingClass::new(0x2000, 0));
    sim.visit_anim(anim, &rules, None, FrameEffects::default())
        .expect("the firing anim stays in domain");
    assert_eq!(
        sim.interner.resolve(sim.anim(anim).unwrap().type_id()),
        "F2",
        "the east direction selects the third 8-way entry"
    );
    assert_eq!(
        sim.anim(anim).unwrap().world_coord(),
        expected_anchor(&sim, &rules)
    );

    // Dir8 = 2 would pick `F3`, but that type has no art config: the retype
    // is refused and the current type stays.
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing = Some(FacingClass::new(0x4000, 0));
    sim.visit_anim(anim, &rules, None, FrameEffects::default())
        .expect("the firing anim stays in domain");
    assert_eq!(
        sim.interner.resolve(sim.anim(anim).unwrap().type_id()),
        "F2"
    );

    // Destruction releases the count through the deferred removal funnel.
    sim.destroy_anim_with_context(anim, &rules);
    sim.flush_pending_delete();
    assert_eq!(sim.firing_anim_update_count(), 0);

    // With the count at zero the early hook skips a marked anim.
    let other = sim
        .spawn_anim_at_world(&rules, descriptor.clone(), owner_coords)
        .expect("second muzzle anim");
    sim.substrate
        .anims
        .get_mut(other)
        .unwrap()
        .set_firing_anim(FiringAnimState {
            weapon: gun,
            weapon_index: 0,
            burst_index: 0,
            last_facing: 0,
            last_coords: AnimWorldCoord { x: 0, y: 0, z: 0 },
        });
    let before = sim.anim(other).unwrap().world_coord();
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing = Some(FacingClass::new(0xE000, 0));
    sim.visit_anim(other, &rules, None, FrameEffects::default())
        .expect("the second anim stays in domain");
    assert_eq!(
        sim.interner.resolve(sim.anim(other).unwrap().type_id()),
        "F0",
        "the count gate suppresses the pass"
    );
    assert_eq!(sim.anim(other).unwrap().world_coord(), before);
}
