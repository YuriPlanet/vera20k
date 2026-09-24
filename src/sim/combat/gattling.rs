//! Gattling weapon stages: how the Gattling Tank (`[YTNK]`) spins up while it
//! fights and winds down when it stops.
//!
//! Native state, on every TechnoClass: `+0x140` CurrentGattlingStage, `+0x144`
//! GattlingValue and `+0x4B8`, the report latch. The constructor zeroes all
//! three (`0x006F2BD0`, `0x006F2BD6`, `0x006F30E2`). The bodies are
//! `TechnoClass::IncreaseGattlingStage @ 0x0070DE70` and
//! `TechnoClass::UpdateGattlingStage @ 0x0070E000`; the accessors `SetStage
//! @ 0x0070DDD0`, `SetValue @ 0x0070DE00` and `DecreaseValue @ 0x0070DE40`.
//! The tables are the type's (`rules::gattling_type`). The stage picks the
//! weapon pair in `What_Weapon_Should_I_Use @ 0x006F3330` arm H
//! (`combat_weapon`): `2s`, or `2s + 1` at a high flier when slot 1 is AA; the
//! reload follows the picked weapon's ROF.
//!
//! A charge adds `RateUp * ticks` while the value is under the cap (the last
//! stage's threshold), then steps the stage up once when the value it started
//! from had reached the next stage's threshold. A decay subtracts
//! `RateDown * ticks` (zero when the result is negative or the step is zero)
//! and steps down once when the value fell under the current stage's
//! threshold.
//!
//! The report: while the latch is clear, a charge plays the stage ground
//! weapon's `Report=` loop on the techno's `+0x4A4` handle, one `g_MainRng`
//! draw picking the item (`0x0070DFC1`), and sets the latch. A stage-up
//! hard-stops the loop (`VocHandle::StopAndClear @ 0x00405D40`) and clears the
//! latch, so the next loop starts in the same call; every decay releases it
//! (`SoundEvent::Release @ 0x00406060`, the loop plays out its `decay` tail)
//! and clears the latch. The draw is simulation state whatever the sound
//! settings: it precedes `VocClass::PlayAt`'s sound-enabled test. A gattling
//! type plays no per-shot report (`TechnoClass::Fire`, `0x006FF349`).
//!
//! The dead latch `+0x4D4` is only ever written 0 in YR (`0x006F30EE`,
//! `0x006F6C87`, `0x0070C225`, `0x0070DF0E`, `0x0070E08B`, `0x0070E10B`), so the
//! arm of UpdateGattlingStage that reads it can only take its quiet exit; it
//! is not modelled. The second handle the bodies stop (`+0x4C0`, at
//! `0x0070DF07` and on a stage-down) is not modelled either: nothing in these
//! bodies starts a sound on it.
//!
//! Callers ported here:
//! - the unit's per-frame firing update (`0x00736DF0`, called from
//!   `UnitClass::AI @ 0x007365E1` every frame its AI reaches that point):
//!   [`unit_fire_tail`];
//! - TemporalClass::InitiateWarp's decay of its victim (`0x0071B10B`, one
//!   tick, gattling types only);
//! - Unit and Infantry PerCellProcess's entry reset (`0x0073A6FC..0x0073A70F`,
//!   `0x0051A40E..0x0051A41C`): `+0xC4 = 0`, `SetValue(0)`, `SetStage(0)`;
//! - TechnoClass::Limbo's latch clear and release (`0x006F6C6B`, `0x006F6C76`).
//!
//! RESIDUAL: the Gattling Cannon (`[YAGGUN]`) does not spin. Its charge and
//! decay run inside `BuildingClass::Mission_Attack @ 0x0044ACF0` (by the
//! mission's `+0xC4` tick count), the head of `BuildingClass::Mission_Guard
//! @ 0x004496DA` and `BuildingClass::Update`'s idle decay
//! (`0x0043FEE9..0x0043FF67`); VERA has none of those building mission
//! handlers (see `world::techno_ai`'s note on the building Guard->Attack flip).
//! Trigger: every Gattling Cannon. Effect: it keeps its stage-0 pair
//! (AGGattling/AAGattCann) and, in place of the stage loop, a per-shot report
//! (the no-report gate is held back for buildings in `emit_admitted_fire`, or
//! the cannon would fire silently). Frequency: every Yuri base. The building
//! attack mission is the next mechanism.
//!
//! RESIDUAL: the unit update's vt+0x4E4 return (`0x00736D50`: codes 0 and 2
//! queue Unload and return before the tail) is not ported. It answers true
//! only for `DeployToFire=` types and for computer-owned units whose
//! `DeploysInto=` building sets `+0x16C4`; no retail gattling type is either.
//!
//! Evidence: `tools/spatial_oracle/gattling_stage.py` runs the original
//! bodies and accessors (68 histories, 3,586 calls); `gattling_unit_fire.py`
//! the unit's firing update `0x00736DF0` with its leaf calls stubbed (not
//! `UnitClass::AI`). `gattling_tests` replays both (54 of the 64 update rows;
//! the rest are unrepresentable codes and the vt+0x4E4 return).

