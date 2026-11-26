//! Decision handlers (RFC 0001 section 6f, appendix 1.2 `on_decide`):
//! [`crate::Trigger::decide_sync`] makes a decision trigger, and
//! [`crate::on_decide!`] calls the handler whose key matches each
//! `DecideRecord`, in place, synchronously at the engine's decision point.
//!
//! A handler may read (`get`, `find`, `var_get`, `random`, ...) but not
//! write: a write stops the module. Its `random` draws from a substream, so
//! decisions never move the module's main stream.
//!
//! ```ignore
//! use vera20k_sdk::prelude::*;
//!
//! const HALVE: i32 = 1;
//!
//! fn start() {
//!     let _ = Trigger::on(Event::TAKE_DAMAGE).bind(some_unit).decide_sync(HALVE);
//! }
//!
//! fn halve(mut d: TakeDamageDecision) {
//!     d.set_result(d.previous() / 2);
//! }
//!
//! vera20k_sdk::on_scenario_start!(start);
//! vera20k_sdk::on_decide!(HALVE => halve);
//! ```

use crate::{Cell, EventRecord, House, Mission, NameKind, Projectile, Ref, Techno, TypeRef, sys};

/// One engine decision and its uniquely borrowed result area, valid during
/// the `on_decide` call.
pub struct DecideRecord<'a> {
    raw: &'a mut sys::DecideRecord,
    result_bytes: &'a mut [u8],
}

impl core::ops::Deref for DecideRecord<'_> {
    type Target = sys::DecideRecord;

    fn deref(&self) -> &Self::Target {
        self.raw
    }
}

impl core::ops::DerefMut for DecideRecord<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.raw
    }
}

impl<'a> DecideRecord<'a> {
    /// The records `on_decide(ptr, count)` received.
    ///
    /// # Safety
    /// The caller must use the engine's original arguments and records, all
    /// initialized, aligned and exclusively accessible, with disjoint result
    /// areas. The iterator and its items must not outlive this callback.
    pub unsafe fn batch(ptr: i32, count: i32) -> impl Iterator<Item = Self> {
        // SAFETY: the engine wrote the records in its aligned delivery area.
        let records = unsafe {
            core::slice::from_raw_parts_mut(ptr as usize as *mut sys::DecideRecord, count as usize)
        };
        records.iter_mut().map(|raw| {
            let result_bytes = if raw.area_cap > 0 {
                // SAFETY: each initialized area is exclusive to this record
                // throughout the callback. Borrow it before exposing the record.
                unsafe {
                    core::slice::from_raw_parts_mut(
                        raw.area as usize as *mut u8,
                        raw.area_cap as usize,
                    )
                }
            } else {
                &mut []
            };
            Self { raw, result_bytes }
        })
    }

    fn reborrow(&mut self) -> DecideRecord<'_> {
        DecideRecord {
            raw: self.raw,
            result_bytes: self.result_bytes,
        }
    }

    /// The event the request is about.
    pub fn record(&self) -> &EventRecord {
        &self.event
    }

    /// The engine's own result.
    pub fn native(&self) -> i64 {
        self.native
    }

    /// The previous link's result (the native one for the first link).
    pub fn previous(&self) -> i64 {
        self.previous
    }

    /// Answers `value` (`result`, with `handled` set). Without an answer
    /// the previous result stands.
    pub fn set_result(&mut self, value: i64) {
        self.result = value;
        self.handled = 1;
    }

    /// The result area (`area`, `area_cap` bytes), empty for a kind
    /// without one. It holds the previous link's result in its first
    /// `previous_len` bytes, the rest zeroed.
    pub fn area(&mut self) -> &mut [u8] {
        self.result_bytes
    }
}

/// Copies `value`'s bytes to `area` at `at`; false when they do not fit.
fn put<T: Copy>(area: &mut [u8], at: usize, value: &T) -> bool {
    let size = size_of::<T>();
    let Some(out) = area.get_mut(at..at + size) else {
        return false;
    };
    // SAFETY: `T` is a plain `repr(C)` layout of integers and bytes.
    let bytes = unsafe { core::slice::from_raw_parts(value as *const T as *const u8, size) };
    out.copy_from_slice(bytes);
    true
}

