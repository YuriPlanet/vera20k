//! The death plan (control-point list DMG-S5; RFC 0001 appendix 4.3,
//! `FatalDamage`'s decision form, tier preview): what the receiver's death
//! arm does for one dying object, computed once at its entry and, when a
//! Wasm module decides `FatalDamage`, answered by it.
//!
//! Decision point: the shared Techno death arm of `TechnoClass::ReceiveDamage
//! @ 0x00701900` (`0x00702050`, `world_receiver::handle_death`), before
//! anything it does (Phobos expire weapons, FreeSlaves, the captives, the
//! death sounds), for an object a damage commit killed. Every input is a
//! read of the object and the killing call; no draw happens before it.
//! Deaths that never reach that arm ask nothing: crushing, a temporal
//! erase, a sold or crashing object's own end, passengers dying with their
//! transport (they take the arm only as their own receivers).
//!
//! The answer is the plan's bits; `native` is the engine's own plan:
//!
//! | Bits | Meaning | Native |
//! | --- | --- | --- |
//! | 0 | Explode: passengers die with it (KillPassengers `0x00702603`) and the death weapon may fire | `Explodes=`, the veteran ability or a `Suicide=` current weapon, unless a low dropped bomb over water (`0x00702232`) |
//! | 1 | Death weapon (`Fire_Death_Weapon @ 0x0070D690`), with bit 0 | 1 unless Ares `SuppressDeathWeapon` |
//! | 2 | Debris (`0x00702281..0x0070256C`) | 1 unless a low dropped bomb over water |
//! | 3 | Survivors: a building's `SpawnSurvivors`, a vehicle's passengers escape | 0 when the killing call ignored defenses (NoSurvivor) |
//! | 4 | Crew: a vehicle's crew escapes | 0 when the killing call's sixth argument forbids it |
//! | 5 | The unit-lost announcement | 0 under Ares `UnitLost.Suppress` |
//! | 6 | Tiberium remains (Ares `TiberiumRemains`, hook `0x702216`): Riparius around the cell | Ares' `TiberiumRemains` (`TiberiumHeal=` and `TiberiumHealEnabled` by default); 0 without Ares |
//! | 8..15 | `InfDeath` an infantryman dies by (0..255) | the killing warhead's |
//! | 32..62 | Death weapon ID + 1, fired in place of the type's (`DeathWeapon=`, the current weapon, `[CombatDamage] DeathWeapon=`) at `DeathWeaponDamageModifier`; 0 for the type's | 0 |
//!
//! Other bits must be 0; an answer with one, a negative answer or an
//! unknown weapon keeps the previous result. The engine has no wreck
//! object: bit 6 is the one leftover the arm leaves, and a module may set it
//! for any type and in a game without Ares (the placement draws from the
//! Scenario RNG, as Ares' does). Sinking, crashing, `DeathFrames=`,
//! `Explosion=`/`DestroyAnim=` and the slaves' and captives' release stay
//! native: they are the object's lifecycle, not choices the arm makes.
//! An `InfDeath` the module leaves as native keeps a native value outside
//! 0..255. A plan that drops the debris, the passengers' deaths or the death
//! weapon drops their Scenario draws with them: a different outcome, still
//! deterministic, not a native computation replaced (control-point list
//! 4.4 asks that only of the latter).

use vera20k_wasm_ext::triggers::EVENT_FATAL_DAMAGE;

use super::EntityDamageEvent;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::sim::ext_modules::ModuleEvent;
use crate::sim::world::Simulation;

const EXPLODE: i64 = 1;
const DEATH_WEAPON: i64 = 1 << 1;
const DEBRIS: i64 = 1 << 2;
const SURVIVORS: i64 = 1 << 3;
const CREW: i64 = 1 << 4;
const ANNOUNCE: i64 = 1 << 5;
const TIBERIUM_REMAINS: i64 = 1 << 6;
const INF_DEATH_SHIFT: u32 = 8;
const WEAPON_SHIFT: u32 = 32;
/// Every bit an answer may set.
const KNOWN: i64 = 0x7F | (0xFF << INF_DEATH_SHIFT) | (0x7FFF_FFFF << WEAPON_SHIFT);