use crate::rules::gattling_type::GattlingStages;
use crate::sim::combat::fire_error::FireError;

/// `TechnoClass+0x140` stage, `+0x144` value, `+0x4B8` report latch.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct GattlingState {
    stage: i32,
    value: i32,
    /// Not saved: `TechnoClass::Load` clears it (`0x0070C20E`) and
    /// reinitialises the handle, so the first charge after a load starts the
    /// loop again.
    #[serde(skip)]
    report_latch: bool,
}

/// What a charge did beyond the stage and value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Increase {
    /// The stage rose: the loop is hard-stopped.
    pub stage_up: bool,
    /// The report branch ran: its draw picked `item` from the `Report=` list
    /// of weapon `weapon_index` (`GetWeapon(2 * stage)`, after any stage-up).
    pub report: Option<Report>,
}

/// One report pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Report {
    pub weapon_index: i32,
    pub item: u32,
}

impl GattlingState {
    /// `GetStage @ 0x0070DDC0`.
    pub fn stage(&self) -> i32 {
        self.stage
    }

    /// `GetValue @ 0x0070DDF0`.
    pub fn value(&self) -> i32 {
        self.value
    }

    /// `+0x4B8`: set once a report loop started, until the next decay,
    /// stage-up or Limbo.
    #[cfg(test)]
    pub(crate) fn report_latch(&self) -> bool {
        self.report_latch
    }

    /// `SetStage @ 0x0070DDD0`: a negative stage is ignored.
    pub(crate) fn set_stage(&mut self, stage: i32) {
        if stage >= 0 {
            self.stage = stage;
        }
    }

    /// `SetValue @ 0x0070DE00`: a negative value is ignored.
    pub(crate) fn set_value(&mut self, value: i32) {
        if value >= 0 {
            self.value = value;
        }
    }

    /// `DecreaseValue @ 0x0070DE40`: subtract, then zero when the result's
    /// sign bit is set or the amount was zero (`JS`, then `TEST EAX, EAX`).
    /// Its one caller, BuildingClass::Update's idle decay (`0x0043FF19`),
    /// arrives with the building attack mission.
    #[cfg(test)]
    pub(crate) fn decrease_value(&mut self, amount: i32) {
        let value = self.value.wrapping_sub(amount);
        self.value = if value < 0 || amount == 0 { 0 } else { value };
    }

    /// `IncreaseGattlingStage(ticks) @ 0x0070DE70`.
    ///
    /// `report_count(i)` is `GetWeapon(i)->WeaponType->Report.Count`, `None`
    /// when slot `i` names no WeaponType. `next_random` is `g_MainRng.Next()`,
    /// called at most once.
    ///
    /// RESIDUAL: native faults at `0x0070DF8A` when the latch is clear and
    /// `GetWeapon(2 * stage)` names no WeaponType (a stage past the authored
    /// weapons); VERA plays nothing and leaves the latch clear. Unreachable
    /// with retail data.
    pub(crate) fn increase(
        &mut self,
        table: &GattlingStages,
        elite: bool,
        ticks: i32,
        report_count: impl Fn(i32) -> Option<i32>,
        next_random: impl FnOnce() -> u32,
    ) -> Increase {
        let stages = table.weapon_stages();
        // The cap is tested before the add (`JGE`), against the value as
        // it was; the stage test below reads that same value.
        let before = self.value;
        if before < table.threshold(stages, elite) {
            self.value = before.wrapping_add(table.rate_up().wrapping_mul(ticks));
        }
        let mut effects = Increase::default();
        let mut stage = self.stage;
        if stage >= 0
            && stage < stages.wrapping_sub(1)
            && table.threshold(stage + 1, elite) <= before
        {
            stage += 1;
            self.stage = stage;
            self.report_latch = false;
            effects.stage_up = true;
        }
        if !self.report_latch {
            let weapon_index = stage.wrapping_mul(2);
            // `JLE` at `0x0070DF92`: an empty or negative count skips the
            // draw and leaves the latch clear.
            if let Some(count) = report_count(weapon_index).filter(|&count| count > 0) {
                // `DIV`: unsigned.
                let item = next_random() % count as u32;
                self.report_latch = true;
                effects.report = Some(Report { weapon_index, item });
            }
        }
        effects
    }

