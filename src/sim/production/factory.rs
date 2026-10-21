//! Per-(house, category) factories and their deterministic registry: the
//! authoritative production state.
//!
//! This module owns production charging and the queue-of-record: a build
//! starts owing its house's Cost_Of without a money check, `step_all` steps
//! each build once per rate and charges `balance/steps_left` per step, a
//! shortfall rewinds the step onto on-hold, a user hold sets suspension across
//! rate refreshes until same-type Resume, and an abandoned build reports the Balance
//! it still owed for the lifecycle owner's refund. State here is serialized
//! and folded into the lockstep hash.
//!
//! Determinism: `BTreeMap<(InternedId, ProductionCategory), Factory>` (both key
//! components derive `Ord`) gives sorted iteration for replay/lockstep; no
//! `HashMap`, no fixed-size player array, no `1<<idx` bitmask — satisfies the
//! 30-player scale target. No RNG. Charges are integer math; `time_to_build`
//! reproduces gamemd's x87 arithmetic through `util::native_x87`.
//!
//! Depends on: `sim/intern`, `sim/production/production_types` (ProductionCategory,
//! BuildQueueState), `sim/economy` (the house wallet), `rules` (type cost and
//! build-time factors), and `sim/world::Simulation` (read-only) for the inputs.
//! NEVER on render/ui/audio/net (sim invariant #1).

use std::collections::{BTreeMap, VecDeque};
use std::hash::{Hash, Hasher};

use crate::rules::object_type::{BuildCategory, FactoryType, ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::economy::Economy;
use crate::sim::house_tracking::FactorySlot;
use crate::sim::intern::InternedId;
use crate::sim::production::production_tech::production_category_for_object;
use crate::sim::production::production_types::ProductionCategory;
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// Build completes at exactly this many progress steps (the engine's step count).
pub const PRODUCTION_STEPS: u16 = 54;
/// Per-step frame-rate clamp (the engine clamps `total/54` into `[1, 255]`).
pub const STEP_RATE_MIN: u16 = 1;
pub const STEP_RATE_MAX: u16 = 255;

/// Replay the per-step charge ladder for `progress` steps to recover the exact
/// running balance the stepper holds at that progress. At most 54 integer
/// iterations; `cost` clamped non-negative; mirrors `advance_one_step`'s charge.
#[cfg(any(test, feature = "test-support"))]
fn remaining_balance_after(cost: i32, progress: u16) -> i32 {
    let mut balance = cost.max(0);
    let steps = progress.min(PRODUCTION_STEPS);
    for value in 1..=steps {
        let steps_left = PRODUCTION_STEPS - value;
        let charge = if steps_left == 0 {
            balance
        } else {
            balance / (steps_left as i32)
        };
        balance -= charge;
    }
    balance
}

/// The object a factory holds from start through delivery. Active-retail
/// `FactoryClass::StartProduction @ 0x004C9C70` calls the type's create-instance
/// virtual immediately and stores that constructed Techno at `Factory+0x58`.
/// Therefore every live active build has one limbo `entity_id` from production
/// start; completion, delivery retries, and cancellation operate on that same
/// identity rather than constructing at completion.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingObject {
    pub type_id: InternedId,
    pub entity_id: Option<u64>,
    /// Native completion accounting belongs to the one held factory object, not to
    /// each delivery attempt. A refused Unlimbo keeps this serialized latch with the
    /// same identity; the next promoted object starts with it clear.
    #[serde(default)]
    pub completion_accounted: bool,
}

/// One queued (not-yet-active) build waiting behind the active object — the
/// queue-of-record element: the type and its enqueue stamp. State, progress and
/// balance belong only to the active build (the `Factory` head fields).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QueueEntry {
    pub type_id: InternedId,
    /// The monotonic temporal stamp minted at enqueue (`next_enqueue_order`).
    pub enqueue_order: u64,
}

/// One authoritative production state machine per (house, category), owned by
/// `FactoryRegistry`. The factory retains its unpaid obligation; Economy owns cash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Factory {
    owner: InternedId,
    category: ProductionCategory,
    /// `0..=54`; completion at `PRODUCTION_STEPS`.
    progress: u16,
    /// Frames per step, `clamp(Time_To_Build / 54, 1, 255)` (Factory `+0x38`);
    /// `0` before the build starts and after Suspend/completion, until the
    /// House's rate-only reassessment may rewrite it independently of +70.
    step_rate_frames: u16,
    /// The step timer (Factory `+0x2C`), anchored to the native frame: a step
    /// comes once `step_rate_frames` frames have passed since the last one.
    step_timer: CdTimer,
    /// Remaining cost still owed (Factory `+0x60`), charged down per step:
    /// StartProduction seeds it with the type's Cost_Of for the owner
    /// (`0x004C9DE1..0x004C9DEA`).
    balance: i32,
    object: Option<PendingObject>,
    /// The last step could not be afforded (Factory `+0x5C`); the step was
    /// rewound and the next one retries at the same rate.
    on_hold: bool,
    /// Factory `+0x70`, shared by Suspend4C9E60 and completion4C9C06.
    /// The rate-only House power pass4CA6E0 preserves this authority even
    /// when it restores a nonzero rate on a user-held or completed factory.
    /// Native evidence: tools/sidebar_oracle/factory_hold_resume.md.
    suspended: bool,
    /// Factory+71: blocks House50927C's automatic recovery while +70 is set.
    /// Constructor4C98F7 and affordable Resume4C9F6C set it; accepted
    /// Suspend4C9E71 writes its argument. It is not a second pause flag.
    /// Only this owner mutates it; save/hash retain the distinction.
    auto_resume_blocked: bool,
    /// FIFO queue-of-record waiting behind the active object (Factory `+0x40`).
    queue: VecDeque<QueueEntry>,
    /// The factory's construction order: gamemd appends each new FactoryClass
    /// to the global Factories vector (ctor `0x004C9974..0x004C9989`), and
    /// `LogicClass` runs `FactoryClass::AI` in that order (`0x0055B66A..`).
    /// Set from the build's enqueue stamp when the factory is created. A
    /// promotion or NULL-product rearm keeps this identity and charge order:
    /// original Begin4FA350 reuses the live Factory after PointerExpired.
    /// Source: cameo_order.md, same-category rearm history.
    insertion_seq: u64,
    /// Factory+5D. StartProduction/Abandon/CompletedProduction and an actual
    /// stage advance set this; StripAI consumes it through4C9C60, even when
    /// the object is unfinished. Completion accounting and +71 are separate.
    /// Native: basic-factory-output-event-admission-research (active gamemd).
    #[serde(default)]
    changed: bool,
}

impl Default for Factory {
    fn default() -> Self {
        Self {
            owner: Default::default(),
            category: Default::default(),
            progress: 0,
            step_rate_frames: 0,
            step_timer: Default::default(),
            balance: 0,
            object: None,
            on_hold: false,
            suspended: false,
            auto_resume_blocked: true,
            queue: Default::default(),
            insertion_seq: 0,
            changed: false,
        }
    }
}

impl Factory {
    /// The owning house (Factory `+0x6C`).
    pub fn owner(&self) -> InternedId {
        self.owner
    }

    pub fn category(&self) -> ProductionCategory {
        self.category
    }

    pub fn progress(&self) -> u16 {
        self.progress
    }

    pub fn step_rate_frames(&self) -> u16 {
        self.step_rate_frames
    }

    pub fn step_timer(&self) -> &CdTimer {
        &self.step_timer
    }

    pub fn balance(&self) -> i32 {
        self.balance
    }

    pub fn object(&self) -> Option<&PendingObject> {
        self.object.as_ref()
    }

