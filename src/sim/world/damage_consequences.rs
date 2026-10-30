//! Consuming delivery of receiver consequences at explicit world boundaries.
//!
//! VERA-internal ownership protocol, gamemd equivalent UNCHECKED. Callers own
//! live-object or remaining global delivery timing; this module does not choose
//! damage callbacks, radiation targets, death sounds, or shared object IDs.

use super::{FrameEffects, SimFireEvent, SimSoundEvent, Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{DeathEffects, UnderAttackEvent};

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

#[derive(Default)]
pub struct DamageCommitReceipt {
    pub(crate) area_result: Option<crate::sim::combat::world_receiver::AreaDamageResult>,
    pub(super) structure_destroyed: bool,
    pub(crate) bridge_state_changed: bool,
}

impl DamageCommitReceipt {
    /// A nested transaction already delivered its effects. Retain only its
    /// frame notifications in the outer transaction; never replay its work.
    pub(crate) fn retain_frame_notifications(self, effects: &mut DeathEffects) {
        effects.structure_destroyed |= self.structure_destroyed;
        effects.bridge_state_changed |= self.bridge_state_changed;
    }
}

impl DamageConsequences {
    #[cfg(test)]
    pub(super) fn finish_navigation(&mut self, cells: Vec<(u16, u16)>) {
        self.terrain_navigation_changed_cells = cells;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn effects(&self) -> &DeathEffects {
        &self.effects
    }

    /// Fold a combat fixture's tail detonations into this frame's effects.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn append_tail_for_test(
        &mut self,
        mut effects: DeathEffects,
        under_attack_events: Vec<UnderAttackEvent>,
    ) {
        effects.under_attack_events = under_attack_events;
        self.effects.append(effects);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn prepend_live_fire_events_for_test(&mut self, mut earlier: Vec<SimFireEvent>) {
        if let DamageDelivery::Ordinary { fire_events } = &mut self.delivery {
            earlier.append(fire_events);
            *fire_events = earlier;
        }
    }

    #[cfg(any(test, feature = "test-support"))]
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

    /// The receiver fixture's combat pass result, whose radiation and death
    /// sounds the pass already delivered.
    #[cfg(test)]
    pub(crate) fn ordinary(
        mut effects: DeathEffects,
        under_attack_events: Vec<UnderAttackEvent>,
        terrain_navigation_changed_cells: Vec<(u16, u16)>,
        fire_events: Vec<SimFireEvent>,
    ) -> Self {
        // Disabled fixture sinks formerly discarded them; carrying the
        // accumulator whole must not replay those leftovers here.
        effects.rad_detonations.clear();
        effects.death_sounds.clear();
        Self::live_fire(
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            fire_events,
        )
    }

    /// A shot committed inside its actor's Logic visit, its radiation and
    /// death sounds delivered with the same ordinary fire-event and effect
    /// delivery owner.
    pub(crate) fn live_fire(
        mut effects: DeathEffects,
        under_attack_events: Vec<UnderAttackEvent>,
        terrain_navigation_changed_cells: Vec<(u16, u16)>,
        fire_events: Vec<SimFireEvent>,
    ) -> Self {
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
        frame_effects: FrameEffects<'_>,
    ) -> DamageCommitReceipt {
        let Self {
            mut effects,
            terrain_navigation_changed_cells,
            delivery,
        } = self;
        let ordinary = matches!(&delivery, DamageDelivery::Ordinary { .. });
        for &dead_id in &effects.immediate_uninit_ids {
            if world
                .substrate
                .entities
                .get(dead_id)
                .and_then(|building| building.bunker.occupant())
                .is_some()
            {
                crate::sim::docking::bunker_link::release_sell_destroy(
                    world,
                    dead_id,
                    rules,
                    frame_effects,
                );
            }
            world.uninit_with_context(
                dead_id,
                UninitContext::new(rules, overlay_registry).with_effects(frame_effects),
            );
        }

        // Apply_area_damage already completed its bridge callbacks before
        // returning to the caller's animation/cluster tail. Delivery carries
        // only the resulting frame notification, never deferred gameplay.
        let bridge_state_changed = effects.bridge_state_changed;
        #[cfg(any(test, feature = "test-support"))]
        debug_assert!(effects.tiberium_reduction_requests.is_empty());
        for detonation in effects.rad_detonations.drain(..) {
            world.radiation.apply_detonation(
                detonation,
                world.session.binary_frame(),
                &world.session.theater(),
                rules,
                world.resolved_terrain.as_ref(),
                &mut world.lighting_sources,
            );
        }

        world.finish_terrain_navigation_changes(rules, &terrain_navigation_changed_cells);

        world.admit_death_debris(std::mem::take(&mut effects.voxel_debris));
        // Explicit callback-disabled fixtures retain their effect packets;
        // production death producers already admitted each birth inline.
        // Remaining effects are `AnimClass` instances, not legacy world effects: only the real
        // constructor reaches `AnimClass::Start @ 0x00424CE0`, which is what
        // plays the art type's `Report=`/`StartSound=`, and only the real
        // AnimType carries its `Translucent=` and `Rate=`.
        for fx in std::mem::take(&mut effects.explosion_effects) {
            admit_explosion_effect(world, rules, fx);
        }
        world
            .combat_light_requests
            .emit_all(effects.combat_light_requests.drain(..));
        // RevealOnFire only lifts the shooter's shroud above. gamemd reaches
        // `CreateRadarEvent @ 0x0065FA70` from no weapon-fire path: its 24
        // constant-type call sites never pass type 0 (the 25th,
        // `TriggerAction::Execute`, takes its type from map data), and the one
        // in `BulletClass::AI` (`0x00467EA7`) is the silent type 13 of the
        // `NUKE` payload (`sim::superweapon::nuke::impact`).
        if let DamageDelivery::Ordinary { fire_events, .. } = delivery {
            world.fire_events.emit_all(fire_events);
        }
        for (die_sound_id, location) in effects.death_sounds.drain(..) {
            world.sound_events.emit(SimSoundEvent::EntityDied {
                die_sound_id,
                world: location,
            });
        }
        Simulation::dispatch_under_attack_events(
            &world.interner,
            &world.session,
            &world.house_alliances,
            &mut world.sound_events,
            &world.houses,
            &effects.under_attack_events,
            rules,
        );
        world.dispatch_unit_lost_events(&effects.unit_lost_events);
        debug_assert!(effects.smudge_spawn_requests.is_empty());
        if ordinary {
            Simulation::flush_smudge_dirty(&mut world.smudge_grid, &mut world.dirty_cells);
        }
        DamageCommitReceipt {
            area_result: None,
            structure_destroyed: effects.structure_destroyed,
            bridge_state_changed,
        }
    }
}

/// One constructor authority for immediate native tails and deferred receiver
/// packets. Bullet469C40 calls this before its next Cluster draw469057.
pub(crate) fn admit_explosion_effect(
    world: &mut Simulation,
    rules: &RuleSet,
    fx: crate::sim::combat::ExplosionEffect,
) {
    match fx.death {
        Some(spawn) => {
            world.admit_death_anim(rules, fx.shp_name, spawn);
        }
        None => {
            world.spawn_combat_explosion_anim_with_z_adjust(
                rules,
                fx.shp_name,
                fx.rx,
                fx.ry,
                fx.sub_x,
                fx.sub_y,
                fx.z,
                fx.world_z,
                fx.z_adjust,
                fx.owner_house,
            );
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
        owner: (!is_building).then_some(event.attacker_id),
        ..crate::sim::components::AnimClassSpawnDescriptor::new(
            type_name, rx, ry, sub_x, sub_y, level,
        )
    };
    if let Err(error) = world.spawn_anim_at_world(rules, descriptor, coord) {
        log::debug!(
            "muzzle anim [{}] did not construct: {error}",
            world.interner.resolve(type_name)
        );
    }
}

#[cfg(test)]
mod muzzle_anim_tests {
    use super::*;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;

    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::projectile::ProjectileCoord;

    fn fixture(category: EntityCategory) -> (Simulation, RuleSet, u64) {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=TOWER\n[TANK]\nStrength=100\n[TOWER]\nStrength=100\n",
        ))
        .expect("rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str("[GUNFIRE]\nRate=900\n"));
        art.bind_anim_frame_count_for_test("GUNFIRE", 6);
        // Synthetic fixture supplies ART directly, without native read-admission replay.
        rules.install_art_fixture(art);

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("A");
        let type_ref = sim
            .interner
            .intern(if category == EntityCategory::Structure {
                "TOWER"
            } else {
                "TANK"
            });
        let id = sim.substrate.allocate_stable_id();
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                10,
                10,
                0,
                0,
                owner,
                Health::new(100),
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
        let relative = sim.anim(id).unwrap().world_coord();

        sim.uninit_with_rules(tank, &rules);

        let anim = sim.anim(id).expect("the anim is still stored this frame");
        assert_eq!(anim.owner_entity(), None, "the teardown detached it");
        assert!(
            anim.runtime().inactive,
            "native19B requests expiry on its next AI"
        );
        assert!(!sim.substrate.pending_delete.contains_object(&id));
        assert_eq!(
            sim.anim_absolute_coord(id),
            Some(relative),
            "expiry preserves the stored delta after clearing the owner"
        );
        assert_eq!(sim.substrate.display.layer_of(id), None);
    }
}