/// One death's plan (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DeathPlan {
    pub(super) explode: bool,
    pub(super) death_weapon: bool,
    pub(super) debris: bool,
    pub(super) survivors: bool,
    pub(super) crew: bool,
    pub(super) announce: bool,
    /// Ares' TiberiumRemains (`Simulation::ares_tiberium_remains`).
    pub(super) tiberium_remains: bool,
    pub(super) inf_death: i32,
    /// A module's death weapon (rules weapon index).
    weapon: Option<u32>,
}

impl DeathPlan {
    fn bits(self) -> i64 {
        [
            (self.explode, EXPLODE),
            (self.death_weapon, DEATH_WEAPON),
            (self.debris, DEBRIS),
            (self.survivors, SURVIVORS),
            (self.crew, CREW),
            (self.announce, ANNOUNCE),
            (self.tiberium_remains, TIBERIUM_REMAINS),
        ]
        .into_iter()
        .filter(|(set, _)| *set)
        .fold(0, |bits, (_, bit)| bits | bit)
            | (i64::from(self.inf_death.clamp(0, 255)) << INF_DEATH_SHIFT)
            | (self.weapon.map_or(0, |id| i64::from(id) + 1) << WEAPON_SHIFT)
    }

    /// The plan `bits` answer, over `native`; `None` when they are not a
    /// plan (module docs).
    fn from_bits(bits: i64, native: &Self, rules: &RuleSet) -> Option<Self> {
        if bits < 0 || bits & !KNOWN != 0 {
            return None;
        }
        let weapon = match (bits >> WEAPON_SHIFT) as u32 {
            0 => None,
            id => Some(id - 1).filter(|&id| rules.weapon_at(id).is_some())?.into(),
        };
        let inf_death = ((bits >> INF_DEATH_SHIFT) & 0xFF) as i32;
        Some(Self {
            explode: bits & EXPLODE != 0,
            death_weapon: bits & DEATH_WEAPON != 0,
            debris: bits & DEBRIS != 0,
            survivors: bits & SURVIVORS != 0,
            crew: bits & CREW != 0,
            announce: bits & ANNOUNCE != 0,
            tiberium_remains: bits & TIBERIUM_REMAINS != 0,
            inf_death: if inf_death == native.inf_death.clamp(0, 255) {
                native.inf_death
            } else {
                inf_death
            },
            weapon,
        })
    }

    /// The death weapon a module chose, if any.
    pub(super) fn weapon<'r>(&self, rules: &'r RuleSet) -> Option<&'r WeaponType> {
        rules.weapon_at(self.weapon?)
    }
}

/// `dead_id`'s death plan at the death arm's entry (module docs): the
/// engine's own, or a `FatalDamage` decider's. `None` for an object that is
/// gone.
pub(super) fn death_plan(
    world: &mut Simulation,
    rules: &RuleSet,
    dead_id: u64,
    damage_events: &[EntityDamageEvent],
) -> Option<DeathPlan> {
    let native = native_death_plan(world, rules, dead_id, damage_events)?;
    if !world
        .trigger_runtime
        .module_decides(EVENT_FATAL_DAMAGE)
    {
        return Some(native);
    }
    let killing = damage_events.iter().rfind(|event| event.target_id == dead_id);
    let event = ModuleEvent {
        other: super::world_receiver::killing_attacker(world, damage_events, dead_id).unwrap_or(0),
        value: killing.map_or(-1, |event| i64::from(event.damage)),
        id: killing
            .and_then(|event| rules.warhead_index(world.interner.resolve(event.warhead_ref)))
            .map_or(-1, |index| index as i32),
        at_subject: true,
        ..ModuleEvent::new(EVENT_FATAL_DAMAGE, dead_id)
    };
    let decided = world.decide_module_event(rules, &event, None, None, native.bits(), |bits| {
        DeathPlan::from_bits(bits, &native, rules).map(|_| bits)
    });
    Some(
        decided
            .and_then(|bits| DeathPlan::from_bits(bits, &native, rules))
            .unwrap_or(native),
    )
}

