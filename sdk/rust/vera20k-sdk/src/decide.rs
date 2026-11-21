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

use crate::{Cell, EventRecord, House, NameKind, Techno, TypeRef, sys};

/// One decision request (`DecideRecord`), valid during its `on_decide`
/// call.
pub type DecideRecord = sys::DecideRecord;

impl DecideRecord {
    /// The records `on_decide(ptr, count)` received.
    ///
    /// # Safety
    /// `ptr` and `count` must be the arguments the engine passed to
    /// `on_decide`, and the slice must not outlive that call.
    pub unsafe fn batch<'a>(ptr: i32, count: i32) -> &'a mut [DecideRecord] {
        // SAFETY: the engine wrote `count` records at `ptr`, in the
        // `vera.delivery` area (8-byte aligned, checked at load).
        unsafe {
            core::slice::from_raw_parts_mut(ptr as usize as *mut DecideRecord, count as usize)
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
}

macro_rules! decision {
    ($(#[$doc:meta])* $name:ident { $($(#[$fdoc:meta])* $field:ident: $ty:ty = |$r:ident| $read:expr),* $(,)? }) => {
        $(#[$doc])*
        pub struct $name<'a>(pub &'a mut DecideRecord);

        impl<'a> From<&'a mut DecideRecord> for $name<'a> {
            fn from(record: &'a mut DecideRecord) -> Self {
                Self(record)
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
            pub fn request(&self) -> &DecideRecord {
                self.0
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
        pub extern "C" fn on_decide(ptr: i32, count: i32) {
            // SAFETY: these are the engine's own arguments, used only here.
            let records = unsafe { $crate::DecideRecord::batch(ptr, count) };
            for record in records.iter_mut() {
                $(
                    if record.event.key == $key {
                        $handler(::core::convert::From::from(&mut *record));
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