    pub fn on_hold(&self) -> bool {
        self.on_hold
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn queue(&self) -> &VecDeque<QueueEntry> {
        &self.queue
    }

    pub fn insertion_seq(&self) -> u64 {
        self.insertion_seq
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_owner_for_test(&mut self, value: InternedId) {
        self.owner = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_category_for_test(&mut self, value: ProductionCategory) {
        self.category = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_object_for_test(&mut self, value: Option<PendingObject>) {
        self.object = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_balance_for_test(&mut self, value: i32) {
        self.balance = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_on_hold_for_test(&mut self, value: bool) {
        self.on_hold = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_progress_for_test(&mut self, value: u16) {
        self.progress = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_step_rate_frames_for_test(&mut self, value: u16) {
        self.step_rate_frames = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_step_timer_for_test(&mut self, value: CdTimer) {
        self.step_timer = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_suspended_for_test(&mut self, value: bool) {
        self.suspended = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn object_mut_for_test(&mut self) -> &mut Option<PendingObject> {
        &mut self.object
    }

    /// The world chooses registry holder order;
    /// this owner folds the complete Factory state, including private latches.
    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        self.owner.hash(hasher);
        (self.category as u8).hash(hasher);
        self.insertion_seq.hash(hasher);
        self.progress.hash(hasher);
        self.step_rate_frames.hash(hasher);
        self.step_timer.start_frame().hash(hasher);
        self.step_timer.duration().hash(hasher);
        self.balance.hash(hasher);
        match &self.object {
            Some(object) => {
                1u8.hash(hasher);
                object.type_id.hash(hasher);
                match object.entity_id {
                    Some(entity) => {
                        1u8.hash(hasher);
                        entity.hash(hasher);
                    }
                    None => 0u8.hash(hasher),
                }
                object.completion_accounted.hash(hasher);
            }
            None => 0u8.hash(hasher),
        }
        self.on_hold.hash(hasher);
        self.suspended.hash(hasher);
        self.auto_resume_blocked.hash(hasher);
        if self.changed {
            b"factory-changed-5d".hash(hasher);
        }
        (self.queue.len() as u64).hash(hasher);
        for entry in &self.queue {
            entry.type_id.hash(hasher);
            entry.enqueue_order.hash(hasher);
        }
    }

    /// One read-only projection for House and Building holders. All queue,
    /// sidebar and input consumers share FactoryView's native running test.
    pub(super) fn view(&self) -> FactoryView<'_> {
        FactoryView {
            construction_order: self.insertion_seq,
            step_rate_frames: self.step_rate_frames,
            progress: self.progress,
            on_hold: self.on_hold,
            suspended: self.suspended,
            object: self.object.as_ref(),
            queue: &self.queue,
        }
    }

    /// The step rate for a build of `time_to_build` frames: `clamp(total / 54,
    /// 1, 255)`, the division truncating. The build start `0x004C9EA0` (Ghidra
    /// label `FactoryClass__SetRate`; it also resumes a suspended build)
    /// computes it at `0x004C9EEF..0x004C9F28`; the house power pass
    /// (`0x004CA6E0`, from `0x00508D88`) rewrites only this rate.
    fn set_rate(&mut self, time_to_build: i32) {
        self.step_rate_frames = build_step_rate(time_to_build);
    }

    /// FactoryResume4C9EA0 for represented Techno-held builds. Admission
    /// precedes every write. An admitted resume clears +70 and restarts its
    /// interval BEFORE checking AvailableMoney. A short wallet returns false
    /// with those effects and the previous +71 intact. Only affordability
    /// sets +71 and permits the held-start re-suspension4C9F74..4C9F99.
    /// Special non-Techno Factory+68 products are not represented here.
    fn resume(
        &mut self,
        time_to_build: i32,
        frame: u32,
        held_start: bool,
        available_money: i32,
    ) -> bool {
        if self.object.is_none() || !self.suspended || self.progress == PRODUCTION_STEPS {
            return false;
        }
        self.suspended = false;
        self.set_rate(time_to_build);
        self.step_timer = CdTimer::started(frame as i32, i32::from(self.step_rate_frames));
        // The represented held-Techno guard above excludes stage54;
        // native4C9F3E..4C9F4D divides by the current remaining stages.
        let charge = self.balance / (i32::from(PRODUCTION_STEPS) - i32::from(self.progress));
        if available_money < charge {
            return false;
        }
        self.auto_resume_blocked = true;
        if held_start {
            self.suspend(true, frame);
        }
        true
    }

    /// `FactoryClass::Suspend @ 0x004C9E60`: refused when
    /// the factory is already stopped (`+0x70`, which completion sets too);
    /// otherwise the rate clears and the step timer restarts empty at `frame`.
    /// The progress, balance and a cash stall are kept.
    fn suspend(&mut self, user: bool, frame: u32) -> bool {
        if self.suspended {
            return false;
        }
        self.auto_resume_blocked = user;
        self.suspended = true;
        self.step_rate_frames = 0;
        self.step_timer = CdTimer::started(frame as i32, 0);
        true
    }

    /// Advance one step against the supplied economy (C2/C3/C4/C12/C15).
    /// Production borrows the house wallet; conservation tests supply a fixture.
    ///
    /// One step per call. The step:
    ///   * increments `progress` first, then reads stepsLeft = 54 - progress;
    ///   * charges `balance / stepsLeft` (the `stepsLeft == 1` step at value 53 thus
    ///     charges `balance/1` = the whole remaining balance; the final
    ///     `stepsLeft == 0` step at value 54 skips the divide (div-by-zero guard) and
    ///     charges 0 — conservation depends on the `/1` step, not the guard step);
    ///   * on a shortfall: rewinds the step, sets `on_hold`, spends nothing;
    ///   * on reaching 54: suspends with the object STILL attached and balance 0
    ///     (delivery, a later slice, clears the object and advances the queue).
    pub fn advance_one_step(&mut self, economy: &mut Economy) -> StepOutcome {
        // ARMED GATE: not stepping this call -> Idle. No object, or suspended
        // (complete-held / paused), or a latched on_hold.
        if self.object.is_none() || self.suspended || self.on_hold {
            return StepOutcome::Idle;
        }
        // Defensive: a settled factory is suspended (caught above); guard anyway.
        if self.progress >= PRODUCTION_STEPS {
            return StepOutcome::Idle;
        }

        // Take one tentative step; the charge reads stepsLeft = 54 - the NEW value.
        self.progress += 1;
        //4C9B9F precedes the credit gate: even a rewound shortfall changes it.
        self.changed = true;
        let steps_left = PRODUCTION_STEPS - self.progress; // 54 - new progress

        // Per-step charge, signed-truncate toward zero (= floor for a non-negative
        // balance). The final step (steps_left == 0) skips the divide (div-by-zero
        // guard) and charges 0; the balance is already drained on the steps_left == 1
        // step (value 53, charge = balance/1).
        let charge = if steps_left == 0 {
            self.balance // 0 here: the balance was drained on the steps_left==1 step
        } else {
            self.balance / (steps_left as i32)
        };

        // The house's Available_Money (`0x004C9BDE`) against the charge: an
        // exactly affordable charge proceeds; a stall spends nothing.
        if economy.available_money() < charge {
            self.progress -= 1; // rewind the tentative step (net-zero advance)
            self.on_hold = true; // UI "On Hold"
            return StepOutcome::Stalled; // nothing spent, balance unchanged
        }

        // Spend_Money(charge) (`0x004C9BF5`), and the balance drops by the same.
        self.on_hold = false; // a successful step clears a prior hold
        economy.spend_money(charge);
        self.balance -= charge; // charge <= balance always (stepsLeft >= 1) -> no underflow

        // Completion settlement on reaching 54. The steps_left==1 charge already
        // zeroed the balance, so there is NO second charge here (the engine's
        // completion spend runs as spend(0); charging the remainder twice double-spends).
        if self.progress >= PRODUCTION_STEPS {
            debug_assert_eq!(
                self.balance, 0,
                "the steps_left==1 step must have zeroed the balance"
            );
            self.balance = 0; // idempotent; the contract value
            self.suspended = true; // complete-but-not-delivered
            // `object` STAYS Some(..); delivery (a later slice) clears it + advances the queue.
            return StepOutcome::Completed;
        }

        StepOutcome::Stepped
    }

    /// `FactoryClass::AbandonProduction @ 0x004C9FF0`: the active object goes
    /// whether or not it is finished (`0x004CA0FC`) and the factory is left
    /// idle with its queue intact. Its refund, `Cost_Of(owner) - Balance` at
    /// cancel time (`0x004CA037..0x004CA046`), needs the owner's live cost, so
    /// the returned object carries the Balance for the lifecycle owner to
    /// credit. `None` without an active object.
    fn pending_abandonment(&self) -> Option<AbandonedObject> {
        let object = self.object.as_ref()?;
        Some(AbandonedObject {
            type_id: object.type_id,
            balance: self.balance,
            entity_id: object.entity_id,
        })
    }

    ///4CA053..4CA07E follows the refund. Keep held identity visible through
    /// the destructor; PointerExpired/4CA0FF clears it afterward. Native+71
    /// automatic-recovery latch+71 and cash-stall+5C are untouched.
    /// NULL held performs no reset.
    fn abandon_production(&mut self, frame: u32) -> Option<AbandonedObject> {
        let abandoned = self.pending_abandonment()?;
        self.balance = 0;
        self.step_rate_frames = 0;
        self.step_timer.start(frame as i32, 0);
        self.progress = 0;
        self.suspended = true;
        self.changed = true;
        Some(abandoned)
    }

    /// Queue-pop kernel of StartNextQueued4CA5A0. The caller must pass the
    /// popped type through HouseBeginProduction(force=1) before constructing
    /// anything (4CA60A). Rejection consumes the entry without a new object,
    /// timer, balance, identity or RNG draw. StartNext never deletes Factory.
    pub(crate) fn start_next_queued(&mut self) -> Option<InternedId> {
        if self.object.is_some() || (self.step_rate_frames != 0 && !self.suspended) {
            return None;
        }
        self.queue.pop_front().map(|entry| entry.type_id)
    }
}

/// The inputs `TechnoClass::Time_To_Build @ 0x006F47A0` reads, as the native
/// fields hold them. The caller resolves them for the object under
/// construction; the port is a pure function of these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeToBuildInputs {
    /// The type's `Cost=` (TechnoType `+0x610`).
    pub cost: i32,
    /// `[General] BuildSpeed=` (Rules `+0x1748`).
    pub build_speed: NativeF64Bits,
    /// The owner's country `BuildTime*Mult=` for the type's class
    /// (`HouseClass @ 0x0050C0A0`).
    pub country_multiplier: NativeF32Bits,
    /// The type's `BuildTimeMultiplier=` (TechnoType `+0x608`).
    pub build_time_multiplier: NativeF32Bits,
    /// The owner's power output and drain (House `+0x53A4`, `+0x53A8`).
    pub power_output: i32,
    pub power_drain: i32,
    /// `[General] LowPowerPenaltyModifier=`, `MinLowPowerProductionSpeed=` and
    /// `MaxLowPowerProductionSpeed=` (Rules `+0x578`, `+0x570`, `+0x574`).
    pub low_power_penalty: NativeF32Bits,
    pub min_low_power_speed: NativeF32Bits,
    pub max_low_power_speed: NativeF32Bits,
    /// The owner's factory count for the object's class (`0x00500910`).
    pub factory_count: i32,
    /// `[General] MultipleFactory=` (Rules `+0x57C`).
    pub multiple_factory: NativeF32Bits,
    /// A wall building (BuildingType `+0x1571`), and
    /// `[General] WallBuildSpeedCoefficient=` (Rules `+0x758`).
    pub wall: bool,
    pub wall_coefficient: NativeF64Bits,
}

/// `0.9` at `0x007F4E80`: 900 frames per minute at 15 fps over 1000 credits,
/// so `BuildSpeed=` is the minutes a 1000-credit object takes.
const BUILD_TIME_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3FEC_CCCC_CCCC_CCCD);
/// `0.01f` at `0x007F4E34`: the low-power speed when it would be zero.
const LOW_POWER_SPEED_FLOOR: NativeF32Bits = NativeF32Bits::from_bits(0x3C23_D70A);

/// `TechnoTypeClass::Time_To_Build @ 0x00711EE0`, virtual +0x88:
/// Cost x BuildSpeed x 0.9, truncated under the process's 53-bit chop
/// control word. Both the real Techno duration and Select6AB8A3's initial
/// sidebar prediction call this type-level kernel.
/// Native executable controls: tools/sidebar_oracle/pending_production.json.
pub fn type_base_build_time(cost: i32, build_speed: NativeF64Bits) -> i32 {
    use crate::util::native_x87::MaskedX87Chop53 as X87;

    X87::ftol_i32_low_masked(X87::mul(
        X87::mul(X87::load_i32(cost), X87::load_f64(build_speed)),
        X87::load_f64(BUILD_TIME_SCALE),
    ))
}

/// Resume4C9EEF and GetRate4C9FC0 share the signed total/54 clamp.
fn build_step_rate(time_to_build: i32) -> u16 {
    let per_step = time_to_build / i32::from(PRODUCTION_STEPS);
    per_step.clamp(i32::from(STEP_RATE_MIN), i32::from(STEP_RATE_MAX)) as u16
}

/// `TechnoClass::Time_To_Build @ 0x006F47A0`: the frames an object takes to
/// build, before the build start (`0x004C9EA0`) divides it into 54 steps. Each stage
/// truncates (`_ftol`) under the process's 53-bit chop control word.
pub fn time_to_build(inputs: &TimeToBuildInputs) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering as Order};

    // TechnoType vt+0x88 (`0x00711EE0`): Cost x BuildSpeed x 0.9.
    let base = type_base_build_time(inputs.cost, inputs.build_speed);
    // `0x006F47CE..0x006F47D7`: the country multiplier times that (FIMUL).
    let mut time = X87::ftol_i32_low_masked(X87::mul(
        X87::load_f32(inputs.country_multiplier),
        X87::load_i32(base),
    ));
    // `0x006F47E4..0x006F47F4`: the type's BuildTimeMultiplier.
    time = X87::ftol_i32_low_masked(X87::mul(
        X87::load_i32(time),
        X87::load_f32(inputs.build_time_multiplier),
    ));
    // `0x006F4803..0x006F4886`: divide by the low-power speed. The power ratio
    // is spilled as a float (`0x006F4808`).
    let ratio = X87::load_f32(X87::store_f32_masked_chop(
        crate::sim::power_system::native_power_ratio(inputs.power_output, inputs.power_drain),
    ));
    let one = X87::load_f32(NativeF32Bits::ONE);
    let mut speed = X87::sub(
        one,
        X87::mul(
            X87::sub(one, ratio),
            X87::load_f32(inputs.low_power_penalty),
        ),
    );
    let min_speed = X87::load_f32(inputs.min_low_power_speed);
    if X87::compare(speed, min_speed) != Order::Greater {
        speed = min_speed;
    }
    // The upper clamp applies only below full power (`0x006F4841..0x006F4869`).
    if matches!(X87::compare(ratio, one), Order::Less | Order::Unordered) {
        let max_speed = X87::load_f32(inputs.max_low_power_speed);
        if !matches!(
            X87::compare(speed, max_speed),
            Order::Less | Order::Unordered
        ) {
            speed = max_speed;
        }
    }
    if matches!(
        X87::compare(speed, X87::load_f32(NativeF32Bits::POSITIVE_ZERO)),
        Order::Equal | Order::Unordered
    ) {
        speed = X87::load_f32(LOW_POWER_SPEED_FLOOR);
    }
    time = X87::ftol_i32_low_masked(X87::div(X87::load_i32(time), speed));
    // `0x006F48E1..0x006F4916`: once more per extra factory, when the
    // multiplier is positive.
    let multiple_factory = X87::load_f32(inputs.multiple_factory);
    if X87::compare(
        multiple_factory,
        X87::load_f32(NativeF32Bits::POSITIVE_ZERO),
    ) == Order::Greater
    {
        for _ in 0..inputs.factory_count.wrapping_sub(1).max(0) {
            time = X87::ftol_i32_low_masked(X87::mul(X87::load_i32(time), multiple_factory));
        }
    }
    // `0x006F4917..0x006F4943`: a wall building's coefficient comes last.
    if inputs.wall {
        time = X87::ftol_i32_low_masked(X87::mul(
            X87::load_i32(time),
            X87::load_f64(inputs.wall_coefficient),
        ));
    }
    time
}

/// Resolve [`time_to_build`]'s inputs for `owner` building `obj`: the rules
/// and type fields, the owner's country multiplier (`0x0050C0A0`), power
/// (House `+0x53A4`/`+0x53A8`) and factory count (`GetFactoryCount @
/// 0x00500910`, called at `0x006F48CF` with the object's class and, for a
/// unit, its type's `Naval=`).
pub(super) fn time_to_build_inputs(
    sim: &crate::sim::world::Simulation,
    rules: &RuleSet,
    owner: InternedId,
    obj: &ObjectType,
) -> TimeToBuildInputs {
    let country_multiplier = sim.houses.get(&owner).map_or(NativeF32Bits::ONE, |house| {
        rules.country_build_time_mult_for_type(sim.interner.resolve(house.house_type_id()), obj)
    });
    let (power_output, power_drain) = sim
        .power_states
        .get(&owner)
        .map_or((0, 0), |power| power.effective_totals());
    let slot = FactorySlot::of(FactoryType::for_category(obj.category), obj.naval);
    let factory_count = sim
        .houses
        .get(&owner)
        .map_or(0, |house| house.tracking().factory_count(slot));
    TimeToBuildInputs {
        cost: obj.cost,
        build_speed: rules.production.build_speed,
        country_multiplier,
        build_time_multiplier: obj.build_time_multiplier,
        power_output,
        power_drain,
        low_power_penalty: rules.production.low_power_penalty_modifier,
        min_low_power_speed: rules.production.min_low_power_production_speed,
        max_low_power_speed: rules.production.max_low_power_production_speed,
        factory_count,
        multiple_factory: rules.production.multiple_factory,
        wall: obj.category == ObjectCategory::Building && obj.wall,
        wall_coefficient: rules.production.wall_build_speed_coefficient,
    }
}

/// Map an object type to the `ProductionCategory` whose factory produces it — the Rust
/// analog of the engine's begin-production factory-slot resolution. A thin tested
/// delegate over `production_category_for_object`: ONE routing source, not a fork. Its
/// value is being the single call site the authority-flip registry sweep will use, and
/// the place the native Vehicle/Ship slot split is pinned by tests. Naval Units route
/// to HouseClass Primary_ForShips (+0x53B8); land Units retain Primary_ForVehicles
/// (+0x53B4), so their active builds, temporal sweep entries, and producer bindings are
/// independent.
pub fn category_for_object(obj: &ObjectType) -> ProductionCategory {
    production_category_for_object(obj)
}

/// Outcome of a single factory step (consumer is the P3 charge stepper + the
/// conservation assert). Derives are serde-free (the no-hash contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    Idle,
    Stepped,
    Stalled,
    Completed,
}

/// The object `FactoryClass::AbandonProduction @ 0x004C9FF0` let go: its type,
/// the Balance still unpaid and its limbo identity, which the Simulation owner
/// destroys without rewinding RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbandonedObject {
    pub type_id: InternedId,
    pub balance: i32,
    pub entity_id: Option<u64>,
}

/// StartProduction stores the Cost_Of as the Balance unclamped
/// (`0x004C9DEA`) and on the object (`+0x300`, `0x004C9DED`).
///
/// RESIDUAL: VERA seeds a negative Cost_Of as 0, since its step charge and
/// wallet assume a non-negative Balance. Trigger: a type whose Cost_Of is
/// negative; no retail type has one. Effect: such a build is free instead of
/// paying the house, and its cancel refund (Cost_Of less that Balance) takes
/// the Cost_Of instead of nothing.
///
/// RESIDUAL: VERA keeps no object `+0x300`. Its gameplay reader, the
/// building's construction completion (`0x00446AE3..0x00446B10`), gives a
/// human's building no FreeUnit when that stored price is non-zero and at most
/// the type's GetCost, which in retail needs the FreeUnit's Cost_Of at 0 or
/// less: 26 Industrial Plants for a refinery's Miner. Effect: VERA still
/// delivers the free unit.
fn seeded_balance(cost: i32) -> i32 {
    cost.max(0)
}

/// Outcome of a `FactoryRegistry::cancel_one`. Serde-free — the same no-hash
/// discipline as `StepOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)] // NO serde
pub enum CancelOutcome {
    /// No factory for (owner, category), or the type matched neither a queued
    /// copy nor the active object. A true no-op.
    NoMatch,
    /// Queued copies of `type_id` were removed and the active object kept. No
    /// refund — a queued item was never charged.
    QueuedRemoved,
    /// Wildcard -1 or nonzero Building BuildCat admitted NULL-held cleanup.
    /// No object exists to refund; HouseAbandon's later queue test decides
    /// StartNext versus Factory deletion.
    AdmittedEmpty,
    /// The active object was admitted for abandonment; the lifecycle owner
    /// credits its refund and destroys it (`FactoryClass::AbandonProduction @ 0x004C9FF0`
    /// deletes the object stored at `Factory+0x58`).
    AbandonedActive(AbandonedObject),
}

/// Outcome of a `FactoryRegistry::enqueue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnqueueOutcome {
    /// A new active object was armed; its Techno still has to be constructed.
    Started,
    /// The build joined the queue behind the active object.
    Queued,
    /// The append was refused: the queue already holds `[General]
    /// MaximumQueuedObjects=` builds (`FactoryClass::StartProduction
    /// 0x004C9CD8..0x004C9CDE`), or the house is at the type's build limit
    /// (`0x004C9CEA`).
    AppendRefused,
}

/// Borrow-only read view of one factory, for the lifecycle and delivery code and
/// tests. Never mutates or owns state; suspension covers user hold and completion.
/// Queue, sidebar and input share its native rate/+70/IsComplete decision.
pub struct FactoryView<'a> {
    construction_order: u64,
    step_rate_frames: u16,
    pub progress: u16,
    pub on_hold: bool,
    pub suspended: bool,
    pub object: Option<&'a PendingObject>,
    pub queue: &'a VecDeque<QueueEntry>,
}

impl<'a> FactoryView<'a> {
    pub fn construction_order(&self) -> u64 {
        self.construction_order
    }

