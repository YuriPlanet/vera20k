//! Consuming delivery of receiver consequences at explicit world boundaries.
//!
//! VERA-internal ownership protocol, gamemd equivalent UNCHECKED. This preserves
//! the existing ordinary/immediate schedules; it does not relocate native damage
//! callbacks, radiation selection, death-sound selection, or the shared ID allocator.

use super::{SimFireEvent, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{DeathEffects, UnderAttackEvent};
use crate::sim::intern::InternedId;
use crate::sim::pathfinding::PathGrid;
use std::sync::Arc;

enum DamageDelivery {
    Immediate,
    Ordinary { fire_events: Vec<SimFireEvent> },
}

/// Pending work has one owner and is consumed once by its world commit.
#[must_use]
pub(crate) struct DamageConsequences {
    effects: DeathEffects,
    terrain_navigation_changed_cells: Vec<(u16, u16)>,
    delivery: DamageDelivery,
}

pub(super) struct DamageCommitReceipt {
    pub(super) structure_destroyed: bool,
    pub(super) bridge_state_changed: bool,
    pub(super) path_grid: Option<Arc<PathGrid>>,
}

impl DamageConsequences {
    pub(super) fn finish_navigation(&mut self, cells: Vec<(u16, u16)>) {
        self.terrain_navigation_changed_cells = cells;
    }

    #[cfg(test)]
    pub(crate) fn effects(&self) -> &DeathEffects {
        &self.effects
    }

    /// Fold a combat fixture's tail detonations into this frame's effects.
    #[cfg(test)]
    pub(crate) fn append_tail_for_test(
        &mut self,
        mut effects: DeathEffects,
        under_attack_events: Vec<UnderAttackEvent>,
    ) {
        effects.under_attack_events = under_attack_events;
        self.effects.append(effects);
    }

    #[cfg(test)]
    pub(crate) fn fire_events(&self) -> &[SimFireEvent] {
        match &self.delivery {
            DamageDelivery::Ordinary { fire_events, .. } => fire_events,
            DamageDelivery::Immediate => &[],
        }
    }

    pub(super) fn immediate(
        mut effects: DeathEffects,
        under_attack_events: Vec<UnderAttackEvent>,
        terrain_navigation_changed_cells: Vec<(u16, u16)>,
    ) -> Self {
        // The caller's ordered receiver pings are the existing delivery source.
        effects.under_attack_events = under_attack_events;
        Self {
            effects,
            terrain_navigation_changed_cells,
            delivery: DamageDelivery::Immediate,
        }
    }

    pub(crate) fn ordinary(
        mut effects: DeathEffects,
        under_attack_events: Vec<UnderAttackEvent>,
        terrain_navigation_changed_cells: Vec<(u16, u16)>,
        fire_events: Vec<SimFireEvent>,
    ) -> Self {
        // Ordinary radiation and death sounds already crossed their earlier
        // receiver barriers. Disabled fixture sinks formerly discarded them;
        // carrying the accumulator whole must not replay those leftovers here.
        effects.rad_detonations.clear();
        effects.death_sounds.clear();
        effects.under_attack_events = under_attack_events;
        Self {
            effects,
            terrain_navigation_changed_cells,
            delivery: DamageDelivery::Ordinary { fire_events },
        }
    }

    pub(super) fn commit(
        self,
        world: &mut Simulation,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        fallback_path_grid: Option<&PathGrid>,
    ) -> DamageCommitReceipt {
        let Self {
            mut effects,
            terrain_navigation_changed_cells,
            delivery,
        } = self;
        let ordinary = matches!(&delivery, DamageDelivery::Ordinary { .. });
        // Capture owner/category now: ordinary delivery follows SpawnManager,
        // while immediate delivery finishes before its caller's next live cursor.
        let dead_infos: Vec<(InternedId, EntityCategory)> = effects
            .despawned_ids
            .iter()
            .filter_map(|&dead_id| {
                world
                    .substrate
                    .entities
                    .get(dead_id)
                    .map(|entity| (entity.owner(), entity.category))
            })
            .collect();

        for &dead_id in &effects.immediate_uninit_ids {
            if world
                .substrate
                .entities
                .get(dead_id)
                .and_then(|building| building.bunker_occupant)
                .is_some()
            {
                crate::sim::docking::bunker_link::release_sell_destroy(world, dead_id);
            }
            world.release_move_sound(dead_id);
            world.uninit_with_rules(dead_id, rules);
        }

        // Apply_area_damage already completed its bridge callbacks before
        // returning to the caller's animation/cluster tail. Delivery carries
        // only the resulting frame notification, never deferred gameplay.
        let bridge_state_changed = effects.bridge_state_changed;
        debug_assert!(effects.tiberium_reduction_requests.is_empty());
        for request in &effects.tiberium_reduction_requests {
            world.reduce_tiberium_at_with_native_context(
                (request.rx, request.ry),
                request.amount,
                Some(rules),
                overlay_registry,
            );
        }
        for detonation in effects.rad_detonations.drain(..) {
            world.radiation.apply_detonation(
                detonation,
                world.session.binary_frame,
                &rules.radiation,
                world.resolved_terrain.as_ref(),
            );
        }

        let path_grid = world.finish_terrain_navigation_changes(
            fallback_path_grid,
            &terrain_navigation_changed_cells,
        );
        if world.session.game_options.super_weapons && effects.structure_destroyed {
            let mut refreshed = Vec::new();
            for &(owner, category) in &dead_infos {
                if category == EntityCategory::Structure && !refreshed.contains(&owner) {
                    refreshed.push(owner);
                    crate::sim::superweapon::refresh_super_weapons_for_owner(world, rules, owner);
                }
            }
        }

        world.admit_death_debris(std::mem::take(&mut effects.voxel_debris));
        // Explosion animations from the completed receiver transaction.
        // `AnimClass` instances, not legacy world effects: only the real
        // constructor reaches `AnimClass::Start @ 0x00424CE0`, which is what
        // plays the art type's `Report=`/`StartSound=`, and only the real
        // AnimType carries its `Translucent=` and `Rate=`.
        for fx in std::mem::take(&mut effects.explosion_effects) {
            match fx.death {
                Some(spawn) => world.admit_death_anim(rules, fx.shp_name, spawn),
                None => {
                    world.spawn_combat_explosion_anim(
                        rules,
                        fx.shp_name,
                        fx.rx,
                        fx.ry,
                        fx.sub_x,
                        fx.sub_y,
                        fx.z,
                        fx.world_z,
                    );
                }
            }
        }
        if let DamageDelivery::Ordinary { fire_events, .. } = &delivery {
            admit_electric_sparks(world, rules, fire_events);
        }
        world
            .combat_light_requests
            .append(&mut effects.combat_light_requests);
        // RevealOnFire only lifts the shooter's shroud above. gamemd reaches
        // `CreateRadarEvent @ 0x0065FA70` from no weapon-fire path: its 24
        // constant-type call sites never pass type 0 (the 25th,
        // `TriggerAction::Execute`, takes its type from map data), and the one
        // in `BulletClass::AI` (`0x00467EA7`) is the silent type 13 of the
        // `NUKE` payload.
        if let DamageDelivery::Ordinary { fire_events, .. } = delivery {
            world.fire_events.extend(fire_events);
        }
        for (die_sound_id, rx, ry) in effects.death_sounds.drain(..) {
            world.sound_events.push(SimSoundEvent::EntityDied {
                die_sound_id,
                rx,
                ry,
            });
        }
        world.dispatch_under_attack_events(&effects.under_attack_events);
        world.dispatch_unit_lost_events(&effects.unit_lost_events);
        debug_assert!(effects.smudge_spawn_requests.is_empty());
        if ordinary {
            debug_assert!(world.pending_smudge_requests.is_empty());
            world.flush_smudge_dirty();
            world.pending_smudge_requests.clear();
        } else {
            world
                .pending_smudge_requests
                .append(&mut effects.smudge_spawn_requests);
        }
        DamageCommitReceipt {
            structure_destroyed: effects.structure_destroyed,
            bridge_state_changed,
            path_grid,
        }
    }
}

/// `AnimClass` draw flags of a muzzle animation (`PUSH 0x600`, `0x006FF3B1`).
const MUZZLE_ANIM_DRAW_FLAGS: u32 = 0x600;

/// Construct one shot's muzzle animation, inside FireAt after GetROF.
///
/// gamemd-derived, the tail of `TechnoClass::Fire_At` (`0x006FF394..0x006FF43F`):
/// `AnimClass(type, &fireCoord, delay 0, loopCount 1, drawFlags 0x600,
/// zAdjust 0, reverse 0)` at `0x006FF3C2`. A building firer (`WhatAmI == 6`)
/// then stores the anim's `ZAdjust` (`+0x100`); every other firer becomes the
/// anim's owner through `AnimClass::SetOwnerObject @ 0x00424B50`, so the flash
/// rides a moving or airborne firer. Weapon `Anim=` and occupant
/// `OccupantAnim=` flashes are this one block; the type is chosen by
/// `fire_coord::muzzle_anim_name`.
///
/// An art type that never bound constructs nothing, as elsewhere in the store.
///
pub(crate) fn admit_muzzle_anim(world: &mut Simulation, rules: &RuleSet, event: &SimFireEvent) {
    let Some(type_name) = event.muzzle_anim else {
        return;
    };
    let coord = crate::sim::anim_class::AnimWorldCoord {
        x: event.fire_coord.x,
        y: event.fire_coord.y,
        z: event.fire_coord.z,
    };
    let is_building = event.firer_category == EntityCategory::Structure;
    let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
        delay: 0,
        loop_count: 1,
        draw_flags: MUZZLE_ANIM_DRAW_FLAGS,
        z_adjust: if is_building {
            crate::sim::combat::fire_coord::building_muzzle_z_adjust(
                event.fire_offset_y,
                event.occupied_building,
            )
        } else {
            0
        },
        reverse: false,
        ..crate::sim::components::AnimClassSpawnDescriptor::new(
            type_name, rx, ry, sub_x, sub_y, level,
        )
    };
    match world.spawn_anim_at_world(rules, descriptor, coord) {
        Ok(anim_id) => {
            if !is_building {
                world.set_anim_owner_object(anim_id, Some(event.attacker_id), rules);
            }
        }
        Err(error) => log::debug!(
            "muzzle anim [{}] did not construct: {error}",
            world.interner.resolve(type_name)
        ),
    }
}