    /// `UpdateGattlingStage(ticks) @ 0x0070E000`. Returns whether a loop was
    /// live (the latch was set) and is released.
    pub(crate) fn update(&mut self, table: &GattlingStages, elite: bool, ticks: i32) -> bool {
        // The release and the latch clear come first, unconditionally. The
        // handle holds a loop only while the latch is set: every writer that
        // clears the latch also stops or releases the handle.
        let released = self.report_latch;
        self.report_latch = false;
        let step = table.rate_down().wrapping_mul(ticks);
        let value = self.value.wrapping_sub(step);
        // `JS` on the subtraction, then the zero step.
        self.value = if value < 0 || step == 0 { 0 } else { value };
        if self.stage > 0 && self.value < table.threshold(self.stage, elite) {
            self.stage -= 1;
        }
        released
    }

    /// TechnoClass::Limbo's `+0x4A4` release and latch clear (`0x006F6C6B`,
    /// `0x006F6C76`). Returns whether a loop was live.
    pub(crate) fn limbo(&mut self) -> bool {
        std::mem::take(&mut self.report_latch)
    }

    /// A state from its native fields, for fixtures and native payloads.
    #[cfg(test)]
    pub(crate) fn from_fields(stage: i32, value: i32, report_latch: bool) -> Self {
        Self {
            stage,
            value,
            report_latch,
        }
    }
}

/// The stage call the unit's per-frame firing update makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StageCall {
    Increase,
    Update,
}

/// What the unit's per-frame firing update (`0x00736DF0`) found ahead of its
/// gattling tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitFireOutcome {
    /// No target (`+0x2B4`), or `GetWeapon(0)` names no WeaponType: the
    /// early arm, `UpdateGattlingStage(1)` at `0x00737116`.
    NoTarget,
    /// GetFireError's code, after the switch at `0x00737148`.
    Code(FireError),
}

/// The tail at `0x00737063`: a unit that is firing, facing, reloading or
/// rotating charges (`0x0073708A`); any other code, or no target, decays
/// (`0x007370A9`, `0x00737116`). Gattling types only.
pub(crate) fn unit_fire_tail(outcome: UnitFireOutcome) -> StageCall {
    match outcome {
        UnitFireOutcome::Code(
            FireError::Ok | FireError::Facing | FireError::Rearm | FireError::Rotating,
        ) => StageCall::Increase,
        UnitFireOutcome::Code(_) | UnitFireOutcome::NoTarget => StageCall::Update,
    }
}

/// Whether the unit update advances `+0x148`, the turret animation counter
/// (`0x007370D5`, `0x007370F2`, `0x0073713A`): a gattling type while its
/// value is positive after the tail, any other type on OK and REARM.
pub(crate) fn unit_turret_anim_advances(
    is_gattling: bool,
    outcome: UnitFireOutcome,
    value_after: i32,
) -> bool {
    if is_gattling {
        value_after > 0
    } else {
        matches!(
            outcome,
            UnitFireOutcome::Code(FireError::Ok | FireError::Rearm)
        )
    }
}

/// The loop-handle owner of a techno's gattling report (`TechnoClass+0x4A4`):
/// a tag bit keeps it apart from the object's own sound handles and every
/// other owner key.
const GATTLING_SOUND_TAG: u64 = 1 << 61;