    /// `FactoryClass::GetRate @ 0x004C9FB0` recomputes the held Techno's
    /// current duration; it does not read the stored (possibly suspended)
    /// rate. Select6AB5B3 adds one when resuming its presentation clock.
    /// An empty Factory supplies zero duration, which the shared clamp maps
    /// to one. Evidence: tools/sidebar_oracle/pending_production.md.
    pub fn build_step_rate(&self, sim: &crate::sim::world::Simulation, rules: &RuleSet) -> u16 {
        let duration = self
            .object
            .and_then(|object| object.entity_id)
            .and_then(|id| sim.substrate.entities.get(id))
            .and_then(|entity| {
                sim.object_type(entity.type_ref(), rules).map(|object| {
                    time_to_build(&time_to_build_inputs(sim, rules, entity.owner(), object))
                })
            })
            .unwrap_or(0);
        build_step_rate(duration)
    }

    /// Select6AAE01/6AAE0D and6AB2A9/6AB2B5, plus Draw6A9E9C/6A9EB0,
    /// read rate and suspension
    /// independently of IsComplete. A cash stall retains a running rate.
    pub fn is_running(&self) -> bool {
        self.step_rate_frames != 0 && !self.suspended
    }

    /// Select6AAE01/6AB2A9 reads the linked Factory, independently of the
    /// currently held object's type. Stopped Left then calls IsComplete at
    ///6AB2BD; stopped Right abandons without that query.
    pub fn cameo_state(&self) -> super::BuildQueueState {
        if self.is_running() {
            super::BuildQueueState::Building
        } else if self.complete_object().is_some() {
            super::BuildQueueState::Done
        } else {
            super::BuildQueueState::Paused
        }
    }

    /// `FactoryClass::IsComplete @ 0x004CA130`: the held object once it has
    /// reached exactly the last stage (`0x36`, CMP4CA13C).
    /// Rate and suspension are caller guards, not part of IsComplete.
    /// Its special-item arm (`+0x68`) is
    /// dormant: FactoryClass's own writers store only -1 there (`0x004C990D`,
    /// `0x004CA058`, `0x004CA1F0`).
    pub fn complete_object(&self) -> Option<&'a PendingObject> {
        self.object.filter(|_| self.progress == PRODUCTION_STEPS)
    }
}

/// Who holds a factory. gamemd keeps every FactoryClass in one vector,
/// stepped in construction order, and points at each from one holder: the
/// House's slot for the category, which a player's production fills
/// (`HouseClass::Begin_Production @ 0x004FA350`), or the building that
/// produces for a computer house (`BuildingClass+0x524`, filled by
/// `BuildingClass::Factory_AI @ 0x004500F0`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FactoryHolder {
    House(InternedId, ProductionCategory),
    Building(u64),
}

/// Deterministic registry of all factories, keyed by their holder rather
/// than gamemd's global factory array, so it needs no fixed-size player array.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FactoryRegistry {
    factories: BTreeMap<FactoryHolder, Factory>,
}

impl FactoryRegistry {
    /// Saved keys participate in factory identity; restoration must validate
    /// them against the embedded values before accepting world relationships.
    pub(super) fn keyed_factories(&self) -> impl Iterator<Item = (&FactoryHolder, &Factory)> {
        self.factories.iter()
    }

