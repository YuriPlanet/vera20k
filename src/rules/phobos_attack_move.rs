//! Phobos `AttackMove.*` (`Ext/TechnoType/Body.cpp:1086-1087`,
//! `Ext/Rules/Body.cpp:377-378,423`).
//!
//! Two per-type keys refine the native attack-move (MegaMission 29):
//! - `.Aggressive` lets an attack-mover treat an enemy building with no
//!   weapon and no `ThreatPosed=` as a legal auto-target;
//! - `.UpdateTarget` lets it swap a held target it cannot use for the
//!   greatest threat within range.
//!
//! Both are `Nullable<bool>` and fall back to the same-named `[General]`
//! flag, which is `Valueable<bool>` and therefore has no unset state
//! (`Ext/Rules/Body.cpp:377-378`, constructor `Ext/Rules/Body.h:800-801`).
//!
//! Three more per-type keys make an attack-mover follow a nearby friendly
//! attack-mover instead of heading for its own destination
//! (`Ext/TechnoType/Body.cpp:1149-1151`, fields `Ext/TechnoType/Body.h:383-385`,
//! constructed `false` at `:805-807`): `.Follow=`, `.Follow.IncludeAir=` and
//! `.Follow.IfMindControlIsFull=`. They are `Valueable<bool>` with no
//! `[General]` counterpart, so an absent key is a plain `false`.
//!
//! The third key of the family, `[General] AttackMove.IgnoreWeaponCheck`
//! (`Ext/Rules/Body.cpp:423`, constructor `Ext/Rules/Body.h:853`), is global
//! only: it drops the native "must own a `Primary=`" test from
//! `TechnoTypeClass::Can_Attack_Move @ 0x00711E90`. It lives on
//! [`crate::rules::phobos_globals::PhobosGlobals`] and is consumed by
//! [`crate::rules::object_type::ObjectType::can_attack_move_with`].
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_globals::PhobosGlobals;

/// `TechnoTypeExt::AttackMove_Aggressive` / `AttackMove_UpdateTarget`
/// (`Ext/TechnoType/Body.h`, constructed unset at `Body.cpp:1086-1087`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PhobosAttackMove {
    /// `AttackMove.Aggressive=`: unarmed enemy buildings become legal
    /// auto-targets while this object is on an attack move. `None` inherits
    /// `[General] AttackMove.Aggressive` (default `false`).
    pub aggressive: Option<bool>,
    /// `AttackMove.UpdateTarget=`: the arrival handler may drop a held target
    /// it cannot use and rescan. `None` inherits
    /// `[General] AttackMove.UpdateTarget` (default `false`).
    pub update_target: Option<bool>,
    /// `AttackMove.Follow=`: on an attack move, take a nearby friendly
    /// attack-mover as the destination and area-guard it instead of walking
    /// to this object's own MegaDestination (`Ext/Techno/Hooks.cpp:1297`).
    pub follow: bool,
    /// `AttackMove.Follow.IncludeAir=`: the follower search also considers
    /// airborne Foot objects (`Ext/Techno/Hooks.cpp:1300`).
    pub follow_include_air: bool,
    /// `AttackMove.Follow.IfMindControlIsFull=`: follow only while this
    /// object has a capture manager with no free control slot
    /// (`Ext/Techno/Hooks.cpp:1297`).
    pub follow_if_mind_control_is_full: bool,
    /// `ApproachTarget.StopWhenInRange=` (`Ext/TechnoType/Body.cpp:1093-1094`,
    /// `Body.h:316`, constructed unset `:750`): the deprecated
    /// `AttackMove.StopWhenTargetAcquired` is read first and the new key
    /// overwrites it. `None` inherits `[General] ApproachTarget.StopWhenInRange`
    /// (`Ext/Rules/Body.cpp:642-643`, constructor `Ext/Rules/Body.h:1036`
    /// false). A close-enough foot that is really moving stops both its
    /// locomotor and its motion (`Ext/Techno/Hooks.Firing.cpp:1311-1340`).
    pub approach_target_stop_when_in_range: Option<bool>,
    /// `ApproachTarget.PursuitTarget=` (`Ext/TechnoType/Body.cpp:1100-1101`,
    /// `Body.h:317`, constructed false `:751`): the deprecated
    /// `AttackMove.PursuitTarget` is read first and the new key overwrites it.
    /// On the Foot approach, a pursue type sets its Target as the destination
    /// and skips the nav recalc and the existing-path/close-enough gate
    /// (`Ext/Techno/Hooks.Firing.cpp:1343-1362`).
    pub approach_target_pursuit_target: bool,
}

impl PhobosAttackMove {
    /// The two `Nullable<bool>` type keys. `None` stays "unset" so the
    /// consumer can see the `[General]` fallback.
    pub fn from_section(section: &IniSection) -> Self {
        let mut approach_target_stop_when_in_range =
            section.read_phobos_nullable_bool("AttackMove.StopWhenTargetAcquired", None);
        approach_target_stop_when_in_range = section.read_phobos_nullable_bool(
            "ApproachTarget.StopWhenInRange",
            approach_target_stop_when_in_range,
        );
        let mut approach_target_pursuit_target =
            section.read_bool("AttackMove.PursuitTarget", false);
        approach_target_pursuit_target = section.read_bool(
            "ApproachTarget.PursuitTarget",
            approach_target_pursuit_target,
        );
        Self {
            aggressive: section.read_phobos_nullable_bool("AttackMove.Aggressive", None),
            update_target: section.read_phobos_nullable_bool("AttackMove.UpdateTarget", None),
            follow: section.read_bool("AttackMove.Follow", false),
            follow_include_air: section.read_bool("AttackMove.Follow.IncludeAir", false),
            follow_if_mind_control_is_full: section
                .read_bool("AttackMove.Follow.IfMindControlIsFull", false),
            approach_target_stop_when_in_range,
            approach_target_pursuit_target,
        }
    }

    /// `AttackMove_Aggressive.Get(RulesExt::Global()->AttackMove_Aggressive)`
    /// (`Ext/Techno/Hooks.TargetEvaluation.cpp:214`).
    pub fn aggressive(&self, globals: &PhobosGlobals) -> bool {
        self.aggressive.unwrap_or(globals.attack_move_aggressive)
    }

    /// `AttackMove_UpdateTarget.Get(RulesExt::Global()->AttackMove_UpdateTarget)`
    /// (`Ext/Techno/Hooks.TargetEvaluation.cpp:185-186`).
    pub fn update_target(&self, globals: &PhobosGlobals) -> bool {
        self.update_target
            .unwrap_or(globals.attack_move_update_target)
    }

    /// `ApproachTarget_StopWhenInRange.Get(global)` (`Ext/Techno/Hooks.Firing.cpp:1326-1329`).
    pub fn stop_when_in_range(&self, globals: &PhobosGlobals) -> bool {
        self.approach_target_stop_when_in_range
            .unwrap_or(globals.approach_target_stop_when_in_range)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_attack_move_tests.rs"]
mod tests;