fn admit_electric_sparks(world: &mut Simulation, rules: &RuleSet, fire_events: &[SimFireEvent]) {
    // gamemd-derived: `EBolt::Init @ 0x004C2A60` creates one spark
    // system per electric bolt at `0x004C2B30`, passing
    // `Rules+0x1020` (`[CombatDamage] DefaultSparkSystem`) and the
    // bolt's TARGET endpoint — `EBolt+0x0C..0x14`, which
    // `TechnoClass::CreateElectricBolt @ 0x006FD516` fills from the
    // target object's coordinate virtual, and which `EBolt::Init`
    // then hands the constructor by address. Owner house,
    // attachment object and target object are all NULL; the
    // fallback aim coordinate is `0x008A0E50`, a static all-zero
    // triple, and `AI_Spark` never reads it. The handle is
    // discarded: the system lives on the global particle list and
    // expires on its own `Lifetime`.
    //
    // The path consumes no `ScenarioClass::Random` draws.
    // `EBolt::Init` does take one `RandomRanged(0, 0x100)` at
    // `0x004C2AA3`, but on the cosmetic `RandomClass` at
    // `0x00886B88` — the one `LaserDrawClass::Draw` and
    // `ThemeClass::Next_Song` also use — not the lockstep scenario
    // stream, so it is not modelled here.
    //
    // VERA-internal ordering, gamemd equivalent UNCHECKED: native
    // constructs the system inside `Fire_At`, i.e. during the
    // firer's own AI. Whether that means it is visited by the SAME
    // frame's object walk is UNCHECKED — the constructor's two
    // appends (`0x0062DD7A` into the ParticleSystemClass instance
    // registry at `0x00A80208`, and `0x0062DEF6` into the abstracts
    // registry at `0x00B0F730`) are neither of them the per-frame
    // walker, and the walker itself was not identified. This engine
    // creates it in the post-combat walk that already admits Sonic
    // and Magnetron waves from the same event list — after the
    // logic walk — so its first burst lands no earlier than
    // native's, and one frame later if native does visit
    // same-frame. Bolt rendering itself is not implemented; the
    // sparks are the part of the discharge that is a simulation
    // object.
    if let Some(spark_system_name) = rules.combat_damage.default_spark_system.as_deref() {
        for event in fire_events {
            let Some(weapon) = rules.weapon(world.interner.resolve(event.weapon_id)) else {
                continue;
            };
            if !weapon.is_electric_bolt {
                continue;
            }
            let Some(system_type) = rules.ps_type_id_by_name(spark_system_name) else {
                continue;
            };
            let coords = match event.target {
                crate::sim::combat::TargetKind::Entity(id) => {
                    let Some(entity) = world.substrate.entities.get(id) else {
                        continue;
                    };
                    glam::IVec3::new(
                        i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
                        i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
                        i32::from(entity.position.z)
                            * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
                    )
                }
                // VERA-internal, gamemd equivalent UNCHECKED: native
                // takes the bolt endpoint from the TARGET OBJECT's
                // coordinate virtual, and a cell target has no object
                // to ask, so ground height is not folded in here.
                crate::sim::combat::TargetKind::Cell(rx, ry) => {
                    glam::IVec3::new(i32::from(rx) * 256 + 128, i32::from(ry) * 256 + 128, 0)
                }
            };
            world.spawn_particle_system(
                system_type,
                coords,
                None,
                None,
                glam::IVec3::ZERO,
                None,
                rules,
            );
        }
    }
}

