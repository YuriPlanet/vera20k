//! `AircraftClass::Fire_At @ 0x00415EE0` (vt+0x3CC), an aircraft's shot.
//! Mission_Attack's strike states call it for each bomb, missile and burst
//! shot ([`super::attack_mission`]).
//!
//! - With a first passenger (`+0x118`) it drops one (`Drop_Payload @
//!   0x00415C60`) and returns NULL (`0x00415EEE..0x00415F05`), before
//!   anything else.
//! - Otherwise `TechnoClass::FireAt @ 0x006FDD50` fires. When it answers a
//!   bullet, Fire_At sets that bullet's course by its type's `ROT=`
//!   (`+0xAC`, `+0x2DC`; [`AircraftBulletCourse`]): ROT 0 flies level along
//!   SecondaryFacing (`+0x3A0`) at the Locomotor's Apparent_Speed, ROT 1 at
//!   the target at weapon 0's `Speed=` (`GetWeapon(0)`, whatever slot
//!   fired), any other ROT keeps FireAt's velocity. In retail rules the
//!   Black Eagle's, Harrier's and Boris MiG's missiles are ROT 100; the
//!   Hornet's bomb (`NormalBomb`) and the Osprey's depth charge
//!   (`DepthCharge`) are ROT 1, where FireAt alone would launch them at
//!   speed 1 and leave the bullet's ramp to accelerate them.
//! - Then, on each client whose `0x0050B6F0` admits the owner (the owner's
//!   own client, or in a campaign the player's for a `PlayerControl=`
//!   house too), it asks `MapClass::IsShrouded @ 0x00586360` of that
//!   client's map about its Location (`+0x9C`), the Location offset by
//!   (0x200, 0x200), (-0x200, -0x200) and (0x200, -0x200), that last one
//!   twice (the fourth corner is never asked), and the target's GetCoords.
//!   At the first shrouded one it reveals
//!   `[General] AttackingAircraftSightRange=` around its Location
//!   (`MapClass::RevealArea @ 0x005678E0` by height, final 0 then final 1),
//!   which maps those cells for good. RevealArea's house gate
//!   (`0x00567AB0..0x00567B12`) writes that client's map for the owner
//!   itself or, under `AllyReveal=`, an ally.
//! - Last, with or without a bullet, an aircraft with `+0x6CA` is removed
//!   (UnInit, vt+0xF8 = `FootClass::UnInit @ 0x004DE5D0`). Only the kamikaze
//!   tracker's Push sets it (`crate::sim::kamikaze`), on a missile whose
//!   launcher lost it, and no `MissileSpawn=` type in retail rules has a
//!   weapon to fire.
//!
//! The strike states ignore the bullet Fire_At returns.
//!
//! RNG: none of its own (TechnoClass::FireAt's draws are its own). Timers:
//! none. Detach: UnInit's.
//!
//! Evidence: `tools/spatial_oracle/aircraft_fire_at.py` runs the original
//! 0x00415EE0 with the trig, Vector3D and Apparent_Speed math native; its
//! tests replay every row through [`fire_at`], the bullet's velocity bit for
//! bit.
//!
//! RESIDUAL: Apparent_Speed scales the type's speed by the Fly's
//! CurrentSpeed, a double natively whose 0.1 ramp rounds
//! (0.7999999999999999 on its eighth frame) where VERA's SimFixed ramp is
//! exact (`movement::air_movement`). On the ramp's eighth to tenth frames a
//! ROT 0 bullet can leave one lepton a frame faster (24 for 23 at `Speed=`
//! 30). The oracle's speeds are exact fractions; no retail aircraft fires a
//! ROT 0 bullet.

use crate::sim::intern::InternedId;
use crate::sim::projectile::launch::{AircraftBulletCourse, aircraft_bullet_velocity};
use crate::sim::projectile::{ProjectileCoord, ProjectileVelocity};

#[cfg(test)]
#[path = "fire_at_tests.rs"]
mod tests;

/// The Location offsets the shroud test asks first (`0x00416332..0x004164FF`),
/// before the target's coordinate.
const SHROUD_PROBES: [(i32, i32); 5] = [
    (0, 0),
    (0x200, 0x200),
    (-0x200, -0x200),
    (0x200, -0x200),
    (0x200, -0x200),
];

