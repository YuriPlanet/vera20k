//! Runtime triggers (RFC 0001 sections 6a and 7, appendix section 3):
//! [`Trigger`] builds a `TriggerDef` and calls `trigger_create` and
//! `trigger_bind`, [`ActionDef`] builds trigger actions, and the event views
//! read `EventRecord`s in place inside `on_events` ([`crate::on_events!`]).
//!
//! ```ignore
//! use vera20k_sdk::prelude::*;
//!
//! vera20k_sdk::named_actions! { ON_HIT = "OnHit" => 1 }
//!
//! fn start() {
//!     let _ = Trigger::on(Event::TAKE_DAMAGE)
//!         .bind(some_unit)
//!         .immediate(ActionDef::set_var(Var::local(3), 1))
//!         .enqueue(ON_HIT)
//!         .done();
//! }
//!
//! fn on_hit(e: TakeDamageEvent) {
//!     let _ = e.damage();
//! }
//!
//! vera20k_sdk::on_scenario_start!(start);
//! vera20k_sdk::on_events!(ON_HIT => on_hit);
//! ```

use crate::{Cell, House, NO_HANDLE, Op, Prop, Ref, Result, Techno, Var, check, sys};

/// Event kinds (appendix 4). New delivered events are numbered from 1000 in
/// appendix 4.3 order; provisional (RFC open question 5).
pub struct Event;

impl Event {
    pub const TAKE_DAMAGE: i32 = 1000;
    pub const KILLED: i32 = 1004;
    pub const FIRE: i32 = 1005;
    pub const TIMER: i32 = 1023;
    /// Decision-only (appendix 4.5): needs [`Trigger::decide_sync`].
    pub const PRODUCTION_QUEUED: i32 = 1032;
    /// Decision-only (appendix 4.5): needs [`Trigger::decide_sync`].
    pub const DEFEAT: i32 = 1033;
}

/// What a trigger action acts on (`ActionTarget`, appendix 5.1).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Subject = 0,
    Other = 1,
    TagHolder = 2,
    Projectile = 3,
    House = 4,
    Cell = 5,
}

/// A trigger action (`ActionDef` with its `ActionParams`), only constructed
/// here; [`Trigger::immediate`] adds it.
#[derive(Clone, Copy, Debug)]
pub struct ActionDef(pub sys::ActionDef);

impl ActionDef {
    pub const ADD_MONEY: i32 = 1015;
    pub const SET_VAR: i32 = 1024;
    pub const MODULE: i32 = 1036;

    /// Any action `kind` on `target` with `params`.
    pub const fn new(kind: i32, target: Target, params: sys::ActionParams) -> Self {
        Self(sys::ActionDef {
            kind,
            target: target as i32,
            params,
        })
    }

    /// Gives the trigger's house `amount` credits.
    pub const fn add_money(amount: i32) -> Self {
        Self::add_money_to(House(NO_HANDLE), amount)
    }

    /// Gives `house` `amount` credits.
    pub const fn add_money_to(house: House, amount: i32) -> Self {
        Self::new(
            Self::ADD_MONEY,
            Target::House,
            sys::ActionParams {
                value: amount as i64,
                house: house.0,
                ..PARAMS
            },
        )
    }

    /// Writes `value` to variable `var` (a Red Alert 2 variable ignores the
    /// target).
    pub const fn set_var(var: Var, value: i64) -> Self {
        Self::new(
            Self::SET_VAR,
            Target::Subject,
            sys::ActionParams {
                id: var.0,
                value,
                ..PARAMS
            },
        )
    }

    /// Queues a record with dispatch key `key` for `on_events`.
    pub const fn module(key: i32) -> Self {
        Self::new(
            Self::MODULE,
            Target::Subject,
            sys::ActionParams { id: key, ..PARAMS },
        )
    }

    /// Writes `line` to the in-game log; `trigger_create` copies the text.
    pub fn log(line: &'static str) -> Self {
        Self::new(
            Action::LOG,
            Target::Subject,
            sys::ActionParams {
                text: line.as_ptr() as i32,
                text_len: line.len() as i32,
                ..PARAMS
            },
        )
    }

