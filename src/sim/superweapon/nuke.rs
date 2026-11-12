//! The nuclear missile's launch: `SuperClass::Launch @ 0x006CC390` case 0
//! (`Type=MultiMissile`, `0x006CDA67`), including the one-time payload drop.
//! The silo's flight (`BuildingClass::Mission_Missile @ 0x0044C980`) and
//! the falling warhead (`NukeMaker=`, `0x0046B310`) belong to the building
//! missions and the projectile detonation.
//!
//! The warhead's impact is here too: `BulletClass::AI`'s `NUKE` block
//! ([`impact`]), whose bullet waits on the `NUKEBALL` anim before it
//! detonates (`sim::projectile`), and the screen flash it starts, which the
//! scenario's lighting runs. Evidence: `tools/superweapon_oracle.py`
//! sections `nuke_impact`, `nuke_wait`, `nuke_flash` and
//! `nuke_lighting_read`, and the NukeFlash rows of `update_lighting`,
//! `ambient_step` and `relight`.
//!
//! Scenario draws: none in the block; NUKEBALL's own (its crater) and the
//! delayed detonation's draw through their owners. Timer writes: the flash's
//! timer (30 frames, then 15) and the ambient fade timer
//! (`ScenarioClass+0x1248`, `[frame, 1]`). Detach calls: none; the anim's
//! UnInit releases the bullet (`BulletClass::PointerExpired 0x004685BE`).
//!
//! The launch ([`launch`]) is compared with native execution in
//! `tools/superweapon_oracle.py` section `nuke_launch`, replayed in
//! `nuke_tests.rs`; the app runs the local player's tail (`0x006CDE06..
//! 0x006CDE27`: the selection cleared, the queued `EVA_NuclearMissileReady`
//! dropped). Ledger: no Scenario draws or detach calls; the silo's Commence
//! (`0x005B3570`) restarts its mission clock (`+0xC0`) and update timer
//! (`+0xC8..+0xD0`) through the mission owner (`queue_and_commence`), and
//! the recharge restart is ClickFire's.
//!
//! RESIDUALS:
//! - A charged launch that finds no silo type (`0x006CDCFA`, `0x006CDD30`)
//!   or no silo (`0x006CDD44`) still runs the player's common tail at
//!   `0x006CDE06`; the sim retains its `+0x1FC` request (`0x006CDE2F`),
//!   but emits no launch event then, so the app's selection-clear tail is
//!   absent. Trigger: a custom no-silo type or loss before the House tail.
//! - The House dirty tail also refreshes local building construction options
//!   (vt+0x4E0) and sidebar strips (`0x006A7D20`); those presentation
//!   consumers are outside the simulation's revoke/grant migration.
//! - The launch-mute global `0x00A8B538` that skips the EVA line
//!   (`0x006CDDEE`) is not modeled: the line always plays.
//! - A `NUKEBALL` VERA cannot construct (a type without its SHP, as in lib
//!   tests) holds no anim, and the bullet detonates at its next AI; retail
//!   binds `NUKEBALL.SHP`.
//! - The head's IsAlive gate (`0x004666F7`) has no counterpart: a detonated
//!   bullet leaves VERA's store at its commit.
//! - The chrono screen (`0x00A9FAB0`) shares the flash's lighting arms. Map
//!   trigger action 127 sets it (`0x006DFABF` -> `0x0053B460`) and
//!   `0x0053B560` steps it through the shared pre-object scheduler. Its
//!   private state and presentation commands are in `chrono_screen`.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::projectile::ProjectileNukeImpact;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::{SimSoundEvent, Simulation};

