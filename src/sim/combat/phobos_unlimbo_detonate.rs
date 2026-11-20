//! Phobos `UnlimboDetonate` (`Ext/WarheadType/Body.cpp:420-423`).
//!
//! A `LimboLaunch=` weapon takes its firer off the map for the bullet's
//! flight (`TechnoClass::Fire @ 0x006FF7F3`). When that bullet detonates with
//! a warhead carrying `UnlimboDetonate=`, `BulletClass_Logics_Extras`
//! (`Ext/Bullet/Hooks.DetonateLogics.cpp:502-592`, hook `0x469AA4`) puts the
//! firer back: at the impact coordinate itself with
//! `UnlimboDetonate.ForceLocation=`, otherwise at the nearby passable cell
//! `MapClass::NearByLocation` finds for its SpeedType/MovementZone. A
//! successful reappear keeps the bullet's target (`KeepTarget`, default yes)
//! and the pre-fire selection (`KeepSelected`, default yes); a refused one
//! self-kills the firer with `[CombatDamage] C4Warhead`. `Parasite=` warheads
//! never take the path.
//!
//! Fire side: `TechnoClass_Fire_SetContext @ 0x6FE562` records the bullet,
//! `IsSelected` and `InLimbo`; `TechnoClass_Fire_UnlimboDetonate @ 0x6FF7FF`
//! (`Ext/WarheadType/Hooks.cpp:609-657`) remembers the selection. A
//! `LimboLaunch=` shot whose warhead qualifies then also cannot fire while
//! the firer's locomotor is really moving (`TechnoClass_FireAt_UpdateWeaponType
//! @ 0x6FDD7D`, `Ext/Techno/Hooks.Firing.cpp:588-607`).
//!
//! VERA owners:
//! - detonation: [`emit`], called from `commit_projectile_detonations_inline`
//!   at the existing Extras-hook site beside `phobos_extra_warheads::emit`.
//! - fire tail: [`fire_context`] records the pre-Limbo facts and [`fire_hook`]
//!   runs after the Limbo, called by `Simulation::parasite_limbo_launch` (the
//!   `TechnoClass::Fire` LimboLaunch block, `0x006FF749..0x006FF872`).
//! - fire admission: [`refuses_moving_limbo_launch`], called from
//!   `emit_admitted_fire`.
//! - remembered selection: [`UnlimboDetonateSelection`] on the entity; an
//!   empty selection snapshot (`MapClass::Unselect_All @ 0x0048DC90`) clears
//!   it in `apply_selection_snapshot`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim::combat and the shared Unlimbo,
//!   target and selection owners.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::sim::combat::receiver_call::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::combat::world_receiver::{ReceiverRun, commit_entities};
use crate::sim::combat::{CombatEmit, TargetKind, UnderAttackEvent};
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::{
    NearbyArgs, NearbyFootprint, NearbySearchOptions, PassabilityArgs,
};
use crate::sim::movement::ground_pose::foot_set_location;
use crate::sim::projectile::{ProjectileDetonation, ProjectileTarget};
use crate::sim::world::{PlacementEvidence, Simulation, UninitContext};

/// Phobos `TechnoExt::IsSelected` (`Ext/Techno/Body.h:67`): the remembered
/// selection bit a `LimboLaunch=` firer takes into limbo, paired with
/// `ScenarioExt::Global()->LimboLaunchers`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct UnlimboDetonateSelection {
    selected: bool,
}

impl UnlimboDetonateSelection {
    /// `0x6FE562`'s `InSelected` into the fire hook's write
    /// (`Ext/WarheadType/Hooks.cpp:643`).
    pub(crate) fn remember(&mut self, selected: bool) {
        self.selected = selected;
    }

    /// `pTechnoExt->IsSelected` at the detonation
    /// (`Hooks.DetonateLogics.cpp:572`, `:582`), consumed like the native
    /// check-then-clear.
    pub(crate) fn take(&mut self) -> bool {
        std::mem::take(&mut self.selected)
    }

    /// The remembered bit, as the detonation arm and regression tests read it
    /// before a consume.
    pub(crate) fn is_remembered(&self) -> bool {
        self.selected
    }