    /// Seed the queue kernel without constructing a world object. External
    /// fixtures explicitly finish construction when their scenario requires it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_enqueue_kernel(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        enqueue_order: u64,
        cost: i32,
    ) -> bool {
        self.enqueue(
            owner,
            category,
            type_id,
            enqueue_order,
            cost,
            i32::MAX,
            false,
        ) == EnqueueOutcome::Started
    }

    /// Read-only sidebar projection. Never mutates.
    pub fn view(&self, owner: InternedId, category: ProductionCategory) -> Option<FactoryView<'_>> {
        self.factories
            .get(&FactoryHolder::House(owner, category))
            .map(Factory::view)
    }

    /// FactoryClass::PointerExpired4CA580, called by Dispatch725957 for
    /// either expiry control: only a matching held product (+58) is cleared.
    /// The factory, queue, stage, balance, rates and timers remain intact.
    /// Source: cameo_order.md current/saved product-expiry histories.
    pub(crate) fn pointer_expired(&mut self, entity_id: u64) {
        for factory in self.factories.values_mut() {
            if factory
                .object
                .as_ref()
                .is_some_and(|object| object.entity_id == Some(entity_id))
            {
                factory.object = None;
            }
        }
    }

    /// `FactoryClass::CountTotal @ 0x004CA670` on `owner`'s factory for
    /// `category`: one for its object if that is `type_id`, finished or not,
    /// and one per queued copy; zero without that factory.
    pub(crate) fn count_total(
        &self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
    ) -> i32 {
        self.factories
            .get(&FactoryHolder::House(owner, category))
            .map_or(0, |f| {
                let object = f.object.as_ref().is_some_and(|o| o.type_id == type_id);
                let queued = f.queue.iter().filter(|e| e.type_id == type_id).count();
                i32::from(object).wrapping_add(queued as i32)
            })
    }

    /// Number of registered factories (test helper).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.factories.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }

    /// Iterate factories in construction (`insertion_seq`) order, the order
    /// `LogicClass` runs `FactoryClass::AI` in (not the BTreeMap key order).
    pub fn iter_insertion_ordered(&self) -> Vec<&Factory> {
        self.holders_insertion_ordered()
            .into_iter()
            .map(|(_, f)| f)
            .collect()
    }

    /// [`Self::iter_insertion_ordered`] with each factory's holder.
    pub(crate) fn holders_insertion_ordered(&self) -> Vec<(FactoryHolder, &Factory)> {
        let mut all: Vec<(FactoryHolder, &Factory)> = self
            .factories
            .iter()
            .map(|(&holder, f)| (holder, f))
            .collect();
        all.sort_by_key(|(_, f)| f.insertion_seq);
        all
    }

    /// The House-slot factories in construction order: a player's production,
    /// which the house revalidates and delivers.
    fn house_factories_insertion_ordered(&self) -> impl Iterator<Item = &Factory> {
        self.holders_insertion_ordered()
            .into_iter()
            .filter(|(holder, _)| matches!(holder, FactoryHolder::House(..)))
            .map(|(_, f)| f)
    }

    /// Each House factory's owner and category with its types: the object's,
    /// then each queued one not yet listed, in construction order.
    pub(super) fn house_build_types(
        &self,
    ) -> Vec<(InternedId, ProductionCategory, Vec<InternedId>)> {
        self.house_factories_insertion_ordered()
            .map(|f| {
                let mut types: Vec<InternedId> = Vec::new();
                let held = f.object.iter().map(|o| o.type_id);
                for type_id in held.chain(f.queue.iter().map(|e| e.type_id)) {
                    if !types.contains(&type_id) {
                        types.push(type_id);
                    }
                }
                (f.owner, f.category, types)
            })
            .collect()
    }

    pub(super) fn factory(&self, holder: FactoryHolder) -> Option<&Factory> {
        self.factories.get(&holder)
    }

    /// Whether a running factory is building `entity`: its object
    /// (`FactoryClass::GetObject @ 0x004CA160`, `+0x58`) is that Techno, its
    /// rate (`+0x38`) is set and `+0x70` (completion or the user's hold) is
    /// clear. `HouseClass::AI_FindBestRallyTarget`'s test for a Hard house
    /// (`0x0050CCA2..0x0050CCDC`, every factory).
    pub(crate) fn is_building(&self, entity: u64) -> bool {
        self.factories.values().any(|factory| {
            factory
                .object
                .as_ref()
                .is_some_and(|object| object.entity_id == Some(entity))
                && factory.view().is_running()
        })
    }

    /// The factory `BuildingClass+0x524` holds for building `building`.
    pub fn building_factory(&self, building: u64) -> Option<&Factory> {
        self.factories.get(&FactoryHolder::Building(building))
    }

    /// Move the existing Building+524 attachment to an empty building slot.
    /// ExitObject44451F..444552 temporarily lends the selected factory to its
    /// alternate receiver, then restores it after that receiver's ExitObject.
    /// The Factory itself, its insertion order, held object and accounting do
    /// not change. An absent source or occupied destination leaves both intact.
    pub(super) fn transfer_building_factory_attachment(&mut self, from: u64, to: u64) -> bool {
        let destination = FactoryHolder::Building(to);
        if self.factories.contains_key(&destination) {
            return false;
        }
        let Some(factory) = self.factories.remove(&FactoryHolder::Building(from)) else {
            return false;
        };
        self.factories.insert(destination, factory);
        true
    }

    /// `new FactoryClass` for building `building` (`0x0045036C..0x00450387`,
    /// ctor `0x004C98B0` appending it to the factory vector) and the create
    /// path of `FactoryClass::StartProduction @ 0x004C9C70` for `type_id`
    /// (`0x004C9D6E..0x004C9DEA`): the object is held with no rate yet and
    /// the Balance seeded from `cost`, the owner's Cost_Of. The caller
    /// constructs the object and starts its rate.
    pub(super) fn create_building_factory(
        &mut self,
        building: u64,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        insertion_seq: u64,
        cost: i32,
    ) {
        self.factories.insert(
            FactoryHolder::Building(building),
            Factory {
                owner,
                category,
                balance: seeded_balance(cost),
                object: Some(PendingObject {
                    type_id,
                    entity_id: None,
                    completion_accounted: false,
                }),
                insertion_seq,
                changed: true,
                suspended: true,
                ..Factory::default()
            },
        );
    }

    /// Delete building `building`'s factory without abandoning it: the
    /// failed StartProduction (`0x004503A7 -> 0x004502DC`, no object held)
    /// and the placed object's release (`FactoryClass::CompletedProduction
    /// @ 0x004CA1A0` lets the object go before the delete at `0x004501C6`).
    pub(super) fn remove_building_factory(&mut self, building: u64) -> Option<Factory> {
        self.factories.remove(&FactoryHolder::Building(building))
    }

    pub(super) fn pending_abandonment(&self, holder: FactoryHolder) -> Option<AbandonedObject> {
        self.factories.get(&holder)?.pending_abandonment()
    }

    pub(super) fn reset_for_abandon(&mut self, holder: FactoryHolder, frame: u32) {
        if let Some(factory) = self.factories.get_mut(&holder) {
            let _ = factory.abandon_production(frame);
        }
    }

    /// Explicit held=NULL4CA0FF follows the product destructor, within the
    /// caller's ScenarioInit depth. PointerExpired may already have cleared it.
    pub(super) fn finish_abandon(&mut self, holder: FactoryHolder) {
        if let Some(factory) = self.factories.get_mut(&holder) {
            factory.object = None;
        }
    }

    pub(super) fn remove_factory(&mut self, holder: FactoryHolder) {
        self.factories.remove(&holder);
    }

    /// The holders of the factories house `owner` owns (`Factory+0x6C`),
    /// from the last in construction order to the first.
    pub(super) fn owner_holders_last_first(&self, owner: InternedId) -> Vec<FactoryHolder> {
        let mut holders: Vec<FactoryHolder> = self
            .holders_insertion_ordered()
            .into_iter()
            .filter(|(_, factory)| factory.owner == owner)
            .map(|(holder, _)| holder)
            .collect();
        holders.reverse();
        holders
    }

    /// Test-only mutable access to the first factory in sweep order (the `factories`
    /// map is private). Used by the authoritative-hash + progress-persist tests, which
    /// live in the `world` module and so cannot reach the private map directly.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_first_mut(&mut self) -> Option<&mut Factory> {
        self.factories.values_mut().min_by_key(|f| f.insertion_seq)
    }

    /// Test-only mutable access to a specific (owner, category) factory (the map is
    /// private). Lets the production-pipeline integration tests drive the registry
    /// (now the completion authority) without running the full charge sweep.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_factory_mut(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> Option<&mut Factory> {
        self.factories
            .get_mut(&FactoryHolder::House(owner, category))
    }

    /// Test-only mutable access to building `building`'s factory.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_building_factory_mut(&mut self, building: u64) -> Option<&mut Factory> {
        self.factories.get_mut(&FactoryHolder::Building(building))
    }

    /// Test-only: force a (owner, category) factory to the completed-and-held state
    /// (progress == `PRODUCTION_STEPS`, balance drained, suspended) — the exact state
    /// `step_all` leaves on completion, so changed-output publication
    /// fires. Returns `false` when no such (object-holding) factory exists. Reconcile
    /// the registry first so the factory exists.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_arm_ready(&mut self, owner: InternedId, category: ProductionCategory) -> bool {
        match self
            .factories
            .get_mut(&FactoryHolder::House(owner, category))
        {
            Some(f) if f.object.is_some() => {
                f.progress = PRODUCTION_STEPS;
                f.balance = 0;
                f.suspended = true;
                f.changed = true;
                true
            }
            _ => false,
        }
    }

    /// `FactoryClass::StartProduction @ 0x004C9C70` for a PRODUCE that is not
    /// a resume. With no factory for `(owner, category)` or an idle one, arm
    /// the active build (object held, progress 0, balance seeded from `cost`).
    /// With an active object held, append a `QueueEntry` unless the queue
    /// already holds `max_queued` builds (`0x004C9CD5..0x004C9CE4`) or
    /// `at_build_limit` (the caller's `HouseClass::CheckBuildLimit`,
    /// `0x004C9CEA`, asked before the append).
    ///
    /// A freshly-armed build has no rate yet; the caller constructs its Techno
    /// and [`Factory::resume`] arms it. `cost` is resolved by the caller
    /// (which holds `&rules`) so this stays `&sim`-free.
    pub(super) fn enqueue(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        enqueue_order: u64,
        cost: i32,
        max_queued: i32,
        at_build_limit: bool,
    ) -> EnqueueOutcome {
        let holder = FactoryHolder::House(owner, category);
        if let Some(f) = self.factories.get_mut(&holder) {
            if f.object.is_some() {
                if i32::try_from(f.queue.len()).unwrap_or(i32::MAX) >= max_queued || at_build_limit
                {
                    return EnqueueOutcome::AppendRefused;
                }
                f.queue.push_back(QueueEntry {
                    type_id,
                    enqueue_order,
                });
                return EnqueueOutcome::Queued;
            }
            // Idle-but-registered (object None, empty queue) -> re-arm the active build.
            f.progress = 0;
            f.step_rate_frames = 0;
            f.step_timer = CdTimer::default();
            f.balance = seeded_balance(cost);
            f.object = Some(PendingObject {
                type_id,
                entity_id: None,
                completion_accounted: false,
            });
            f.on_hold = false;
            // StartProduction4C9D72 stops the new head until Resume.
            f.suspended = true;
            f.changed = true;
            return EnqueueOutcome::Started;
        }
        // No factory yet -> create one with the active build armed.
        self.factories.insert(
            holder,
            Factory {
                owner,
                category,
                progress: 0,
                step_rate_frames: 0,
                step_timer: CdTimer::default(),
                balance: seeded_balance(cost),
                object: Some(PendingObject {
                    type_id,
                    entity_id: None,
                    completion_accounted: false,
                }),
                on_hold: false,
                suspended: true,
                auto_resume_blocked: true,
                queue: VecDeque::new(),
                insertion_seq: enqueue_order,
                changed: true,
            },
        );
        EnqueueOutcome::Started
    }

    /// The SUSPEND event: `HouseClass::Suspend_Production @ 0x004FA910` holds
    /// whatever the category's factory is building (`Suspend(1)` at
    /// `0x004FA9A5`; the event's type is not checked). Returns whether a
    /// running build was put on hold.
    pub(super) fn suspend(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        frame: u32,
        user: bool,
    ) -> bool {
        self.factories
            .get_mut(&FactoryHolder::House(owner, category))
            .is_some_and(|f| f.suspend(user, frame))
    }

    /// The sole registry entry to FactoryResume4C9EA0 for both House and
    /// Building holders. Callers supply the existing TimeToBuild and
    /// AvailableMoney owners; the Factory owns admission and every write.
    pub(super) fn resume(
        &mut self,
        holder: FactoryHolder,
        time_to_build: i32,
        frame: u32,
        held_start: bool,
        available_money: i32,
    ) -> bool {
        self.factories.get_mut(&holder).is_some_and(|factory| {
            factory.resume(time_to_build, frame, held_start, available_money)
        })
    }

    /// House509275..509287 only recovers a system-held head. A user hold,
    /// completion or poor-wallet resume keeps its own native latch history.
    pub(super) fn automatic_resume_pending(
        &self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> bool {
        self.factories
            .get(&FactoryHolder::House(owner, category))
            .is_some_and(|factory| factory.suspended && !factory.auto_resume_blocked)
    }

    /// The type and +70 suspension of the category's held object. Completion
    /// and user hold take the same native Begin_Production branch.
    pub(super) fn active_object(
        &self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> Option<(InternedId, bool)> {
        let f = self.factories.get(&FactoryHolder::House(owner, category))?;
        let object = f.object.as_ref()?;
        Some((object.type_id, f.suspended))
    }

    /// CompletedProduction4CA1A0 releases a stage54 product without
    /// removing its House slot or queue. Balance retains its value;
    /// stage/rate/timer/suspension follow4CA1BA..4CA1DB. The +71 latch is
    /// preserved; this body does not read or write it.
    pub(super) fn complete_production(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        frame: u32,
    ) -> bool {
        let Some(factory) = self
            .factories
            .get_mut(&FactoryHolder::House(owner, category))
        else {
            return false;
        };
        if factory.progress != PRODUCTION_STEPS {
            return false;
        }
        factory.object = None;
        factory.suspended = true;
        factory.changed = true;
        factory.progress = 0;
        factory.step_rate_frames = 0;
        factory.step_timer.start(frame as i32, 0);
        true
    }

    pub(super) fn start_next_queued(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> Option<InternedId> {
        self.factories
            .get_mut(&FactoryHolder::House(owner, category))?
            .start_next_queued()
    }

    pub(super) fn remove_queued(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        index: usize,
    ) {
        if let Some(factory) = self
            .factories
            .get_mut(&FactoryHolder::House(owner, category))
        {
            factory.queue.remove(index);
        }
    }

    /// Link the EntityStore identity created at StartProduction to the active
    /// factory object. The link is established at progress zero and is retained
    /// through completion and every delivery retry.
    pub(super) fn link_active_entity(
        &mut self,
        holder: FactoryHolder,
        entity_id: u64,
    ) -> Option<u64> {
        let factory = self.factories.get_mut(&holder)?;
        let object = factory.object.as_mut()?;
        if object.entity_id.is_none() {
            object.entity_id = Some(entity_id);
        }
        object.entity_id
    }

    /// Claim the score-screen completion accounting edge for the one active,
    /// completed object. Delivery refusal does not clear the object, so later
    /// retries observe `false`; StartProduction constructs the next head with
    /// a fresh `false` latch.
    pub(super) fn account_completed_object_once(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> bool {
        let Some(factory) = self
            .factories
            .get_mut(&FactoryHolder::House(owner, category))
        else {
            return false;
        };
        if factory.progress < PRODUCTION_STEPS {
            return false;
        }
        let Some(object) = factory.object.as_mut() else {
            return false;
        };
        if object.completion_accounted {
            return false;
        }
        object.completion_accounted = true;
        true
    }

    /// Strip6A8DD3 calls HasChanged4C9C60 before IsComplete4CA130.
    /// Consume every House factory's change flag, including unfinished heads;
    /// only changed, completed, held heads can issue a ready edge. A suspended
    /// complete head with no new writer cannot emit again on later frames.
    pub(crate) fn take_changed_completed_keys(&mut self) -> Vec<(InternedId, ProductionCategory)> {
        let order: Vec<FactoryHolder> = self
            .holders_insertion_ordered()
            .into_iter()
            .filter_map(|(holder, _)| matches!(holder, FactoryHolder::House(..)).then_some(holder))
            .collect();
        let mut completed = Vec::new();
        for holder in order {
            let factory = self.factories.get_mut(&holder).expect("retained Factory");
            if std::mem::take(&mut factory.changed)
                && factory.progress >= PRODUCTION_STEPS
                && factory.object.is_some()
            {
                completed.push((factory.owner, factory.category));
            }
        }
        completed
    }

    /// `FactoryClass::Update_Build_Rate @ 0x004CA6E0`, called by the
    /// reached House power assessment at508D88. The whole global Factory
    /// array is walked in construction order, filtering only Owner. Held,
    /// suspended and completed objects still receive SetRate; a null object
    /// receives1. SetRate never restarts the already armed timer.
    /// Native comparisons: engineer-repair power/rate consumer packet.
    pub(super) fn refresh_rates_for_house(
        &mut self,
        sim: &crate::sim::world::Simulation,
        rules: &RuleSet,
        owner: InternedId,
    ) {
        let order: Vec<FactoryHolder> = self
            .holders_insertion_ordered()
            .into_iter()
            .map(|(holder, _)| holder)
            .collect();
        for holder in order {
            let Some(factory) = self.factories.get_mut(&holder) else {
                continue;
            };
            if factory.owner != owner {
                continue;
            }
            let rate = factory
                .object
                .as_ref()
                .and_then(|object| sim.object_type(object.type_id, rules))
                .map_or(1, |object| {
                    time_to_build(&time_to_build_inputs(sim, rules, owner, object))
                });
            factory.set_rate(rate);
        }
    }

    /// The authoritative per-tick factory sweep (the charge flip). Walks the registry in
    /// construction (`insertion_seq`) order — the SAME order the hash folds in, and the
    /// order `LogicClass` runs `FactoryClass::AI` in — and, for each armed factory whose
    /// per-step cadence timer has expired, charges ONE step against the
    /// owner's REAL wallet (`house.economy.credits`) at its retained rate.
    /// Reproduces the engine's per-tick factory loop (C1), walked before the house tail.
    ///
    /// Borrow the house's sole economy directly, charging cash and accumulating
    /// spent credits together. Rate changes belong to the build-start and
    /// reached House power-assessment receivers, after this global sweep.
    pub(super) fn step_all(&mut self, houses: &mut crate::sim::houses::Houses, frame: u32) {
        // Sweep order = construction order (a strictly monotonic enqueue stamp at each
        // creation -> no ties -> total order -> deterministic).
        let order: Vec<FactoryHolder> = self
            .holders_insertion_ordered()
            .into_iter()
            .map(|(holder, _)| holder)
            .collect();

        for holder in order {
            let Some(f) = self.factories.get_mut(&holder) else {
                continue;
            };
            let owner = f.owner;
            // Only an armed, in-flight build steps: object held, not complete
            // (held for delivery), with the one suspension authority clear.
            if f.object.is_none() || f.suspended || f.progress >= PRODUCTION_STEPS {
                continue;
            }
            let Some(house) = houses.get_mut(&owner) else {
                continue; // a vanished house is skipped (NEVER auto-create)
            };

            // (Cadence) `FactoryClass::AI @ 0x004C9B20` steps once the timer has run
            // out and a rate is set (`0x004C9B63..0x004C9B76`), restarting the timer
            // with the rate before it charges (`0x004C9B78..0x004C9B97`).
            if f.step_rate_frames == 0 || !f.step_timer.expired(frame as i32) {
                continue;
            }
            f.step_timer
                .start(frame as i32, i32::from(f.step_rate_frames));

            // (Charge) one authoritative step against the house's economy.
            // Clear the latched on-hold first so an under-funded build RE-ATTEMPTS this
            // cadence (gamemd re-checks affordability each step; advance_one_step's
            // on_hold gate exists for the off-path clone callers).
            f.on_hold = false;
            let outcome = f.advance_one_step(house.economy_mut());

            // Completion clears the rate and restarts the timer empty
            // (`0x004C9C0C..0x004C9C25`); the object is held for delivery.
            if matches!(outcome, StepOutcome::Completed) {
                f.step_rate_frames = 0;
                f.step_timer.start(frame as i32, 0);
            }
        }
    }

    /// Optional developer fast-forward through the existing payment/completion
    /// owner. Held/paused factories stay held; delivery still runs normally.
    #[cfg(feature = "debug-tools")]
    pub(super) fn debug_complete(
        &mut self,
        houses: &mut crate::sim::houses::Houses,
        frame: u32,
        owner: InternedId,
    ) {
        let order: Vec<_> = self
            .holders_insertion_ordered()
            .into_iter()
            .map(|(holder, _)| holder)
            .collect();
        for holder in order {
            let Some(factory) = self.factories.get_mut(&holder) else {
                continue;
            };
            if factory.owner != owner || factory.object.is_none() || factory.suspended {
                continue;
            }
            let Some(house) = houses.get_mut(&owner) else {
                continue;
            };
            for _ in 0..PRODUCTION_STEPS {
                if factory.progress >= PRODUCTION_STEPS {
                    break;
                }
                factory.on_hold = false;
                if !matches!(
                    factory.advance_one_step(house.economy_mut()),
                    StepOutcome::Stepped
                ) {
                    break;
                }
            }
            if factory.progress >= PRODUCTION_STEPS {
                factory.step_rate_frames = 0;
                factory.step_timer.start(frame as i32, 0);
            }
        }
    }

    /// The ABANDON / ABANDON_ALL events for `type_id` in (owner, category):
    /// `HouseClass::Abandon_Production @ 0x004FAA10`. A single abandon removes
    /// the first queued copy and stops there (`0x004FAAEE`, `0x004FAB29`); with
    /// no queued copy, or for ABANDON_ALL once every copy is gone
    /// (`0x004FAB01..0x004FAB0B`), the active object goes if it is this type,
    /// finished or not (`0x004FAB3D..0x004FAB5E`), for the lifecycle owner to
    /// refund/destruction (`0x004FABA6`). The held pointer remains visible
    /// until that synchronous lifecycle owner finishes. The caller promotes
    /// the next queued build.
    pub(super) fn cancel_one(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: Option<InternedId>,
        all: bool,
        build_cat: Option<BuildCategory>,
    ) -> CancelOutcome {
        let Some(f) = self
            .factories
            .get_mut(&FactoryHolder::House(owner, category))
        else {
            return CancelOutcome::NoMatch;
        };
        // Indexed BuildCat0 first removes matching queued types. Nonzero
        // Building BuildCat and the -1 wildcard skip this arm4FAAC4..4FAB0B.
        let mut removed = false;
        if let Some(type_id) = type_id
            && build_cat.is_none()
        {
            while let Some(index) = f.queue.iter().position(|entry| entry.type_id == type_id) {
                f.queue.remove(index);
                removed = true;
                if !all {
                    return CancelOutcome::QueuedRemoved;
                }
            }
        }
        if f.object
            .as_ref()
            .is_some_and(|object| type_id.is_none_or(|type_id| object.type_id == type_id))
        {
            return CancelOutcome::AbandonedActive(f.pending_abandonment().unwrap());
        }
        //4FAB45 NULL-held indexed BuildCat0 refuses. Wildcard and nonzero
        //BuildCat proceed to the ordinary prefix and queue-count policy.
        if f.object.is_none() && (type_id.is_none() || build_cat.is_some()) {
            return CancelOutcome::AdmittedEmpty;
        }
        if removed {
            CancelOutcome::QueuedRemoved
        } else {
            CancelOutcome::NoMatch
        }
    }
}

/// A native oracle row's `Time_To_Build` inputs (the keys
/// `tools/spatial_oracle/time_to_build.py` writes; the cadence oracle reuses them).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn native_time_to_build_inputs(row: &serde_json::Value) -> TimeToBuildInputs {
    let bits32 = |key: &str| NativeF32Bits::from_bits(row[key].as_u64().unwrap() as u32);
    let bits64 = |key: &str| NativeF64Bits::from_bits(row[key].as_u64().unwrap());
    let int = |key: &str| row[key].as_i64().unwrap() as i32;
    TimeToBuildInputs {
        cost: int("cost"),
        build_speed: bits64("build_speed_bits"),
        country_multiplier: bits32("country_bits"),
        build_time_multiplier: bits32("btm_bits"),
        power_output: int("power_output"),
        power_drain: int("power_drain"),
        low_power_penalty: bits32("penalty_bits"),
        min_low_power_speed: bits32("min_speed_bits"),
        max_low_power_speed: bits32("max_speed_bits"),
        factory_count: int("factory_count"),
        multiple_factory: bits32("multiple_factory_bits"),
        // The native wall test also requires a Building (`0x006F491E`).
        wall: row["kind"] == "building" && row["wall"].as_bool().unwrap(),
        wall_coefficient: bits64("wall_coefficient_bits"),
    }
}

/// The native `Time_To_Build` oracle rows: each row's inputs, the original's
/// result, and the row itself.
#[cfg(any(test, feature = "test-support"))]
fn native_time_to_build_rows() -> Vec<(TimeToBuildInputs, i32, serde_json::Value)> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/time_to_build.json",
    ))
    .expect("oracle rows");
    rows.into_iter()
        .map(|row| {
            let native = row["time_to_build"].as_i64().unwrap() as i32;
            (native_time_to_build_inputs(&row), native, row)
        })
        .collect()
}