/// The rest of a `NUKE` warhead's impact in `BulletClass::AI`
/// (`0x00467E87..0x00467F36`), the bullet on its clamped Location:
/// - `ScreenNukeFlash @ 0x0053AB70` (`ScenarioLightingState::
///   start_nuke_flash`, then the relight);
/// - a type-13 radar event at the bullet's cell (`CreateRadarEvent @
///   0x0065FA70`, `0x00467EA7`; `ObjectClass::GetMapCoords @ 0x0041BEA0`
///   truncates toward zero);
/// - when the bullet waits, the `NUKEBALL` anim at its Location, which it
///   then holds: `AnimClass::AnimClass @ 0x00421EA0` (`0x00467F2D`) with a
///   combat explosion's arguments (delay 0, one loop, draw flags `0x2600`,
///   z-adjust `0x0048ACE0`'s -15, not reversed).
pub(crate) fn impact(sim: &mut Simulation, rules: &RuleSet, impact: ProjectileNukeImpact) {
    let Some(location) = sim
        .projectiles
        .get(impact.projectile_id())
        .map(|bullet| bullet.position())
    else {
        return;
    };
    let frame = sim.session.binary_frame() as i32;
    let cell = |leptons: i32| (leptons / 256) as i16 as u16;
    let radar = RadarEventRequest::new(
        RadarEventType::ImpactSilent,
        cell(location.x),
        cell(location.y),
    );
    // Ares hook 467E59 (10036960): the superweapon's radar event, then the
    // flash for its duration (the manual FadeIn and ChangeLighting), then
    // the wait on the warhead's PreImpactAnim.
    #[cfg(feature = "ares")]
    if let Some(ball) = impact.ares() {
        if ball.radar {
            sim.sound_events.emit(SimSoundEvent::RadarEvent { radar });
        }
        if let Some(duration) = ball.flash {
            sim.session
                .lighting_mut()
                .start_nuke_flash_for(frame, duration);
            // SWTypeExt::ChangeLighting(NukeSW): the type's Light.* overrides.
            let light = ball
                .nuke_sw
                .and_then(|sw| rules.super_weapon_metadata(sim.interner.resolve(sw)))
                .map(|metadata| metadata.launch().lighting());
            sim.session.lighting_mut().select_ares(
                crate::sim::scenario_session::ScenarioLightingProfile::Nuke,
                light,
            );
            sim.publish_global_lighting(crate::sim::light_sources::PaletteUpdate::from_scenario(
                sim.session.lighting(),
            ));
        }
        let anim = sim
            .projectiles
            .get(impact.projectile_id())
            .and_then(|bullet| sim.interner.warhead_type(bullet.payload().warhead, rules))
            .and_then(|warhead| warhead.ares_rules())
            .and_then(|ares| ares.combat().pre_impact_anim())
            .map(str::to_owned);
        if let Some(anim) = anim.filter(|_| impact.waits()) {
            let anim = sim.interner.intern(&anim);
            await_impact_anim(sim, rules, impact, anim, location);
        }
        return;
    }
    sim.session.lighting_mut().start_nuke_flash(frame);
    // ScreenNukeFlash53AC39 calls RecalcLighting with alternate flag1.
    sim.publish_global_lighting(crate::sim::light_sources::PaletteUpdate::from_scenario(
        sim.session.lighting(),
    ));
    sim.sound_events.emit(SimSoundEvent::RadarEvent { radar });
    if !impact.waits() {
        return;
    }
    let nuke_ball = sim
        .interner
        .intern(crate::rules::effect_asset_catalog::NUKE_BALL_ANIM);
    await_impact_anim(sim, rules, impact, nuke_ball, location);
}

/// The waiting bullet's anim (`0x00467EB6..0x00467F36`): constructed at its
/// Location with a combat explosion's arguments and held.
fn await_impact_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    impact: ProjectileNukeImpact,
    nuke_ball: InternedId,
    location: crate::sim::projectile::ProjectileCoord,
) {
    let fx = crate::sim::combat::detonation_anim::placed_effect(nuke_ball, location);
    let anim = sim.spawn_combat_explosion_anim(
        rules,
        fx.shp_name,
        fx.rx,
        fx.ry,
        fx.sub_x,
        fx.sub_y,
        fx.z,
        fx.world_z,
    );
    sim.projectiles.await_anim(impact.projectile_id(), anim);
}

/// Ares's impact hook (467E59, 10036960) for a bullet of `warhead` whose
/// BulletExt NukeSW is `nuke_sw`: see [`crate::sim::projectile::AresNukeBall`].
/// The flash's `Light.Enabled` gate is the superweapon type's.
#[cfg(feature = "ares")]
pub(crate) fn ares_nuke_ball(
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    warhead: InternedId,
    nuke_sw: Option<InternedId>,
) -> crate::sim::projectile::AresNukeBall {
    let warhead = interner.warhead_type(warhead, rules);
    let combat = warhead
        .and_then(|warhead| warhead.ares_rules())
        .map(|ares| ares.combat());
    let nuke_maker = warhead.is_some_and(|warhead| warhead.nuke_maker);
    let metadata = nuke_sw
        .filter(|_| !nuke_maker)
        .and_then(|sw| rules.super_weapon_metadata(interner.resolve(sw)));
    let sw_bullet = metadata.is_some();
    let (allow_flash, flash_default) = match metadata {
        Some(metadata) => (metadata.launch().lighting().enabled(), 30),
        None => (true, 0),
    };
    let duration = combat
        .and_then(|combat| combat.nuke_flash_duration())
        .unwrap_or(flash_default);
    crate::sim::projectile::AresNukeBall {
        sw_bullet,
        radar: metadata.is_some_and(|metadata| metadata.launch().radar_event()),
        flash: (allow_flash && duration > 0).then_some(duration),
        pre_impact_anim: combat.is_some_and(|combat| combat.pre_impact_anim().is_some()),
        nuke_sw: nuke_sw.filter(|_| sw_bullet),
    }
}

