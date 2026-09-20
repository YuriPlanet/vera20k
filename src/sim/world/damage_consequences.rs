//! Consuming delivery of receiver consequences at explicit world boundaries.
//!
//! VERA-internal ownership protocol, gamemd equivalent UNCHECKED. This preserves
//! the existing ordinary/immediate schedules; it does not relocate native damage
//! callbacks, radiation selection, death-sound selection, or the shared ID allocator.

use super::{SimFireEvent, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{DeathEffects, RevealEvent, UnderAttackEvent};
use crate::sim::intern::InternedId;
use crate::sim::pathfinding::PathGrid;
use crate::sim::production;
use std::sync::Arc;

enum DamageDelivery {
    Immediate,
    Ordinary {
        reveal_events: Vec<RevealEvent>,
        fire_events: Vec<SimFireEvent>,
    },
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
        reveal_events: Vec<RevealEvent>,
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
            delivery: DamageDelivery::Ordinary {
                reveal_events,
                fire_events,
            },
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
            world.undock_refinery_unit_on_death(rules, dead_id);
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

        let bridge_state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
            world,
            rules,
            &effects.bridge_damage_events,
            overlay_registry,
        );
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
        if let DamageDelivery::Ordinary { reveal_events, .. } = &delivery {
            for event in reveal_events {
                crate::sim::vision::reveal_radius(
                    &mut world.fog,
                    event.owner,
                    event.rx,
                    event.ry,
                    event.radius,
                );
            }
        }

        for building in &effects.destroyed_crewed_buildings {
            production::eject_destruction_survivors(
                world,
                rules,
                building.type_id,
                building.owner,
                building.rx,
                building.ry,
                building.z,
            );
        }
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
            world.spawn_combat_explosion_anim(
                rules,
                fx.shp_name,
                fx.rx,
                fx.ry,
                fx.sub_x,
                fx.sub_y,
                fx.z,
            );
        }
        if let DamageDelivery::Ordinary { fire_events, .. } = &delivery {
            admit_electric_sparks(world, rules, fire_events);
        }
        world
            .invulnerability_impact_effects
            .append(&mut effects.invulnerability_impact_effects);
        // RevealOnFire only lifts the shooter's shroud above. gamemd reaches
        // `CreateRadarEvent @ 0x0065FA70` from no weapon-fire path: none of its
        // 25 callers passes type 0, and the one in `BulletClass::AI`
        // (`0x00467EA7`) is the silent type 13 of the `NUKE` payload.
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