#[cfg(test)]
mod muzzle_anim_tests {
    use super::*;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::anim_class::AnimWorldCoord;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::projectile::ProjectileCoord;

    fn fixture(category: EntityCategory) -> (Simulation, RuleSet, u64) {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=TOWER\n[TANK]\nStrength=100\n[TOWER]\nStrength=100\n",
        ))
        .expect("rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str("[GUNFIRE]\nRate=900\n"));
        art.bind_anim_frame_count_for_test("GUNFIRE", 6);
        rules.merge_art_data(&art);
        rules.art_registry = art;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("A");
        let type_ref = sim
            .interner
            .intern(if category == EntityCategory::Structure {
                "TOWER"
            } else {
                "TANK"
            });
        let id = sim.allocate_stable_id();
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                10,
                10,
                0,
                0,
                owner,
                Health { current: 100 },
                type_ref,
                category,
                0,
                5,
                false,
            ));
        (sim, rules, id)
    }

    fn shot(sim: &mut Simulation, attacker_id: u64, category: EntityCategory) -> SimFireEvent {
        let mut event = SimFireEvent::for_test(attacker_id);
        event.firer_category = category;
        event.muzzle_anim = Some(sim.interner.intern("GUNFIRE"));
        event.fire_coord = ProjectileCoord::new(10 * 256 + 200, 10 * 256 + 60, 105);
        event
    }

    /// `0x006FF3C2` then `0x006FF43A`: the constructor row, then
    /// `SetOwnerObject(firer)` for anything that is not a building.
    #[test]
    fn a_units_muzzle_anim_is_built_at_the_fire_coordinate_and_rides_the_firer() {
        let (mut sim, rules, tank) = fixture(EntityCategory::Unit);
        let event = shot(&mut sim, tank, EntityCategory::Unit);
        admit_muzzle_anim(&mut sim, &rules, &event);

        let (id, anim) = sim.substrate.anims.iter().next().expect("one muzzle anim");
        let id = *id;
        assert_eq!(sim.interner.resolve(anim.type_id), "GUNFIRE");
        assert_eq!((anim.draw_flags, anim.z_adjust), (0x600, 0));
        assert_eq!(anim.runtime.loop_remaining, 1);
        assert_eq!(anim.owner_entity, Some(tank));
        let at_fire = AnimWorldCoord {
            x: 10 * 256 + 200,
            y: 10 * 256 + 60,
            z: 105,
        };
        assert_eq!(sim.anim_absolute_coord(id), Some(at_fire));

        // The firer drives one cell east and climbs: the flash goes with it.
        {
            let firer = sim.substrate.entities.get_mut(tank).unwrap();
            firer.position.rx += 1;
            firer.position.exact_z_leptons = Some(70);
        }
        assert_eq!(
            sim.anim_absolute_coord(id),
            Some(AnimWorldCoord {
                x: at_fire.x + 256,
                y: at_fire.y,
                z: at_fire.z + 70,
            })
        );
    }

    /// `0x006FF3D9..0x006FF427`: a building's anim is not attached; it takes a
    /// `ZAdjust` from the fire offset, or -200 when occupants fired.
    #[test]
    fn a_buildings_muzzle_anim_stays_put_and_takes_the_native_z_adjust() {
        let (mut sim, rules, tower) = fixture(EntityCategory::Structure);
        let mut own_weapon = shot(&mut sim, tower, EntityCategory::Structure);
        own_weapon.fire_offset_y = 130;
        let mut occupants = own_weapon.clone();
        occupants.occupied_building = true;
        for event in [own_weapon, occupants] {
            admit_muzzle_anim(&mut sim, &rules, &event);
        }

        let anims: Vec<_> = sim.substrate.anims.iter().map(|(_, anim)| anim).collect();
        assert_eq!(anims.len(), 2);
        assert!(anims.iter().all(|anim| anim.owner_entity.is_none()));
        assert_eq!(anims[0].z_adjust, -32);
        assert_eq!(anims[1].z_adjust, -200);
    }

    /// Firer expiry425150 removes Display and clears ownership without
    /// converting stored relative coordinates. The hidden flash remains in
    /// storage until its own AI reaches Destroy. Native owner histories are
    /// preserved in tools/spatial_oracle/display_anim_owner.json.
    #[test]
    fn firer_expiry_hides_the_flash_without_converting_its_relative_coordinate() {
        let (mut sim, rules, tank) = fixture(EntityCategory::Unit);
        sim.reveal(tank);
        let event = shot(&mut sim, tank, EntityCategory::Unit);
        admit_muzzle_anim(&mut sim, &rules, &event);
        let id = *sim.substrate.anims.iter().next().expect("muzzle anim").0;
        let relative = sim.anim(id).unwrap().world_coord;

        sim.uninit_with_rules(tank, &rules);

        let anim = sim.anim(id).expect("the anim is still stored this frame");
        assert_eq!(anim.owner_entity, None, "the teardown detached it");
        assert!(anim.runtime.inactive, "and marked it for removal");
        assert_eq!(
            sim.anim_absolute_coord(id),
            Some(relative),
            "expiry preserves the stored delta after clearing the owner"
        );
        assert_eq!(sim.substrate.display.layer_of(id), None);
    }

    #[test]
    fn a_shot_without_a_bound_muzzle_type_constructs_nothing() {
        let (mut sim, rules, tank) = fixture(EntityCategory::Unit);
        let mut unbound = shot(&mut sim, tank, EntityCategory::Unit);
        unbound.muzzle_anim = Some(sim.interner.intern("NOSUCHANIM"));
        let mut none = unbound.clone();
        none.muzzle_anim = None;
        for event in [unbound, none] {
            admit_muzzle_anim(&mut sim, &rules, &event);
        }
        assert_eq!(sim.substrate.anims.iter().count(), 0);
    }
}