/// `AnimTypeClass::FindIndex("NUKEBALL") @ 0x00427CB0` (`0x00467EB1`)
/// finds a type: a `NUKE` bullet then waits on its anim.
pub(crate) fn nuke_ball_type(rules: &RuleSet) -> bool {
    rules
        .anim_type_names
        .contains(crate::rules::effect_asset_catalog::NUKE_BALL_ANIM)
}

/// Launch case 0 for `owner`'s Super of type `sw` at `cell`: a charged
/// Super (`+0x6F`, `0x006CDA67`) takes the first BuildingType in
/// BuildingTypeClass::Array order with `NukeSilo=` (`+0x16BA`) whose
/// `SuperWeapon=` (`+0x16F0`) or `SuperWeapon2=` (`+0x16F4`) is the type's
/// array index (`+0x98`) as the silo type (`0x006CDCF0..0x006CDD30`); the
/// owner's first building of it ([`find_building_of_type`]) queues and
/// commences Missile, the owner's NukeTarget (`+0x5784`) becomes the cell
/// and the silo's firing type (`+0x5F8`) the type's `Type=`
/// (`0x006CDDA3..0x006CDDD7`). The app plays `DigSound=` at the cell's
/// centre and ground height (no deck, `0x006CDD62..0x006CDDE9`) and
/// `EVA_NuclearMissileLaunched` (`0x006CDDE9`, `0x006CDE01`) for the launch
/// event. A charged call still requests the House tail without a silo
/// (`0x006CDE06..0x006CDE2F`; executed `nuke_launch.recheck`).
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) -> bool {
    if !super::is_charged(sim, owner, sw_type_id) {
        return false;
    }
    if sim
        .super_weapons
        .get(&owner)
        .and_then(|weapons| weapons.get(&sw_type_id))
        .is_some_and(|instance| instance.is_active && instance.one_time)
    {
        //6CDA72..6CDCB3: skip silo selection/mission/NukeTarget. Speed comes
        //from NukePayload; the common projectile owner also creates PSIWARN.
        let launched = sim.launch_nuke_payload_at_cell(
            rules,
            (cell.0 as i16, cell.1 as i16),
            None,
            Some(owner),
        );
        // The cue (`0x006CDCAE`) plays at the strike coordinate
        // `0x006CDA88..0x006CDAF5` built.
        if let Some(location) = launched {
            sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type: sw_type_id,
                world: location.into(),
            });
        }
        sim.houses.request_house_tech_tree_recheck(owner); //common6CDE2F tail
        return launched.is_some();
    }
    let Some(index) = rules.super_weapon_index(&sw.id) else {
        return false;
    };
    let fires = |object: &ObjectType| {
        object.nuke_silo
            && [&object.super_weapon, &object.super_weapon2]
                .into_iter()
                .any(|name| {
                    name.as_deref()
                        .and_then(|name| rules.super_weapon_index(name))
                        == Some(index)
                })
    };
    let ids = rules.type_array_ids(ObjectCategory::Building);
    let Some(silo_type) = (0..ids.len())
        .find(|&slot| rules.building_type_at(slot as i32).is_some_and(fires))
        .and_then(|slot| sim.interner.get(&ids[slot]))
    else {
        sim.houses.request_house_tech_tree_recheck(owner);
        return false;
    };
    let Some(silo) = find_building_of_type(sim, owner, silo_type) else {
        sim.houses.request_house_tech_tree_recheck(owner);
        return false;
    };
    crate::sim::world::queue_and_commence(sim, silo, MissionType::Missile, rules);
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.set_nuke_target(cell);
    }
    if let Some(entity) = sim.substrate.entities.get_mut(silo) {
        entity
            .mission_leaf
            .set_building_firing_super_weapon(sw.kind.native_index());
    }
    // `0x006CDD62..0x006CDDE9`: the cell centre at Map578080's ground height,
    // no deck. At its centre that is the cell's own GetCoords (`0x00486840`).
    let ground =
        crate::sim::projectile::cell_ground_coord(sim.resolved_terrain.as_ref(), cell.0, cell.1);
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type: sw_type_id,
        world: ground.into(),
    });
    sim.houses.request_house_tech_tree_recheck(owner); //Launch common tail6CDE2F
    true
}