/// The bullet TechnoClass::FireAt answered, as Fire_At reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AircraftShot {
    /// Its type's `ROT=` (`+0xAC`, `+0x2DC`).
    rot: i32,
    /// `+0xE8`.
    velocity: ProjectileVelocity,
}

impl AircraftShot {
    pub(crate) fn new(rot: i32, velocity: ProjectileVelocity) -> Self {
        Self { rot, velocity }
    }
}

/// What Fire_At asks and does, each where the original does it.
pub(crate) trait FireAtHost {
    /// `+0x118`: the aircraft carries a first passenger.
    fn carries_passenger(&mut self) -> bool;
    /// `AircraftClass::Drop_Payload @ 0x00415C60`.
    fn drop_payload(&mut self);
    /// `TechnoClass::FireAt @ 0x006FDD50` with Fire_At's target and weapon.
    fn techno_fire_at(&mut self) -> Option<AircraftShot>;
    /// The Locomotor's (`+0x674`) Apparent_Speed (interface `+0x84`).
    fn apparent_speed(&mut self) -> i32;
    /// SecondaryFacing's (`+0x3A0`) Current (`0x004C93D0`).
    fn facing(&mut self) -> u16;
    /// The aircraft's GetCoords (vt+0x48).
    fn coords(&mut self) -> ProjectileCoord;
    /// The target's GetCoords (vt+0x48).
    fn target_coords(&mut self) -> ProjectileCoord;
    /// GetWeapon(0)'s `Speed=` (vt+0x3F8, `+0xA8`).
    fn weapon0_speed(&mut self) -> i32;
    /// The bullet's new velocity.
    fn set_velocity(&mut self, velocity: ProjectileVelocity);
    /// The clients on which `0x0050B6F0` admits the owner (`+0x21C`), each
    /// asking with its own PlayerPtr.
    fn player_clients(&mut self) -> Vec<InternedId>;
    /// The Location (`+0x9C`).
    fn location(&mut self) -> ProjectileCoord;
    /// `MapClass::IsShrouded @ 0x00586360` on `client`'s map.
    fn is_shrouded(&mut self, client: InternedId, point: ProjectileCoord) -> bool;
    /// `MapClass::RevealArea(Location, AttackingAircraftSightRange, owner, 0,
    /// 0, 0, 1, final)` on `client`'s map.
    fn reveal_area(&mut self, client: InternedId, final_pass: bool);
    /// `+0x6CA`.
    fn destroy_after_firing(&mut self) -> bool;
    /// UnInit (vt+0xF8).
    fn uninit(&mut self);
}

/// `AircraftClass::Fire_At @ 0x00415EE0`: see the module doc.
pub(crate) fn fire_at(host: &mut impl FireAtHost) {
    if host.carries_passenger() {
        host.drop_payload();
        return;
    }
    if let Some(shot) = host.techno_fire_at() {
        let course = match shot.rot {
            0 => Some(AircraftBulletCourse::Level {
                apparent_speed: host.apparent_speed(),
                facing: host.facing(),
            }),
            1 => Some(AircraftBulletCourse::AtTarget {
                from: host.coords(),
                to: host.target_coords(),
                speed: host.weapon0_speed(),
            }),
            _ => None,
        };
        if let Some(course) = course {
            host.set_velocity(aircraft_bullet_velocity(shot.velocity, course));
        }
        for client in host.player_clients() {
            let location = host.location();
            let shrouded = SHROUD_PROBES.iter().any(|&(dx, dy)| {
                host.is_shrouded(
                    client,
                    ProjectileCoord::new(
                        location.x.wrapping_add(dx),
                        location.y.wrapping_add(dy),
                        location.z,
                    ),
                )
            }) || {
                let target = host.target_coords();
                host.is_shrouded(client, target)
            };
            if shrouded {
                host.reveal_area(client, false);
                host.reveal_area(client, true);
            }
        }
    }
    if host.destroy_after_firing() {
        host.uninit();
    }
}