    /// `MapClass::Unselect_All @ 0x0048DC90` clearing the remembered bits
    /// before its own clear of `LimboLaunchers`.
    pub(crate) fn clear(&mut self) {
        self.selected = false;
    }
}

/// `TechnoClass_Fire_SetContext @ 0x6FE562`'s `InSelected` and `InLimbo`:
/// the two facts the `TechnoClass_Fire_UnlimboDetonate` hook reads back after
/// the LimboLaunch Limbo has already cleared the live ones.
#[derive(Clone, Copy)]
pub(super) struct FireContext {
    pub(super) in_selected: bool,
    pub(super) in_limbo: bool,
}

/// Capture the `0x6FE562` context for one `LimboLaunch=` shot.
pub(super) fn fire_context(world: &Simulation, firer: u64) -> FireContext {
    world.substrate.entities.get(firer).map_or(
        FireContext {
            in_selected: false,
            in_limbo: false,
        },
        |entity| FireContext {
            in_selected: entity.selection.is_selected(),
            in_limbo: entity.lifecycle.in_limbo(),
        },
    )
}

/// `TechnoClass_Fire_UnlimboDetonate @ 0x6FF7FF`
/// (`Ext/WarheadType/Hooks.cpp:630-657`), after the LimboLaunch Limbo
/// (`0x006FF7F3`): when the fired warhead qualifies and the firer was not
/// already in limbo at `TechnoClass::Fire` entry, the bullet's `Owner`
/// (`+0xB0`) is written back to the firer, which Limbo's Detach_All cleared,
/// and with `KeepSelected` the pre-fire selection is remembered so the
/// detonation can restore it.
pub(super) fn fire_hook(
    world: &mut Simulation,
    firer: u64,
    weapon: &WeaponType,
    bullet: Option<u64>,
    fired: FireContext,
    rules: &RuleSet,
) {
    if !weapon.limbo_launch {
        return;
    }
    let Some(warhead) = rules.weapon_warhead(weapon) else {
        return;
    };
    if warhead.parasite || !warhead.unlimbo_detonate {
        return;
    }
    let Some(entity) = world.substrate.entities.get(firer) else {
        return;
    };
    if !entity.lifecycle.object_alive()
        || entity.health.current() <= 0
        || bullet.is_none()
        || fired.in_limbo
    {
        return;
    }
    // `pBullet->Owner = pThis` (`:649-654`): Limbo's Detach_All cleared the
    // owner, so the detonation hook needs this write to still find the firer.
    if let Some(bullet) = bullet {
        world.projectiles.set_owner(bullet, firer);
    }
    if warhead.unlimbo_detonate_keep_selected
        && let Some(entity) = world.substrate.entities.get_mut(firer)
    {
        entity.unlimbo_detonate.remember(fired.in_selected);
    }
}

/// `TechnoClass_FireAt_UpdateWeaponType @ 0x6FDD7D`
/// (`Ext/Techno/Hooks.Firing.cpp:588-607`): a `LimboLaunch=` shot whose
/// warhead has `UnlimboDetonate` and is not `Parasite=` is refused while the
/// firing Foot's locomotor `Is_Really_Moving_Now`. A Building never reaches
/// the `abstract_cast<FootClass*>` arm.
pub(super) fn refuses_moving_limbo_launch(
    world: &Simulation,
    rules: &RuleSet,
    weapon: &WeaponType,
    firer: u64,
    category: EntityCategory,
) -> bool {
    if !weapon.limbo_launch || category == EntityCategory::Structure {
        return false;
    }
    let Some(warhead) = rules.weapon_warhead(weapon) else {
        return false;
    };
    if warhead.parasite || !warhead.unlimbo_detonate {
        return false;
    }
    let Some(entity) = world.substrate.entities.get(firer) else {
        return false;
    };
    crate::sim::movement::motion_query::is_really_moving_now(
        entity,
        crate::sim::movement::SpeedRules::new(rules, &world.interner, &world.houses),
        world.session.binary_frame(),
    )
}