/// `HouseClass::Find_Building_Of_Type @ 0x004FD060` with no edge (`-1`):
/// when the house's owned-type counter (`+0x5500`, `0x0049FAE0`) holds the
/// type, its first building (`+0x68` order) of that type outside Limbo.
fn find_building_of_type(sim: &Simulation, owner: InternedId, type_id: InternedId) -> Option<u64> {
    let house = sim.houses.get(&owner)?;
    if house
        .tracking()
        .owned_count(EntityCategory::Structure, type_id)
        <= 0
    {
        return None;
    }
    house
        .base_projection()
        .buildings()
        .iter()
        .copied()
        .find(|&id| {
            sim.substrate.entities.get(id).is_some_and(|building| {
                !building.lifecycle.in_limbo() && building.type_ref() == type_id
            })
        })
}

/// SW_NuclearMissile::GetWarhead (Ares 3.0 `10073320`) and GetDamage
/// (`100732F0`) with the type's `Nuke.Payload`: the payload weapon, the
/// type's non-null `SW.Warhead` else the payload's warhead, and its
/// nonnegative `SW.Damage` else the payload's damage. `None` without a
/// payload.
#[cfg(feature = "ares")]
pub(crate) fn ares_payload_hit<'r>(
    rules: &'r RuleSet,
    sw: &SuperWeaponType,
) -> Option<(
    &'r crate::rules::weapon_type::WeaponType,
    Option<&'r crate::rules::warhead_type::WarheadType>,
    i32,
)> {
    let launch = rules.super_weapon_metadata(&sw.id)?.launch();
    let payload = rules.weapon(launch.nuke().payload.as_deref()?)?;
    let warhead = launch
        .warhead()
        .flatten()
        .or(payload.warhead.as_deref())
        .and_then(|name| rules.warhead(name));
    let damage = launch
        .damage()
        .filter(|&damage| damage >= 0)
        .unwrap_or(payload.damage);
    Some((payload, warhead, damage))
}

/// SW_NuclearMissile::Activate (Ares 3.0 `10072FF0`, hook 6CC390) for
/// `owner`'s charged Super of `sw` at `cell`:
/// 1. unless a one-time grant (`+0x6D` and `+0x6E` both set), with
///    `Nuke.SiloLaunch`, the owner's first building in its list order that is
///    a launch site of the type (`IsLaunchSiteEligible` 1006DA20: a
///    `NukeSilo=` building that provides it, alive, out of limbo and powered,
///    within `SW.RangeMinimum`/`.RangeMaximum` of the cell) takes the type's
///    index as its firing type (`+0x5F8`), queues and commences Missile, and
///    the owner's NukeTarget becomes the cell;
/// 2. otherwise a `Nuke.Payload` with a BulletType drops at once: a bullet
///    of the payload's speed and Bright, with the type's GetDamage and
///    GetWarhead, no owner, aimed at the cell; the House's PsiWarn of
///    `Nuke.PsiWarning`; the bullet remembers the type; a `Vertical=`
///    BulletType starts 20000 leptons over the cell's deck coordinates
///    falling at (0, 0, -100), any other at them moving (0, 100, 0);
/// 3. once fired, a type-13 radar event at the cell for the clients allied
///    with the owner (with `SW.CreateRadarEvent`), `SW.ActivationSound`
///    (else `[AudioVisual] DigSound=`) at the deck coordinates, and the
///    owner's tech-tree recheck.
///
/// Returns whether it fired; Launch otherwise runs the stock case.
#[cfg(feature = "ares")]
pub(super) fn ares_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) -> bool {
    if !super::is_charged(sim, owner, sw_type_id) {
        return false;
    }
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return false;
    };
    let nuke = metadata.launch().nuke();
    let [x, y, z] = super::deck_coords(sim, cell);
    let target = crate::sim::components::DriveCoord { x, y, z };
    let one_time_grant = sim
        .super_weapons
        .get(&owner)
        .and_then(|weapons| weapons.get(&sw_type_id))
        .is_some_and(|instance| instance.is_active && instance.one_time);
    let mut fired = false;
    if !one_time_grant && nuke.silo_launch {
        let range = metadata.targeting().range();
        let silo = super::targeting::launch_sites(sim, rules, owner, sw)
            .into_iter()
            .find(|(id, site)| {
                sim.substrate
                    .entities
                    .get(*id)
                    .and_then(|building| sim.object_type(building.type_ref(), rules))
                    .is_some_and(|object| object.nuke_silo)
                    && site.admits((i32::from(cell.0), i32::from(cell.1)), range)
            })
            .map(|(id, _)| id);
        if let (Some(silo), Some(index)) = (silo, rules.super_weapon_index(&sw.id)) {
            if let Some(entity) = sim.substrate.entities.get_mut(silo) {
                entity
                    .mission_leaf
                    .set_building_firing_super_weapon(index as i32);
            }
            crate::sim::world::queue_and_commence(sim, silo, MissionType::Missile, rules);
            if let Some(house) = sim.houses.get_mut(&owner) {
                house.set_nuke_target(cell);
            }
            fired = true;
        }
    }
    if !fired && let Some((weapon, warhead, damage)) = ares_payload_hit(rules, sw) {
        fired = drop_payload(sim, rules, owner, sw_type_id, weapon, warhead, damage, cell);
    }
    if !fired {
        return false;
    }
    if metadata.launch().radar_event() {
        sim.sound_events.emit(SimSoundEvent::AresAlliedRadarEvent {
            owner,
            radar: RadarEventRequest::new(RadarEventType::ImpactSilent, cell.0, cell.1),
        });
    }
    let sound = match &sw.presentation.activation_sound {
        Some(sound) => sound.clone(),
        None => rules.general.dig_sound.clone(),
    };
    if let Some(sound_id) = sound {
        sim.sound_events.emit(SimSoundEvent::VocAtCoord {
            sound_id,
            coordinate: target,
            flags: 0,
        });
    }
    sim.houses.request_house_tech_tree_recheck(owner);
    true
}

