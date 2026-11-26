//! Ordered receiver immunity gates. Pure over ImmunityInputs. Returns the gate
//! decision; the caller short-circuits on Nullified/MindControlled.
//!
//! Order verified against TechnoClass::ReceiveDamage (0x00701900): the armor
//! divides + min-1 run FIRST (in the caller), then these gates in the order
//! below. TypeImmune -> IronCurtain/ForceShield -> WarpingOut -> Bunker -> Radiation ->
//! Psychic -> Poison -> AffectsAllies -> Psychedelic (each short-circuits).

use super::{DamageGate, ImmunityInputs};

/// Evaluate the receiver immunity gates in gamemd's verified order. Each gate
/// short-circuits. Runs AFTER the armor divides + min-1 (see the receiver
/// pipeline), matching the binary's gate placement.
pub(crate) fn evaluate_early_gates(g: &ImmunityInputs) -> Option<DamageGate> {
    // 1. TypeImmune: attacker present + same WhatAmI + same owner.
    if g.attacker_present && g.type_immune {
        return Some(DamageGate::Nullified {
            clear_damage: false,
        });
    }
    // 2. Active IronCurtain/ForceShield. This precedes WarpingOut and has a
    // distinct combat-light side effect at the receiver callsite.
    if g.invulnerable {
        return Some(DamageGate::Invulnerable);
    }
    // 3. WarpingOut silently nullifies only when invulnerability did not win.
    if g.warping_out {
        return Some(DamageGate::Nullified { clear_damage: true });
    }
    None
}

/// ReceiveDamage701B67 resumes here after the readiness count/clock write.
pub(crate) fn evaluate_late_gates(g: &ImmunityInputs) -> DamageGate {
    // 4. Bunker/garrison-link block (target bunkered AND warhead lacks
    //    PenetratesBunker). NOT a wall check.
    if g.bunker_blocked {
        return DamageGate::Nullified { clear_damage: true };
    }
    // 5. Radiation immune.
    if g.radiation_immune {
        return DamageGate::Nullified { clear_damage: true };
    }
    // 6. PsychicDamage immune.
    if g.psychic_immune {
        return DamageGate::Nullified { clear_damage: true };
    }
    // 7. Poison immune.
    if g.poison_immune {
        return DamageGate::Nullified { clear_damage: true };
    }
    // 8. !AffectsAllies && attacker present && allied (AffectsAllies default true).
    if !g
        .ares_house_damage_allowed
        .unwrap_or(g.affects_allies || !g.attacker_present || !g.attacker_is_allied)
    {
        return DamageGate::Nullified { clear_damage: true };
    }
    // 9. Psychedelic/MindControl: allied -> 0; psionics-immune -> 0; building ->
    //    0; else MindControlled (0 HP, return-code-1 marker).
    if g.psychedelic {
        if g.source_house_is_allied || g.psionics_immune || g.target_is_building {
            return DamageGate::Nullified {
                clear_damage: false,
            };
        }
        return DamageGate::MindControlled;
    }
    DamageGate::Pass
}
