//! A nuclear silo's launch: `BuildingClass::Mission_Missile @ 0x0044C980`,
//! the mission `SuperClass::Launch` queues on a `NukeSilo=` building
//! (`superweapon::nuke`).
//!
//! The warning anim's visibility (`AnimClass::AI 0x00423B29` rewrites
//! `+0x19D` from `0x0043B4C0`, a test of the local player) is the
//! presentation's, through [`Simulation::psi_warning_detected_by`].
//!
//! The handler's other arm (`0x0044CCDE..0x0044D584`, [`emp_cannon`]) is
//! Tiberian Sun's EMP cannon. Stock YR reaches it with no retail data: only
//! SuperClass::Launch's MultiMissile case queues Missile (`0x006CDDA5`, the
//! one `PUSH 0x16` before a Queue_Mission call), on a `NukeSilo=` building,
//! and no retail type sets `EMPulseCannon=`. Ares's `Type=EMPulse`
//! (`superweapon::emp_pulse`) is its live caller.
//!
//! RESIDUALS:
//! - A refused missile (`BulletClass::Fire` false, `0x0044CC40`: the bullet
//!   is deleted, which also removes the warning anim, and no take-off anim
//!   plays): VERA admits every bullet, and the silo's coordinate is on the
//!   map.
//!
//! The missile's `Trailer=` smoke (`[NKMSLUP] Trailer=NUKEPUFF`) comes from
//! the shared Bullet AI's trailer (`0x00466826..0x004668B8`,
//! `ProjectileStore::trailer`), not from this handler.
//!
//! EVIDENCE BOUNDS: `tools.superweapon_oracle` supplies GetFLH (`vt+0xB0`)
//! and the silo's coordinate, so the missile's origin rests on the shared
//! FLH owner (`combat::fire_coord`), and its one fixture cell stands 104
//! leptons a level with no ramp. No native run times the flight from launch
//! to impact; the bullets move on the shared projectile owner's cadence.

use super::ObjectAiCtx;
use crate::rules::flh::Flh;
use crate::rules::ruleset::RuleSet;
use crate::sim::building_construction::BuildingBodyMode;
use crate::sim::combat::fire_coord::{self, FireSource, fire_coordinate};
use crate::sim::combat::{FiredBullet, TargetKind, admit_fired_bullet};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::projectile::{
    ProjectileCoord, ProjectilePayload, ProjectileTarget, cell_ground_coord,
};
use crate::sim::world::{SimSoundEvent, Simulation};
#[cfg(not(feature = "ares"))]
use crate::util::native_x87::NativeF64Bits;

/// `PSIWARN`, the warning anim's type (`0x0081907C`, found by name at
/// `0x0044C9FA`).
#[cfg(not(feature = "ares"))]
const WARNING_ANIM: &str = crate::rules::effect_asset_catalog::PSI_WARNING_ANIM;
/// The missile's Construct speed, pushed as an immediate (`0x0044CAC8`).
const MISSILE_SPEED: i32 = 0xFF;
/// `[0x007E44A8]` = 10.0, the missile's launch velocity scale.
#[cfg(not(feature = "ares"))]
const MISSILE_VELOCITY_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x4024_0000_0000_0000);
/// The take-off anim's ZAdjust, written after its constructor
/// (`0x0044CC9D`).
const TAKE_OFF_Z_ADJUST: i32 = -100;
/// Draw flags of both anims (`0x0044CA40`, `0x0044CC7D`).
const ANIM_DRAW_FLAGS: u32 = 0x600;

/// Mission_Missile for a `NukeSilo=` type (`+0x16BA`) by its status
/// (`+0xBC`, jump table `0x0044D5A4`):
/// - 0 (`0x0044C9BC`): the ready byte (`+0x6DD`) clears, Begin_Mode(Active),
///   status 1, and the warning anim stands on the owner's NukeTarget
///   ([`spawn_warning`]); falls into 1.
/// - 1 (`0x0044CA7A`): with the ready byte set, Begin_Mode(Aux1) and status
///   2; falls into 2 either way.
/// - 2 (`0x0044CA97`): the missile ([`launch_missile`]); returns 1.
/// - 3 (`0x0044CCBE`): Begin_Mode(Aux2), status 4; returns 6.
/// - 4 (`0x0044D53C`): Begin_Mode(Idle) and Guard queued, not commenced;
///   returns 60.
///
/// So the missile leaves on the first visit and the silo is back on Guard
/// 67 frames later. Any other building, or a silo past status 4, takes the
/// other arm ([`emp_cannon`]).
pub(super) fn mission_missile(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
    bridge_state_changed: &mut bool,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let silo = sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.nuke_silo);
    let status = entity.mission.handler_state();
    if !silo || status > 4 {
        return emp_cannon(sim, id, rules, ctx, bridge_state_changed);
    }
    let now = sim.session.binary_frame() as i32;
    let mut warning = None;
    if status == 0 {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.mission_leaf.set_building_ready_latch(0);
            entity.begin_building_body(BuildingBodyMode::Active, now);
            entity.mission.set_handler_state(1);
        }
        warning = spawn_warning(sim, rules, id);
    }
    match status {
        0..=2 => {
            if status <= 1
                && let Some(entity) = sim.substrate.entities.get_mut(id)
                && entity.building_ready_latch() != 0
            {
                entity.begin_building_body(BuildingBodyMode::Aux1, now);
                entity.mission.set_handler_state(2);
            }
            launch_missile(sim, rules, id, warning)
        }
        3 => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.begin_building_body(BuildingBodyMode::Aux2, now);
                entity.mission.set_handler_state(4);
            }
            6
        }
        _ => park(sim, id, rules),
    }
}

