//! The launch source retained on a HunterSeeker, independently of later capture.

use crate::sim::intern::InternedId;

/// Ares TechnoExt::SuperWeapon (+0xA0), assigned before Put by
/// SW_HunterSeeker::Activate's callback (installed Ares 0x10072190).
/// House supers retain their identity for the scenario's lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct HunterSeekerOrigin {
    pub owner: InternedId,
    pub sw_type: InternedId,
}

#[cfg(feature = "ares")]
impl crate::sim::world::Simulation {
    /// Fly Acquire_Hunter_Seeker_Target (Ares 0x1004D770), delegated to
    /// TechnoExt::AcquireHunterSeekerTarget 0x10042E40..0x10043171. Installed
    /// 3.0 also excludes zero HP, out-of-playfield, invulnerable/warped targets;
    /// older public Ares source lacks those gates.
    pub(crate) fn acquire_hunter_seeker_target(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> bool {
        use crate::map::entities::EntityCategory;
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::rules::superweapon_targeting::{HouseMask, TargetMask};
        use crate::sim::combat::TargetKind;
        let Some(hunter) = self.substrate.entities.get(id) else {
            return false;
        };
        // Other locomotors' virtual method is a no-op, even when the SW uses
        // them as a helper. Do not turn stationary helper units into seekers.
        if hunter.attack_target.target().is_some()
            || hunter
                .locomotor
                .as_ref()
                .is_none_or(|l| l.active_kind() != LocomotorKind::Fly)
        {
            return false;
        }
        let origin = hunter.hunter_seeker_origin();
        let source = origin.map_or(hunter.owner(), |origin| origin.owner);
        let Some(house) = self.houses.get(&source) else {
            return false;
        };
        let settings = origin
            .and_then(|origin| rules.super_weapon(self.interner.resolve(origin.sw_type)))
            .and_then(|sw| sw.hunter_seeker.as_ref());
        let policy = origin
            .and_then(|origin| rules.super_weapon_metadata(self.interner.resolve(origin.sw_type)))
            .map(|metadata| metadata.targeting());
        let multiplayer = self.session.game_mode_nonzero();
        let human = house.is_controlled_by_human(multiplayer);
        let favourite_only = multiplayer && !human && house.enemy_house().is_some();
        let prefer_players = multiplayer && human && settings.is_none_or(|s| !s.random_only);
        let mut preferred = false;
        let mut candidates = Vec::new();
        // EntityStore preserves TechnoClass::Array's construction order.
        for candidate in self.substrate.entities.values() {
            if if favourite_only {
                Some(candidate.owner()) != house.enemy_house()
            } else {
                !policy
                    .map_or(HouseMask::ENEMIES, |p| p.affects_house())
                    .intersects(self.ares_house_relation(source, candidate.owner()))
            } {
                continue;
            }
            if candidate.health.current() <= 0
                || candidate.lifecycle.in_limbo()
                || !candidate.is_object_alive()
                || !candidate.in_playfield()
                || candidate.temporal.is_warped()
                || crate::sim::superweapon::invulnerability::is_invulnerable(
                    candidate.invulnerability.as_ref(),
                    self.session.binary_frame(),
                )
            {
                continue;
            }
            let Some(ty) = self.interner.object_type(candidate.type_ref(), rules) else {
                continue;
            };
            if ty.invisible
                || !ty.legal_target
                || ty.hunter_seeker_ignored()
                || (self.session.harvester_immune()
                    && candidate.category() == EntityCategory::Unit
                    && ty.harvester)
            {
                continue;
            }
            if let Some(policy) = policy {
                let mask = policy.affects_target();
                if mask.intersects(TargetMask::ALL_CELLS) {
                    let water = self
                        .resolved_terrain
                        .as_ref()
                        .and_then(|terrain| {
                            terrain.cell(candidate.position.rx(), candidate.position.ry())
                        })
                        .is_some_and(|cell| {
                            cell.land_type
                                == crate::rules::terrain_rules::LandType::Water.as_index()
                        });
                    if !mask.intersects(if water {
                        TargetMask::WATER
                    } else {
                        TargetMask::LAND
                    }) {
                        continue;
                    }
                }
                let category = match candidate.category() {
                    EntityCategory::Infantry => TargetMask::INFANTRY,
                    EntityCategory::Structure => TargetMask::BUILDING,
                    EntityCategory::Unit | EntityCategory::Aircraft => TargetMask::UNIT,
                };
                if mask.intersects(TargetMask::ALL_CONTENTS) && !mask.intersects(category) {
                    continue;
                }
            }
            let player = prefer_players
                && self
                    .houses
                    .get(&candidate.owner())
                    .is_some_and(|h| !h.multiplay_passive());
            if player && !preferred {
                candidates.clear();
                preferred = true;
            }
            if player || !preferred {
                candidates.push(candidate.stable_id());
            }
        }
        if candidates.is_empty() {
            return false;
        }
        // One inclusive Scenario RandomRanged (65C7E0); a one-element pool
        // returns at min == max (je 65C889) without drawing.
        let picked = self
            .scenario_rng
            .next_range_i32_inclusive(0, candidates.len() as i32 - 1);
        self.assign_target_represented(
            id,
            Some(TargetKind::Entity(candidates[picked as usize])),
            rules,
            registry,
            effects,
        )
        .expect("live Fly target setter");
        true
    }
}