/// Reads a `T` from `area` at `at`.
fn take<T: Copy + Default>(area: &[u8], at: usize) -> Option<T> {
    let bytes = area.get(at..at + size_of::<T>())?;
    let mut value = T::default();
    // SAFETY: as in `put`; any bit pattern is a valid `T`.
    unsafe {
        core::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            &mut value as *mut T as *mut u8,
            size_of::<T>(),
        )
    };
    Some(value)
}

macro_rules! decision {
    ($(#[$doc:meta])* $name:ident { $($(#[$fdoc:meta])* $field:ident: $ty:ty = |$r:ident| $read:expr),* $(,)? }) => {
        $(#[$doc])*
        pub struct $name<'a>(pub DecideRecord<'a>);

        impl<'a> From<&'a mut DecideRecord<'_>> for $name<'a> {
            fn from(record: &'a mut DecideRecord<'_>) -> Self {
                Self(record.reborrow())
            }
        }

        impl $name<'_> {
            $(
                $(#[$fdoc])*
                pub fn $field(&self) -> $ty {
                    let $r = &self.0.event;
                    $read
                }
            )*

            pub fn native(&self) -> i64 {
                self.0.native
            }

            pub fn previous(&self) -> i64 {
                self.0.previous
            }

            /// Answers `value`; see [`DecideRecord::set_result`].
            pub fn set_result(&mut self, value: i64) {
                self.0.set_result(value)
            }

            /// The whole request.
            pub fn request(&self) -> &DecideRecord<'_> {
                &self.0
            }
        }
    };
}