    /// Restores `amount` health to `target`.
    pub const fn heal(target: Target, amount: i32) -> Self {
        Self::new(
            Action::HEAL,
            target,
            sys::ActionParams {
                value: amount as i64,
                ..PARAMS
            },
        )
    }

    /// Writes `prop` of `target`, as `set` does.
    pub const fn set_property(target: Target, prop: Prop, value: i64) -> Self {
        Self::new(
            Action::SET_PROPERTY,
            target,
            sys::ActionParams {
                id: prop.0,
                value,
                ..PARAMS
            },
        )
    }
}

const PARAMS: sys::ActionParams = sys::ActionParams {
    id: 0,
    arg: 0,
    arg2: 0,
    reserved: 0,
    value: 0,
    value2: 0,
    cell: 0,
    house: 0,
    other: 0,
    text: 0,
    text_len: 0,
    extra: 0,
    extra_len: 0,
};

/// Most events, clauses per event, actions and bindings one [`Trigger`]
/// holds; more are dropped and make [`Trigger::done`] answer `Refused`.
pub const MAX_EVENTS: usize = 4;
pub const MAX_EVENT_CLAUSES: usize = 4;
pub const MAX_ACTIONS: usize = 8;
pub const MAX_BINDS: usize = 8;

const CLAUSE: sys::Clause = sys::Clause {
    subject: 0,
    op: 0,
    scope: 0,
    reserved: 0,
    prop: 0,
    value: 0,
};

#[derive(Clone, Copy)]
struct EventSpec {
    def: sys::EventDef,
    clauses: [sys::Clause; MAX_EVENT_CLAUSES],
}

/// A trigger handle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TriggerRef(pub i64);

impl Ref for TriggerRef {
    fn raw(self) -> i64 {
        self.0
    }
}

impl TriggerRef {
    pub fn bind(self, reference: impl Ref) -> Result<()> {
        check(unsafe { sys::trigger_bind(self.0, reference.raw()) })
    }

    pub fn unbind(self, reference: impl Ref) -> Result<()> {
        check(unsafe { sys::trigger_unbind(self.0, reference.raw()) })
    }

    /// Removes the trigger and every instance of it.
    pub fn remove(self) -> Result<()> {
        check(unsafe { sys::trigger_remove(self.0) })
    }

    /// Writes the trigger's `Enabled` property.
    pub fn set_enabled(self, enabled: bool) -> Result<()> {
        crate::set(self, Prop::named("Enabled")?, i64::from(enabled))
    }
}

/// A trigger under construction (appendix section 3, "Triggers"). Nothing
/// reaches the engine before [`Trigger::done`].
pub struct Trigger {
    header: sys::TriggerDef,
    events: [EventSpec; MAX_EVENTS],
    actions: [sys::ActionDef; MAX_ACTIONS],
    binds: [i64; MAX_BINDS],
    binds_len: usize,
    overflow: bool,
}

impl Trigger {
    /// A repeating trigger on event `kind` for every difficulty.
    pub fn on(kind: i32) -> Self {
        let trigger = Self {
            header: sys::TriggerDef {
                repeat: 2,
                difficulty: 0b111,
                decide_key: -1,
                ..Default::default()
            },
            events: [EventSpec {
                def: sys::EventDef::default(),
                clauses: [CLAUSE; MAX_EVENT_CLAUSES],
            }; MAX_EVENTS],
            actions: [sys::ActionDef::default(); MAX_ACTIONS],
            binds: [NO_HANDLE; MAX_BINDS],
            binds_len: 0,
            overflow: false,
        };
        trigger.and(kind)
    }

    /// A trigger that fires every `interval` frames (`Timer`), counted from
    /// its creation.
    pub fn every(interval: i32) -> Self {
        let mut trigger = Self::on(Event::TIMER);
        trigger.events[0].def.interval = interval;
        trigger
    }