/// `BulletClass_Logics_Extras`'s `UnlimboDetonate` arm
/// (`Ext/Bullet/Hooks.DetonateLogics.cpp:502-592`). `detonation.source_id` is
/// the bullet's `Owner` (`+0xB0`), the `LimboLaunch=` firer; the warhead is
/// the detonating bullet's.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    detonation: &ProjectileDetonation,
    warhead: &WarheadType,
    out: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    if !warhead.unlimbo_detonate || warhead.parasite {
        return;
    }
    let owner_id = detonation.source_id;
    let Some(owner) = world.substrate.entities.get(owner_id) else {
        return;
    };
    if !owner.lifecycle.in_limbo() {
        return;
    }
    let Some(object) = world.object_type(owner.type_ref(), rules) else {
        return;
    };
    let speed_type = object.speed_type;
    let movement_zone = object.movement_zone;
    let target = detonation.target;
    // `pThis->Target` is a Foot's `IsInAir` (`:511`); a Cell or Terrain
    // target has no Foot virtual.
    let target_in_air = match target {
        ProjectileTarget::Entity(id) => target_high_flying(world, rules, id),
        _ => false,
    };
    let impact = detonation.impact;
    let frame = world.session.binary_frame();
    let facing = owner.body_facing_dir(frame);

    // `CoordStruct location = *coords` (`:509`).
    let mut location = DriveCoord {
        x: impact.x,
        y: impact.y,
        z: impact.z,
    };
    let on_bridge = if warhead.unlimbo_detonate_force_location {
        // `TryGetCellAt(location)` only feeds OnBridge (`:540-545`).
        let cell = crate::sim::cell_rect::get_cellclass_fallback(
            world.resolved_terrain.as_ref(),
            impact.x / 256,
            impact.y / 256,
        );
        cell_bridge(&cell)
    } else {
        // `NearByLocation(Coord2Cell(location), SpeedType, -1,
        // MovementZone, ...)`; its null cell `{0,0}` is looked up like any
        // other (`:517-531`).
        let seed = ((impact.x / 256) as i16, (impact.y / 256) as i16);
        let cell = search_landing_cell(world, seed, speed_type, movement_zone).unwrap_or((0, 0));
        let cell = crate::sim::cell_rect::get_cellclass_fallback(
            world.resolved_terrain.as_ref(),
            i32::from(cell.0),
            i32::from(cell.1),
        );
        if matches!(cell, crate::sim::cell_rect::CellRef::Real(_)) {
            let at = cell.get_coords();
            let bridge = cell_bridge(&cell);
            location = DriveCoord {
                x: at.x,
                y: at.y,
                z: at.z.wrapping_add(if bridge {
                    crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
                } else {
                    0
                }),
            };
            bridge
        } else {
            false
        }
    };
    if !warhead.unlimbo_detonate_force_location && target_in_air {
        // `:533-534`.
        location.z = impact.z;
    }
    if let Some(entity) = world.substrate.entities.get_mut(owner_id) {
        // `Object+0x8C`, written before Unlimbo (`:525`, `:543`).
        entity.on_bridge.store(on_bridge);
    }

    let place = |world: &mut Simulation| {
        world
            .reveal_constructed_object_at_coord_with_overlay_context(
                owner_id,
                location,
                facing,
                PlacementEvidence::EvaluateMark,
                rules,
                overlay_registry,
                run.effects(),
            )
            .is_some()
    };
    // `ForceLocation` brackets Unlimbo with `++/--Unsorted::ScenarioInit`
    // (`:547-549`); the search arm does not.
    let success = if warhead.unlimbo_detonate_force_location {
        world.with_object_placement_scope(place)
    } else {
        place(world)
    };

    if success {
        if target_in_air && let Some(entity) = world.substrate.entities.get_mut(owner_id) {
            // `pTechno->IsFallingDown = true` (`:556-560`) and then
            // `TechnoExt::Fetch(pTechno)->OnParachuted = true` (`:607-610`).
            entity.raise_falling_down();
            entity.mark_on_parachuted();
        }
        // `SetTarget(pTarget)` for an Object target, otherwise NULL
        // (`:562-570`).
        let kept = warhead
            .unlimbo_detonate_keep_target
            .then(|| object_target(target))
            .flatten();
        let _ = world.assign_target_represented(owner_id, kept, rules, run.effects());
        let reselect = world
            .substrate
            .entities
            .get_mut(owner_id)
            .is_some_and(|entity| entity.unlimbo_detonate.take());
        if reselect {
            // `pTechno->Select()` (`:572-578`); the detonation runs in the
            // logic pass, whose voice latch is set.
            world.techno_select(
                owner_id,
                UninitContext::new(rules).with_effects(run.effects()),
            );
        }
    } else {
        // A refused reappear clears the remembered selection without a
        // Select, puts the firer at the requested Location and kills it with
        // the C4 warhead (`:580-591`).
        if let Some(entity) = world.substrate.entities.get_mut(owner_id) {
            entity.unlimbo_detonate.take();
        }
        set_location(world, owner_id, location, rules);
        let Some(health) = world
            .substrate
            .entities
            .get(owner_id)
            .map(|entity| entity.health.current())
        else {
            return;
        };
        let source_house = world
            .substrate
            .entities
            .get(owner_id)
            .map(|entity| entity.owner());
        let c4 = world.interner.intern(&rules.bridge_warheads.c4_name);
        let event = EntityDamageEvent::direct_receiver(
            owner_id,
            health,
            0,
            RAD_NO_ATTACKER,
            source_house,
            c4,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        let (nested, mut pings) = commit_entities(
            world,
            run,
            std::slice::from_ref(&event),
            Some(false),
            rules,
            overlay_registry,
        );
        out.effects.append(nested);
        under_attack_events.append(&mut pings);
    }
}

