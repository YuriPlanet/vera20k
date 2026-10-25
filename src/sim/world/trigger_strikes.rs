//! TAction95/96's direct missiles. Projectile construction, flight, impact
//! and retirement stay with the existing combat owners.

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::{CellCoordinate, cell_center};
use crate::sim::combat::{FiredBullet, RAD_NO_ATTACKER, admit_fired_bullet};
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::query_ground_height;
use crate::sim::projectile::{ProjectileCoord, ProjectilePayload, ProjectileTarget};
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::native_x87::NativeF64Bits;

impl Simulation {
    /// Execute96 -> ChemMissileAtWP6E38F0..6E3B13, Steam gamemd.
    /// Literal ChemLauncher (773030 only finds an already allocated type),
    /// null firer, speed20/Bright=false, ground+structural bridge target.
    /// Map586AC0's non-inset LocalSize edge supplies launch XY, Z=0.
    /// Unlike 95, this caller never assigns Bullet+130's Weapon pointer.
    /// Fire's return is ignored; failed Bullet construction returns false.
    /// Missing safe prerequisites return false instead of native unchecked
    /// dereferences. All flight/RNG/damage/timer/cleanup use combat owners.
    pub(crate) fn trigger_chemical_strike(&mut self, rules: &RuleSet, waypoint: i32) -> bool {
        let Some(cell) = self.session.waypoint_cell(waypoint) else {
            return false;
        };
        let Some(location) = self.direct_strike_coord(cell) else {
            return false;
        };
        let Some(bounds) = self.playfield_bounds else {
            return false;
        };
        let edge = crate::map::playfield::playfield_edge_cell(
            bounds,
            ((location.x / 256) as i16, (location.y / 256) as i16),
            false,
        );
        let Some(weapon) = rules.weapon("ChemLauncher") else {
            return false;
        };
        let Some(warhead) = weapon.warhead.as_deref().and_then(|id| rules.warhead(id)) else {
            return false;
        };
        let Some(projectile) = weapon
            .projectile
            .as_deref()
            .and_then(|id| rules.projectile(id))
        else {
            return false;
        };
        let target = self.direct_strike_target(location);
        let payload = ProjectilePayload::new(
            weapon.damage,
            self.interner.intern(&warhead.id),
            Some(self.interner.intern(&projectile.id)),
            false,
        );
        let origin = ProjectileCoord::new(
            i32::from(edge.0) * 256 + 128,
            i32::from(edge.1) * 256 + 128,
            0,
        );
        // Fixed native expressions6E3A18..6E3AC5: pitch BF19222D989F5E57,
        // yaw3FF921C90FE8FBDA, scale100 at7E2AC0. Executed using the
        // existing nuke_strike_dispatch_case/Emu transport with Execute96,
        // real586AC0/46B050/4CACB0/4CAD00; nine controls give these double
        // stores. Keep the tiny X rather than rounding it to (0,100,0).
        let velocity = crate::sim::projectile::ProjectileVelocity::from_native([
            NativeF64Bits::from_bits(0x3d0b_9360_0000_0000),
            NativeF64Bits::from_bits(0x4059_0000_0000_0000),
            NativeF64Bits::from_bits(0),
        ]);
        let stable_id = self.substrate.allocate_stable_id();
        let native_id = self.next_native_runtime_id();
        if let Some(bullet) = FiredBullet::launched(
            self,
            rules,
            native_id,
            RAD_NO_ATTACKER,
            target,
            payload,
            20,
            origin,
            velocity,
        ) {
            admit_fired_bullet(self, rules, weapon, stable_id, bullet);
        }
        true
    }