decision!(
    /// `TakeDamage`'s decision (appendix 4.3): the result is the final
    /// damage, before it is written (0 blocks the hit).
    TakeDamageDecision {
        victim: Techno = |r| Techno(r.subject),
        /// The source; `NO_HANDLE` without one.
        attacker: Techno = |r| Techno(r.other),
        /// The native damage after armor.
        damage: i64 = |r| r.value,
        /// Warhead ID, −1 when unknown.
        warhead: i32 = |r| r.id,
        /// The victim's position in leptons.
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

decision!(
    /// `ProductionAllowed` (appendix 4.5): 1 allows, 0 disallows the type
    /// to the house until the next frame's request; the sidebar and the AI
    /// read that answer. `native` is the engine's own answer (1 when the
    /// house can build it now); 1 does not lift the engine's refusal.
    ProductionAllowedDecision {
        house: House = |r| House(r.house),
        /// The TechnoType asked about.
        techno_type: TypeRef = |r| TypeRef::new(NameKind::TechnoType, r.id),
    }
);

decision!(
    /// `ProductionQueued` (appendix 4.5): 1 allows, 0 refuses the type.
    ProductionQueuedDecision {
        house: House = |r| House(r.house),
        /// The TechnoType being queued.
        techno_type: TypeRef = |r| TypeRef::new(NameKind::TechnoType, r.id),
    }
);

decision!(
    /// `Defeat` (appendix 4.5): 1 defeats the house, 0 vetoes this check.
    DefeatDecision {
        house: House = |r| House(r.house),
    }
);

decision!(
    /// `CommandAllowed` (appendix 4.5): 1 allows, 0 refuses a player
    /// command (a module order included) for the bound unit; a refused
    /// command does nothing for it.
    CommandAllowedDecision {
        unit: Techno = |r| Techno(r.subject),
        /// The command's number; compare with `Command::Move as i32` and
        /// the other [`crate::Command`] values.
        command: i32 = |r| r.value as i32,
        /// The target object; `NO_HANDLE` without one.
        target: Techno = |r| Techno(r.other),
        /// The target cell; `Cell(NO_HANDLE)` without one.
        target_cell: Cell = |r| Cell(r.cell),
    }
);

decision!(
    /// `DamageIncoming` (appendix 4.5, preview): the incoming damage
    /// continuing into the native flow, at the receiver's entry. The result
    /// must be 0 or keep the sign of [`Self::previous`]; 0 also suppresses
    /// the minimum of 1.
    DamageIncomingDecision {
        victim: Techno = |r| Techno(r.subject),
        /// The source; `NO_HANDLE` without one.
        attacker: Techno = |r| Techno(r.other),
        /// Warhead ID.
        warhead: i32 = |r| r.id,
        /// This link's incoming damage (its `previous`).
        incoming: i64 = |r| r.value,
        /// `incoming` through falloff, Verses and the cap (see the event).
        preview: i64 = |r| r.value2,
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

decision!(
    /// `FireAllowed` (RFC 0001 0.0.22, appendix 4.5): 0 vetoes this shot
    /// (the shooter waits as if rearming); 1 leaves it.
    FireAllowedDecision {
        shooter: Techno = |r| Techno(r.subject),
        /// The target object; `NO_HANDLE` for a cell.
        target: Techno = |r| Techno(r.other),
        target_cell: Cell = |r| Cell(r.cell),
        weapon_slot: i64 = |r| r.value,
        /// Weapon ID, -1 unknown.
        weapon: i32 = |r| r.id,
    }
);

decision!(
    /// `Retaliate` (appendix 4.5): 1 retaliates against the hit's source, 0
    /// does not; `native` is the engine's answer.
    RetaliateDecision {
        victim: Techno = |r| Techno(r.subject),
        source: Techno = |r| Techno(r.other),
        /// Warhead ID.
        warhead: i32 = |r| r.id,
    }
);

decision!(
    /// `DestinationSet` (appendix 4.5): the destination reference (an
    /// object or a cell), or 0 for none; `native` is the engine's.
    DestinationSetDecision {
        object: Techno = |r| Techno(r.subject),
        /// The native destination object; `NO_HANDLE` for a cell.
        destination: Techno = |r| Techno(r.other),
        destination_cell: Cell = |r| Cell(r.cell),
    }
);

decision!(
    /// `CrushAllowed` (appendix 4.5): 0 spares the victim; 1 leaves it.
    CrushAllowedDecision {
        victim: Techno = |r| Techno(r.subject),
        crusher: Techno = |r| Techno(r.other),
    }
);

decision!(
    /// `HarvestCell` (appendix 4.5): the cell reference to harvest, or 0 for
    /// none; `native` is the scan's.
    HarvestCellDecision {
        harvester: Techno = |r| Techno(r.subject),
        /// The scanned cell; `Cell(NO_HANDLE)` without one.
        scanned: Cell = |r| Cell(r.cell),
    }
);

decision!(
    /// `DeployAllowed` (appendix 4.5): 0 refuses the deploy; 1 leaves it.
    DeployAllowedDecision {
        object: Techno = |r| Techno(r.subject),
    }
);

decision!(
    /// `Fire` (appendix 4.3, preview): the shot plan, just before the
    /// shooter's weapon launches its bullet. The result is the number of
    /// [`sys::ProjectileSpec`] entries in the result area; the first link
    /// gets the native shot as one entry (its projectile type and weapon,
    /// target and offset 0). The engine launches the plan's projectiles in
    /// place of the native one; an empty plan launches nothing. Every other
    /// effect of the shot (rearm, ammo, burst, report, recoil) stays.
    FireDecision {
        shooter: Techno = |r| Techno(r.subject),
        /// The target object; `NO_HANDLE` for a cell.
        target: Techno = |r| Techno(r.other),
        target_cell: Cell = |r| Cell(r.cell),
        /// The weapon slot.
        slot: i64 = |r| r.value,
        /// The weapon's type ID.
        weapon: i32 = |r| r.id,
    }
);

impl FireDecision<'_> {
    /// The previous link's plan entry `index`.
    pub fn previous_entry(&mut self, index: usize) -> Option<sys::ProjectileSpec> {
        let count = usize::try_from(self.0.previous).ok()?;
        let len = usize::try_from(self.0.previous_len).ok()?;
        let at = index * size_of::<sys::ProjectileSpec>();
        if index >= count || at >= len {
            return None;
        }
        take(self.0.area(), at)
    }

    /// Answers `plan`; false (answering nothing) when it does not fit the
    /// result area.
    pub fn set_plan(&mut self, plan: &[sys::ProjectileSpec]) -> bool {
        let area = self.0.area();
        for (i, entry) in plan.iter().enumerate() {
            if !put(area, i * size_of::<sys::ProjectileSpec>(), entry) {
                return false;
            }
        }
        self.0.set_result(plan.len() as i64);
        true
    }
}

decision!(
    /// `FatalDamage` (appendix 4.3, preview): the death plan, at the entry
    /// of the death arm of a unit or building a hit killed. The result is
    /// the plan's bits ([`FatalDamageDecision`]'s constants); `native` is
    /// the engine's own plan.
    FatalDamageDecision {
        victim: Techno = |r| Techno(r.subject),
        /// The killer; `NO_HANDLE` without one.
        killer: Techno = |r| Techno(r.other),
        /// The killing hit's damage.
        damage: i64 = |r| r.value,
        /// The killing warhead's ID, −1 when unknown.
        warhead: i32 = |r| r.id,
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

impl FatalDamageDecision<'_> {
    /// Passengers die with it and the death weapon may fire.
    pub const EXPLODE: i64 = 1;
    /// The death weapon fires (with [`Self::EXPLODE`]).
    pub const DEATH_WEAPON: i64 = 1 << 1;
    /// Debris is thrown.
    pub const DEBRIS: i64 = 1 << 2;
    /// A building's survivors appear and a vehicle's passengers escape.
    pub const SURVIVORS: i64 = 1 << 3;
    /// A vehicle's crew escapes.
    pub const CREW: i64 = 1 << 4;
    /// The unit-lost announcement plays.
    pub const ANNOUNCE: i64 = 1 << 5;
    /// Tiberium remains: Riparius on and around the cell (Ares'
    /// `TiberiumRemains`; natively set only under Ares).
    pub const TIBERIUM_REMAINS: i64 = 1 << 6;

    /// `previous` with `bit` set or cleared.
    pub fn with(&self, bit: i64, on: bool) -> i64 {
        if on {
            self.0.previous | bit
        } else {
            self.0.previous & !bit
        }
    }

    /// `plan` with infantry death `inf_death` (0..255).
    pub const fn with_inf_death(plan: i64, inf_death: u8) -> i64 {
        (plan & !(0xFF << 8)) | ((inf_death as i64) << 8)
    }

    /// `plan` firing weapon ID `weapon` as the death weapon (−1 for the
    /// type's own).
    pub const fn with_death_weapon(plan: i64, weapon: i32) -> i64 {
        (plan & 0xFFFF_FFFF) | (((weapon as i64) + 1) << 32)
    }
}

decision!(
    /// `IdleMission` (appendix 4.5, preview): the mission a unit or
    /// infantryman queues as it goes idle. Answer a [`crate::Mission`]
    /// number, 255 to queue nothing; `native` is the engine's pick.
    IdleMissionDecision {
        unit: Techno = |r| Techno(r.subject),
    }
);

decision!(
    /// `MissionStep` (appendix 4.5, preview): after a unit's or
    /// infantryman's Guard, Area Guard, Hunt or Attack mission body ran.
    /// The result area holds its outcome (the previous link's answer for a
    /// later link): change what you want with the setters, each of which
    /// answers. Without an answer the outcome stands.
    MissionStepDecision {
        unit: Techno = |r| Techno(r.subject),
        /// The mission whose body ran ([`Mission`] number).
        mission: i64 = |r| r.value,
        /// The target the body left; `NO_HANDLE` for none or a cell.
        target: Techno = |r| Techno(r.other),
    }
);

impl MissionStepDecision<'_> {
    /// The outcome so far: the body's, or the previous link's answer.
    pub fn step(&mut self) -> Option<sys::MissionStepResult> {
        take(self.0.area(), 0)
    }

    /// Answers `step` whole.
    pub fn set_step(&mut self, step: &sys::MissionStepResult) -> bool {
        self.answer_at(0, step)
    }

    /// Answers with the target `target`, an object or a cell
    /// (`Techno(NO_HANDLE)` for none).
    pub fn set_target(&mut self, target: impl Ref) -> bool {
        self.answer_at(0, &target.raw())
    }

    /// Answers with the destination `destination`, an object or a cell
    /// (`Cell(NO_HANDLE)` for none).
    pub fn set_destination(&mut self, destination: impl Ref) -> bool {
        self.answer_at(8, &destination.raw())
    }

    /// Answers with `mission` to queue, `None` to queue nothing.
    pub fn set_mission(&mut self, mission: Option<Mission>) -> bool {
        self.answer_at(16, &mission.map_or(-1, |mission| mission as i32))
    }

    /// Answers with the frames until the body runs again (at least 1).
    pub fn set_delay(&mut self, frames: i32) -> bool {
        self.answer_at(20, &frames)
    }

    fn answer_at<T: Copy>(&mut self, at: usize, value: &T) -> bool {
        if !put(self.0.area(), at, value) {
            return false;
        }
        self.0.set_result(0);
        true
    }
}

/// `PathKind` (appendix 8.3, enum 22).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    /// Control points after the launch position; ends on the last.
    Bezier = 0,
    /// To the one point; `param[0]` is the apex height in leptons.
    Parabola = 1,
    /// Along the line to the one point; `param[0]` radius in leptons,
    /// `param[1]` frames per turn.
    Spiral = 2,
    /// Straight through each point in turn.
    Waypoints = 3,
}

decision!(
    /// `Launch` (appendix 4.5, preview): the bullet's path, once, when a
    /// bullet of a bound type is launched. Without an answer the bullet
    /// flies natively.
    LaunchDecision {
        projectile: Projectile = |r| Projectile(r.projectile),
        launcher: Techno = |r| Techno(r.subject),
        target: Techno = |r| Techno(r.other),
        target_cell: Cell = |r| Cell(r.cell),
    }
);

impl LaunchDecision<'_> {
    /// Answers a path of `kind` through `points` (leptons) at `speed`
    /// leptons per frame with 16 fraction bits (0 for the bullet's own);
    /// false when it does not fit the result area.
    pub fn set_path(
        &mut self,
        kind: PathKind,
        speed: i64,
        param: [i64; 2],
        points: &[[i64; 3]],
    ) -> bool {
        let header = sys::PathSpecHeader {
            kind: kind as i32,
            point_count: points.len() as i32,
            speed,
            param,
        };
        let area = self.0.area();
        if !put(area, 0, &header) {
            return false;
        }
        for (i, point) in points.iter().enumerate() {
            if !put(area, size_of::<sys::PathSpecHeader>() + i * 24, point) {
                return false;
            }
        }
        self.0
            .set_result((size_of::<sys::PathSpecHeader>() + points.len() * 24) as i64);
        true
    }
}

decision!(
    /// `ProjectileFrame` (appendix 4.5, preview): before a bound bullet's
    /// candidate position is computed, every frame it moves. Read its
    /// `X`..`Z`, `VX`..`VZ`, `Data0`, `Data1`; answer a step. Without an
    /// answer it moves natively (or along its `Launch` path).
    ProjectileFrameDecision {
        projectile: Projectile = |r| Projectile(r.projectile),
        /// Frames since it was launched.
        frames: i64 = |r| r.value,
    }
);

impl ProjectileFrameDecision<'_> {
    /// Answers `step` (the previous link's, if any, is replaced).
    pub fn set_step(&mut self, step: &sys::ProjectileStep) -> bool {
        if !put(self.0.area(), 0, step) {
            return false;
        }
        self.0.set_result(0);
        true
    }

    /// The previous link's step, when one answered.
    pub fn previous_step(&mut self) -> Option<sys::ProjectileStep> {
        (self.0.previous_len > 0)
            .then(|| take(self.0.area(), 0))
            .flatten()
    }
}