/// The native build-start and step-cadence oracle
/// (`tools/spatial_oracle/factory_cadence.py`): `starts` rows run the build
/// start alone, `builds` rows then run `FactoryClass::AI` once per frame.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn native_factory_cadence() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/spatial_oracle/factory_cadence.json")
}

/// One representation adapter for the consumed E2 held-head controls.
#[cfg(test)]
fn native_e2_head(
    reg: &FactoryRegistry,
    houses: &crate::sim::houses::Houses,
    owner: InternedId,
    type_id: InternedId,
) -> serde_json::Value {
    let holder = FactoryHolder::House(owner, ProductionCategory::Infantry);
    let f = &reg.factories[&holder];
    serde_json::json!({
        "stage": f.progress, "rate": f.step_rate_frames,
        "timer_start": f.step_timer.start_frame(), "timer_duration": f.step_timer.duration(),
        "balance": f.balance, "on_hold": f.on_hold, "suspended": f.suspended,
        "changed": f.changed, "latch": f.auto_resume_blocked, "credits": houses[&owner].economy().credits(),
        "spent": houses[&owner].economy().spent_credits(),
        "held_type": f.object.as_ref().map(|obj| { assert_eq!(obj.type_id, type_id); "E2" }),
        "held_id": f.object.as_ref().and_then(|obj| obj.entity_id),
        "queue": f.queue.iter().map(|entry| { assert_eq!(entry.type_id, type_id); "E2" }).collect::<Vec<_>>()
    })
}