    /// Adds another event; all must occur before the trigger fires.
    pub fn and(mut self, kind: i32) -> Self {
        match self.events.get_mut(self.header.event_count as usize) {
            Some(event) => {
                event.def = sys::EventDef {
                    kind,
                    id: -1,
                    ..Default::default()
                };
                self.header.event_count += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// Fires once per instance instead of every time.
    pub fn once(mut self) -> Self {
        self.header.repeat = 0;
        self
    }

    /// The trigger's house, in place of the subject's owner.
    pub fn house(mut self, house: House) -> Self {
        self.header.house = house.0;
        self
    }

    /// Starts disabled (`TriggerRef::set_enabled` turns it on).
    pub fn disabled(mut self) -> Self {
        self.header.flags |= 1;
        self
    }

    fn clause(mut self, subject: u8, scope: u8, prop: i32, op: Op, value: i64) -> Self {
        let Some(index) = (self.header.event_count as usize).checked_sub(1) else {
            self.overflow = true;
            return self;
        };
        let event = &mut self.events[index];
        match event.clauses.get_mut(event.def.clause_count as usize) {
            Some(clause) => {
                *clause = sys::Clause {
                    subject,
                    op: op as u8,
                    scope,
                    prop,
                    value,
                    ..CLAUSE
                };
                event.def.clause_count += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// The latest event counts only when the subject's `prop` compares.
    pub fn where_prop(self, prop: Prop, op: Op, value: i64) -> Self {
        self.clause(0, 0, prop.0, op, value)
    }

    /// As [`Trigger::where_prop`], on the event's other object.
    pub fn where_other(self, prop: Prop, op: Op, value: i64) -> Self {
        self.clause(1, 0, prop.0, op, value)
    }

    /// As [`Trigger::where_prop`], on the trigger's house.
    pub fn where_house(self, prop: Prop, op: Op, value: i64) -> Self {
        self.clause(2, 0, prop.0, op, value)
    }

    /// As [`Trigger::where_prop`], on a global variable.
    pub fn where_var(self, var: Var, op: Op, value: i64) -> Self {
        self.clause(3, 0, var.0, op, value)
    }

    /// Binds the trigger to `reference` (an object or a type) at
    /// [`Trigger::done`].
    pub fn bind(mut self, reference: impl Ref) -> Self {
        match self.binds.get_mut(self.binds_len) {
            Some(slot) => {
                *slot = reference.raw();
                self.binds_len += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// Runs `action` when the trigger fires.
    pub fn immediate(mut self, action: ActionDef) -> Self {
        match self.actions.get_mut(self.header.action_count as usize) {
            Some(slot) => {
                *slot = action.0;
                self.header.action_count += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// Delivers a record with dispatch key `key` to `on_events` in the
    /// module phase.
    pub fn enqueue(self, key: i32) -> Self {
        self.immediate(ActionDef::module(key))
    }

    /// Makes this a decision trigger (RFC 6f) whose requests reach the
    /// [`crate::on_decide!`] handler for `key`, and finalizes it as
    /// [`Trigger::done`] does. Its event must have a decision form; such a
    /// trigger takes no actions.
    pub fn decide_sync(mut self, key: i32) -> Result<TriggerRef> {
        self.header.decide_key = key;
        self.done()
    }

    /// Creates the trigger, then binds it. A builder that overflowed creates
    /// nothing and answers `Refused`; a failed binding answers its status,
    /// the trigger already made.
    pub fn done(self) -> Result<TriggerRef> {
        if self.overflow {
            return Err(crate::Status::Refused);
        }
        let mut buffer = [0u8; BUFFER];
        let mut len = 0;
        let mut put = |bytes: &[u8]| {
            buffer[len..len + bytes.len()].copy_from_slice(bytes);
            len += bytes.len();
        };
        put(bytes_of(&self.header));
        for event in &self.events[..self.header.event_count as usize] {
            put(bytes_of(&event.def));
            for clause in &event.clauses[..event.def.clause_count as usize] {
                put(bytes_of(clause));
            }
        }
        for action in &self.actions[..self.header.action_count as usize] {
            put(bytes_of(action));
        }
        let mut handle = NO_HANDLE;
        check(unsafe { sys::trigger_create(buffer.as_ptr(), len as i32, &mut handle) })?;
        let trigger = TriggerRef(handle);
        for reference in &self.binds[..self.binds_len] {
            check(unsafe { sys::trigger_bind(handle, *reference) })?;
        }
        Ok(trigger)
    }
}

const BUFFER: usize = size_of::<sys::TriggerDef>()
    + MAX_EVENTS * (size_of::<sys::EventDef>() + MAX_EVENT_CLAUSES * size_of::<sys::Clause>())
    + MAX_ACTIONS * size_of::<sys::ActionDef>();

/// The bytes of a padding-free `repr(C)` layout.
fn bytes_of<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: only the `sys` layouts above reach here; each is `repr(C)`
    // with no padding (their sizes are asserted), so every byte is
    // initialized.
    unsafe { core::slice::from_raw_parts((value as *const T).cast(), size_of::<T>()) }
}

// ---------------------------------------------------------------- Records

/// One delivered record (`EventRecord`), valid during its `on_events` call.
pub type EventRecord = sys::EventRecord;

impl EventRecord {
    /// The records `on_events(ptr, count)` received.
    ///
    /// # Safety
    /// `ptr` and `count` must be the arguments the engine passed to
    /// `on_events`, and the slice must not outlive that call.
    pub unsafe fn batch<'a>(ptr: i32, count: i32) -> &'a [EventRecord] {
        // SAFETY: the engine wrote `count` aligned records at `ptr`, in the
        // `vera.delivery` area (8-byte aligned, checked at load).
        unsafe { core::slice::from_raw_parts(ptr as usize as *const EventRecord, count as usize) }
    }

    pub fn trigger(&self) -> TriggerRef {
        TriggerRef(self.trigger)
    }

    pub fn subject(&self) -> Techno {
        Techno(self.subject)
    }

    pub fn other(&self) -> Techno {
        Techno(self.other)
    }

    pub fn house(&self) -> House {
        House(self.house)
    }
}

macro_rules! view {
    ($(#[$doc:meta])* $name:ident { $($(#[$fdoc:meta])* $field:ident: $ty:ty = |$r:ident| $read:expr),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy)]
        pub struct $name<'a>(pub &'a EventRecord);

        impl<'a> From<&'a EventRecord> for $name<'a> {
            fn from(record: &'a EventRecord) -> Self {
                Self(record)
            }
        }

        impl $name<'_> {
            $(
                $(#[$fdoc])*
                pub fn $field(&self) -> $ty {
                    let $r = self.0;
                    $read
                }
            )*

            /// The whole record.
            pub fn record(&self) -> &EventRecord {
                self.0
            }
        }
    };
}

view!(
    /// `TakeDamage` (appendix 3 and 4.3).
    TakeDamageEvent {
        victim: Techno = |r| Techno(r.subject),
        /// The source; `NO_HANDLE` without one.
        attacker: Techno = |r| Techno(r.other),
        /// Damage after armor.
        damage: i64 = |r| r.value,
        /// Warhead ID, −1 when unknown.
        warhead: i32 = |r| r.id,
        /// The victim's position in leptons.
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

view!(
    /// `Killed` (appendix 4.3).
    KilledEvent {
        victim: Techno = |r| Techno(r.subject),
        killer: Techno = |r| Techno(r.other),
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

view!(
    /// `Fire` (appendix 3 and 4.3).
    FireEvent {
        shooter: Techno = |r| Techno(r.subject),
        target: Techno = |r| Techno(r.other),
        target_cell: Cell = |r| Cell(r.cell),
        weapon_slot: i64 = |r| r.value,
        /// Weapon ID.
        weapon: i32 = |r| r.id,
        projectile: i64 = |r| r.projectile,
    }
);

view!(
    /// `Timer` (appendix 4.3).
    TimerEvent {
        /// The frame of the previous firing, or of the trigger's creation.
        last_frame: i64 = |r| r.value,
    }
);

// ----------------------------------------------------------------- Exports

/// Exports `on_events`, calling the handler whose key matches each record,
/// in order. A handler takes `&EventRecord` or an event view. The macro also
/// declares the module's delivery area ([`DELIVERY_SECTION`]).
///
/// ```ignore
/// vera20k_sdk::on_events!(ON_HIT => on_hit, ON_DEATH => on_death);
/// ```
#[macro_export]
macro_rules! on_events {
    ($($key:path => $handler:path),* $(,)?) => {
        #[used]
        #[unsafe(link_section = "vera.delivery")]
        static __VERA_DELIVERY: [u8; 9] = $crate::DELIVERY_SECTION;

        #[unsafe(no_mangle)]
        pub extern "C" fn on_events(ptr: i32, count: i32) {
            // SAFETY: these are the engine's own arguments, used only here.
            let records = unsafe { $crate::EventRecord::batch(ptr, count) };
            for record in records {
                $(
                    if record.key == $key {
                        $handler(::core::convert::From::from(record));
                        continue;
                    }
                )*
            }
        }
    };
}

/// Where the SDK's delivery area lies: nine records from address 8.
///
/// A `link_section` static holds plain bytes only (rustc refuses an address
/// there), so the area cannot be a static buffer whose address the linker
/// fills in. The first KiB of memory is free in both wasm32 layouts
/// wasm-ld produces: below the 1024-byte global base without
/// `--stack-first`, and the far end of the 1 MiB stack with it (rustc's
/// default). Limitation: a module whose stack comes within a KiB of its
/// 1 MiB limit during `on_events` would overwrite records it has not read.
pub const DELIVERY_OFFSET: u32 = 8;
pub const DELIVERY_RECORDS: u32 = 9;

/// The `vera.delivery` section (appendix "Custom sections") for
/// [`DELIVERY_OFFSET`] and [`DELIVERY_RECORDS`].
pub const DELIVERY_SECTION: [u8; 9] = {
    let offset = DELIVERY_OFFSET.to_le_bytes();
    let bytes = (DELIVERY_RECORDS * 112).to_le_bytes();
    [
        1, offset[0], offset[1], offset[2], offset[3], bytes[0], bytes[1], bytes[2], bytes[3],
    ]
};

/// Declares the module's named actions as one `vera.actions` section and a
/// `const` dispatch key for each. A module has exactly one such section: a
/// second `named_actions!` would append a second blob, and the engine
/// rejects the module (trailing bytes).
///
/// ```ignore
/// vera20k_sdk::named_actions! { ON_HIT = "OnHit" => 1, ON_DEATH = "OnDeath" => 2 }
/// ```
#[macro_export]
macro_rules! named_actions {
    ($($key:ident = $name:literal => $value:expr),* $(,)?) => {
        $(pub const $key: i32 = $value;)*

        #[used]
        #[unsafe(link_section = "vera.actions")]
        static __VERA_ACTIONS: [u8; $crate::actions_section_len(&[$($name),*])] =
            $crate::actions_section(&[$(($name, $value)),*]);
    };
}

/// The byte length of a `vera.actions` section naming `names`.
pub const fn actions_section_len(names: &[&str]) -> usize {
    let mut len = 1 + 4;
    let mut i = 0;
    while i < names.len() {
        len += 2 + names[i].len() + 4;
        i += 1;
    }
    len
}

/// A `vera.actions` section (appendix "Custom sections"): format 1, count,
/// then each name's `u16` length, bytes and `i32` key. Names must be ASCII
/// and unique ignoring case; the engine checks.
pub const fn actions_section<const N: usize>(entries: &[(&str, i32)]) -> [u8; N] {
    let mut out = [0u8; N];
    out[0] = 1;
    let count = (entries.len() as u32).to_le_bytes();
    let mut at = 1;
    let mut i = 0;
    while i < 4 {
        out[at + i] = count[i];
        i += 1;
    }
    at += 4;
    let mut e = 0;
    while e < entries.len() {
        let (name, key) = entries[e];
        let name = name.as_bytes();
        let len = (name.len() as u16).to_le_bytes();
        out[at] = len[0];
        out[at + 1] = len[1];
        at += 2;
        let mut b = 0;
        while b < name.len() {
            out[at + b] = name[b];
            b += 1;
        }
        at += name.len();
        let key = key.to_le_bytes();
        let mut k = 0;
        while k < 4 {
            out[at + k] = key[k];
            k += 1;
        }
        at += 4;
        e += 1;
    }
    out
}
