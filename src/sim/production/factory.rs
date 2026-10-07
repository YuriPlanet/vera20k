//! Per-(house, category) factories and their deterministic registry: the
//! authoritative production state.
//!
//! This module owns production charging and the queue-of-record: a build
//! starts owing its house's Cost_Of without a money check, `step_all` steps
//! each build once per rate and charges `balance/steps_left` per step, a
//! shortfall rewinds the step onto on-hold, a user hold clears the rate until
//! the same type is produced again, and an abandoned build reports the Balance
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
//! NEVER on render/ui/sidebar/audio/net (sim invariant #1).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::economy::Economy;
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
#[cfg(test)]
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

/// Engine special/superweapon discriminator. The study proves the writer of the
/// engine's special-item field was never located, so value `0` cannot be proven
/// unreachable and `0`-vs-`(-1)` MUST NOT be collapsed. Three states keep them
/// distinct. In P1-P3 (normal builds) this is always `NoneNeg1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SpecialItem {
    NoneNeg1,
    NoneZero,
    Item(u32),
}

impl Default for SpecialItem {
    fn default() -> Self {
        SpecialItem::NoneNeg1
    }
}

/// One authoritative production state machine per (house, category), owned by
/// `FactoryRegistry`. The factory retains its unpaid obligation; Economy owns cash.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Factory {
    pub owner: InternedId,
    pub category: ProductionCategory,
    /// `0..=54`; completion at `PRODUCTION_STEPS`.
    pub progress: u16,
    /// Frames per step, `clamp(Time_To_Build / 54, 1, 255)` (Factory `+0x38`);
    /// `0` before the build starts and once it completes.
    pub step_rate_frames: u16,
    /// The step timer (Factory `+0x2C`), anchored to the native frame: a step
    /// comes once `step_rate_frames` frames have passed since the last one.
    pub step_timer: CdTimer,
    /// Remaining cost still owed (Factory `+0x60`), charged down per step:
    /// StartProduction seeds it with the type's Cost_Of for the owner
    /// (`0x004C9DE1..0x004C9DEA`).
    pub balance: i32,
    pub object: Option<PendingObject>,
    /// The last step could not be afforded (Factory `+0x5C`); the step was
    /// rewound and the next one retries at the same rate.
    pub on_hold: bool,
    /// Complete and not yet delivered: the completing step set Factory `+0x70`
    /// and cleared the rate (`0x004C9C06..`).
    pub suspended: bool,
    /// The user's hold: `FactoryClass::Suspend(1) @ 0x004C9E60` set Factory
    /// `+0x70` with its `+0x71` latch and cleared the rate. A PRODUCE of the
    /// same type restarts it (`Factory::resume`). The system hold (`Suspend(0)`,
    /// which `HouseClass::Update_Factory_Queue @ 0x00509140` takes at
    /// `0x0050924D` when only offline factories could build the object and
    /// lifts at `0x00509283`) is not ported: see
    /// `production_tech::revalidate_eligibility`.
    pub manual: bool,
    pub special: SpecialItem,
    /// FIFO queue-of-record waiting behind the active object (Factory `+0x40`).
    pub queue: VecDeque<QueueEntry>,
    /// The factory's construction order: gamemd appends each new FactoryClass
    /// to the global Factories vector (ctor `0x004C9974..0x004C9989`), and
    /// `LogicClass` runs `FactoryClass::AI` in that order (`0x0055B66A..`).
    /// Set from the build's enqueue stamp when the factory is created or an
    /// idle one re-armed (gamemd deletes an idle factory and makes a new one);
    /// a promotion keeps it.
    pub insertion_seq: u64,
    /// Factory+5D. StartProduction/Abandon/CompletedProduction and an actual
    /// stage advance set this; StripAI consumes it through4C9C60, even when
    /// the object is unfinished. Completion accounting and +71 are separate.
    /// Native: basic-factory-output-event-admission-research (active gamemd).
    #[serde(default)]
    changed: bool,
}

impl Factory {
    pub(crate) fn has_changed(&self) -> bool {
        self.changed
    }

    /// The step rate for a build of `time_to_build` frames: `clamp(total / 54,
    /// 1, 255)`, the division truncating. The build start `0x004C9EA0` (Ghidra
    /// label `FactoryClass__SetRate`; it also resumes a suspended build)
    /// computes it at `0x004C9EEF..0x004C9F28`; the house power pass
    /// (`0x004CA6E0`, from `0x00508D88`) rewrites only this rate.
    pub fn set_rate(&mut self, time_to_build: i32) {
        let per_step = time_to_build / i32::from(PRODUCTION_STEPS);
        self.step_rate_frames =
            per_step.clamp(i32::from(STEP_RATE_MIN), i32::from(STEP_RATE_MAX)) as u16;
    }

    /// The build start `0x004C9EA0` for a build starting at `frame`: take the
    /// rate and restart the step timer with it (`0x004C9F20..0x004C9F34`), so
    /// the first step comes one full rate later.
    ///
    /// Its tail (`0x004C9F37..0x004C9F99`) sets the `+0x71` latch when the
    /// house can afford the next charge, which only the system hold reads, and
    /// re-suspends the build when Begin_Production passes its held-start
    /// argument. That argument is set only when a promoted build finds no free
    /// factory for `FindFactory(0,1,1) @ 0x005F7900` but one for `(1,0,1)`
    /// (`0x004FA45B`): an airfield with no free dock, or only offline
    /// factories. Not ported (recorded residual): the aircraft that promotes
    /// into a full airfield runs instead of starting on hold.
    pub fn start_rate(&mut self, time_to_build: i32, frame: u32) {
        self.set_rate(time_to_build);
        self.step_timer = CdTimer::started(frame as i32, i32::from(self.step_rate_frames));
    }