#[cfg(test)]
#[path = "factory_system_hold_tests.rs"]
mod system_hold_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_base_duration_matches_executed_pending_production_controls() {
        let corpus = crate::test_fixture::json("tools/sidebar_oracle/pending_production.json");
        let rows = corpus["numeric_controls"].as_array().unwrap();
        let mut compared = 0;
        for row in rows {
            let Some(expected) = row["base_duration_i32"].as_i64() else {
                continue;
            };
            let inputs = &row["inputs"];
            let speed = u64::from_str_radix(inputs["build_speed_bits"].as_str().unwrap(), 16)
                .unwrap()
                .swap_bytes();
            assert_eq!(
                type_base_build_time(
                    inputs["cost"].as_i64().unwrap() as i32,
                    NativeF64Bits::from_bits(speed),
                ),
                expected as i32,
                "{}: original711EE0 return",
                row["name"],
            );
            compared += 1;
        }
        assert_eq!(compared, 7);
    }

    /// Whole original Suspend/Resume and following FactoryAI, including the
    /// short-wallet return AFTER unpausing and the affordable held-start tail.
    /// TimeToBuild's argument comes from its recorded original return value;
    /// its numeric owner has separate consumed native controls.
    #[test]
    fn system_hold_resume_kernels_match_native() {
        use crate::sim::house_state::HouseState;
        use crate::sim::rng::trace_draws;

        let corpus = crate::test_fixture::json("tools/sidebar_oracle/factory_system_hold.json");
        let rows = corpus["resume"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        let owner = InternedId::from_index(1);
        let type_id = InternedId::from_index(2);
        let category = ProductionCategory::Infantry;
        let holder = FactoryHolder::House(owner, category);
        let int = |state: &serde_json::Value, key: &str| state[key].as_i64().unwrap() as i32;
        let mut phases = 0;
        for row in rows {
            let initial = &row["initial"]["factory"];
            let factory = Factory {
                owner,
                category,
                progress: int(initial, "stage") as u16,
                step_rate_frames: int(initial, "rate") as u16,
                step_timer: CdTimer::from_raw(
                    int(initial, "timer_start"),
                    int(initial, "timer_duration"),
                ),
                balance: int(initial, "balance"),
                object: Some(PendingObject {
                    type_id,
                    entity_id: initial["held_id"].as_u64(),
                    ..Default::default()
                }),
                on_hold: initial["on_hold"].as_bool().unwrap(),
                suspended: initial["suspended"].as_bool().unwrap(),
                changed: initial["changed"].as_bool().unwrap(),
                queue: VecDeque::from([QueueEntry {
                    type_id,
                    enqueue_order: 1,
                }]),
                ..Default::default()
            };
            let mut reg = reg_with(owner, category, factory);
            let mut house = HouseState::new(
                owner,
                0,
                None,
                true,
                int(initial, "credits") + int(initial, "spent"),
                10,
            );
            house.economy_mut().spend_money(int(initial, "spent"));
            let mut houses = [house].into_iter().collect::<crate::sim::houses::Houses>();
            assert_eq!(native_e2_head(&reg, &houses, owner, type_id), *initial);
            for phase in row["phases"].as_array().unwrap() {
                let name = phase["name"].as_str().unwrap();
                let frame = phase["after"]["frame"].as_u64().unwrap() as u32;
                let label = format!("{} {name}", row["name"]);
                let ((), draws) = trace_draws(|| match name {
                    "system_suspend_kernel" => assert!(reg.suspend(owner, category, frame, false)),
                    "external_spend_wallet" => {
                        let economy = houses.get_mut(&owner).unwrap().economy_mut();
                        economy.spend_money(economy.credits());
                    }
                    "resume_kernel" => {
                        let calls = phase["calls"].as_array().unwrap();
                        let time_to_build = calls
                            .iter()
                            .find(|c| c["name"] == "time_to_build")
                            .unwrap()["return_eax_raw"]
                            .as_i64()
                            .unwrap() as i32;
                        let native_return = calls
                            .iter()
                            .find(|c| c["name"] == "set_production_rate")
                            .unwrap()["return_low_byte"]
                            .as_u64()
                            .unwrap()
                            != 0;
                        let actual = reg.resume(
                            holder,
                            time_to_build,
                            frame,
                            row["inputs"]["held_start"].as_bool().unwrap(),
                            houses[&owner].economy().credits(),
                        );
                        assert_eq!(actual, native_return, "{label}");
                    }
                    "same_frame_factory_ai" | "next_frame_factory_ai" => {
                        reg.step_all(&mut houses, frame)
                    }
                    other => panic!("unconsumed native phase {other}"),
                });
                assert!(draws.is_empty(), "{label}: kernel has no RNG draws");
                assert_eq!(phase["rng"]["unchanged"], true, "{label}");
                assert_eq!(phase["rng"]["raw_advance_count"], 0, "{label}");
                assert_eq!(
                    native_e2_head(&reg, &houses, owner, type_id),
                    phase["after"]["factory"],
                    "{label}"
                );
                phases += 1;
            }
            assert_eq!(
                row["rng_before"], row["rng_after"],
                "all three native buffers"
            );
        }
        assert_eq!(phases, 18);
    }

    // ---- P2 pure-type tests ----

    #[test]
    fn registry_iter_insertion_ordered_not_map_order() {
        // Keys order Building < Infantry, but seqs are 1, 0 — iteration must follow
        // insertion_seq (=> [0, 1]), not the BTreeMap key order (=> [1, 0]).
        let mut reg = FactoryRegistry::default();
        let owner = InternedId::default();
        let fa = Factory {
            owner,
            category: ProductionCategory::Building,
            insertion_seq: 1,
            ..Factory::default()
        };
        let fb = Factory {
            owner,
            category: ProductionCategory::Infantry,
            insertion_seq: 0,
            ..Factory::default()
        };
        reg.factories.insert(
            FactoryHolder::House(owner, ProductionCategory::Building),
            fa,
        );
        reg.factories.insert(
            FactoryHolder::House(owner, ProductionCategory::Infantry),
            fb,
        );
        let ordered: Vec<u64> = reg
            .iter_insertion_ordered()
            .iter()
            .map(|f| f.insertion_seq)
            .collect();
        assert_eq!(
            ordered,
            vec![0, 1],
            "iteration is insertion_seq order, not map key order"
        );
    }

    // ---- P3 set_rate / charge tests ----

    /// A fresh armed factory holding `cost` credits of work.
    fn armed_factory(cost: i32) -> Factory {
        Factory {
            object: Some(PendingObject::default()),
            balance: cost,
            ..Factory::default()
        }
    }

    /// The build start (`0x004C9EA0`, run by the cadence oracle): the rate is
    /// `Time_To_Build / 54`, truncated and clamped to 1..=255
    /// (`0x004C9EF6..0x004C9F1B`), and the step timer starts with it at the
    /// start frame (`0x004C9F20..0x004C9F34`). The rows cover the division edges
    /// and both clamps.
    #[test]
    fn start_rate_matches_the_native_start() {
        let oracle = native_factory_cadence();
        let start_frame = oracle["start_frame"].as_u64().unwrap() as u32;
        let starts = oracle["starts"].as_array().unwrap();
        for row in starts.iter().chain(oracle["builds"].as_array().unwrap()) {
            let native = row["time_to_build"].as_i64().unwrap() as i32;
            assert_eq!(
                time_to_build(&native_time_to_build_inputs(row)),
                native,
                "{row}"
            );
            // The native fixture holds its dummy Techno at +58, starts
            // suspended, and supplies Cost/credits before calling Resume.
            let mut f = armed_factory(native_time_to_build_inputs(row).cost);
            f.suspended = true;
            let started = f.resume(
                native,
                start_frame,
                false,
                row["credits"].as_i64().unwrap() as i32,
            );
            assert_eq!(started, row["after_start"]["started"].as_bool().unwrap());
            let after = &row["after_start"];
            let int = |key: &str| after[key].as_i64().unwrap() as i32;
            assert_eq!(
                (i32::from(f.step_rate_frames), f.step_timer),
                (
                    int("rate"),
                    CdTimer::started(int("timer_start"), int("timer_duration"))
                ),
                "Time_To_Build {native}"
            );
        }
    }

    /// Full original4CA6E0 controls include live, held, complete, null and
    /// foreign factories. Resolve the supplied scalars through the production
    /// reader and existing House/type/factory-count owners, then compare the
    /// registry receiver. Only +38 changes: the armed +2C timer is retained.
    #[test]
    fn house_power_rate_rewrite_matches_native_statuses_clamps_and_armed_timer() {
        use crate::map::entities::EntityCategory;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::house_state::HouseState;
        use crate::sim::power_system::PowerState;
        use crate::sim::world::Simulation;

        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_power_consumers.json",
        ))
        .unwrap();
        let rows = corpus["rewrite_controls"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let input = &row["input"];
            let cost = input["cost"].as_i64().unwrap() as i32;
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[General]\nBuildSpeed=.7\nMultipleFactory=.8\n\
                 LowPowerPenaltyModifier=1\nMinLowPowerProductionSpeed=.5\n\
                 MaxLowPowerProductionSpeed=.8\n[Countries]\n0=Americans\n\
                 [Americans]\nSide=GDI\n[VehicleTypes]\n0=PENDING\n\
                 [PENDING]\nCost={cost}\nStrength=100\nBuildTimeMultiplier=1\n\
                 [BuildingTypes]\n0=PRODUCER\n[PRODUCER]\nStrength=500\nFactory=UnitType\n",
            )))
            .unwrap();
            let mut sim = Simulation::with_seed(31);
            let owner = sim.interner.intern("Americans");
            let foreign = sim.interner.intern("Foreign");
            sim.houses
                .register(HouseState::new(owner, 0, Some(owner), true, 5000, 10));
            let producer_type = sim.interner.intern("PRODUCER");
            let mut producer = GameEntity::new_at_frame_zero_for_test(
                1,
                5,
                5,
                0,
                0,
                owner,
                Health::new(500),
                producer_type,
                EntityCategory::Structure,
                0,
                5,
                false,
            );
            producer.lifecycle.set_in_limbo_for_test(false);
            producer.tracking_facts = crate::sim::house_tracking::TrackingFacts::of(
                EntityCategory::Structure,
                rules.object("PRODUCER"),
                &rules,
            );
            sim.substrate.entities.insert(producer);
            crate::sim::world::Simulation::update_house_tracking(
                &mut sim.substrate,
                &mut sim.houses,
                1,
                |tracking, entity| tracking.increment_factory_count(entity),
            );
            let mut power = PowerState::default();
            power.set_total_output_for_test(input["power_output"].as_i64().unwrap() as i32);
            power.set_total_drain_for_test(input["power_drain"].as_i64().unwrap() as i32);
            sim.power_states.map_mut_for_test().insert(owner, power);
            let type_id = sim.interner.intern("PENDING");
            let inputs =
                time_to_build_inputs(&sim, &rules, owner, rules.object("PENDING").unwrap());
            // Pin inputs independently of the resulting rate. The shared
            // numeric owner is already compared by time_to_build.json.
            let supplied = native_time_to_build_inputs(input);
            assert_eq!(inputs.cost, supplied.cost);
            assert_eq!(inputs.factory_count, supplied.factory_count);
            assert_eq!(inputs.build_speed, supplied.build_speed);
            assert_eq!(inputs.country_multiplier, supplied.country_multiplier);
            assert_eq!(inputs.build_time_multiplier, supplied.build_time_multiplier);
            assert_eq!(inputs.multiple_factory, supplied.multiple_factory);
            assert_eq!(inputs.low_power_penalty, supplied.low_power_penalty);
            assert_eq!(inputs.min_low_power_speed, supplied.min_low_power_speed);
            assert_eq!(inputs.max_low_power_speed, supplied.max_low_power_speed);

            let mut registry = FactoryRegistry::default();
            let statuses = row["statuses"].as_array().unwrap();
            assert_eq!(statuses.len(), 5);
            for (index, status) in statuses.iter().enumerate() {
                let timer_hex = row["timer_bytes_before"][index].as_str().unwrap();
                let word = |offset| {
                    let bytes: [u8; 4] = std::array::from_fn(|byte| {
                        u8::from_str_radix(&timer_hex[offset + byte * 2..offset + byte * 2 + 2], 16)
                            .unwrap()
                    });
                    i32::from_le_bytes(bytes)
                };
                assert_eq!(word(8), 0, "supplied native middle timer dword");
                registry.factories.insert(
                    FactoryHolder::Building(100 + index as u64),
                    Factory {
                        owner: if status == "foreign" { foreign } else { owner },
                        category: ProductionCategory::Vehicle,
                        object: (status != "null").then_some(PendingObject {
                            type_id,
                            ..Default::default()
                        }),
                        progress: if status == "complete" {
                            PRODUCTION_STEPS
                        } else {
                            12
                        },
                        suspended: status == "held" || status == "complete",
                        on_hold: status == "held",
                        balance: cost,
                        step_rate_frames: 77,
                        step_timer: CdTimer::from_raw(word(0), word(16)),
                        insertion_seq: index as u64,
                        ..Default::default()
                    },
                );
            }
            let prior = registry.clone();
            registry.refresh_rates_for_house(&sim, &rules, owner);
            for (index, actual) in registry.iter_insertion_ordered().iter().enumerate() {
                assert_eq!(
                    i64::from(actual.step_rate_frames),
                    row["rates"][index].as_i64().unwrap(),
                    "cost{cost} output{} status{}",
                    input["power_output"],
                    statuses[index]
                );
                assert_eq!(
                    row["timer_bytes_before"][index], row["timer_bytes_after"][index],
                    "original receiver did not rearm"
                );
                let holder = FactoryHolder::Building(100 + index as u64);
                let mut expected = prior.factories[&holder].clone();
                expected.step_rate_frames = actual.step_rate_frames;
                assert_eq!(**actual, expected, "only the rate changes");
            }
        }
    }

    /// Original whole Select/receive controls expose the reported hold/rate
    /// mismatch, paid-poor Resume0 and complete same-type refusal. This compares
    /// the existing Factory kernels at every measured boundary, using native
    /// pre-state and actual Time_To_Build return values as inputs. +71/system
    /// holds and whole UI/audio/save behavior remain outside this kernel test.
    #[test]
    fn factory_hold_resume_matches_native_paid_boundaries() {
        use crate::sim::house_state::HouseState;

        let corpus = crate::test_fixture::json("tools/sidebar_oracle/factory_hold_resume.json");
        let controls = corpus["controls"].as_array().unwrap();
        assert_eq!(controls.len(), 3);
        let owner = InternedId::from_index(1);
        let type_id = InternedId::from_index(2);
        let category = ProductionCategory::Infantry;
        let holder = FactoryHolder::House(owner, category);
        for row in controls {
            let initial = &row["initial"]["factory"];
            let int = |state: &serde_json::Value, key: &str| state[key].as_i64().unwrap() as i32;
            assert_eq!(initial["held_type"], "E2");
            let mut reg = reg_with(
                owner,
                category,
                Factory {
                    owner,
                    category,
                    progress: int(initial, "stage") as u16,
                    step_rate_frames: int(initial, "rate") as u16,
                    step_timer: CdTimer::started(
                        int(initial, "timer_start"),
                        int(initial, "timer_duration"),
                    ),
                    balance: int(initial, "balance"),
                    object: Some(PendingObject {
                        type_id,
                        entity_id: initial["held_id"].as_u64(),
                        ..Default::default()
                    }),
                    on_hold: initial["on_hold"].as_bool().unwrap(),
                    suspended: initial["suspended"].as_bool().unwrap(),
                    changed: initial["changed"].as_bool().unwrap(),
                    queue: initial["queue"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|name| {
                            assert_eq!(name, "E2");
                            QueueEntry {
                                type_id,
                                enqueue_order: 1,
                            }
                        })
                        .collect(),
                    ..Default::default()
                },
            );
            let mut house = HouseState::new(
                owner,
                0,
                None,
                true,
                int(initial, "credits") + int(initial, "spent"),
                10,
            );
            house.economy_mut().spend_money(int(initial, "spent"));
            let mut houses = [house].into_iter().collect::<crate::sim::houses::Houses>();
            let project = |reg: &FactoryRegistry, houses: &crate::sim::houses::Houses| {
                native_e2_head(reg, houses, owner, type_id)
            };
            let normalized = |state: &serde_json::Value| state["factory"].clone();
            for phase in row["phases"].as_array().unwrap() {
                let label = format!("{} {}", row["name"], phase["name"]);
                assert_eq!(
                    project(&reg, &houses),
                    normalized(&phase["before"]),
                    "{label} before"
                );
                let frame = phase["before"]["frame"].as_u64().unwrap() as u32;
                let ttb = || {
                    let call = phase["calls"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|call| call["name"] == "time_to_build")
                        .expect("original Time_To_Build output supplies the kernel input");
                    assert_eq!(call["held_object"], true, "{label} original TTB receiver");
                    call["return_eax_raw"].as_u64().unwrap() as u32 as i32
                };
                match phase["name"].as_str().unwrap() {
                    "suspend_receive" => {
                        assert!(reg.suspend(owner, category, frame, true));
                    }
                    "rate_refresh" => reg.factories.get_mut(&holder).unwrap().set_rate(ttb()),
                    "resume_receive" => {
                        reg.resume(
                            holder,
                            ttb(),
                            frame,
                            false,
                            houses[&owner].economy().credits(),
                        );
                    }
                    "completed_same_type_begin" => {
                        // Native Resume refuses before reading Time_To_Build.
                        reg.resume(holder, 0, frame, false, houses[&owner].economy().credits());
                    }
                    "spend_wallet" => {
                        let economy = houses.get_mut(&owner).unwrap().economy_mut();
                        economy.spend_money(economy.credits());
                    }
                    "paused_ai" | "refreshed_paused_ai" | "same_frame_ai" | "next_frame_ai" => {
                        reg.step_all(&mut houses, frame)
                    }
                    "right_select" | "left_select" => {}
                    name => panic!("unconsumed native phase {name}"),
                }
                assert_eq!(
                    project(&reg, &houses),
                    normalized(&phase["after"]),
                    "{label} after"
                );
            }
        }
    }

    /// Whole builds against the originals' per-frame `FactoryClass::AI`
    /// (`0x004C9B20`, the cadence oracle): every step attempt with the progress,
    /// hold flag and credits after it, and the state at the end. The rows cover
    /// the retail MTNK, FV and E1, a rate-1, a free and a low-power build, and
    /// MTNK with no money, with too little, with exactly one charge and with a
    /// deposit arriving while it waits. The hold rows put the retail MTNK on
    /// hold with `Suspend(1)` (`0x004C9E60`) and resume it with the build start
    /// (`0x004C9EA0`), including refused holds and resumes and a hold during a
    /// cash stall.
    #[test]
    fn step_all_and_holds_match_the_native_cadence() {
        let oracle = native_factory_cadence();
        let start_frame = oracle["start_frame"].as_u64().unwrap() as u32;
        let owner = InternedId::from_index(1);
        let builds = oracle["builds"].as_array().unwrap().iter();
        for row in builds.chain(oracle["holds"].as_array().unwrap()) {
            let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
            let inputs = native_time_to_build_inputs(row);
            let category = if row["kind"] == "infantry" {
                ProductionCategory::Infantry
            } else {
                ProductionCategory::Vehicle
            };
            let mut initial = armed_factory(inputs.cost);
            initial.suspended = true;
            let mut reg = reg_with(owner, category, initial);
            reg.resume(
                FactoryHolder::House(owner, category),
                time_to_build(&inputs),
                start_frame,
                false,
                int(&row["credits"]),
            );
            let mut houses = [crate::sim::house_state::HouseState::new(
                owner,
                0,
                None,
                true,
                int(&row["credits"]),
                10,
            )]
            .into_iter()
            .collect::<crate::sim::houses::Houses>();
            let deposits: BTreeMap<u32, i32> = row["deposits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|deposit| (int(&deposit[0]) as u32, int(&deposit[1])))
                .collect();
            let orders: Vec<(u32, &str)> = row["order_results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|order| (int(&order[0]) as u32, order[1].as_str().unwrap()))
                .collect();
            let mut attempts = Vec::new();
            let mut order_results = Vec::new();
            for frame in start_frame..=row["last_frame"].as_u64().unwrap() as u32 {
                let economy = houses.get_mut(&owner).unwrap().economy_mut();
                economy.add_credits(deposits.get(&frame).copied().unwrap_or(0));
                // A SUSPEND is `Suspend(1)`; a resume is Begin_Production's
                // same-type build start, which VERA takes only for a user hold.
                for &(_, kind) in orders.iter().filter(|(at, _)| *at == frame) {
                    let accepted = if kind == "suspend" {
                        reg.suspend(owner, category, frame, true)
                    } else {
                        reg.resume(
                            FactoryHolder::House(owner, category),
                            time_to_build(&inputs),
                            frame,
                            false,
                            houses[&owner].economy().credits(),
                        )
                    };
                    let f = &reg.factories[&FactoryHolder::House(owner, category)];
                    order_results.push(serde_json::json!([
                        frame,
                        kind,
                        accepted,
                        f.step_rate_frames,
                        f.step_timer.start_frame(),
                        f.step_timer.duration(),
                        f.progress,
                        f.suspended,
                    ]));
                }
                let before = reg.factories[&FactoryHolder::House(owner, category)]
                    .step_timer
                    .start_frame();
                reg.step_all(&mut houses, frame);
                let f = &reg.factories[&FactoryHolder::House(owner, category)];
                if f.step_timer.start_frame() != before {
                    let credits = houses[&owner].economy().credits();
                    attempts.push(serde_json::json!([frame, f.progress, f.on_hold, credits]));
                }
            }
            let label = format!(
                "{} cost {} credits {} orders {}",
                row["kind"], row["cost"], row["credits"], row["orders"]
            );
            assert_eq!(
                serde_json::Value::from(attempts),
                row["attempts"],
                "{label}"
            );
            assert_eq!(
                serde_json::Value::from(order_results),
                row["order_results"],
                "{label}: the holds and resumes"
            );
            let f = &reg.factories[&FactoryHolder::House(owner, category)];
            let economy = houses[&owner].economy();
            let end = &row["final"];
            assert_eq!(
                (
                    i32::from(f.progress),
                    i32::from(f.step_rate_frames),
                    f.step_timer,
                    f.balance,
                    f.on_hold,
                    f.suspended,
                    economy.credits(),
                    economy.spent_credits(),
                ),
                (
                    int(&end["stage"]),
                    int(&end["rate"]),
                    CdTimer::started(int(&end["timer_start"]), int(&end["timer_duration"])),
                    int(&end["balance"]),
                    end["on_hold"].as_bool().unwrap(),
                    end["suspended"].as_bool().unwrap(),
                    int(&end["credits"]),
                    int(&end["spent"]),
                ),
                "{label}: the state at the end"
            );
        }
    }

    #[test]
    fn factory_exact_cost_conservation() {
        // Sum of oracle spend over a full build == the full type cost; balance ends 0
        // (C3/C15). Boundary set {1, 25, 700, 99991}.
        for cost in [1i32, 25, 700, 99991] {
            let mut f = armed_factory(cost);
            let mut econ = Economy::new(cost);
            loop {
                match f.advance_one_step(&mut econ) {
                    StepOutcome::Stepped => {}
                    StepOutcome::Completed => break,
                    other => panic!("cost {cost}: unexpected {other:?} with exact funds"),
                }
            }
            assert_eq!(
                econ.spent_credits(),
                cost,
                "cost {cost}: total spent == full cost"
            );
            assert_eq!(
                econ.credits(),
                0,
                "cost {cost}: oracle drained to exactly 0"
            );
            assert_eq!(f.balance, 0, "cost {cost}: balance ends 0");
        }
    }

    #[test]
    fn remaining_balance_ladder_matches_stepper() {
        // remaining_balance_after must equal the balance the stepper actually holds.
        for cost in [1i32, 25, 700, 99991] {
            let mut f = armed_factory(cost);
            let mut econ = Economy::new(cost);
            for k in 0..PRODUCTION_STEPS {
                assert_eq!(
                    f.balance,
                    remaining_balance_after(cost, k),
                    "cost {cost}: ladder replay must match the stepper at progress {k}"
                );
                let _ = f.advance_one_step(&mut econ);
            }
            assert_eq!(remaining_balance_after(cost, PRODUCTION_STEPS), 0);
        }
    }

    // ---- P4 cancel / refund / FIFO tests ----

    /// Insert a factory at (owner, category) into a registry (test helper; the
    /// `factories` map is private but in-module).
    fn reg_with(owner: InternedId, category: ProductionCategory, f: Factory) -> FactoryRegistry {
        let mut reg = FactoryRegistry::default();
        reg.factories.insert(
            FactoryHolder::House(owner, category),
            Factory {
                owner,
                category,
                ..f
            },
        );
        reg
    }

    /// Test queue entry of `ty` (stamp/ETA irrelevant to these direct-call unit tests).
    fn qe(ty: InternedId) -> QueueEntry {
        QueueEntry {
            type_id: ty,
            enqueue_order: 0,
        }
    }

    /// Original whole BuildingLimbo reaches this reset with the held pointer
    /// still visible at Infantry's scalar-destructor entry. Compare its
    /// executed state, including timer anchors and suspension, before the
    /// world owner completes disposal/PointerExpired. Native Resume's +71
    /// latch remains separate from VERA's existing user-hold model.
    #[test]
    fn abandon_reset_matches_native_sale_destructor_boundary() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/factory_sale_queue.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in native["controls"].as_array().unwrap() {
            let Some(checkpoint) = row["checkpoints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|checkpoint| checkpoint["event"] == "infantry_destructor_entry")
            else {
                continue;
            };
            let input = &row["initial"];
            let expected = &checkpoint["state"];
            let int = |state: &serde_json::Value, key: &str| state[key].as_i64().unwrap() as i32;
            let product = PendingObject {
                entity_id: input["held_id"].as_u64(),
                ..Default::default()
            };
            let mut factory = Factory {
                object: Some(product.clone()),
                progress: int(input, "stage") as u16,
                balance: int(input, "balance"),
                step_rate_frames: int(input, "rate") as u16,
                step_timer: CdTimer::from_raw(
                    int(&input["timer"], "start"),
                    int(&input["timer"], "duration"),
                ),
                suspended: int(input, "suspended") != 0,
                on_hold: int(input, "cash_stall") != 0,
                ..Default::default()
            };
            let abandoned = factory
                .abandon_production(int(expected, "frame") as u32)
                .unwrap();
            assert_eq!(abandoned.balance, int(input, "balance"));
            assert_eq!(
                factory.object.as_ref().unwrap().entity_id,
                product.entity_id
            );
            assert!(expected["held_present"].as_bool().unwrap());
            assert_eq!(i32::from(factory.progress), int(expected, "stage"));
            assert_eq!(factory.balance, int(expected, "balance"));
            assert_eq!(i32::from(factory.step_rate_frames), int(expected, "rate"));
            assert_eq!(
                factory.step_timer,
                CdTimer::from_raw(
                    int(&expected["timer"], "start"),
                    int(&expected["timer"], "duration")
                )
            );
            assert_eq!(factory.suspended, int(expected, "suspended") != 0);
            assert_eq!(factory.on_hold, int(expected, "cash_stall") != 0);
            compared += 1;
        }
        assert_eq!(compared, 2);
    }

    #[test]
    fn abandon_of_a_finished_object_refunds_its_whole_cost() {
        // `FactoryClass::AbandonProduction @ 0x004C9FF0` deletes the object finished or
        // not (`0x004CA0FC`) and refunds Cost - Balance, the whole cost once finished.
        let mut f = armed_factory(700);
        let mut econ = Economy::new(700);
        loop {
            if matches!(f.advance_one_step(&mut econ), StepOutcome::Completed) {
                break;
            }
        }
        assert_eq!(f.progress, PRODUCTION_STEPS);
        assert!(f.suspended && f.object.is_some(), "completed-but-held");
        assert_eq!(
            f.abandon_production(0).map(|abandoned| abandoned.balance),
            Some(0),
            "nothing is owed, so the refund is the whole Cost_Of"
        );
        assert!(
            f.object.is_some(),
            "destructor has not run in this scalar kernel"
        );
        assert!(f.suspended);
    }

    #[test]
    fn cancel_one_removes_first_matching() {
        // queue [A,B,A,C] (all queued, no active), cancel A -> [B,A,C]: the FIRST
        // (front-most) A is removed, NOT the last (the legacy .rev() DRIFT).
        let owner = InternedId::default();
        let a = InternedId::from_index(1);
        let b = InternedId::from_index(2);
        let c = InternedId::from_index(3);
        let f = Factory {
            owner,
            category: ProductionCategory::Vehicle,
            queue: std::collections::VecDeque::from(vec![qe(a), qe(b), qe(a), qe(c)]),
            object: None,
            ..Factory::default()
        };
        let mut reg = reg_with(owner, ProductionCategory::Vehicle, f);
        let outcome = reg.cancel_one(owner, ProductionCategory::Vehicle, Some(a), false, None);
        assert_eq!(outcome, CancelOutcome::QueuedRemoved);
        let q: Vec<InternedId> = reg
            .view(owner, ProductionCategory::Vehicle)
            .unwrap()
            .queue
            .iter()
            .map(|e| e.type_id)
            .collect();
        assert_eq!(q, vec![b, a, c], "first A removed -> [B,A,C]");
    }

    #[test]
    fn cancel_one_queued_preferred_over_active_same_type() {
        // active = A (mid-build), tail = [A]; cancel A removes the TAIL copy
        // (QueuedRemoved), the active build is UNTOUCHED (queued-first precedence).
        let owner = InternedId::default();
        let a = InternedId::from_index(1);
        let f = Factory {
            owner,
            category: ProductionCategory::Vehicle,
            object: Some(PendingObject {
                type_id: a,
                entity_id: None,
                completion_accounted: false,
            }),
            balance: 300,
            progress: 20,
            queue: std::collections::VecDeque::from(vec![qe(a)]),
            ..Factory::default()
        };
        let mut reg = reg_with(owner, ProductionCategory::Vehicle, f);
        let outcome = reg.cancel_one(owner, ProductionCategory::Vehicle, Some(a), false, None);
        assert_eq!(
            outcome,
            CancelOutcome::QueuedRemoved,
            "tail copy removed first"
        );
        let view = reg.view(owner, ProductionCategory::Vehicle).unwrap();
        assert!(view.queue.is_empty(), "the one tail copy is gone");
        assert!(view.object.is_some(), "the active build is untouched");
        assert_eq!(view.progress, 20, "active progress unchanged");
    }

    /// ABANDON_ALL (`0x004FAB01..0x004FAB0B`) removes every queued copy and
    /// then the active object when it is the same type; a single ABANDON stops
    /// after the first queued copy.
    #[test]
    fn abandon_all_clears_every_copy_then_the_active_build() {
        let owner = InternedId::default();
        let a = InternedId::from_index(1);
        let b = InternedId::from_index(2);
        let factory = |active: InternedId| Factory {
            owner,
            category: ProductionCategory::Infantry,
            object: Some(PendingObject {
                type_id: active,
                entity_id: None,
                completion_accounted: false,
            }),
            balance: 150,
            progress: 12,
            queue: std::collections::VecDeque::from(vec![qe(a), qe(b), qe(a)]),
            ..Factory::default()
        };
        let queued = |reg: &FactoryRegistry| -> Vec<InternedId> {
            reg.view(owner, ProductionCategory::Infantry)
                .unwrap()
                .queue
                .iter()
                .map(|e| e.type_id)
                .collect()
        };

        let mut reg = reg_with(owner, ProductionCategory::Infantry, factory(a));
        assert_eq!(
            reg.cancel_one(owner, ProductionCategory::Infantry, Some(a), true, None),
            CancelOutcome::AbandonedActive(AbandonedObject {
                type_id: a,
                balance: 150,
                entity_id: None,
            })
        );
        assert_eq!(queued(&reg), vec![b], "every queued copy is gone");
        assert!(
            reg.view(owner, ProductionCategory::Infantry)
                .unwrap()
                .object
                .is_some()
        );

        let mut reg = reg_with(owner, ProductionCategory::Infantry, factory(b));
        assert_eq!(
            reg.cancel_one(owner, ProductionCategory::Infantry, Some(a), true, None),
            CancelOutcome::QueuedRemoved,
            "another type's active build stays"
        );
        assert_eq!(queued(&reg), vec![b]);
        assert!(
            reg.view(owner, ProductionCategory::Infantry)
                .unwrap()
                .object
                .is_some()
        );
    }

    /// StartNext does not manufacture an object/balance before forced Begin
    /// admission. A running unsuspended Factory refuses the pop as4CA5B3 does.
    #[test]
    fn start_next_queued_requires_stopped_or_suspended_factory() {
        let x = InternedId::from_index(1);
        let mut f = Factory {
            object: None,
            queue: std::collections::VecDeque::from(vec![QueueEntry {
                type_id: x,
                enqueue_order: 42,
            }]),
            insertion_seq: 7,
            step_rate_frames: 9,
            ..Factory::default()
        };
        assert_eq!(f.start_next_queued(), None);
        assert_eq!(f.queue.len(), 1);
        f.suspended = true;
        let prior = f.clone();
        assert_eq!(f.start_next_queued(), Some(x));
        assert_eq!(f.insertion_seq, prior.insertion_seq);
        assert_eq!(f.balance, prior.balance);
        assert_eq!(f.progress, prior.progress);
        assert_eq!(f.step_rate_frames, prior.step_rate_frames);
        assert_eq!(f.step_timer, prior.step_timer);
        assert_eq!(f.suspended, prior.suspended);
        assert_eq!(f.changed, prior.changed);
        assert!(f.object.is_none());
        assert!(f.queue.is_empty());
    }

    /// `LogicClass` runs `FactoryClass::AI` in the order the factories were made
    /// (`0x0055B66A..`), and a promotion keeps its factory. With money for one of
    /// two steps due on the same frame, the older factory pays, even after it
    /// promoted a build queued after the younger factory was made.
    #[test]
    fn a_promoted_build_keeps_its_factory_sweep_turn() {
        let owner = InternedId::from_index(1);
        let tank = InternedId::from_index(2);
        let soldier = InternedId::from_index(3);
        let (vehicle, infantry) = (ProductionCategory::Vehicle, ProductionCategory::Infantry);
        let mut reg = FactoryRegistry::default();
        assert_eq!(
            reg.enqueue(owner, vehicle, tank, 0, 700, 5, false),
            EnqueueOutcome::Started
        );
        assert_eq!(
            reg.enqueue(owner, infantry, soldier, 1, 200, 5, false),
            EnqueueOutcome::Started
        );
        assert_eq!(
            reg.enqueue(owner, vehicle, tank, 2, 700, 5, false),
            EnqueueOutcome::Queued
        );
        reg.reset_for_abandon(FactoryHolder::House(owner, vehicle), 0);
        reg.finish_abandon(FactoryHolder::House(owner, vehicle));
        assert_eq!(reg.start_next_queued(owner, vehicle), Some(tank));
        assert_eq!(
            reg.enqueue(owner, vehicle, tank, 2, 700, 5, false),
            EnqueueOutcome::Started
        );
        reg.resume(FactoryHolder::House(owner, vehicle), 540, 100, false, 13);
        reg.resume(FactoryHolder::House(owner, infantry), 540, 100, false, 13);
        // The first steps charge 700 / 53 = 13 and 200 / 53 = 3; the house has 13.
        let mut houses = [crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 13, 10,
        )]
        .into_iter()
        .collect::<crate::sim::houses::Houses>();
        reg.step_all(&mut houses, 110);
        let (tank_factory, soldier_factory) = (
            &reg.factories[&FactoryHolder::House(owner, vehicle)],
            &reg.factories[&FactoryHolder::House(owner, infantry)],
        );
        assert_eq!((tank_factory.progress, tank_factory.on_hold), (1, false));
        assert_eq!(
            (soldier_factory.progress, soldier_factory.on_hold),
            (0, true)
        );
        assert_eq!(houses[&owner].economy().credits(), 0);
    }

    /// Original `TechnoClass::Time_To_Build @ 0x006F47A0` totals from
    /// `tools/spatial_oracle/time_to_build.py` (Unicorn on the retail
    /// executable): stock costs and multipliers per class, other BuildSpeed and
    /// country values, power ratios and clamps, factory counts and walls.
    #[test]
    fn time_to_build_matches_the_native_oracle() {
        let rows = native_time_to_build_rows();
        assert_eq!(rows.len(), 332);
        for (inputs, native, row) in &rows {
            assert_eq!(time_to_build(inputs), *native, "{row}");
        }
    }

    // ---- P5a category_for_object routing delegate ----

    #[test]
    fn category_for_object_matches_rtti_table() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        // One object per category; a Combat-categorized building routes to Defense.
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=GI\n[VehicleTypes]\n0=GRIZZLY\n[AircraftTypes]\n0=BEAG\n\
             [BuildingTypes]\n0=GAPOWR\n1=GAPILL\n\
             [GI]\nCost=100\n[GRIZZLY]\nCost=700\n[BEAG]\nCost=600\n\
             [GAPOWR]\nCost=800\n[GAPILL]\nCost=500\nBuildCat=Combat\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let inf = rules.object("GI").unwrap();
        let veh = rules.object("GRIZZLY").unwrap();
        let air = rules.object("BEAG").unwrap();
        let bld = rules.object("GAPOWR").unwrap();
        let def = rules.object("GAPILL").unwrap();
        assert_eq!(
            category_for_object(inf),
            ProductionCategory::Infantry,
            "infantry -> Infantry (NOT the refuted inverse)"
        );
        assert_eq!(
            category_for_object(veh),
            ProductionCategory::Vehicle,
            "vehicle -> Vehicle"
        );
        assert_eq!(
            category_for_object(air),
            ProductionCategory::Aircraft,
            "aircraft -> Aircraft (NOT the refuted inverse)"
        );
        assert_eq!(
            category_for_object(bld),
            ProductionCategory::Building,
            "plain building -> Building"
        );
        assert_eq!(
            category_for_object(def),
            ProductionCategory::Defense,
            "BuildCat=Combat building -> Defense"
        );
        // The delegate must agree with the routing source it wraps (no fork).
        assert_eq!(
            category_for_object(veh),
            production_category_for_object(veh),
            "delegate == production_category_for_object (single routing source)"
        );
    }

    #[test]
    fn category_for_object_routes_naval_unit_to_ship_slot() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=DEST\n1=GRIZZLY\n[DEST]\nCost=1000\nNaval=yes\n\
             [GRIZZLY]\nCost=700\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let naval = rules.object("DEST").unwrap();
        let land = rules.object("GRIZZLY").unwrap();
        assert_eq!(
            category_for_object(naval),
            ProductionCategory::Ship,
            "naval Unit binds HouseClass Primary_ForShips"
        );
        assert_eq!(
            category_for_object(land),
            ProductionCategory::Vehicle,
            "land Unit independently binds HouseClass Primary_ForVehicles"
        );
    }
    #[cfg(feature = "debug-tools")]
    #[test]
    fn debug_completion_preserves_payment_and_explicit_hold() {
        let owner = InternedId::from_index(1);
        let mut reg = reg_with(owner, ProductionCategory::Infantry, armed_factory(540));
        let mut houses = crate::sim::houses::Houses::default();
        houses.register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 540, 10,
        ));
        reg.factories
            .get_mut(&FactoryHolder::House(owner, ProductionCategory::Infantry))
            .unwrap()
            .suspended = false;
        reg.debug_complete(&mut houses, 10, owner);
        assert_eq!(
            reg.view(owner, ProductionCategory::Infantry)
                .unwrap()
                .progress,
            PRODUCTION_STEPS
        );
        assert_eq!(houses[&owner].economy().credits(), 0);
        assert!(
            reg.view(owner, ProductionCategory::Infantry)
                .unwrap()
                .complete_object()
                .is_some()
        );

        let mut held = armed_factory(540);
        held.suspended = true;
        let mut reg = reg_with(owner, ProductionCategory::Infantry, held);
        reg.debug_complete(&mut houses, 10, owner);
        assert_eq!(
            reg.view(owner, ProductionCategory::Infantry)
                .unwrap()
                .progress,
            0
        );
    }
}
