//! TAction95's direct descending missile. Projectile construction, flight,
//! impact and retirement stay with the existing combat owners.

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::{CellCoordinate, cell_center};
use crate::sim::combat::{FiredBullet, RAD_NO_ATTACKER, admit_fired_bullet};
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose::query_ground_height;
use crate::sim::projectile::{ProjectileCoord, ProjectilePayload, ProjectileTarget};
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::native_x87::NativeF64Bits;

impl Simulation {
    /// Steam gamemd 3e81a617..08600, Execute95 at6DD9DA ->6E3410..6E35E2.
    /// Uses the literal NukePayload, not a Super or the numeric action value.
    /// Constructor speed50/Bright=false, height+20000 and velocity scale-100
    /// are independent of Weapon.Speed and BulletType.DetonationAltitude.
    /// Native execution and limits: trigger_nuke_strike.{json,meta.json} in
    /// tools/spatial_oracle/ (producer: trigger_action_values --nuke-strike).
    ///
    /// No separate timer, RNG or detach owner: construction/Fire admission,
    /// Bullet AI, NUKEBALL wait and detonation use the existing owners.
    /// Missing safe inputs return false; original unchecked accesses on
    /// malformed rules or missing waypoints are outside the comparison.
    pub(crate) fn trigger_nuke_strike(&mut self, rules: &RuleSet, waypoint: i32) -> bool {
        let Some(cell) = self.session.waypoint_cell(waypoint) else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let centre = cell_center(
            CellCoordinate {
                x: i32::from(cell.0),
                y: i32::from(cell.1),
            },
            0,
        );
        let mut location = DriveCoord {
            x: centre.x,
            y: centre.y,
            z: 0,
        };
        location.z = query_ground_height(&cells, location)
            .expect("NukeStrike ground query requires a supported native slope");
        // 6E3479..6E3495 tests only structural bit100; camera/Action42's
        // bit100/400 bridge admission is a different native call site.
        if cells.flags(cells.lookup(cell)) & 0x100 != 0 {
            location.z = location.z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
        }
        let Some(weapon) = rules.weapon("NukePayload") else {
            return false;
        };
        let Some(warhead) = weapon.warhead.as_deref().and_then(|id| rules.warhead(id)) else {
            return false;
        };
        // 6E34D3's Map565730 looks up world XY again. Preserve truncation
        // and the retained Dummy rather than assuming the packed waypoint
        // is always the CellClass that becomes the Bullet's target.
        let target = match cells.lookup_world(location.x, location.y) {
            NativeCellIdentity::Real(index) => {
                let (x, y) = cells.coord(NativeCellIdentity::Real(index));
                ProjectileTarget::Cell {
                    rx: x as u16,
                    ry: y as u16,
                }
            }
            NativeCellIdentity::Dummy => ProjectileTarget::DummyCell,
        };
        let payload = ProjectilePayload::new(
            weapon.damage,
            self.interner.intern(&warhead.id),
            weapon
                .projectile
                .as_deref()
                .map(|id| self.interner.intern(id)),
            false,
        )
        .with_installed_weapon(Some(self.interner.intern(&weapon.id)));
        let stable_id = self.allocate_stable_id();
        let native_id = self.next_native_runtime_id();
        let origin = ProjectileCoord::new(location.x, location.y, location.z.wrapping_add(20000));
        // Double7EDA88 is -100. The shared table/pitch kernel also serves
        // the silo and NukeMaker, whose callers pass different scales.
        let velocity = crate::sim::projectile::launch::missile_launch_velocity(
            NativeF64Bits::from_bits(0xc059_0000_0000_0000),
        );
        if let Some(bullet) = FiredBullet::launched(
            self,
            native_id,
            RAD_NO_ATTACKER,
            target,
            payload,
            50,
            origin,
            velocity,
        ) {
            admit_fired_bullet(self, rules, weapon, stable_id, bullet);
        }
        // 6E35D1 returns true independently of Fire's Boolean result.
        true
    }
}
