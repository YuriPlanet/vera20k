//! Ares 3.0 BulletClass_Detonate (hook 0x46920B, installed 100536E0): the
//! WarheadTypeExt effects every detonation applies at its local effect
//! coordinate ([`super::world_receiver::ares_warhead_effect_coordinate`]),
//! after radiation and before the vanilla special arms. The installed order
//! is applyRipples, the abduction of a snapped bullet, applyIronCurtain
//! (here), applyEMP and applyAttachedEffect (`game_entity::{emp,
//! attach_effect}`), the occupant damage of a snapped bullet, then
//! applyPermaMC (here); a permanent capture skips the vanilla arms and the
//! area damage (0x469AA4) but not the anim tail.
//!
//! Ledger: applyIronCurtain draws nothing itself; the receivers it calls
//! (the pre-curtain ReceiveDamage and the vt+0x154 overrides) keep their own
//! draws, timer writes and detach calls. It writes each victim's curtain
//! timer (`TechnoClass+0x18C`) and, on a refresh, IronTintStage (`+0x1A4`).
//! applyPermaMC's capture writes the owner, `+0x2C4`, the queued mission and
//! the ring (`+0x2C8`); FreeUnit's own effects stay with the capture manager.
//!
//! The victims come from the shared getCellSpreadItems port
//! (`ares_cell_spread_items`, Ares.dll 10076660) with includeInAir, as for
//! EMP and AttachEffect: the cell lists plus the airborne FootClass objects,
//! a flying aircraft at half its distance.

use crate::rules::{
    overlay_types::OverlayTypeRegistry, ruleset::RuleSet, warhead_type::WarheadType,
};
use crate::sim::combat::world_receiver::{ReceiverRun, commit_entities};
use crate::sim::combat::{DeathEffects, EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::superweapon::invulnerability::{
    InvulnKind, ares_curtain_duration, ares_curtain_time_left, ares_write_curtain_timer,
};
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// WarheadTypeExt::applyIronCurtain (installed 10052890) with a nonzero
    /// `IronCurtain.Duration`, for each Techno getCellSpreadItems collects
    /// in UniqueID order: a live one (not in limbo, IsAlive, Health not 0)
    /// the warhead's CanAffectHouse (inlined; no source house admits) and
    /// Verses (`|v| >= 0.001`) admit. A positive duration first scales by
    /// the type's `IronCurtain.Modifier`; with the curtain's time left it
    /// goes through getCappedDuration with `IronCurtain.Cap`. Off a curtain,
    /// a positive result first deals the bullet weapon's Damage (nonzero) as
    /// ReceiveDamage(ignoreDefenses, no attacker, the source house); a
    /// survivor then gets IronCurtain (vt+0x154, the source house, force 0)
    /// and its timer restarts at the result, undoing the type modifier the
    /// Techno curtain applied again. On a curtain, a positive result
    /// restarts the timer and IronTintStage 4; anything else stops it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_ares_iron_curtain_detonation(
        &mut self,
        run: &mut ReceiverRun<'_>,
        coordinate: ProjectileCoord,
        source_house: Option<InternedId>,
        warhead: &WarheadType,
        weapon_damage: i32,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: &mut DeathEffects,
    ) {
        let Some(extension) = warhead.ares_rules() else {
            return;
        };
        let (duration, cap) = extension.combat().iron_curtain();
        if duration == 0 {
            return;
        }
        let warhead_ref = self.interner.intern(&warhead.id);
        for target in self.ares_cell_spread_items(coordinate, warhead.cell_spread_f64, rules) {
            let Some(victim) = self.substrate.entities.get(target) else {
                continue;
            };
            if victim.lifecycle.in_limbo()
                || !victim.lifecycle.object_alive()
                || victim.health.current() == 0
            {
                continue;
            }
            let allied = source_house.is_some_and(|source| {
                crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(&self.house_alliances),
                    &self.interner,
                    source,
                    victim.owner(),
                )
            });
            if !extension.can_affect_house(
                source_house.is_some(),
                source_house == Some(victim.owner()),
                allied,
                warhead.affects_allies,
            ) {
                continue;
            }
            let Some(object) = self.interner.object_type(victim.type_ref(), rules) else {
                continue;
            };
            let verses = f64::from_bits(warhead.interaction(object.armor_id()).multiplier().bits());
            if verses.abs() < 0.001 {
                continue;
            }
            let duration = if duration > 0 {
                ares_curtain_duration(Some(object), duration, InvulnKind::IronCurtain)
            } else {
                duration
            };
            let frame = self.session.binary_frame();
            let old = ares_curtain_time_left(victim, frame);
            let new = crate::sim::timer::ares_capped_duration(old, duration, cap);
            if old > 0 {
                if let Some(victim) = self.substrate.entities.get_mut(target) {
                    ares_write_curtain_timer(victim, frame, (new > 0).then_some(new), new > 0);
                }
                continue;
            }
            if new <= 0 {
                continue;
            }
            if weapon_damage != 0 {
                let event = EntityDamageEvent::direct_receiver(
                    target,
                    weapon_damage,
                    0,
                    RAD_NO_ATTACKER,
                    source_house,
                    warhead_ref,
                    ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: false,
                    },
                );
                let (death, mut pings) =
                    commit_entities(self, run, &[event], None, rules, overlay_registry);
                effects.append(death);
                effects.under_attack_events.append(&mut pings);
            }
            let Some(category) = self
                .substrate
                .entities
                .get(target)
                .filter(|victim| victim.lifecycle.object_alive())
                .map(|victim| victim.category())
            else {
                continue;
            };
            crate::sim::superweapon::iron_curtain::curtain_object(
                self,
                rules,
                overlay_registry,
                target,
                category,
                source_house,
                new,
                run.effects(),
            );
            if let Some(victim) = self.substrate.entities.get_mut(target) {
                ares_write_curtain_timer(victim, self.session.binary_frame(), Some(new), false);
            }
        }
    }

    /// WarheadTypeExt::applyPermaMC (installed 10052CD0), last in 100536E0:
    /// with `MindControl.Permanent=yes`, a source house and a Techno bullet
    /// target, TechnoExt::ApplyPermaMC ([`Self::ares_apply_perma_mc`]).
    /// True skips the vanilla detonation arms (0x469AA4).
    pub(crate) fn apply_ares_perma_mc_detonation(
        &mut self,
        target: Option<u64>,
        source_house: Option<InternedId>,
        warhead: &WarheadType,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if !warhead
            .ares_combat()
            .is_some_and(|combat| combat.mind_control_permanent())
        {
            return false;
        }
        let (Some(house), Some(target)) = (source_house, target) else {
            return false;
        };
        // generic_cast<TechnoClass*>: every entity is a Techno; a terrain
        // or cell target never reaches here.
        if !self.substrate.entities.contains(target) {
            return false;
        }
        self.ares_apply_perma_mc(target, house, rules, overlay_registry, effects)
    }
}