    /// Steam gamemd, Execute95 at6DD9DA ->6E3410..6E35E2.
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
        self.launch_nuke_payload_at_cell(rules, cell, Some(50), None)
            .is_some()
    }

    /// Shared descending NukePayload construction for Execute95 6E3410 and
    /// one-time Super Launch6CDA88..6CDCB3. The former overrides speed50;
    /// the latter uses Weapon.Speed and constructs a House-owned PSIWARN
    /// attached to the new bullet before Fire. Both use null bullet owner,
    /// Bright=false, structural bridge100, height20000 and scale-100.
    /// Returns the strike coordinate on a launch: the one-time Super's
    /// DigSound plays there (`0x006CDCAE`). RESIDUAL: when the one-time
    /// Super's bullet creation returns null (`0x006CDB93`), native skips the
    /// PSIWARN, Fire, EVA line and DigSound; VERA still plays them. Only a
    /// failed bullet creation reaches it.
    pub(crate) fn launch_nuke_payload_at_cell(
        &mut self,
        rules: &RuleSet,
        cell: (i16, i16),
        speed_override: Option<i32>,
        warning_house: Option<InternedId>,
    ) -> Option<DriveCoord> {
        let location = self.direct_strike_coord(cell)?;
        let weapon = rules.weapon("NukePayload")?;
        let warhead = weapon.warhead.as_deref().and_then(|id| rules.warhead(id))?;
        // 6E34D3's Map565730 looks up world XY again. Preserve truncation
        // and the retained Dummy rather than assuming the packed waypoint
        // is always the CellClass that becomes the Bullet's target.
        let target = self.direct_strike_target(location);
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
        let stable_id = self.substrate.allocate_stable_id();
        let native_id = self.next_native_runtime_id();
        if let Some(owner) = warning_house {
            //43B5E0 uses the target Cell's GetCoords, not this producer's
            //raised bridge coordinate. The House belongs to the warning;
            //the bullet keeps its original null source/firer House.
            let coord = match target {
                ProjectileTarget::Cell { rx, ry } => crate::sim::projectile::cell_ground_coord(
                    self.resolved_terrain.as_ref(),
                    rx,
                    ry,
                ),
                ProjectileTarget::DummyCell => crate::sim::projectile::dummy_cell_ground_coord(
                    &self.effective_shared_cell_dummy(),
                ),
                _ => unreachable!("direct nuke target is a Cell"),
            };
            self.spawn_bullet_warning(
                rules,
                crate::rules::effect_asset_catalog::PSI_WARNING_ANIM,
                crate::sim::anim_class::AnimWorldCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                },
                stable_id,
                owner,
            );
        }
        let origin = ProjectileCoord::new(location.x, location.y, location.z.wrapping_add(20000));
        // Double7EDA88 is -100. The shared table/pitch kernel also serves
        // the silo and NukeMaker, whose callers pass different scales.
        let velocity = crate::sim::projectile::launch::missile_launch_velocity(
            NativeF64Bits::from_bits(0xc059_0000_0000_0000),
        );
        if let Some(bullet) = FiredBullet::launched(
            self,
            rules,
            native_id,
            RAD_NO_ATTACKER,
            target,
            payload,
            speed_override.unwrap_or(weapon.speed),
            origin,
            velocity,
        ) {
            admit_fired_bullet(self, rules, weapon, stable_id, bullet);
        }
        // 6E35D1 returns true independently of Fire's Boolean result.
        Some(location)
    }

    /// Shared coordinate setup in 95's6E3446..6E3495 and 96's6E3917..6E3975.
    /// Ground578080 and packed lookup5657A0 remain separate, in that order.
    /// These callers test structural bit100 alone (camera uses100/400).
    fn direct_strike_coord(&self, cell: (i16, i16)) -> Option<DriveCoord> {
        let cells = NativeCellQuery::canonical(self.resolved_terrain.as_ref()?);
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
            .expect("direct strike ground query requires a supported native slope");
        if cells.flags(cells.lookup(cell)) & 0x100 != 0 {
            location.z = location.z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
        }
        Some(location)
    }

    /// Both callers re-query Map565730 with the world XY after Weapon lookup;
    /// signed division, aliases and retained Dummy belong to NativeCellQuery.
    fn direct_strike_target(&self, location: DriveCoord) -> ProjectileTarget {
        let cells =
            NativeCellQuery::canonical(self.resolved_terrain.as_ref().expect("strike terrain"));
        match cells.lookup_world(location.x, location.y) {
            NativeCellIdentity::Real(index) => {
                let (x, y) = cells.coord(NativeCellIdentity::Real(index));
                ProjectileTarget::Cell {
                    rx: x as u16,
                    ry: y as u16,
                }
            }
            NativeCellIdentity::Dummy => ProjectileTarget::DummyCell,
        }
    }
}