/// The shared tail (`0x0044D53C`, again at `0x0044D560`): Begin_Mode(Idle)
/// and Guard queued, not commenced; returns 60.
fn park(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    let now = sim.session.binary_frame();
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.begin_building_body(BuildingBodyMode::Idle, now as i32);
    }
    queue_guard(sim, id, rules);
    60
}

/// `Queue_Mission(Guard, false)` (vt+0x1E8, `0x0044CD12`, `0x0044D54D`).
fn queue_guard(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let frame = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Guard),
        0,
        frame,
        &LiveReadyInputProvider { rules },
    );
}

/// `PULSBALL`, the pulse's anim type (`0x00819070`, found by index at
/// `0x0044CE66`; Ares hook 44CE46 looks it up by name, 10036790).
const PULSE_BALL_ANIM: &str = "PULSBALL";
/// The pulse ball's wait before the shot (`0x0044CEBD`).
const PULSE_BALL_DELAY: i32 = 0x20;
/// The fallback pitch when neither arc reaches the cell (`0x0044D2FA`).
const EMP_FALLBACK_PITCH: u16 = 0xFA00;

/// Mission_Missile's other arm (`0x0044CCDE..0x0044D584`). With a firing
/// type (`+0x5F8` not -1) the building first fires weapon 0 at
/// `Map[NukeTarget]` (vt+0x3CC, TechnoClass::FireAt `0x006FDD50`) and queues
/// Guard (`0x0044CCE7..0x0044CD12`). Then a type without `EMPulseCannon=`
/// (`+0x16C3`) waits the mission rate (`0x0044D584`); a cannon runs its
/// status (`+0xBC`, jump table `0x0044D5B8`; past 3 the mission rate):
/// - 0 (`0x0044CD42`): PrimaryFacing turns to its direction to
///   `Map[EMPTarget]` (vt+0x4E8), then BarrelFacing to the shell's pitch
///   ([`emp_barrel_pitch`], vt+0x2C8); each Set_Desired returns 1, and once
///   both stand there status 1; returns 1.
/// - 1 (`0x0044CE46`): the `PULSBALL` anim at GetFLH(0); status 2; returns 32.
/// - 2 (`0x0044CEEC`): the shot ([`emp_shot`]).
/// - 3 (`0x0044CECA`): BarrelFacing back to level (0x4000), then the tail.
///
/// Ares hook 44CCE7 (10036570) replaces the fire: a cannon only takes the
/// cell its Super aimed it at (TechnoExt::SuperTarget) as the House's
/// EMPTarget, and any other building fires weapon 0 at that cell (else
/// `Map[NukeTarget]`), queues Guard and returns 1. Hook 44CE46 (10036790)
/// takes the attached Super's `EMPulse.PulseBall` (else `PULSBALL`, none
/// allowed) and `EMPulse.PulseDelay` (else 32).
fn emp_cannon(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
    bridge_state_changed: &mut bool,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return 1;
    };
    let cannon = object.emp_pulse_cannon;
    let owner = entity.owner();
    let firing = entity
        .mission_leaf
        .as_building()
        .map_or(-1, |leaf| leaf.firing_super_weapon());
    if firing != -1 {
        #[cfg(feature = "ares")]
        {
            let super_target = entity.ares_super_target();
            if cannon {
                if let Some(cell) = super_target
                    && let Some(house) = sim.houses.get_mut(&owner)
                {
                    house.aim_emp_cannons(cell);
                }
            } else {
                let Some(target) =
                    super_target.or_else(|| sim.houses.get(&owner).map(|h| h.nuke_target()))
                else {
                    return 1;
                };
                fire_weapon_zero(sim, id, rules, ctx, bridge_state_changed, target);
                queue_guard(sim, id, rules);
                return 1;
            }
        }
        #[cfg(not(feature = "ares"))]
        if let Some(target) = sim.houses.get(&owner).map(|house| house.nuke_target()) {
            fire_weapon_zero(sim, id, rules, ctx, bridge_state_changed, target);
            queue_guard(sim, id, rules);
        }
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let status = entity.mission.handler_state();
    if !cannon || status > 3 {
        return rules.mission_control.rate_frames(MissionType::Missile);
    }
    let Some(cell) = sim.houses.get(&owner).map(|house| house.emp_target()) else {
        return 1;
    };
    let now = sim.session.binary_frame();
    match status {
        0 => {
            let Some(body) = fire_coord::building_direction_to(
                sim,
                rules,
                entity,
                TargetKind::Cell(cell.0, cell.1),
            ) else {
                return 1;
            };
            let barrel = emp_barrel_pitch(sim, rules, id, cell);
            let Some(entity) = sim.substrate.entities.get_mut(id) else {
                return 1;
            };
            if entity.body_facing.current(now) != body {
                entity.body_facing.set(body, now);
            } else if let Some(barrel) = barrel
                && entity.barrel_elevation().current(now) != barrel
            {
                entity.set_barrel_elevation(barrel, now);
            } else {
                entity.mission.set_handler_state(1);
            }
            1
        }
        1 => {
            #[cfg(feature = "ares")]
            let (ball, delay) = match entity
                .ares_super_weapon()
                .and_then(|attached| {
                    rules.super_weapon_metadata(sim.interner.resolve(attached.sw_type))
                })
                .map(|metadata| metadata.launch().emp_pulse())
            {
                Some(emp) => (
                    emp.pulse_ball
                        .clone()
                        .unwrap_or_else(|| Some(PULSE_BALL_ANIM.to_owned())),
                    emp.pulse_delay,
                ),
                None => (Some(PULSE_BALL_ANIM.to_owned()), PULSE_BALL_DELAY),
            };
            #[cfg(not(feature = "ares"))]
            let (ball, delay) = (Some(PULSE_BALL_ANIM.to_owned()), PULSE_BALL_DELAY);
            let flh = fire_coordinate(
                sim,
                rules,
                &FireSource::of_entity(entity),
                object,
                0,
                0,
                Flh::default(),
            )
            .coord;
            if let Some(ball) = ball {
                spawn_anim(sim, rules, &ball, flh, false);
            }
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_handler_state(2);
            }
            delay
        }
        2 => emp_shot(sim, rules, id, cell),
        _ => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.set_barrel_elevation(crate::sim::game_entity::BARREL_LEVEL, now);
            }
            park(sim, id, rules)
        }
    }
}

