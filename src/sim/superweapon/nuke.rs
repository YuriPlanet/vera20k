//! The nuclear missile's launch: `SuperClass::Launch @ 0x006CC390` case 0
//! (`Type=MultiMissile`, `0x006CDA67`) for a Super that is not one-time.
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
//! - The one-time arm (`0x006CDA88..0x006CDCB3`: a NukePayload bullet
//!   dropped straight on the cell): VERA grants no one-time Super.
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
//! - `DigSound=` plays at the cell (the launch event carries it); native's
//!   coordinate has the cell's floor height (`0x00578080`). Presentation
//!   only.
//! - The impact's wait reads the anim's liveness (`begin_ai_visit`)
//!   instead of the holder list `0x00B0F5B8` whose PointerExpired clears
//!   `+0x154`; VERA reuses no anim id, so the two answer alike.
//! - A `NUKEBALL` VERA cannot construct (a type without its SHP, as in lib
//!   tests) holds no anim, and the bullet detonates at its next AI; retail
//!   binds `NUKEBALL.SHP`.
//! - The head's IsAlive gate (`0x004666F7`) has no counterpart: a detonated
//!   bullet leaves VERA's store at its commit.
//! - The chrono screen (`0x00A9FAB0`) shares the flash's lighting arms. Map
//!   trigger action 127 sets it (`0x006DFABF` -> `0x0053B460`) and
//!   `0x0053B560` steps it through the shared pre-object scheduler. Its
//!   private state and presentation commands are in `chrono_screen`.
//! - The bridge units' draw brightening by four times the active profile's
//!   Level (`UnitClass::DrawIt 0x0073D03B`, `InfantryClass::DrawIt
//!   0x00519319`), the nuke's included, is not ported for any profile.
//! - `LightConvertClass`'s constructor (`0x00555DA0`) tints a new palette
//!   with the Ion profile while a storm rages, then the nuke's while the
//!   flash fades in (`0x00555EE8`), then the Dominator's; VERA tints every
//!   palette with UpdateLighting's choice, which puts the nuke first. Only a
//!   palette created while a storm rages over a fading-in flash differs.

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
        .map(|bullet| bullet.position)
    else {
        return;
    };
    sim.session
        .lighting
        .start_nuke_flash(sim.session.binary_frame as i32);
    sim.publish_global_lighting();
    let cell = |leptons: i32| (leptons / 256) as i16 as u16;
    sim.sound_events.push(SimSoundEvent::RadarEvent {
        radar: RadarEventRequest::new(
            RadarEventType::ImpactSilent,
            cell(location.x),
            cell(location.y),
        ),
    });
    if !impact.waits() {
        return;
    }
    let nuke_ball = sim
        .interner
        .intern(crate::rules::effect_asset_catalog::NUKE_BALL_ANIM);
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
/// (`0x006CDDA3..0x006CDDD7`). The app plays `DigSound=` at the cell and
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
        sim.request_house_tech_tree_recheck(owner);
        return false;
    };
    let Some(silo) = find_building_of_type(sim, owner, silo_type) else {
        sim.request_house_tech_tree_recheck(owner);
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
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type: sw_type_id,
        rx: cell.0,
        ry: cell.1,
    });
    sim.request_house_tech_tree_recheck(owner); //Launch common tail6CDE2F
    true
}

/// `HouseClass::Find_Building_Of_Type @ 0x004FD060` with no edge (`-1`):
/// when the house's owned-type counter (`+0x5500`, `0x0049FAE0`) holds the
/// type, its first building (`+0x68` order) of that type outside Limbo.
fn find_building_of_type(sim: &Simulation, owner: InternedId, type_id: InternedId) -> Option<u64> {
    let house = sim.houses.get(&owner)?;
    if house
        .tracking
        .owned_count(EntityCategory::Structure, type_id)
        <= 0
    {
        return None;
    }
    house
        .base_projection
        .buildings()
        .iter()
        .copied()
        .find(|&id| {
            sim.substrate.entities.get(id).is_some_and(|building| {
                !building.lifecycle.in_limbo() && building.type_ref() == type_id
            })
        })
}
