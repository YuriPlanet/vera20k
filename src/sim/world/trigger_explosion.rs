//! TAction42's direct detonation uses the existing coordinate, animation,
//! flash and synchronous area-damage owners. It does not construct a Bullet.

use super::{FrameEffects, Simulation};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{CombatLightRequest, RAD_NO_ATTACKER, detonation_anim, world_receiver};
use crate::sim::projectile::ProjectileCoord;

impl Simulation {
    /// Original DoExplosionAt6E2390..6E251B, reached by Execute42 at6DECDD.
    /// `weapon_index` is an ordinal in the live WeaponType array88756C;
    /// neither the incoming Trigger House nor an attacker's modifiers apply.
    /// The array's order, Damage and Warhead are native goldens
    /// (tools/spatial_oracle/trigger_do_explosion.{json,meta.json}); the
    /// body's call order and arguments rest on instruction reading.
    ///
    /// The result carries the shared area owner's bridge-change receipt to
    /// the dispatcher. Unavailable safe Rust inputs return None; original
    /// unchecked array/null accesses are outside the comparison coverage.
    pub(crate) fn trigger_explosion_at_waypoint(
        &mut self,
        rules: &RuleSet,
        weapon_index: i32,
        waypoint: i32,
        overlays: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Option<bool> {
        // 6E23A8..6E2426: signed centre, ground, then separate raw bridge
        // flag queries. Share their existing owner, including Dummy writes.
        let [x, y, z] = self.trigger_waypoint_coord(waypoint)?;
        let coordinate = ProjectileCoord::new(x, y, z);
        let weapon = rules.weapon_by_array_index(weapon_index)?;
        let damage = weapon.damage;
        // A null Warhead makes SelectAnim (48A4FF), Flash (48A652) and
        // AreaDamage (4892D7) each return early; 6E2513 still succeeds.
        let Some(warhead) = weapon.warhead.as_deref().and_then(|id| rules.warhead(id)) else {
            return Some(false);
        };

        // 6E244F..6E24B8: raw cell land feeds SelectAnim48A4F0. Construct
        // immediately (2600h/-15), so Anim.Start precedes the outer damage.
        let land = detonation_anim::land_at(self, coordinate);
        if let Some(effect) =
            detonation_anim::effect(self, rules, warhead, damage, land, coordinate, coordinate)
        {
            super::damage_consequences::admit_explosion_effect(self, rules, effect);
        }

        // 6E24BD..6E250B: Flash48A620, then AreaDamage489280. Rules remain
        // immutable during a VERA action; both consume the same selected WH.
        let warhead_ref = self.interner.intern(&warhead.id);
        self.combat_light_requests.emit(CombatLightRequest::Impact {
            target_id: None,
            damage,
            warhead_ref,
            coord: coordinate,
            force_create: false,
            flags: 0,
        });
        Some(world_receiver::apply_area_damage(
            self,
            rules,
            overlays,
            coordinate,
            damage,
            warhead,
            (RAD_NO_ATTACKER, None, warhead_ref),
            frame_effects,
        ))
    }
}