pub(crate) const fn gattling_sound_owner(techno: u64) -> u64 {
    techno | GATTLING_SOUND_TAG
}

pub(crate) const fn gattling_sound_techno(owner: u64) -> Option<u64> {
    if owner & GATTLING_SOUND_TAG != 0 {
        Some(owner & !GATTLING_SOUND_TAG)
    } else {
        None
    }
}

impl crate::sim::world::Simulation {
    /// The tail of the unit's per-frame firing update: the gattling call
    /// (`0x00737063`, or `0x00737116` with no target) and the `+0x148` count.
    pub(crate) fn unit_fire_update_tail(
        &mut self,
        id: u64,
        outcome: UnitFireOutcome,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some((is_gattling, slot0)) = self.substrate.entities.get(id).and_then(|entity| {
            let obj = self.object_type(entity.type_ref(), rules)?;
            Some((
                obj.is_gattling,
                crate::sim::combat::combat_weapon::weapon_for_index(obj, entity.veterancy, 0)
                    .is_some(),
            ))
        }) else {
            return;
        };
        // No WeaponType in slot 0 takes the no-target arm whatever the unit
        // holds (`0x00736E0C`).
        let outcome = if slot0 {
            outcome
        } else {
            UnitFireOutcome::NoTarget
        };
        if is_gattling {
            match unit_fire_tail(outcome) {
                StageCall::Increase => self.gattling_increase(id, rules, 1),
                StageCall::Update => self.gattling_update(id, rules, 1),
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && unit_turret_anim_advances(is_gattling, outcome, entity.gattling.value())
        {
            entity.turret_anim_frame = entity.turret_anim_frame.wrapping_add(1);
        }
    }

    /// `IncreaseGattlingStage(ticks)` on one techno, with its report.
    pub(crate) fn gattling_increase(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        ticks: i32,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(obj) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let veterancy = entity.veterancy;
        let world = Self::movement_sound_world(entity);
        let mut state = entity.gattling;
        let weapon = |index: i32| {
            crate::sim::combat::combat_weapon::weapon_for_index(obj, veterancy, index)
                .map(|(weapon_id, _)| rules.weapon(weapon_id))
        };
        let main_rng = &mut self.main_rng;
        let effects = state.increase(
            &obj.gattling_stages,
            crate::sim::combat::veterancy::rank_from_u16(veterancy)
                == crate::sim::combat::veterancy::VeterancyRank::Elite,
            ticks,
            |index| weapon(index).map(|weapon| weapon.map_or(0, |weapon| weapon.report_count())),
            || main_rng.next_u32(),
        );
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.gattling = state;
        }
        let owner = gattling_sound_owner(id);
        if effects.stage_up {
            self.sound_events
                .push(crate::sim::world::SimSoundEvent::GattlingLoopStop { owner });
        }
        if let Some(report) = effects.report
            && let Some(sound) = weapon(report.weapon_index)
                .flatten()
                .and_then(|weapon| weapon.report_item(report.item as usize))
        {
            let sound_id = self.interner.intern(sound);
            self.sound_events
                .push(crate::sim::world::SimSoundEvent::GattlingLoop {
                    owner,
                    sound_id,
                    world,
                });
        }
    }

    /// `UpdateGattlingStage(ticks)` on one techno.
    pub(crate) fn gattling_update(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        ticks: i32,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(obj) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let elite = crate::sim::combat::veterancy::rank_from_u16(entity.veterancy)
            == crate::sim::combat::veterancy::VeterancyRank::Elite;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if entity.gattling.update(&obj.gattling_stages, elite, ticks) {
            self.sound_events
                .push(crate::sim::world::SimSoundEvent::GattlingLoopRelease {
                    owner: gattling_sound_owner(id),
                });
        }
    }

    /// TechnoClass::Limbo's `+0x4A4` release and latch clear.
    pub(crate) fn gattling_limbo(&mut self, id: u64) {
        if self
            .substrate
            .entities
            .get_mut(id)
            .is_some_and(|entity| entity.gattling.limbo())
        {
            self.sound_events
                .push(crate::sim::world::SimSoundEvent::GattlingLoopRelease {
                    owner: gattling_sound_owner(id),
                });
        }
    }
}

#[cfg(test)]
#[path = "gattling_tests.rs"]
mod tests;