impl Simulation {
    /// SW_EMPulse::Activate's `EMPulse.TargetSelf` arm (Ares 3.0
    /// `0x100712B4..0x1007134A`): BulletTypeExt::CreateBullet of building
    /// `id`'s weapon 0 (GetWeapon vt+0x3F8), the building its target and
    /// owner, given the weapon, Removed, detonated at the building's
    /// GetCoords (vt+0x48, `0x1007132B`) and Released. That is
    /// Fire_Death_Weapon's bare `DetonateAtCoord`
    /// (`ProjectileDetonationReason::DeathWeapon`). Nothing without a
    /// weapon. Returns whether a bridge changed.
    #[cfg(feature = "ares")]
    pub(crate) fn emp_cannon_detonate_self(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlays: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(weapon) = weapon_zero(self, rules, entity) else {
            return false;
        };
        let Some(warhead) = weapon.warhead.as_deref() else {
            return false;
        };
        let impact = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let payload = ProjectilePayload::new(
            weapon.damage,
            self.interner.intern(warhead),
            weapon
                .projectile
                .as_deref()
                .map(|id| self.interner.intern(id)),
            weapon.bright,
        )
        .with_installed_weapon(Some(self.interner.intern(&weapon.id)));
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: id,
            source_id: id,
            target: ProjectileTarget::Entity(id),
            impact: ProjectileCoord::new(impact.x, impact.y, impact.z),
            payload,
            reason: crate::sim::projectile::ProjectileDetonationReason::DeathWeapon,
        };
        self.commit_logic_projectile_detonations(rules, overlays, &[detonation], effects)
    }
}

/// `TechnoClass::FireAt @ 0x006FDD50` with weapon 0 at the cell (vt+0x3CC
/// at `0x0044CD04`; Ares 10036570's `Fire(pTarget, 0)`), with no fire-error
/// question.
fn fire_weapon_zero(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
    bridge_state_changed: &mut bool,
    (rx, ry): (u16, u16),
) {
    *bridge_state_changed |= sim
        .commit_fire_visit(
            crate::sim::combat::world_fire::FireVisit::Direct {
                id,
                target: TargetKind::Cell(rx, ry),
                weapon_index: 0,
            },
            rules,
            ctx.overlay_registry,
            ctx.effects,
        )
        .bridge_state_changed;
}

/// The shell's launch speed from the cannon to `cell` (`0x0043EBED..
/// 0x0043EC00`, `0x0044D06A..0x0044D085`): `Ballistic_Launch_Speed` of
/// `ObjectClass::Distance` (`0x005F6360`) under `[General] Gravity=`.
fn emp_launch_speed(sim: &Simulation, rules: &RuleSet, id: u64, cell: (u16, u16)) -> i32 {
    let distance = sim
        .object_distance_to(id, TargetKind::Cell(cell.0, cell.1))
        .unwrap_or(0);
    crate::util::native_ballistics::ballistic_launch_speed(
        distance,
        crate::util::native_ballistics::BallisticGravity::Rules(rules.general.gravity),
        false,
    )
}

/// The shell's pitch from `source` (vt+0x300 with weapon 0) to the cell's
/// GetCoords at `speed` (`0x0043EC18..0x0043ED02`, `0x0044D1F9..0x0044D2F8`):
/// the planar distance of the integer `dy·dy + dx·dx` through
/// `Sqrt_Approx` and ftol, the height `dz`, and
/// `Ballistics::SolveLaunchDirection` with ShouldUseHighTrajectory
/// (`0x0070D590`), retried at `speed * 10 / 8`.
fn emp_arc_pitch(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    source: ProjectileCoord,
    cell: (u16, u16),
    speed: i32,
) -> Option<u16> {
    let entity = sim.substrate.entities.get(id)?;
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    let to = crate::sim::movement::ground_pose::target_get_coords(
        TargetKind::Cell(cell.0, cell.1),
        sim,
        cells.as_ref(),
    )?;
    let target = ProjectileCoord::new(to.x, to.y, to.z);
    let range = crate::sim::projectile::launch::fireat_launch_distance(source, target);
    let height = target.z.wrapping_sub(source.z);
    // 0x0070D590 with weapon 0: no WeaponType is a high arc; else Lobber=,
    // or a TarCom above the Location higher than its planar distance.
    let high_root = match weapon_zero(sim, rules, entity) {
        None => true,
        Some(weapon) => {
            let location = crate::sim::movement::ground_pose::object_location(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            let tar_com = entity.attack_target.target().and_then(|target| {
                crate::sim::movement::ground_pose::target_get_coords(target, sim, cells.as_ref())
            });
            crate::sim::projectile::launch::high_arc_root(
                weapon.lobber,
                ProjectileCoord::new(location.x, location.y, location.z),
                tar_com.map(|coord| ProjectileCoord::new(coord.x, coord.y, coord.z)),
            )
        }
    };
    let gravity = crate::sim::projectile::projectile_gravity(rules.general.gravity, None, false);
    let solve = |speed| {
        crate::sim::projectile::launch::solve_launch_direction(
            range, height, speed, gravity, high_root,
        )
    };
    solve(speed).or_else(|| solve(speed.wrapping_mul(10) / 8))
}

/// GetWeapon(0)'s WeaponType (vt+0x3F8, `0x004526F0`).
fn weapon_zero<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    entity: &crate::sim::game_entity::GameEntity,
) -> Option<&'r crate::rules::weapon_type::WeaponType> {
    let object = sim.object_type(entity.type_ref(), rules)?;
    crate::sim::combat::combat_weapon::get_weapon(
        entity,
        object,
        0,
        &sim.substrate.entities,
        rules,
        &sim.interner,
    )
}