    /// `FactoryClass::Suspend(1) @ 0x004C9E60`, the user's hold: refused when
    /// the factory is already stopped (`+0x70`, which completion sets too);
    /// otherwise the rate clears and the step timer restarts empty at `frame`.
    /// The progress, balance and a cash stall are kept.
    fn suspend(&mut self, frame: u32) -> bool {
        if self.object.is_none() || self.manual || self.suspended {
            return false;
        }
        self.manual = true;
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
        // (complete-held / paused), or a latched on_hold, or a manual pause.
        if self.object.is_none() || self.suspended || self.on_hold || self.manual {
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

        // Affordability PRE-CHECK (no spend on a stall, so the oracle's spent total
        // stays clean). Exactly-affordable (available == charge) PROCEEDS (strict <).
        if economy.available() < charge {
            self.progress -= 1; // rewind the tentative step (net-zero advance)
            self.on_hold = true; // UI "On Hold"
            return StepOutcome::Stalled; // nothing spent, balance unchanged
        }

        // Pay-as-you-go: spend exactly `charge`, decrement balance by the same.
        self.on_hold = false; // a successful step clears a prior hold
        let paid = economy.spend(charge);
        debug_assert_eq!(paid, charge, "an afforded charge must be paid in full");
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
    fn abandon_production(&mut self) -> Option<AbandonedObject> {
        let object = self.object.take()?;
        self.changed = true; //4CA07E; a null held object does not set it.
        let abandoned = AbandonedObject {
            type_id: object.type_id,
            balance: self.balance,
            entity_id: object.entity_id,
        };
        self.progress = 0;
        self.balance = 0;
        self.step_rate_frames = 0;
        self.step_timer = CdTimer::default();
        self.on_hold = false;
        self.suspended = false;
        self.manual = false;
        self.special = SpecialItem::NoneNeg1; // canonical "none"; do NOT collapse 0/-1
        // The queue is left intact; the caller promotes its front.
        Some(abandoned)
    }

    /// `FactoryClass::StartNextQueued @ 0x004CA5A0`: pop the queue's front into
    /// a fresh active object seeded from `cost` (the popped type's Cost_Of,
    /// resolved by the caller while it holds `&rules`). Returns the popped
    /// `type_id`, or `None` when an object is still held (in flight or
    /// finished) or the queue is empty. Like `FactoryClass::StartProduction
    /// @ 0x004C9C70`, the new build has no rate yet; the caller's
    /// [`Factory::start_rate`] arms it. The factory keeps its construction
    /// order.
    pub(crate) fn start_next_queued(&mut self, cost: i32) -> Option<InternedId> {
        if self.object.is_some() {
            return None;
        }
        let next = self.queue.pop_front()?;
        self.object = Some(PendingObject {
            type_id: next.type_id,
            entity_id: None,
            completion_accounted: false,
        });
        self.changed = true; // StartProduction4C9D6E, reached by StartNext.
        self.progress = 0;
        self.balance = seeded_balance(cost);
        self.step_rate_frames = 0;
        self.step_timer = CdTimer::default();
        self.suspended = false;
        self.on_hold = false;
        self.manual = false;
        Some(next.type_id)
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

/// `TechnoClass::Time_To_Build @ 0x006F47A0`: the frames an object takes to
/// build, before the build start (`0x004C9EA0`) divides it into 54 steps. Each stage
/// truncates (`_ftol`) under the process's 53-bit chop control word.
pub fn time_to_build(inputs: &TimeToBuildInputs) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering as Order};

    // TechnoType vt+0x88 (`0x00711EE0`): Cost x BuildSpeed x 0.9.
    let base = X87::ftol_i32_low_masked(X87::mul(
        X87::mul(
            X87::load_i32(inputs.cost),
            X87::load_f64(inputs.build_speed),
        ),
        X87::load_f64(BUILD_TIME_SCALE),
    ));
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

/// Resolve [`time_to_build`]'s inputs for `owner` building `obj` in `category`:
/// the rules and type fields, the owner's country multiplier (`0x0050C0A0`),
/// power (House `+0x53A4`/`+0x53A8`) and factory count (`0x00500910`).
pub(super) fn time_to_build_inputs(
    sim: &crate::sim::world::Simulation,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
    obj: &ObjectType,
) -> TimeToBuildInputs {
    let country_multiplier = sim.houses.get(&owner).map_or(NativeF32Bits::ONE, |house| {
        rules.country_build_time_mult_for_type(sim.interner.resolve(house.house_type_id()), obj)
    });
    let (power_output, power_drain) = sim
        .power_states
        .get(&owner)
        .map_or((0, 0), |power| (power.total_output, power.total_drain));
    let factory_count = crate::sim::production::production_tech::matching_factory_count_for_owner(
        &sim.substrate.entities,
        rules,
        sim.interner.resolve(owner),
        category,
        &sim.interner,
    );
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
        factory_count: i32::try_from(factory_count).unwrap_or(i32::MAX),
        multiple_factory: rules.production.multiple_factory,
        wall: obj.category == crate::rules::object_type::ObjectCategory::Building && obj.wall,
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
    /// The active object was abandoned; the lifecycle owner credits its
    /// refund and destroys it (`FactoryClass::AbandonProduction @ 0x004C9FF0`
    /// deletes the object stored at `Factory+0x58`). `finished` marks an
    /// object that had completed: a finished building also waits in
    /// `ready_by_owner`.
    AbandonedActive {
        object: AbandonedObject,
        finished: bool,
    },
}

/// Outcome of a `FactoryRegistry::enqueue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnqueueOutcome {
    /// A new active object was armed; its Techno still has to be constructed.
    Started,
    /// The build joined the queue behind the active object.
    Queued,
    /// The queue already holds `[General] MaximumQueuedObjects=` builds
    /// (`FactoryClass::StartProduction 0x004C9CD8..0x004C9CDE`).
    QueueFull,
}

/// Entity lifecycle work produced by prerequisite revalidation while the
/// registry is temporarily split from `Simulation`.
pub(crate) struct RevalidationLifecycle {
    /// Each abandoned object with its owner, in plan order, for its refund and
    /// destruction.
    pub(crate) abandoned: Vec<(InternedId, AbandonedObject)>,
    pub(crate) promoted: Vec<(InternedId, ProductionCategory, InternedId)>,
    /// `(owner, type)` of each abandoned object that had finished: a finished
    /// building also waits in `ready_by_owner`.
    pub(crate) abandoned_finished: Vec<(InternedId, InternedId)>,
}

/// Whether a revalidated build may stay: the `FindFactory(1,0,1)` check of
/// `HouseClass::Update_Factory_Queue @ 0x00509140`
/// (`production_tech::revalidate_eligibility`).
pub enum BuildEligibility {
    Buildable,
    PermanentlyBlocked,
}

/// Borrow-only read view of one factory, for the lifecycle and delivery code and
/// tests. Never mutates; never hashed. The user hold is not in it: the sidebar reads
/// that through `production_queue::queue_view_for_owner`.
pub struct FactoryView<'a> {
    pub progress: u16,
    pub on_hold: bool,
    pub suspended: bool,
    pub object: Option<&'a PendingObject>,
    pub queue: &'a VecDeque<QueueEntry>,
    /// `true` when the active object has reached `PRODUCTION_STEPS`.
    pub ready: bool,
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

/// One planned P6 revalidation disposition for a `(owner, category)` factory — the output of
/// the read/classify phase (`plan_revalidation`), consumed by the write phase
/// (`apply_revalidation`). Split so the classify can borrow `&Simulation` (build eligibility)
/// while the apply borrows `&mut houses` (refund) without aliasing. Fields are module-private;
/// the caller holds the plan opaquely.
pub(crate) struct RevalAction {
    owner: InternedId,
    category: ProductionCategory,
    /// The active in-progress object is permanently blocked -> abandon (partial refund).
    abandon_active: bool,
    /// Queued (tail) indices to remove (ascending; removed back-to-front in apply). No refund.
    drop_queued: Vec<usize>,
    /// Cost_Of of the first SURVIVING queued entry, when abandoning, so the apply can
    /// promote it.
    promote_cost: Option<i32>,
}

impl FactoryRegistry {
    /// Saved keys participate in factory identity; restoration must validate
    /// them against the embedded values before accepting world relationships.
    pub(super) fn keyed_factories(&self) -> impl Iterator<Item = (&FactoryHolder, &Factory)> {
        self.factories.iter()
    }

    /// Seed the queue kernel without constructing a world object. External
    /// fixtures explicitly finish construction when their scenario requires it.
    #[cfg(test)]
    pub(crate) fn test_enqueue_kernel(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        enqueue_order: u64,
        cost: i32,
    ) -> bool {
        self.enqueue(owner, category, type_id, enqueue_order, cost, i32::MAX)
            == EnqueueOutcome::Started
    }

    /// Apply the save/load swizzle result to the optional produced-object
    /// pointer. An unmatched saved identity becomes null; the Factory and its
    /// type/progress state remain intact.
    pub(crate) fn fixup_object_references(&mut self, object_ids: &BTreeSet<u64>) {
        for factory in self.factories.values_mut() {
            if let Some(object) = factory.object.as_mut()
                && object
                    .entity_id
                    .is_some_and(|object_id| !object_ids.contains(&object_id))
            {
                object.entity_id = None;
            }
        }
    }

    /// Read-only sidebar projection. Never mutates.
    pub fn view(&self, owner: InternedId, category: ProductionCategory) -> Option<FactoryView<'_>> {
        let f = self.factories.get(&FactoryHolder::House(owner, category))?;
        Some(FactoryView {
            progress: f.progress,
            on_hold: f.on_hold,
            suspended: f.suspended,
            object: f.object.as_ref(),
            queue: &f.queue,
            ready: f.progress >= PRODUCTION_STEPS,
        })
    }

    /// Number of registered factories (test/observation helper).
    pub fn len(&self) -> usize {
        self.factories.len()
    }

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
                && factory.step_rate_frames != 0
                && !factory.suspended
                && !factory.manual
        })
    }

    /// The factory `BuildingClass+0x524` holds for building `building`.
    pub(crate) fn building_factory(&self, building: u64) -> Option<&Factory> {
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

    /// `FactoryClass::AbandonProduction @ 0x004C9FF0`, then the delete, for
    /// building `building`'s factory: `0x0045022C`, `0x004502D1`,
    /// `BuildingClass::Detach_All(1)` (`0x0044EC0B`) and
    /// `BuildingClass::ChangeOwner` (`0x004486EB`). Returns the object to
    /// refund and destroy.
    pub(super) fn abandon_building_factory(&mut self, building: u64) -> Option<AbandonedObject> {
        self.factories
            .remove(&FactoryHolder::Building(building))?
            .abandon_production()
    }

    /// Test-only mutable access to the first factory in sweep order (the `factories`
    /// map is private). Used by the authoritative-hash + progress-persist tests, which
    /// live in the `world` module and so cannot reach the private map directly.
    #[cfg(test)]
    pub(crate) fn test_first_mut(&mut self) -> Option<&mut Factory> {
        self.factories.values_mut().min_by_key(|f| f.insertion_seq)
    }

    /// Test-only mutable access to a specific (owner, category) factory (the map is
    /// private). Lets the production-pipeline integration tests drive the registry
    /// (now the completion authority) without running the full charge sweep.
    #[cfg(test)]
    pub(crate) fn test_factory_mut(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> Option<&mut Factory> {
        self.factories
            .get_mut(&FactoryHolder::House(owner, category))
    }

    /// Test-only: force a (owner, category) factory to the completed-and-held state
    /// (progress == `PRODUCTION_STEPS`, balance drained, suspended) — the exact state
    /// `step_all` leaves on completion, so changed-output publication
    /// fires. Returns `false` when no such (object-holding) factory exists. Reconcile
    /// the registry first so the factory exists.
    #[cfg(test)]
    pub(crate) fn test_arm_ready(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> bool {
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
    /// already holds `max_queued` builds (`0x004C9CD5..0x004C9CE4`).
    ///
    /// A freshly-armed build has no rate yet; the caller constructs its Techno
    /// and [`Factory::start_rate`] arms it. `cost` is resolved by the caller
    /// (which holds `&rules`) so this stays `&sim`-free.
    pub(super) fn enqueue(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        enqueue_order: u64,
        cost: i32,
        max_queued: i32,
    ) -> EnqueueOutcome {
        let holder = FactoryHolder::House(owner, category);
        if let Some(f) = self.factories.get_mut(&holder) {
            if f.object.is_some() {
                if i32::try_from(f.queue.len()).unwrap_or(i32::MAX) >= max_queued {
                    return EnqueueOutcome::QueueFull;
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
            f.suspended = false;
            f.manual = false;
            f.special = SpecialItem::NoneNeg1;
            f.insertion_seq = enqueue_order;
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
                suspended: false,
                manual: false,
                special: SpecialItem::NoneNeg1,
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
    ) -> bool {
        self.factories
            .get_mut(&FactoryHolder::House(owner, category))
            .is_some_and(|f| f.suspend(frame))
    }

    /// Begin_Production's same-type branch (`0x004FA5A8..0x004FA5C4` skips
    /// StartProduction) for a user-held build: the build start `0x004C9EA0`
    /// at `0x004FA628` clears the hold and restarts the rate at `frame`.
    pub(super) fn resume(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        time_to_build: i32,
        frame: u32,
    ) {
        if let Some(f) = self
            .factories
            .get_mut(&FactoryHolder::House(owner, category))
            && f.manual
        {
            f.manual = false;
            f.start_rate(time_to_build, frame);
        }
    }

    /// The type and state of the category's active object: `(type, held,
    /// finished)` where `held` is the user's hold.
    pub(super) fn active_object(
        &self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> Option<(InternedId, bool, bool)> {
        let f = self.factories.get(&FactoryHolder::House(owner, category))?;
        let object = f.object.as_ref()?;
        Some((object.type_id, f.manual, f.progress >= PRODUCTION_STEPS))
    }

    /// Peek the type of the next QUEUED (tail) entry for `(owner, category)` so the caller
    /// can resolve its cost (in `rules`, not the registry) before promoting it.
    pub(crate) fn peek_next_queued(
        &self,
        owner: InternedId,
        category: ProductionCategory,
    ) -> Option<InternedId> {
        self.factories
            .get(&FactoryHolder::House(owner, category))
            .and_then(|f| f.queue.front())
            .map(|e| e.type_id)
    }

    /// Clear a delivered/abandoned active object and promote the next queued entry into the
    /// active slot (C7 StartNextQueued), seeding it from `next_cost`.
    /// Returns the promoted type. An empty tail releases the House factory
    /// synchronously: HouseAbandon4FAA10 clears the Infantry slot at4FAC21
    /// and deletes the factory at4FAC2D after Completed4CA1A0.
    /// Original final GI PLACE485 clears House and Strip before frame486;
    /// source: basic-factory-output-prerequisites-research, no-rally control.
    pub(super) fn clear_active_and_advance(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        next_cost: i32,
    ) -> Option<InternedId> {
        let holder = FactoryHolder::House(owner, category);
        let f = self.factories.get_mut(&holder)?;
        f.object = None;
        f.changed = true; // CompletedProduction4CA1C0 releases the held head.
        let promoted = f.start_next_queued(next_cost);
        if promoted.is_none() {
            self.factories.remove(&holder);
        }
        promoted
    }

    /// Arm the active build's rate and step timer at `frame`
    /// ([`Factory::start_rate`]).
    pub(super) fn start_rate(&mut self, holder: FactoryHolder, time_to_build: i32, frame: u32) {
        if let Some(f) = self.factories.get_mut(&holder) {
            f.start_rate(time_to_build, frame);
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
    /// retries observe `false`; `start_next_queued` constructs the next head with
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

    /// Drop EVERY idle House-slot factory (no active object AND empty queue) — the
    /// post-cancel / post-delivery sweep (replaces the `queues_by_owner.retain` prune). An
    /// idle factory should never persist into a hashed tick; this enforces it. A
    /// building's factory is deleted by its own operations (`production::factory_ai`).
    pub(crate) fn prune_all_idle(&mut self) {
        self.factories.retain(|holder, f| {
            matches!(holder, FactoryHolder::Building(_))
                || f.object.is_some()
                || !f.queue.is_empty()
        });
    }

    /// P6 read/classify phase: re-validate every factory's active + queued builds and plan
    /// the disposition (abandon active / drop queued) for those whose prerequisites or
    /// producing factory were lost. READ-ONLY over `Simulation`; returns an owned plan so the
    /// write phase can borrow `&mut houses` without aliasing. Walks `iter_insertion_ordered`
    /// (construction order = `step_all` charge order = hash fold order) so the plan — and
    /// the refund application order — is replay-stable.
    pub(super) fn plan_revalidation(
        &self,
        sim: &crate::sim::world::Simulation,
        rules: &RuleSet,
    ) -> Vec<RevalAction> {
        use crate::sim::production::production_tech::revalidate_eligibility;
        let mut plan: Vec<RevalAction> = Vec::new();
        for f in self.house_factories_insertion_ordered() {
            let owner_name = sim.interner.resolve(f.owner).to_string();
            let permanent = |type_id: InternedId| -> bool {
                matches!(
                    revalidate_eligibility(sim, rules, &owner_name, sim.interner.resolve(type_id)),
                    BuildEligibility::PermanentlyBlocked
                )
            };
            // Abandon an IN-PROGRESS active object when it is permanently blocked. A user
            // (manual) pause is NOT a guard here — gamemd abandons a paused build too on
            // permanent block. A finished object held for delivery goes only with its
            // factory: `BuildingClass::Detach_All(1) @ 0x0044EBF0`, run at a building's
            // kill, abandons the building's own factory (`0x0044EC01..0x0044EC21`) and, for
            // a Construction Yard, every factory whose object no other factory can build
            // (`0x0044EC2B..0x0044EEC8`), finished or not. VERA has no per-building
            // factory, so it abandons a finished object once no factory of its category
            // remains (see `Simulation::object_destroy_callback`'s residual).
            let abandon_active = f.object.as_ref().is_some_and(|o| {
                if f.progress < PRODUCTION_STEPS {
                    permanent(o.type_id)
                } else {
                    !crate::sim::production::production_tech::has_factory_for_owner(
                        &sim.substrate.entities,
                        rules,
                        &owner_name,
                        f.category,
                        &sim.interner,
                    )
                }
            });
            // Queued tail: collect permanently-blocked indices to drop + the first survivor
            // (the promote target after an abandon).
            let mut drop_queued: Vec<usize> = Vec::new();
            let mut first_surviving: Option<InternedId> = None;
            for (i, e) in f.queue.iter().enumerate() {
                if permanent(e.type_id) {
                    drop_queued.push(i);
                } else if first_surviving.is_none() {
                    first_surviving = Some(e.type_id);
                }
            }
            if !abandon_active && drop_queued.is_empty() {
                continue; // nothing to dispose for this factory
            }
            let promote_cost = if abandon_active {
                first_surviving
                    .and_then(|t| sim.object_type(t, rules))
                    .map(|object| sim.cost_of(f.owner, object, rules))
            } else {
                None
            };
            plan.push(RevalAction {
                owner: f.owner,
                category: f.category,
                abandon_active,
                drop_queued,
                promote_cost,
            });
        }
        plan
    }

    /// P6 write/apply phase: apply a `plan_revalidation` plan. Drops permanently-blocked
    /// queued entries (no refund — never charged), abandons a permanently-blocked active
    /// build (the caller credits its refund and destroys it), then promotes the first
    /// surviving queued entry (C7 StartNextQueued, cost-seeded; the caller arms its
    /// rate). Idle factories are pruned.
    pub(super) fn apply_revalidation(&mut self, plan: &[RevalAction]) -> RevalidationLifecycle {
        let mut lifecycle = RevalidationLifecycle {
            abandoned: Vec::new(),
            promoted: Vec::new(),
            abandoned_finished: Vec::new(),
        };
        for action in plan {
            let Some(f) = self
                .factories
                .get_mut(&FactoryHolder::House(action.owner, action.category))
            else {
                continue;
            };
            // Remove dropped queued entries back-to-front so the ascending indices stay valid.
            for &i in action.drop_queued.iter().rev() {
                if i < f.queue.len() {
                    f.queue.remove(i);
                }
            }
            if action.abandon_active {
                if f.progress >= PRODUCTION_STEPS
                    && let Some(object) = f.object.as_ref()
                {
                    lifecycle
                        .abandoned_finished
                        .push((action.owner, object.type_id));
                }
                if let Some(abandoned) = f.abandon_production() {
                    lifecycle.abandoned.push((action.owner, abandoned));
                }
                if let Some(cost) = action.promote_cost
                    && let Some(type_id) = f.start_next_queued(cost)
                {
                    lifecycle
                        .promoted
                        .push((action.owner, action.category, type_id));
                }
            }
        }
        self.prune_all_idle();
        lifecycle
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
                    time_to_build(&time_to_build_inputs(
                        sim,
                        rules,
                        owner,
                        factory.category,
                        object,
                    ))
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
    pub(super) fn step_all(
        &mut self,
        houses: &mut BTreeMap<InternedId, crate::sim::house_state::HouseState>,
        frame: u32,
    ) {
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
            // Only an armed, in-flight build steps: object held, not complete (held for
            // delivery), not suspended, not manually paused.
            if f.object.is_none() || f.suspended || f.manual || f.progress >= PRODUCTION_STEPS {
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
            let outcome = f.advance_one_step(&mut house.economy);

            // Completion clears the rate and restarts the timer empty
            // (`0x004C9C0C..0x004C9C25`); the object is held for delivery.
            if matches!(outcome, StepOutcome::Completed) {
                f.step_rate_frames = 0;
                f.step_timer.start(frame as i32, 0);
            }
        }
    }

    /// The ABANDON / ABANDON_ALL events for `type_id` in (owner, category):
    /// `HouseClass::Abandon_Production @ 0x004FAA10`. A single abandon removes
    /// the first queued copy and stops there (`0x004FAAEE`, `0x004FAB29`); with
    /// no queued copy, or for ABANDON_ALL once every copy is gone
    /// (`0x004FAB01..0x004FAB0B`), the active object goes if it is this type,
    /// finished or not (`0x004FAB3D..0x004FAB5E`), for the lifecycle owner to
    /// refund (`0x004FABA6`). The caller promotes the next queued build.
    pub(super) fn cancel_one(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        all: bool,
    ) -> CancelOutcome {
        let Some(f) = self
            .factories
            .get_mut(&FactoryHolder::House(owner, category))
        else {
            return CancelOutcome::NoMatch;
        };
        // `FactoryClass::Remove_First @ 0x004CA620`: the first match, the
        // survivors keeping their order.
        let mut removed = false;
        while let Some(idx) = f.queue.iter().position(|e| e.type_id == type_id) {
            f.queue.remove(idx);
            removed = true;
            if !all {
                return CancelOutcome::QueuedRemoved;
            }
        }
        if f.object.as_ref().is_some_and(|o| o.type_id == type_id) {
            let finished = f.progress >= PRODUCTION_STEPS;
            if let Some(object) = f.abandon_production() {
                return CancelOutcome::AbandonedActive { object, finished };
            }
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
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
pub(super) fn native_factory_cadence() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/factory_cadence.json",
    ))
    .expect("cadence oracle")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- P2 pure-type tests ----

    #[test]
    fn special_item_none_zero_and_neg1_distinct() {
        // The 0/-1 collapse the study forbids: the three states must compare unequal.
        assert_ne!(SpecialItem::NoneNeg1, SpecialItem::NoneZero);
        assert_ne!(SpecialItem::NoneNeg1, SpecialItem::Item(0));
        assert_ne!(SpecialItem::NoneZero, SpecialItem::Item(0));
        assert_eq!(SpecialItem::default(), SpecialItem::NoneNeg1);
    }

    #[test]
    fn factory_default_progress_zero_no_object() {
        let f = Factory::default();
        assert_eq!(f.progress, 0);
        assert!(f.object.is_none());
        assert_eq!(f.step_rate_frames, 0);
    }

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
            let mut f = Factory::default();
            f.start_rate(native, start_frame);
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
        use crate::sim::components::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::house_state::HouseState;
        use crate::sim::power_system::PowerState;
        use crate::sim::world::Simulation;

        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_power_consumers.json",
        ))
        .unwrap();
        assert_eq!(
            corpus["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
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
            sim.houses.insert(
                owner,
                HouseState::new(owner, 0, Some(owner), true, 5000, 10),
            );
            let producer_type = sim.interner.intern("PRODUCER");
            let mut producer = GameEntity::new_at_frame_zero_for_test(
                1,
                5,
                5,
                0,
                0,
                owner,
                Health { current: 500 },
                producer_type,
                EntityCategory::Structure,
                0,
                5,
                false,
            );
            producer.lifecycle.in_limbo = false;
            sim.substrate.entities.insert(producer);
            let mut power = PowerState::default();
            power.total_output = input["power_output"].as_i64().unwrap() as i32;
            power.total_drain = input["power_drain"].as_i64().unwrap() as i32;
            sim.power_states.insert(owner, power);
            let type_id = sim.interner.intern("PENDING");
            let inputs = time_to_build_inputs(
                &sim,
                &rules,
                owner,
                ProductionCategory::Vehicle,
                rules.object("PENDING").unwrap(),
            );
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
                        manual: status == "held",
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

    #[test]
    fn no_object_factory_does_not_step() {
        let mut f = Factory {
            suspended: true,
            ..Factory::default()
        };
        assert!(matches!(
            f.advance_one_step(&mut Economy::default()),
            StepOutcome::Idle
        ));
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
            let mut reg = reg_with(owner, category, armed_factory(inputs.cost));
            reg.start_rate(
                FactoryHolder::House(owner, category),
                time_to_build(&inputs),
                start_frame,
            );
            let mut houses = BTreeMap::from([(
                owner,
                crate::sim::house_state::HouseState::new(
                    owner,
                    0,
                    None,
                    true,
                    int(&row["credits"]),
                    10,
                ),
            )]);
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
                let economy = &mut houses.get_mut(&owner).unwrap().economy;
                economy.credits += deposits.get(&frame).copied().unwrap_or(0);
                // A SUSPEND is `Suspend(1)`; a resume is Begin_Production's
                // same-type build start, which VERA takes only for a user hold.
                for &(_, kind) in orders.iter().filter(|(at, _)| *at == frame) {
                    let accepted = if kind == "suspend" {
                        reg.suspend(owner, category, frame)
                    } else {
                        let held = reg.factories[&FactoryHolder::House(owner, category)].manual;
                        reg.resume(owner, category, time_to_build(&inputs), frame);
                        held
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
                        f.manual || f.suspended,
                    ]));
                }
                let before = reg.factories[&FactoryHolder::House(owner, category)]
                    .step_timer
                    .start_frame();
                reg.step_all(&mut houses, frame);
                let f = &reg.factories[&FactoryHolder::House(owner, category)];
                if f.step_timer.start_frame() != before {
                    let credits = houses[&owner].economy.credits;
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
            let economy = &houses[&owner].economy;
            let end = &row["final"];
            assert_eq!(
                (
                    i32::from(f.progress),
                    i32::from(f.step_rate_frames),
                    f.step_timer,
                    f.balance,
                    f.on_hold,
                    f.suspended,
                    economy.credits,
                    economy.spent_credits,
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
    fn factory_54_steps_to_complete() {
        // From a fresh armed start with funds: 53 `Stepped` then 1 `Completed` (C2);
        // progress reaches 54 (E1: the 54th call is Completed, not a plain Stepped).
        let mut f = armed_factory(700);
        let mut econ = Economy {
            credits: 700,
            ..Economy::default()
        };
        let mut stepped = 0;
        let mut completed = 0;
        for _ in 0..PRODUCTION_STEPS {
            match f.advance_one_step(&mut econ) {
                StepOutcome::Stepped => stepped += 1,
                StepOutcome::Completed => completed += 1,
                other => panic!("unexpected outcome {other:?} before completion"),
            }
        }
        assert_eq!(stepped, 53, "exactly 53 Stepped before the final Completed");
        assert_eq!(completed, 1, "exactly one Completed at step 54");
        assert_eq!(f.progress, PRODUCTION_STEPS, "progress reaches 54");
        assert!(
            f.suspended && f.object.is_some(),
            "complete-but-not-delivered"
        );
        assert!(
            matches!(f.advance_one_step(&mut econ), StepOutcome::Idle),
            "a settled factory is Idle"
        );
    }

    #[test]
    fn factory_exact_cost_conservation() {
        // Sum of oracle spend over a full build == the full type cost; balance ends 0
        // (C3/C15). Boundary set {1, 25, 700, 99991}.
        for cost in [1i32, 25, 700, 99991] {
            let mut f = armed_factory(cost);
            let mut econ = Economy {
                credits: cost,
                ..Economy::default()
            };
            loop {
                match f.advance_one_step(&mut econ) {
                    StepOutcome::Stepped => {}
                    StepOutcome::Completed => break,
                    other => panic!("cost {cost}: unexpected {other:?} with exact funds"),
                }
            }
            assert_eq!(
                econ.spent_credits, cost,
                "cost {cost}: total spent == full cost"
            );
            assert_eq!(econ.credits, 0, "cost {cost}: oracle drained to exactly 0");
            assert_eq!(f.balance, 0, "cost {cost}: balance ends 0");
        }
    }

    #[test]
    fn factory_exact_cost_conservation_cost1_corner() {
        // The cost-1 corner: 1/k == 0 for k>=2 (the value-1..52 steps charge 0); the
        // lone credit is charged on the steps_left==1 step (value 53, 1/1==1, a
        // Stepped); the final value-54 step charges 0. Conservation depends on the
        // /1 step, not the guard step.
        let mut f = armed_factory(1);
        let mut econ = Economy {
            credits: 1,
            ..Economy::default()
        };
        let mut total = 0;
        loop {
            let before = econ.spent_credits;
            match f.advance_one_step(&mut econ) {
                StepOutcome::Stepped => total += econ.spent_credits - before,
                StepOutcome::Completed => {
                    total += econ.spent_credits - before;
                    break;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(
            total, 1,
            "the single credit is charged exactly once across the build"
        );
        assert_eq!(f.balance, 0);
    }

    #[test]
    fn factory_steps_left_one_charges_full_remainder() {
        // The balance drains on the steps_left==1 step (value 53, charge=balance/1),
        // NOT the final value-54 step (the div-by-zero guard, which charges 0).
        let mut f = armed_factory(700);
        let mut econ = Economy {
            credits: 700,
            ..Economy::default()
        };
        while f.progress < PRODUCTION_STEPS - 2 {
            assert!(matches!(
                f.advance_one_step(&mut econ),
                StepOutcome::Stepped
            ));
        }
        assert_eq!(
            f.progress,
            PRODUCTION_STEPS - 2,
            "stopped two before completion (progress 52)"
        );
        let remainder = f.balance;
        assert!(remainder > 0, "balance is nonzero at progress 52");
        // The steps_left==1 step (value 53) charges the WHOLE remainder, once.
        let spent_before = econ.spent_credits;
        assert!(
            matches!(f.advance_one_step(&mut econ), StepOutcome::Stepped),
            "value-53 is a Stepped"
        );
        assert_eq!(
            econ.spent_credits - spent_before,
            remainder,
            "drains the whole remainder once"
        );
        assert_eq!(f.balance, 0, "balance zeroed on the steps_left==1 step");
        // The final value-54 step is the div-by-zero guard: charges 0, Completed.
        let spent_before2 = econ.spent_credits;
        assert!(matches!(
            f.advance_one_step(&mut econ),
            StepOutcome::Completed
        ));
        assert_eq!(
            econ.spent_credits - spent_before2,
            0,
            "the final step charges 0 (guard)"
        );
        assert_eq!(
            f.balance, 0,
            "completion leaves balance 0 (no second remainder charge)"
        );
    }

    #[test]
    fn factory_stall_on_no_funds_rewinds() {
        // Oracle one credit below the first step's charge -> Stalled: on_hold set,
        // progress unchanged, NOTHING spent (C4). cost 700 -> first charge 700/53 = 13.
        let mut f = armed_factory(700);
        let first_charge = 700 / (PRODUCTION_STEPS as i32 - 1); // 700/53 = 13
        let mut econ = Economy {
            credits: first_charge - 1,
            ..Economy::default()
        };
        assert!(matches!(
            f.advance_one_step(&mut econ),
            StepOutcome::Stalled
        ));
        assert!(f.on_hold, "a shortfall latches on_hold");
        assert_eq!(
            f.progress, 0,
            "the tentative step is rewound (net-zero advance)"
        );
        assert_eq!(econ.spent_credits, 0, "a stall spends nothing");
        assert_eq!(
            econ.credits,
            first_charge - 1,
            "the oracle wallet is untouched"
        );
    }

    #[test]
    fn factory_exactly_affordable_step_proceeds() {
        // available == charge PROCEEDS (the strict-< boundary).
        let mut f = armed_factory(700);
        let first_charge = 700 / (PRODUCTION_STEPS as i32 - 1); // 13
        let mut econ = Economy {
            credits: first_charge,
            ..Economy::default()
        };
        assert!(matches!(
            f.advance_one_step(&mut econ),
            StepOutcome::Stepped
        ));
        assert_eq!(f.progress, 1);
        assert_eq!(econ.spent_credits, first_charge);
    }

    #[test]
    fn factory_cost_zero_completes_free() {
        // A cost-0 type: every charge is 0, completes with zero spend; conservation
        // holds trivially (sum 0 == Balance 0).
        let mut f = armed_factory(0);
        let mut econ = Economy::default(); // 0 credits, but every charge is 0
        let mut steps = 0;
        loop {
            match f.advance_one_step(&mut econ) {
                StepOutcome::Stepped => steps += 1,
                StepOutcome::Completed => {
                    steps += 1;
                    break;
                }
                other => panic!("unexpected {other:?} for a free build"),
            }
        }
        assert_eq!(
            steps, PRODUCTION_STEPS as i32,
            "a free build still takes 54 steps"
        );
        assert_eq!(econ.spent_credits, 0, "free build spends nothing");
        assert_eq!(f.balance, 0);
    }

    #[test]
    fn remaining_balance_ladder_matches_stepper() {
        // remaining_balance_after must equal the balance the stepper actually holds.
        for cost in [1i32, 25, 700, 99991] {
            let mut f = armed_factory(cost);
            let mut econ = Economy {
                credits: cost,
                ..Economy::default()
            };
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

    #[test]
    fn cost25_ladder_sums_to_exactly_25() {
        // floor division never loses/gains a credit: the last charging step takes the
        // whole remainder, so the per-step charges sum to exactly the cost.
        let mut f = armed_factory(25);
        let mut econ = Economy {
            credits: 25,
            ..Economy::default()
        };
        loop {
            if matches!(f.advance_one_step(&mut econ), StepOutcome::Completed) {
                break;
            }
        }
        assert_eq!(econ.spent_credits, 25);
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

    /// AbandonProduction hands back the Balance still owed and leaves the wallet to
    /// the lifecycle refund: with an unchanged Cost_Of, `Cost_Of - Balance` is
    /// exactly what the steps paid.
    #[test]
    fn abandon_reports_the_unpaid_balance() {
        let mut f = armed_factory(700);
        let mut econ = Economy {
            credits: 700,
            ..Economy::default()
        };
        while f.progress < 20 {
            assert!(matches!(
                f.advance_one_step(&mut econ),
                StepOutcome::Stepped
            ));
        }
        let balance = f.balance;
        let abandoned = f.abandon_production().expect("active build is abandonable");
        assert_eq!(
            abandoned,
            AbandonedObject {
                type_id: InternedId::default(),
                balance,
                entity_id: None,
            }
        );
        assert_eq!(700 - abandoned.balance, econ.spent_credits);
        assert_eq!(
            econ.credits,
            700 - econ.spent_credits,
            "cancel pays nothing"
        );
        assert!(f.object.is_none(), "the partial object is destroyed");
        assert_eq!(f.progress, 0);
        assert_eq!(f.balance, 0);
        assert_eq!(f.step_rate_frames, 0, "no-object => rate-0 sentinel");
        assert!(!f.suspended && !f.on_hold && !f.manual);
    }

    #[test]
    fn abandon_at_progress_zero_owes_the_whole_cost() {
        // A never-stepped build still owes its whole Balance, so the refund is 0.
        let mut f = armed_factory(700);
        assert_eq!(
            f.abandon_production().map(|abandoned| abandoned.balance),
            Some(700)
        );
        assert!(
            f.object.is_none(),
            "factory reset even on a zero-refund cancel"
        );
        assert_eq!(f.progress, 0);
    }

    #[test]
    fn abandon_without_an_object_is_a_noop() {
        assert_eq!(Factory::default().abandon_production(), None);
    }

    #[test]
    fn abandon_of_a_finished_object_refunds_its_whole_cost() {
        // `FactoryClass::AbandonProduction @ 0x004C9FF0` deletes the object finished or
        // not (`0x004CA0FC`) and refunds Cost - Balance, the whole cost once finished.
        let mut f = armed_factory(700);
        let mut econ = Economy {
            credits: 700,
            ..Economy::default()
        };
        loop {
            if matches!(f.advance_one_step(&mut econ), StepOutcome::Completed) {
                break;
            }
        }
        assert_eq!(f.progress, PRODUCTION_STEPS);
        assert!(f.suspended && f.object.is_some(), "completed-but-held");
        assert_eq!(
            f.abandon_production().map(|abandoned| abandoned.balance),
            Some(0),
            "nothing is owed, so the refund is the whole Cost_Of"
        );
        assert!(f.object.is_none(), "the finished object is destroyed");
        assert!(!f.suspended);
    }

    #[test]
    fn abandon_round_trip_conserves() {
        // C15 cancel-side telescoping: stepping k times then refunding
        // `cost - Balance` returns the wallet to its start wherever the cancel lands.
        for cost in [1i32, 25, 700, 99991] {
            for stop_at in [0u16, 1, 20, 53, PRODUCTION_STEPS] {
                let mut f = armed_factory(cost);
                let mut econ = Economy {
                    credits: cost,
                    ..Economy::default()
                };
                while f.progress < stop_at {
                    if matches!(f.advance_one_step(&mut econ), StepOutcome::Completed) {
                        break;
                    }
                }
                if let Some(abandoned) = f.abandon_production() {
                    assert_eq!(
                        econ.credits + cost - abandoned.balance,
                        cost,
                        "cost {cost} stop {stop_at}: the refund returns the wallet to start"
                    );
                }
            }
        }
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
        let outcome = reg.cancel_one(owner, ProductionCategory::Vehicle, a, false);
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
        let outcome = reg.cancel_one(owner, ProductionCategory::Vehicle, a, false);
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

    #[test]
    fn cancel_one_active_when_no_queued_copy() {
        // active = A (mid-build), tail = [B]; cancel A abandons the ACTIVE (no queued A).
        let owner = InternedId::default();
        let a = InternedId::from_index(1);
        let b = InternedId::from_index(2);
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
            queue: std::collections::VecDeque::from(vec![qe(b)]),
            ..Factory::default()
        };
        let mut reg = reg_with(owner, ProductionCategory::Vehicle, f);
        let outcome = reg.cancel_one(owner, ProductionCategory::Vehicle, a, false);
        assert_eq!(
            outcome,
            CancelOutcome::AbandonedActive {
                object: AbandonedObject {
                    type_id: a,
                    balance: 300,
                    entity_id: None,
                },
                finished: false,
            }
        );
        let view = reg.view(owner, ProductionCategory::Vehicle).unwrap();
        assert!(view.object.is_none(), "active object abandoned");
        let q: Vec<InternedId> = view.queue.iter().map(|e| e.type_id).collect();
        assert_eq!(
            q,
            vec![b],
            "the tail is left intact (no auto-advance in P4)"
        );
    }

    #[test]
    fn cancel_one_abandons_a_finished_active_object() {
        // active object completed-but-held (progress 54, suspended), no queued copy:
        // Abandon_Production's active check has no stage test (`0x004FAB3D..0x004FAB5E`),
        // so the finished object goes with its whole cost refunded.
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
            progress: PRODUCTION_STEPS,
            suspended: true,
            balance: 0,
            ..Factory::default()
        };
        let mut reg = reg_with(owner, ProductionCategory::Vehicle, f);
        let outcome = reg.cancel_one(owner, ProductionCategory::Vehicle, a, false);
        assert_eq!(
            outcome,
            CancelOutcome::AbandonedActive {
                object: AbandonedObject {
                    type_id: a,
                    balance: 0,
                    entity_id: None,
                },
                finished: true,
            },
            "nothing is owed, so the refund is the whole Cost_Of"
        );
        let view = reg.view(owner, ProductionCategory::Vehicle).unwrap();
        assert!(view.object.is_none(), "the finished object is destroyed");
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
            reg.cancel_one(owner, ProductionCategory::Infantry, a, true),
            CancelOutcome::AbandonedActive {
                object: AbandonedObject {
                    type_id: a,
                    balance: 150,
                    entity_id: None,
                },
                finished: false,
            }
        );
        assert_eq!(queued(&reg), vec![b], "every queued copy is gone");
        assert!(
            reg.view(owner, ProductionCategory::Infantry)
                .unwrap()
                .object
                .is_none()
        );

        let mut reg = reg_with(owner, ProductionCategory::Infantry, factory(b));
        assert_eq!(
            reg.cancel_one(owner, ProductionCategory::Infantry, a, true),
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

    /// `FactoryClass::StartProduction` refuses an append once the queue holds
    /// `MaximumQueuedObjects` builds (`0x004C9CDE`); the active build is not
    /// counted.
    #[test]
    fn enqueue_refuses_an_append_at_the_queue_cap() {
        let owner = InternedId::default();
        let a = InternedId::from_index(1);
        let category = ProductionCategory::Infantry;
        let mut reg = FactoryRegistry::default();
        assert_eq!(
            reg.enqueue(owner, category, a, 1, 100, 2),
            EnqueueOutcome::Started
        );
        assert_eq!(
            reg.enqueue(owner, category, a, 2, 100, 2),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            reg.enqueue(owner, category, a, 3, 100, 2),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            reg.enqueue(owner, category, a, 4, 100, 2),
            EnqueueOutcome::QueueFull
        );
        assert_eq!(reg.view(owner, category).unwrap().queue.len(), 2);
    }

    #[test]
    fn cancel_one_no_match_is_noop() {
        // (1) no factory for the key -> NoMatch; (2) type absent from active+tail -> NoMatch.
        let owner = InternedId::default();
        let a = InternedId::from_index(1);
        let z = InternedId::from_index(9);
        let mut empty = FactoryRegistry::default();
        assert_eq!(
            empty.cancel_one(owner, ProductionCategory::Vehicle, a, false),
            CancelOutcome::NoMatch,
            "no factory -> NoMatch"
        );
        let f = Factory {
            owner,
            category: ProductionCategory::Vehicle,
            object: Some(PendingObject {
                type_id: a,
                entity_id: None,
                completion_accounted: false,
            }),
            balance: 300,
            queue: std::collections::VecDeque::from(vec![qe(a)]),
            ..Factory::default()
        };
        let mut reg = reg_with(owner, ProductionCategory::Vehicle, f);
        assert_eq!(
            reg.cancel_one(owner, ProductionCategory::Vehicle, z, false),
            CancelOutcome::NoMatch,
            "type absent -> NoMatch"
        );
    }

    #[test]
    fn start_next_queued_pops_front() {
        // queue [X,Y,Z], no active object -> active = X, queue [Y,Z] (FIFO front pop).
        let x = InternedId::from_index(1);
        let y = InternedId::from_index(2);
        let z = InternedId::from_index(3);
        let mut f = Factory {
            object: None,
            queue: std::collections::VecDeque::from(vec![qe(x), qe(y), qe(z)]),
            ..Factory::default()
        };
        assert_eq!(f.start_next_queued(0), Some(x), "the FRONT is popped");
        assert_eq!(f.object.as_ref().map(|o| o.type_id), Some(x), "active = X");
        assert_eq!(f.progress, 0, "fresh active object starts at progress 0");
        let q: Vec<InternedId> = f.queue.iter().map(|e| e.type_id).collect();
        assert_eq!(q, vec![y, z], "queue advanced to [Y,Z]");
    }

    #[test]
    fn start_next_queued_blocked_while_object_held() {
        // object Some -> None, queue unchanged (the "Object null required" guard).
        let x = InternedId::from_index(1);
        let mut f = Factory {
            object: Some(PendingObject::default()),
            queue: std::collections::VecDeque::from(vec![qe(x)]),
            progress: 30,
            ..Factory::default()
        };
        assert_eq!(
            f.start_next_queued(0),
            None,
            "a held object blocks the advance"
        );
        let q: Vec<InternedId> = f.queue.iter().map(|e| e.type_id).collect();
        assert_eq!(q, vec![x], "queue unchanged while blocked");
        assert_eq!(f.progress, 30, "the held object's progress is untouched");
    }

    #[test]
    fn start_next_queued_empty_queue_is_noop() {
        let mut f = Factory::default();
        assert_eq!(f.start_next_queued(0), None);
        assert!(f.object.is_none(), "no object created from an empty queue");
    }

    /// A promoted queue entry seeds `balance == cost` and resets progress; the
    /// factory keeps its construction order (gamemd's Factories vector never
    /// reorders, `0x0055B66A`). Like StartProduction it leaves the rate to the
    /// build start.
    #[test]
    fn start_next_queued_keeps_the_construction_order_and_seeds_the_balance() {
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
        let popped = f.start_next_queued(500);
        assert_eq!(popped, Some(x));
        assert_eq!(f.insertion_seq, 7, "the construction order stays");
        assert_eq!(f.balance, 500);
        assert_eq!(f.progress, 0);
        assert_eq!(f.step_rate_frames, 0, "the build start arms the new build");
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
            reg.enqueue(owner, vehicle, tank, 0, 700, 5),
            EnqueueOutcome::Started
        );
        assert_eq!(
            reg.enqueue(owner, infantry, soldier, 1, 200, 5),
            EnqueueOutcome::Started
        );
        assert_eq!(
            reg.enqueue(owner, vehicle, tank, 2, 700, 5),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            reg.clear_active_and_advance(owner, vehicle, 700),
            Some(tank)
        );
        reg.start_rate(FactoryHolder::House(owner, vehicle), 540, 100);
        reg.start_rate(FactoryHolder::House(owner, infantry), 540, 100);
        // The first steps charge 700 / 53 = 13 and 200 / 53 = 3; the house has 13.
        let mut houses = BTreeMap::from([(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 13, 10),
        )]);
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
        assert_eq!(houses[&owner].economy.credits, 0);
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
}