/// Step 2 of [`ares_launch`]: CreateBullet (`0x0046B050`) over the
/// payload's BulletType, then SetWeaponType, PsiWarn and MoveTo.
#[cfg(feature = "ares")]
#[allow(clippy::too_many_arguments)]
fn drop_payload(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    weapon: &crate::rules::weapon_type::WeaponType,
    warhead: Option<&crate::rules::warhead_type::WarheadType>,
    damage: i32,
    cell: (u16, u16),
) -> bool {
    use crate::sim::projectile::{ProjectileCoord, ProjectilePayload, ProjectileTarget};
    let Some(projectile) = weapon
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name))
    else {
        return false;
    };
    let Some(warhead) = warhead else {
        // Native constructs the bullet with a null warhead.
        return false;
    };
    let target = ProjectileTarget::looked_up_cell(sim.resolved_terrain.as_ref(), cell.0, cell.1);
    let payload = ProjectilePayload::new(
        damage,
        sim.interner.intern(&warhead.id),
        Some(sim.interner.intern(&projectile.id)),
        weapon.bright,
    )
    .with_installed_weapon(Some(sim.interner.intern(&weapon.id)));
    let bullet_id = sim.substrate.allocate_stable_id();
    let native_id = sim.next_native_runtime_id();
    if let Some(psi_warning) = rules
        .super_weapon_metadata(sim.interner.resolve(sw_type_id))
        .and_then(|metadata| metadata.launch().nuke().psi_warning.clone())
    {
        // HouseClass::PsiWarn 43B5E0: at the target Cell's GetCoords.
        let at = crate::sim::projectile::cell_ground_coord(
            sim.resolved_terrain.as_ref(),
            cell.0,
            cell.1,
        );
        sim.spawn_bullet_warning(
            rules,
            &psi_warning,
            crate::sim::anim_class::AnimWorldCoord {
                x: at.x,
                y: at.y,
                z: at.z,
            },
            bullet_id,
            owner,
        );
    }
    let [x, y, z] = super::deck_coords(sim, cell);
    let (origin, velocity) = if projectile.vertical {
        (
            ProjectileCoord::new(x, y, z.wrapping_add(20000)),
            crate::sim::projectile::ProjectileVelocity::new(0, 0, -100),
        )
    } else {
        (
            ProjectileCoord::new(x, y, z),
            crate::sim::projectile::ProjectileVelocity::new(0, 100, 0),
        )
    };
    let Some(bullet) = crate::sim::combat::FiredBullet::launched(
        sim,
        rules,
        native_id,
        crate::sim::combat::RAD_NO_ATTACKER,
        target,
        payload,
        weapon.speed,
        origin,
        velocity,
    ) else {
        return false;
    };
    crate::sim::combat::admit_fired_bullet(sim, rules, weapon, bullet_id, bullet);
    sim.projectiles
        .set_ares_nuke_sw(bullet_id, Some(sw_type_id));
    true
}