/// BuildingClass vt+0x2C8 (`0x0043E940`) for an EMP cannon whose weapon 0 is
/// not `IsLaser=` (`+0x149`; `0x0043EB58..0x0043ED11`): the pitch
/// [`emp_arc_pitch`] gives from vt+0x300 at the launch speed. The target's
/// two ground-height reads and the FLH heading are discarded
/// (`0x0043EB6D`, `0x0043EBEB`). `None` where neither arc reaches the cell:
/// native then returns an uninitialized stack word (`0x0043ED05`).
fn emp_barrel_pitch(sim: &Simulation, rules: &RuleSet, id: u64, cell: (u16, u16)) -> Option<u16> {
    let speed = emp_launch_speed(sim, rules, id, cell);
    let entity = sim.substrate.entities.get(id)?;
    let source = fire_coord::turret_pivot_coordinate(sim, rules, entity, 0)?;
    emp_arc_pitch(sim, rules, id, source, cell, speed)
}

/// Status 2 (`0x0044CEEC..0x0044D535`). Over the cell (0, 0) the cannon parks
/// (`0x0044CF3B..0x0044CF5D`). Otherwise the barrel turns to its pitch
/// (`0x0044CF7A`) and a bullet of weapon 0's BulletType, damage, warhead and
/// Bright, at three quarters of the launch speed (`0x0044D09A..0x0044D0BE`),
/// targets the cell from vt+0x300: its velocity
/// ([`crate::sim::projectile::launch::emp_cannon_launch_velocity`]) heads
/// along the FLH-to-cell-centre direction (`atan2` at `0x0044D04F`) at the
/// full speed, pitched to the arc (or 0xFA00). Once fired, a `Turret=`
/// type's recoil arms (`0x0044D48C..0x0044D4BB`) and, unless `IsGattling=`
/// (`+0xCD5`), one Main draw picks a `Report=` sound at GetFLH(0)
/// (`0x0044D4D4..0x0044D51A`); status 3, returns 1. A refused shot is deleted
/// and the cannon parks.
fn emp_shot(sim: &mut Simulation, rules: &RuleSet, id: u64, cell: (u16, u16)) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let (Some(weapon), Some(object)) = (
        weapon_zero(sim, rules, entity),
        sim.object_type(entity.type_ref(), rules),
    ) else {
        // Native dereferences the missing WeaponType (`0x0044D08A`).
        return park(sim, id, rules);
    };
    if cell == (0, 0) {
        return park(sim, id, rules);
    }
    let now = sim.session.binary_frame();
    if let Some(barrel) = emp_barrel_pitch(sim, rules, id, cell)
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        entity.set_barrel_elevation(barrel, now);
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let flh = fire_coordinate(
        sim,
        rules,
        &FireSource::of_entity(entity),
        object,
        0,
        0,
        Flh::default(),
    )
    .coord;
    let centre = (
        i32::from(cell.0 as i16) * 256 + 128,
        i32::from(cell.1 as i16) * 256 + 128,
    );
    let speed = emp_launch_speed(sim, rules, id, cell);
    let Some(source) = fire_coord::turret_pivot_coordinate(sim, rules, entity, 0) else {
        return park(sim, id, rules);
    };
    let pitch = emp_arc_pitch(sim, rules, id, source, cell, speed).unwrap_or(EMP_FALLBACK_PITCH);
    let velocity = crate::sim::projectile::launch::emp_cannon_launch_velocity(
        crate::sim::projectile::launch::emp_cannon_heading(flh, centre),
        speed,
        pitch,
    );
    let Some(warhead) = weapon
        .warhead
        .as_deref()
        .and_then(|name| rules.warhead(name))
    else {
        // Native constructs the bullet with a null warhead.
        return park(sim, id, rules);
    };
    let bullet_id = sim.substrate.allocate_stable_id();
    let native_unique_id = sim.next_native_runtime_id();
    let payload = ProjectilePayload::new(
        weapon.damage,
        sim.interner.intern(&warhead.id),
        weapon
            .projectile
            .as_deref()
            .map(|id| sim.interner.intern(id)),
        weapon.bright,
    )
    .with_installed_weapon(Some(sim.interner.intern(&weapon.id)));
    let target = ProjectileTarget::looked_up_cell(sim.resolved_terrain.as_ref(), cell.0, cell.1);
    let Some(bullet) = FiredBullet::launched(
        sim,
        rules,
        native_unique_id,
        id,
        target,
        payload,
        speed.wrapping_mul(3) / 4,
        source,
        velocity,
    ) else {
        return park(sim, id, rules);
    };
    admit_fired_bullet(sim, rules, weapon, bullet_id, bullet);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.fire_voxel_recoil(object.has_turret);
    }
    // Native divides by a zero Report count; Ares hook 44D4CA (10014130)
    // skips the sound then.
    let count = weapon.report_count();
    if !object.is_gattling && count > 0 {
        let pick = sim.main_rng.next_u32() % count as u32;
        if let Some(sound) = weapon.report_item(pick as usize) {
            sim.sound_events.emit(SimSoundEvent::voc_at_world_coord(
                sound.to_owned(),
                crate::sim::anim_class::AnimWorldCoord {
                    x: flh.x,
                    y: flh.y,
                    z: flh.z,
                },
            ));
        }
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.set_handler_state(3);
    }
    1
}