/// The engine's own plan: the reads the death arm and the concrete
/// receivers make of the object and the killing call.
fn native_death_plan(
    world: &Simulation,
    rules: &RuleSet,
    dead_id: u64,
    damage_events: &[EntityDamageEvent],
) -> Option<DeathPlan> {
    let entity = world.substrate.entities.get(dead_id)?;
    let object = world.interner.object_type(entity.type_ref(), rules);
    // Techno702232..70227B: a low dropped bomb over water skips the debris,
    // KillPassengers and the death weapon.
    let low_water_bomb = world.dropped_bomb_water_death_location(dead_id).is_some();
    let explode = !low_water_bomb
        && object.is_some_and(|object| {
            let numbered_weapon = super::combat_weapon::get_weapon(
                entity,
                object,
                super::combat_weapon::attacker_facts(entity, object).current_weapon_number,
                &world.substrate.entities,
                rules,
                &world.interner,
            );
            super::death_arm_explodes(object, entity.veterancy.rank(), numbered_weapon)
        });
    let killing = damage_events.iter().rfind(|event| event.target_id == dead_id);
    let warhead = killing.and_then(|event| world.interner.warhead_type(event.warhead_ref, rules));
    // Ares TechnoClass_ReceiveDamage_SuppressDeathWeapon (hook 0x702669).
    let suppressed = rules.ares()
        && warhead
            .and_then(WarheadType::ares_combat)
            .zip(object)
            .is_some_and(|(combat, object)| {
                combat.suppresses_death_weapon(object.category, &object.id)
            });
    // Ares TechnoClass_ReceiveDamage_SuppressUnitLost (1004B020).
    let unit_lost_suppressed = rules.ares_form()
        && warhead
            .and_then(WarheadType::ares_combat)
            .is_some_and(|combat| combat.unit_lost_suppress());
    let flags = killing.map(|event| event.receiver_flags);
    Some(DeathPlan {
        explode,
        death_weapon: !suppressed,
        debris: !low_water_bomb,
        survivors: !flags.is_some_and(|flags| flags.ignore_defenses),
        crew: !flags.is_some_and(|flags| flags.arg6),
        announce: !unit_lost_suppressed,
        // Ares 3.0 1004B050 (hook 0x702216).
        tiberium_remains: rules.ares() && world.ares_leaves_tiberium_remains(dead_id, rules),
        inf_death: warhead.map_or(WarheadType::DEFAULT_INF_DEATH, |warhead| warhead.inf_death),
        weapon: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> DeathPlan {
        DeathPlan {
            explode: true,
            death_weapon: true,
            debris: false,
            survivors: true,
            crew: false,
            announce: true,
            tiberium_remains: false,
            inf_death: 300,
            weapon: None,
        }
    }

    #[test]
    fn the_tiberium_remains_bit_round_trips() {
        let rules = RuleSet::empty_for_test();
        let native = plan();
        let bits = native.bits();
        assert_eq!(bits & TIBERIUM_REMAINS, 0);
        assert_eq!(DeathPlan::from_bits(bits, &native, rules), Some(native));
        let remains = DeathPlan::from_bits(bits | TIBERIUM_REMAINS, &native, rules)
            .expect("bit 6 is a plan bit");
        assert!(remains.tiberium_remains);
        assert_eq!(
            remains,
            DeathPlan {
                tiberium_remains: true,
                ..native
            },
            "only the remains changed; the out-of-range native InfDeath stays"
        );
        assert_eq!(remains.bits(), bits | TIBERIUM_REMAINS);
        assert_eq!(DeathPlan::from_bits(bits | 1 << 7, &native, rules), None);
    }
}