/// `MapClass::NearByLocation`'s passable-cell search at the impact seed:
/// `Find_Nearby_Passable_Cell` with the object's SpeedType and MovementZone,
/// a 1x1 footprint, bridge cells allowed and no overlay/height/occupancy
/// gate. The zone is `-1`'s GetZoneID on the seed without the bridge lookup.
fn search_landing_cell(
    world: &Simulation,
    seed: (i16, i16),
    speed_type: SpeedType,
    movement_zone: MovementZone,
) -> Option<(u16, u16)> {
    let zone = world
        .zone_grid
        .as_ref()
        .zip(world.resolved_terrain.as_ref())
        .and_then(|(zones, terrain)| {
            zones.get_zone_id_native(
                terrain,
                (seed.0 as u16, seed.1 as u16),
                movement_zone,
                false,
            )
        });
    world.nearby_passable_cell(
        (i32::from(seed.0), i32::from(seed.1)),
        NearbyArgs {
            require_burrowable: false,
            passability: PassabilityArgs {
                speed_type,
                required_zone_id: zone,
                movement_zone,
                bridge_aware_zone: false,
            },
            footprint: NearbyFootprint::SINGLE,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: false,
            target_cell: None,
            options: NearbySearchOptions::default(),
        },
    )
}

/// `CellClass::ContainsBridge`: the structural bridge bit.
fn cell_bridge(cell: &crate::sim::cell_rect::CellRef<'_>) -> bool {
    cell.bridge_flags_0x1180() & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0
}

/// The bullet target's `FootClass::IsInAir` (`ObjectClass::IsInAir
/// 0x005F6B90`), false for a non-Foot entity.
fn target_high_flying(world: &Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(target) = world.substrate.entities.get(id) else {
        return false;
    };
    if !matches!(
        target.category(),
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    ) {
        return false;
    }
    crate::sim::movement::air_movement::is_high_flying(
        target,
        world.resolved_terrain.as_ref(),
        Some((rules, &world.interner)),
    )
}

/// `pTarget->AbstractFlags & AbstractFlags::Object`: a Techno or Terrain
/// target is set, a Cell or NULL is cleared.
fn object_target(target: ProjectileTarget) -> Option<TargetKind> {
    match target {
        ProjectileTarget::Entity(id) | ProjectileTarget::Terrain(id) => {
            Some(TargetKind::Entity(id))
        }
        ProjectileTarget::Cell { .. } | ProjectileTarget::DummyCell | ProjectileTarget::None => {
            None
        }
    }
}

/// `pTechno->SetLocation(location)` on the refused arm (`:589`): a Foot
/// through `FootClass::SetLocation`, a Building through Object's.
fn set_location(world: &mut Simulation, id: u64, location: DriveCoord, rules: &RuleSet) {
    let foot = world
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.category() != EntityCategory::Structure);
    if foot {
        foot_set_location(
            &mut world.substrate.entities,
            id,
            location,
            rules,
            &world.interner,
        );
    } else if let Some(entity) = world.substrate.entities.get_mut(id) {
        entity.position.put_location(location);
    }
}