/// Status 0's warning (`0x0044C9DC..0x0044CA74`): a PSIWARN anim at the
/// owner's NukeTarget cell (its GetCoords, vt+0x48), constructed with
/// (delay 0, loop 1, flags 0x600, ZAdjust 0, not reversed), its bullet
/// (`+0x17C`, `0x00424C90`) none, its house (`+0x180`, `0x00424CA0`) the
/// silo's owner and hidden (`+0x19D`). The silo holds it at `+0x54C`
/// (`0x0044CA74`) until status 2 hands it to the missile and clears the field
/// (`0x0044CB04..0x0044CB14`); both run in one visit, so VERA passes it on.
fn spawn_warning(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
) -> Option<crate::sim::anim_class::AnimId> {
    let owner = sim.substrate.entities.get(id)?.owner();
    let (rx, ry) = sim.houses.get(&owner)?.nuke_target();
    let coord = cell_ground_coord(sim.resolved_terrain.as_ref(), rx, ry);
    // Ares hook 44C9FF (10036750): the firing type's `Nuke.PsiWarning`;
    // none skips the warning (0x0044CA7A).
    #[cfg(feature = "ares")]
    let warning = firing_nuke(sim, rules, id)?.psi_warning.clone()?;
    #[cfg(not(feature = "ares"))]
    let warning = WARNING_ANIM.to_owned();
    let anim = spawn_anim(sim, rules, &warning, coord, true)?;
    sim.substrate.set_anim_owner_house(anim, owner);
    Some(anim)
}

/// Status 2 (`0x0044CA97..0x0044CCB6`): the missile, a bullet of the firing
/// type's `WeaponType=` (SuperWeaponType `+0x9C`, the type at the silo's
/// `+0x5F8`) built by `CreateBullet @ 0x0046B050` with the weapon's
/// BulletType, damage and warhead, the NukeTarget cell as target, the silo as
/// owner and speed 255, given the weapon (`SetWeaponType @ 0x0046B260`) and
/// handed to the warning anim (`0x0044CB0F`). It leaves the silo's weapon-0
/// fire coordinate with no offset (GetFLH, vt+0xB0) straight up
/// ([`crate::sim::projectile::launch::missile_launch_velocity`] at 10.0), and
/// the take-off anim (`[General] NukeTakeOff=`, `Rules+0x98`) plays there
/// with ZAdjust -100. Status 3; returns 1.
fn launch_missile(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    warning: Option<crate::sim::anim_class::AnimId>,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let owner = entity.owner();
    let firing = entity
        .mission_leaf
        .as_building()
        .map_or(-1, |leaf| leaf.firing_super_weapon());
    let weapon = usize::try_from(firing)
        .ok()
        .and_then(|index| rules.super_weapon_order.get(index))
        .and_then(|name| rules.super_weapon(name))
        .and_then(|sw| sw.weapon_type.as_deref())
        .and_then(|name| rules.weapon(name));
    let warhead = weapon.and_then(|weapon| rules.weapon_warhead(weapon));
    let (Some(weapon), Some(warhead), Some(object)) =
        (weapon, warhead, sim.object_type(entity.type_ref(), rules))
    else {
        // Native dereferences the missing WeaponType or warhead.
        return 1;
    };
    let Some((rx, ry)) = sim.houses.get(&owner).map(|house| house.nuke_target()) else {
        return 1;
    };
    let origin = fire_coordinate(
        sim,
        rules,
        &FireSource::of_entity(entity),
        object,
        0,
        0,
        Flh::default(),
    )
    .coord;
    let bullet_id = sim.substrate.allocate_stable_id();
    let native_unique_id = sim.next_native_runtime_id();
    if let Some(warning) = warning {
        sim.substrate
            .set_anim_attached_bullet(warning, Some(bullet_id));
    }
    // Ares hook 44CB4C (10036660) replaces the vanilla velocity with (0, 0,
    // 10) for a `Vertical=` BulletType, else (0, 0, 100).
    #[cfg(feature = "ares")]
    let velocity = crate::sim::projectile::ProjectileVelocity::new(
        0,
        0,
        if weapon
            .projectile
            .as_deref()
            .and_then(|name| rules.projectile(name))
            .is_some_and(|projectile| projectile.vertical)
        {
            10
        } else {
            100
        },
    );
    #[cfg(not(feature = "ares"))]
    let velocity = crate::sim::projectile::launch::missile_launch_velocity(MISSILE_VELOCITY_SCALE);
    // The target is the CellClass `Map[NukeTarget]` gives (`0x0044CAB0..0x0044CAB5`,
    // then CreateBullet at `0x0044CAED`): the shared dummy for a cell off the map.
    let target = ProjectileTarget::looked_up_cell(sim.resolved_terrain.as_ref(), rx, ry);
    let payload = ProjectilePayload::new(
        weapon.damage,
        sim.interner.intern(&warhead.id),
        weapon
            .projectile
            .as_deref()
            .map(|id| sim.interner.intern(id)),
        weapon.bright,
    )
    .with_installed_weapon(Some(sim.interner.intern(&weapon.id)));
    if let Some(bullet) = FiredBullet::launched(
        sim,
        rules,
        native_unique_id,
        id,
        target,
        payload,
        MISSILE_SPEED,
        origin,
        velocity,
    ) {
        admit_fired_bullet(sim, rules, weapon, bullet_id, bullet);
    }
    // The hook also remembers the firing type on the missile (BulletExt
    // NukeSW) and takes its `Nuke.TakeOff`, else `[General] NukeTakeOff=`,
    // whose ZAdjust becomes -100 only while still 0.
    #[cfg(feature = "ares")]
    let take_off = {
        let firing_type = usize::try_from(firing)
            .ok()
            .and_then(|index| rules.super_weapon_order.get(index))
            .map(|name| sim.interner.intern(name));
        sim.projectiles.set_ares_nuke_sw(bullet_id, firing_type);
        firing_nuke(sim, rules, id)
            .and_then(|nuke| nuke.take_off.clone())
            .unwrap_or_else(|| Some(rules.general.nuke_take_off.clone()))
    };
    #[cfg(not(feature = "ares"))]
    let take_off = Some(rules.general.nuke_take_off.clone());
    if let Some(anim) = take_off.and_then(|name| spawn_anim(sim, rules, &name, origin, false)) {
        #[cfg(feature = "ares")]
        let unset = sim
            .substrate
            .anims
            .get(anim)
            .is_some_and(|anim| anim.z_adjust() == 0);
        #[cfg(not(feature = "ares"))]
        let unset = true;
        if unset {
            sim.substrate.set_anim_z_adjust(anim, TAKE_OFF_Z_ADJUST);
        }
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.set_handler_state(3);
    }
    1
}