decision!(
    /// `MoveFrame` (appendix 4.5, preview): each frame a bound vehicle or
    /// infantryman's locomotor (Drive, Ship, Walk, Mech, Hover) could start
    /// a step. Answer a [`sys::MoveStep`]: [`Self::MOVE`] takes the frame
    /// and moves it by the displacement (the engine checks the cell it
    /// enters), [`Self::FACING`] turns it, [`Self::STOP`] drops its
    /// destination. Without an answer, or without [`Self::MOVE`], its
    /// locomotor moves it natively.
    MoveFrameDecision {
        unit: Techno = |r| Techno(r.subject),
        /// 1 while its locomotor is moving.
        moving: bool = |r| r.value == 1,
        /// Its NavCom object; `NO_HANDLE` for a cell or none.
        destination: Techno = |r| Techno(r.other),
        /// Its NavCom cell; `Cell(NO_HANDLE)` for an object or none.
        destination_cell: Cell = |r| Cell(r.cell),
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

impl MoveFrameDecision<'_> {
    pub const MOVE: i32 = 1;
    pub const FACING: i32 = 2;
    pub const STOP: i32 = 4;

    /// Answers `step` (the previous link's is replaced).
    pub fn set_step(&mut self, step: &sys::MoveStep) -> bool {
        if !put(self.0.area(), 0, step) {
            return false;
        }
        self.0.set_result(0);
        true
    }

    /// Takes the frame and moves by `dx`, `dy` leptons.
    pub fn step_by(&mut self, dx: i32, dy: i32) -> bool {
        self.set_step(&sys::MoveStep {
            flags: Self::MOVE,
            facing: 0,
            displacement: [dx, dy],
        })
    }

    /// The previous link's step (all zero for the first link).
    pub fn previous_step(&mut self) -> Option<sys::MoveStep> {
        (self.0.previous_len > 0)
            .then(|| take(self.0.area(), 0))
            .flatten()
    }
}

decision!(
    /// `PathFound` (appendix 4.5, preview): when a path search's route is
    /// adopted. The area holds the route's cells (up to 24), the first link
    /// the native ones; `native` is their count, 0 for no route. Answer up
    /// to 24 cells, each next to the one before (the first next to the
    /// unit's cell), or no route.
    PathFoundDecision {
        unit: Techno = |r| Techno(r.subject),
        /// The search's goal cell.
        goal: Cell = |r| Cell(r.cell),
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

impl PathFoundDecision<'_> {
    /// Cell `index` of the previous link's route.
    pub fn previous_cell(&mut self, index: usize) -> Option<sys::PathCell> {
        let count = usize::try_from(self.0.previous).ok()?;
        let len = usize::try_from(self.0.previous_len).ok()?;
        let at = index * size_of::<sys::PathCell>();
        if index >= count || at >= len {
            return None;
        }
        take(self.0.area(), at)
    }

    /// Answers the route `cells`; false (answering nothing) when it does
    /// not fit the result area.
    pub fn set_cells(&mut self, cells: &[sys::PathCell]) -> bool {
        let area = self.0.area();
        for (i, cell) in cells.iter().enumerate() {
            if !put(area, i * size_of::<sys::PathCell>(), cell) {
                return false;
            }
        }
        self.0.set_result(cells.len() as i64);
        true
    }

    /// Answers no route: the search fails.
    pub fn no_route(&mut self) {
        self.0.set_result(0);
    }
}

decision!(
    /// `ApproachPoint` (appendix 4.5, preview): when a unit approaching its
    /// target has chosen the cell to move to, before it is set as the
    /// destination. The result is a cell reference on the map; anything
    /// else (0 included) keeps the previous link's cell, the native one
    /// ([`Self::native`]) for the first.
    ApproachPointDecision {
        unit: Techno = |r| Techno(r.subject),
        /// The cell the engine chose.
        chosen: Cell = |r| Cell(r.cell),
        /// The target object; `NO_HANDLE` for a cell target.
        target: Techno = |r| Techno(r.other),
        /// The target's cell.
        target_cell: Cell = |r| Cell(r.value2),
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

impl ApproachPointDecision<'_> {
    /// Heads for `cell` instead.
    pub fn set_cell(&mut self, cell: Cell) {
        self.0.set_result(cell.0)
    }
}

/// Exports `on_decide`, calling the handler whose key (the trigger's
/// `decide_sync` key) matches each request. A handler takes
/// `&mut DecideRecord` or a decision view. The macro also declares the
/// module's delivery area; a module that also uses [`crate::on_events!`],
/// which declares it too, writes `on_decide!(shared_delivery: ...)`.
///
/// ```ignore
/// vera20k_sdk::on_decide!(HALVE => halve, NO_DEFEAT => no_defeat);
/// ```
#[macro_export]
macro_rules! on_decide {
    (shared_delivery: $($key:path => $handler:path),* $(,)?) => {
        #[unsafe(no_mangle)]
        /// # Safety
        /// Only the engine may call this export with its current delivery records.
        pub unsafe extern "C" fn on_decide(ptr: i32, count: i32) {
            // SAFETY: these are the engine's own arguments, used only here.
            let records = unsafe { $crate::DecideRecord::batch(ptr, count) };
            for mut record in records {
                $(
                    if record.event.key == $key {
                        $handler(::core::convert::From::from(&mut record));
                        continue;
                    }
                )*
            }
        }
    };
    ($($key:path => $handler:path),* $(,)?) => {
        #[used]
        #[unsafe(link_section = "vera.delivery")]
        static __VERA_DELIVERY: [u8; 9] = $crate::DELIVERY_SECTION;

        $crate::on_decide!(shared_delivery: $($key => $handler),*);
    };
}