/// The SWTypeExt Nuke fields of the type the silo fires (`+0x5F8`, the
/// type's `[SuperWeaponTypes]` index SW_NuclearMissile::Activate stores).
#[cfg(feature = "ares")]
fn firing_nuke<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    id: u64,
) -> Option<&'r crate::rules::superweapon_type::AresNukeRules> {
    let firing = sim
        .substrate
        .entities
        .get(id)?
        .mission_leaf
        .as_building()?
        .firing_super_weapon();
    let name = rules
        .super_weapon_order
        .get(usize::try_from(firing).ok()?)?;
    Some(rules.super_weapon_metadata(name)?.launch().nuke())
}

/// `AnimClass::AnimClass @ 0x00421EA0` with (delay 0, loop 1, flags 0x600,
/// ZAdjust 0, not reversed) at `coord`; `hidden` is the producer's `+0x19D`.
fn spawn_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    name: &str,
    coord: ProjectileCoord,
    hidden: bool,
) -> Option<crate::sim::anim_class::AnimId> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let world = crate::sim::anim_class::AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    };
    let type_name = sim.interner.intern(&name.to_ascii_uppercase());
    let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
    let descriptor = AnimClassSpawnDescriptor {
        delay: 0,
        loop_count: 1,
        draw_flags: ANIM_DRAW_FLAGS,
        z_adjust: 0,
        reverse: false,
        draw_runtime: crate::sim::anim_class::AnimDrawRuntime {
            hidden,
            ..Default::default()
        },
        ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
    };
    match sim.spawn_anim_at_world(rules, descriptor, world) {
        Ok(anim) => Some(anim),
        Err(error) => {
            log::debug!("nuclear silo anim [{name}] did not construct: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    //! Native comparisons (`tools/superweapon_oracle.py` section
    //! `mission_missile`; `--check` regenerates it).

    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, test_flat_cell, test_grid};
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;
    use crate::sim::projectile::ProjectileTrajectory;
    use serde_json::Value;

    /// Only the superweapon names `NukeCarrier`, as in a rules file
    /// without retail's `[WEEDGUY] Primary=NukeCarrier`.
    const RULES: &str = "[General]\nNukeTakeOff=NUKETO\n\
        [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
        [BuildingTypes]\n0=NAMISL\n1=GAPILE\n\
        [SuperWeaponTypes]\n0=NukeSpecial\n\
        [NukeSpecial]\nType=MultiMissile\nWeaponType=NukeCarrier\n\
        [NAMISL]\nStrength=1000\nNukeSilo=yes\nSuperWeapon=NukeSpecial\n\
        [GAPILE]\nStrength=1000\n\
        [NukeCarrier]\nDamage=1000\nProjectile=GiantNukeUp\nSpeed=100\nWarhead=NukeMaker\n\
        [GiantNukeUp]\nArm=2\nAcceleration=1\nVertical=yes\nDetonationAltitude=20000\n\
        [Warheads]\n0=NukeMaker\n\
        [NukeMaker]\nNukeMaker=yes\n\
        [Animations]\n0=PSIWARN\n1=NUKETO\n\
        [Missile]\nRate=1\n";
    const ART: &str = "[NAMISL]\nFoundation=1x1\n[GAPILE]\nFoundation=1x1\n\
        [PSIWARN]\n[NUKETO]\n";

    fn oracle() -> &'static Value {
        crate::test_fixture::json("tools/superweapon_oracle.json")
    }

    fn int(value: &Value) -> i32 {
        i32::try_from(value.as_i64().unwrap()).unwrap()
    }

    fn rules() -> RuleSet {
        let art = IniFile::from_str(ART);
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(RULES),
            &art,
        )
        .unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules
    }

    /// A silo (or a building that is not one) at (20, 30) whose owner's
    /// NukeTarget is `target`, on ground raised `target_level` there.
    fn world(
        rules: &RuleSet,
        kind: &str,
        target: (u16, u16),
        target_level: u8,
    ) -> (Simulation, u64) {
        let mut sim = Simulation::with_seed(5);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let owner = sim.interner.intern("Russians");
        sim.houses
            .register(HouseState::new(owner, 1, None, true, 0, 10));
        sim.resolved_terrain = Some(test_grid(80, 220, |x, y| ResolvedTerrainCell {
            level: if (x, y) == target { target_level } else { 0 },
            ..test_flat_cell(x, y)
        }));
        crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, (target, target_level));
        let silo = sim
            .spawn_object_at_height(kind, "Russians", 20, 30, 0, 0, rules)
            .unwrap();
        sim.houses.get_mut(&owner).unwrap().set_nuke_target(target);
        sim.substrate
            .entities
            .get_mut(silo)
            .unwrap()
            .mission_leaf
            .set_building_firing_super_weapon(0);
        (sim, silo)
    }

    fn velocity_bits(velocity: crate::sim::projectile::ProjectileVelocity) -> Vec<u64> {
        velocity
            .native()
            .iter()
            .map(|component| component.bits())
            .collect()
    }

    /// Each status of a silo's Missile against the original, for a missile
    /// that constructs and fires; the anims, bullet and stores follow it.
    #[test]
    fn mission_missile_matches_native() {
        let oracle = oracle();
        let rows = oracle["mission_missile"].as_array().unwrap();
        // A refused Fire or a failed construction (three rows) and the other
        // arm (one row) are not replayed: VERA admits every bullet, and the
        // other arm only waits its mission rate here.
        let fired: Vec<&Value> = rows
            .iter()
            .filter(|row| {
                row["silo"] == true && row["fire_accepts"] == true && row["bullet"] == true
            })
            .collect();
        assert_eq!((rows.len(), fired.len()), (15, 11));
        let rules = rules();
        for row in fired {
            let target = (
                u16::try_from(int(&row["target"][0])).unwrap(),
                u16::try_from(int(&row["target"][1])).unwrap(),
            );
            let level = u8::try_from(int(&row["target_level"])).unwrap();
            let (mut sim, silo) = world(&rules, "NAMISL", target, level);
            let owner = sim.substrate.entities.get(silo).unwrap().owner();
            {
                let entity = sim.substrate.entities.get_mut(silo).unwrap();
                entity
                    .mission
                    .set_handler_state(u32::try_from(int(&row["status"])).unwrap());
                entity
                    .mission_leaf
                    .set_building_ready_latch(u8::from(row["ready"] == true));
            }
            let mode_before = sim
                .substrate
                .entities
                .get(silo)
                .unwrap()
                .queued_building_body_state();
            let anims_before: Vec<_> = sim.substrate.anims.iter().map(|(&id, _)| id).collect();
            let delay = mission_missile(&mut sim, silo, &rules, ObjectAiCtx::default(), &mut false);
            assert_eq!(delay, int(&row["delay"]), "{row}");
            let entity = sim.substrate.entities.get(silo).unwrap();
            assert_eq!(
                (
                    entity.mission.handler_state(),
                    entity.building_ready_latch()
                ),
                (
                    u32::try_from(int(&row["status_after"])).unwrap(),
                    u8::try_from(int(&row["ready_after"])).unwrap()
                ),
                "{row}"
            );
            let events = row["events"].as_array().unwrap();
            let modes: Vec<i32> = events
                .iter()
                .filter(|event| event[0] == "begin_mode")
                .map(|event| int(&event[1]))
                .collect();
            assert_eq!(
                entity.queued_building_body_state(),
                modes.last().copied().or(mode_before),
                "{row}"
            );
            let new_anims: Vec<_> = sim
                .substrate
                .anims
                .iter()
                .filter(|(id, _)| !anims_before.contains(id))
                .collect();
            let native_anims: Vec<&Value> =
                events.iter().filter(|event| event[0] == "anim").collect();
            assert_eq!(new_anims.len(), native_anims.len(), "{row}");
            let bullets: Vec<_> = sim
                .projectiles
                .iter()
                .filter(|(_, bullet)| bullet.source_id() == silo)
                .collect();
            let fire = events.iter().find(|event| event[0] == "fire");
            assert_eq!(bullets.len(), usize::from(fire.is_some()), "{row}");
            let Some(fire) = fire else { continue };
            let (&bullet_id, bullet) = bullets[0];
            let native_velocity: Vec<u64> = fire[2]
                .as_array()
                .unwrap()
                .iter()
                .map(|bits| bits.as_u64().unwrap())
                .collect();
            // Ares hook 44CB4C (10036660): (0, 0, 10) for the Vertical carrier.
            #[cfg(feature = "ares")]
            let native_velocity = [0.0_f64, 0.0, 10.0].map(f64::to_bits).to_vec();
            assert_eq!(velocity_bits(bullet.velocity()), native_velocity, "{row}");
            // Construct's damage and speed; the carrier's BulletType.
            let construct = events
                .iter()
                .find(|event| event[0] == "create_bullet")
                .unwrap();
            assert_eq!(bullet.payload().base_damage, int(&construct[2]), "{row}");
            assert_eq!(
                bullet.trajectory(),
                ProjectileTrajectory::Vertical {
                    detonation_altitude: 20000,
                    acceleration: 1,
                    max_speed: int(&construct[3]),
                },
                "{row}"
            );
            assert_eq!(
                bullet.target(),
                crate::sim::projectile::ProjectileTarget::Cell {
                    rx: target.0,
                    ry: target.1
                }
            );
            for (&(&id, anim), native) in new_anims.iter().zip(&native_anims) {
                let name = sim.interner.resolve(anim.type_id());
                let coord = sim.anim_absolute_coord(id).unwrap();
                match native[1].as_str().unwrap() {
                    "PSIWARN" => {
                        assert_eq!(name, "PSIWARN");
                        let native_coord: Vec<i32> =
                            native[2].as_array().unwrap().iter().map(int).collect();
                        assert_eq!(vec![coord.x, coord.y, coord.z], native_coord, "{row}");
                        assert!(anim.draw_runtime().hidden, "{row}");
                        assert_eq!(anim.owner_house(), Some(owner));
                        assert_eq!(anim.attached_bullet(), Some(bullet_id));
                    }
                    "take_off" => {
                        assert_eq!(name, "NUKETO");
                        // At the bullet's launch point, as native's FLH result
                        // feeds both.
                        assert_eq!(native[2], fire[1], "{row}");
                        assert_eq!(
                            [coord.x, coord.y, coord.z],
                            [
                                bullet.launch_origin().x,
                                bullet.launch_origin().y,
                                bullet.launch_origin().z
                            ],
                            "{row}"
                        );
                        assert_eq!(anim.z_adjust(), int(&row["take_off_z_adjust"]));
                    }
                    other => panic!("anim {other}"),
                }
                assert_eq!(anim.draw_flags(), u32::try_from(int(&native[5])).unwrap());
            }
        }
    }

    /// Ares hooks 44C9FF (10036750) and 44CB4C (10036660): the firing
    /// type's `Nuke.PsiWarning=none` skips the warning, its `Nuke.TakeOff`
    /// plays at the launch point, and the missile remembers the type.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_silo_missile_takes_the_firing_types_anims() {
        let art = IniFile::from_str(&format!("{ART}[FLAME]\n"));
        let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
        ini.merge(&IniFile::from_str(
            "[NukeSpecial]\nNuke.PsiWarning=none\nNuke.TakeOff=FLAME\n\
             [Animations]\n0=PSIWARN\n1=NUKETO\n2=FLAME\n",
        ));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        let (mut sim, silo) = world(&rules, "NAMISL", (50, 60), 0);
        sim.substrate
            .entities
            .get_mut(silo)
            .unwrap()
            .mission
            .set_handler_state(0);
        mission_missile(&mut sim, silo, &rules, ObjectAiCtx::default(), &mut false);
        let names: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| sim.interner.resolve(anim.type_id()).to_owned())
            .collect();
        assert_eq!(names, ["FLAME"]);
        let (_, bullet) = sim.projectiles.iter().next().unwrap();
        assert_eq!(
            bullet.ares_nuke_sw().map(|id| sim.interner.resolve(id)),
            Some("NukeSpecial")
        );
    }

    /// A building that is neither a silo nor an EMP cannon only waits its
    /// mission rate: the oracle's row and the fixture's `[Missile] Rate=1`
    /// are one minute. Its firing type also makes it fire weapon 0 (it has
    /// none) at the NukeTarget.
    #[test]
    fn a_building_that_is_not_a_silo_waits_the_mission_rate() {
        let oracle = oracle();
        let row = oracle["mission_missile"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["silo"] == false)
            .unwrap();
        assert!(row["events"].as_array().unwrap().is_empty());
        let rules = rules();
        let (mut sim, building) = world(&rules, "GAPILE", (50, 60), 0);
        sim.substrate
            .entities
            .get_mut(building)
            .unwrap()
            .mission
            .set_handler_state(0);
        // Ares hook 44CCE7 (10036570): with a firing type, a building that
        // is not a cannon fires weapon 0, queues Guard and returns 1.
        #[cfg(feature = "ares")]
        let delay = 1;
        #[cfg(not(feature = "ares"))]
        let delay = int(&row["delay"]);
        assert_eq!(
            mission_missile(
                &mut sim,
                building,
                &rules,
                ObjectAiCtx::default(),
                &mut false
            ),
            delay
        );
        assert!(sim.projectiles.iter().next().is_none());
    }

    /// `BulletClass::Fire` keeps the target cell's vt+0x58 (`0x00468707`):
    /// over a structural bridge deck, 416 leptons above its ground.
    #[test]
    fn the_missile_aims_at_the_bridge_deck_of_its_target_cell() {
        let rules = rules();
        let target = (50, 60);
        let (mut sim, silo) = world(&rules, "NAMISL", target, 0);
        let grid = sim.resolved_terrain.as_mut().unwrap();
        grid.cell_mut_for_test(target.0, target.1)
            .unwrap()
            .bridge_facts
            .raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        sim.substrate
            .entities
            .get_mut(silo)
            .unwrap()
            .mission
            .set_handler_state(2);
        mission_missile(&mut sim, silo, &rules, ObjectAiCtx::default(), &mut false);
        let (_, bullet) = sim.projectiles.iter().next().unwrap();
        let ground = cell_ground_coord(sim.resolved_terrain.as_ref(), target.0, target.1);
        assert_eq!(
            [
                bullet.launch_target().x,
                bullet.launch_target().y,
                bullet.launch_target().z
            ],
            [ground.x, ground.y, ground.z + 416]
        );
    }
}
