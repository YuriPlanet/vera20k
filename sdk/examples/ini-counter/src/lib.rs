//! "IniCounter", a demo mod for the INI names a module registers
//! (RFC 0001 section 6h): the modder writes INI, the
//! module implements it.
//!
//! A community idea, made generic: "while the house owns a radar, every 10
//! conscripts it trains earn it a free Rhino tank from its war factory".
//! Each entry of the registry `[Demo.Counters]` names a counter section:
//!
//! ```ini
//! [Demo.Counters]
//! 1=SovietDraft
//!
//! [SovietDraft]
//! Count=E2              ; TechnoTypes counted when born for a house
//! Threshold=10          ; births per reward
//! Requires=NARADR       ; optional: counted only while the house owns one
//! Reward=HTNK           ; the free unit
//! Factory=NAWEAP        ; optional: the reward appears at it (else beside any building)
//! Houses=Russians       ; optional: countries the counter is for (default: all)
//! Stage=4               ; optional, demo only: see below
//! ```
//!
//! - `IniEntry` (one per counter) parses the section and creates a `Birth`
//!   trigger bound to the counted types; it queues `onCounted` with the
//!   counter's index as parameter text.
//! - The tallies are per house, in module memory (saved with the match).
//! - A reward is created with `CreateObject`: first at the factory itself
//!   (the engine answers `Unsupported` for a factory target so far), then
//!   on a free cell around it.
//! - Each reward raises two module events: the global
//!   `Demo.RewardGranted` (value = counter index, house = the house), heard
//!   by a trigger made in code with `Trigger::on_module_event`, and the
//!   object event `Demo.RewardArrived` on the new unit, which INI triggers
//!   can hear (`Event=Wasm:Demo.RewardArrived` behind a `Wasm.Tags`). An
//!   INI trigger is always bound to the types that reach it through
//!   `Wasm.Tags`, and a global event's trigger takes no binding, so the
//!   global event is heard from code.
//! - `Stage=N` makes the demo play itself: once a computer house of the
//!   counter's `Houses` has a building (from frame 150), the module gives
//!   it the `Requires` and `Factory` buildings near it and N units of the
//!   first `Count` type, whose `Birth` counts like a trained unit.
//!
//! The first time each step works it logs `DEMO-OK entry`, `DEMO-OK count`,
//! `DEMO-OK reward` and `DEMO-OK event`.
#![no_std]

use core::cell::UnsafeCell;

use vera20k_sdk::prelude::*;
use vera20k_sdk::triggers::BirthEvent;
use vera20k_sdk::{NO_HANDLE, get};

vera20k_sdk::named_actions! {
    ON_COUNTED = "onCounted" => 1,
    ON_REWARD_GRANTED = "onRewardGranted" => 2,
    ON_REWARD_ARRIVED = "onRewardArrived" => 3,
}

/// The dispatch key of the `Demo.Counters` entries.
const COUNTERS: i32 = 10;

static REWARD_GRANTED: ini::EventSlot = ini::EventSlot::new();
static REWARD_ARRIVED: ini::EventSlot = ini::EventSlot::new();

const MAX_COUNTERS: usize = 8;
/// Types in `Count=` and countries in `Houses=`; also a trigger's bindings.
const MAX_TYPES: usize = 8;
const MAX_HOUSES: usize = 32;
const MAX_STAGE: i64 = 20;
/// The parameter text of each counter's `Birth` trigger.
const INDEX_TEXT: [&str; MAX_COUNTERS] = ["0", "1", "2", "3", "4", "5", "6", "7"];

/// Staging waits for a computer house's first building from this frame,
/// takes any of its objects from the second and gives up after the third.
const STAGE_FROM: i32 = 150;
const STAGE_ANY_OBJECT_FROM: i32 = 600;
const STAGE_UNTIL: i32 = 2400;

// ------------------------------------------------------------------ State

#[derive(Clone, Copy)]
struct Counter {
    count: [TypeRef; MAX_TYPES],
    count_len: usize,
    /// Country type references (`Houses=`); none means every house.
    countries: [i64; MAX_TYPES],
    countries_len: usize,
    threshold: i64,
    /// `NO_HANDLE` when not configured.
    requires: TypeRef,
    reward: TypeRef,
    factory: TypeRef,
    stage: i64,
    /// Births counted so far, by house index.
    tally: [i64; MAX_HOUSES],
    /// Whether the demo staging is done for that house index.
    staged: [bool; MAX_HOUSES],
}

const NONE: TypeRef = TypeRef(NO_HANDLE);

const EMPTY: Counter = Counter {
    count: [NONE; MAX_TYPES],
    count_len: 0,
    countries: [0; MAX_TYPES],
    countries_len: 0,
    threshold: 0,
    requires: NONE,
    reward: NONE,
    factory: NONE,
    stage: 0,
    tally: [0; MAX_HOUSES],
    staged: [false; MAX_HOUSES],
};

struct State {
    counters: [Counter; MAX_COUNTERS],
    len: usize,
    /// `DEMO-OK` lines already written (bits below).
    marks: u8,
    staging_done: bool,
    /// Rewards logged so far; the log keeps only its latest lines.
    reward_lines: u8,
}

const MARK_ENTRY: u8 = 1;
const MARK_COUNT: u8 = 2;
const MARK_REWARD: u8 = 4;
const MARK_EVENT: u8 = 8;
const MARK_ARRIVED: u8 = 16;

struct Shared(UnsafeCell<State>);

// SAFETY: a module runs on one thread.
unsafe impl Sync for Shared {}

static STATE: Shared = Shared(UnsafeCell::new(State {
    counters: [EMPTY; MAX_COUNTERS],
    len: 0,
    marks: 0,
    staging_done: false,
    reward_lines: 0,
}));

/// The module's state. Each export takes it once and passes it down; the
/// host never calls back into the module during a host call (raised
/// events queue their module actions), so the borrows never overlap.
fn state() -> &'static mut State {
    // SAFETY: as above.
    unsafe { &mut *STATE.0.get() }
}

/// Logs `line` the first time `bit` is marked.
fn mark(s: &mut State, bit: u8, line: &str) {
    if s.marks & bit == 0 {
        s.marks |= bit;
        log(line);
    }
}

// ------------------------------------------------------------ Registration

fn init() {
    if ini::registry("Demo.Counters", COUNTERS).is_err() {
        log("IniCounter: another module already registered Demo.Counters");
    }
    match ini::event("Demo.RewardGranted", EventScope::Global) {
        Ok(event) => REWARD_GRANTED.set(event),
        Err(_) => log("IniCounter: another module already registered Demo.RewardGranted"),
    }
    match ini::event("Demo.RewardArrived", EventScope::Object) {
        Ok(event) => REWARD_ARRIVED.set(event),
        Err(_) => log("IniCounter: another module already registered Demo.RewardArrived"),
    }
}

/// Stops the module with a message naming the section and key, as a
/// handler should for a bad value (RFC 0001 section 6h).
fn stop(section: &str, key: &str, why: &str) -> ! {
    Line::new()
        .text("IniCounter: [")
        .text(section)
        .text("] ")
        .text(key)
        .text(": ")
        .text(why)
        .log();
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    loop {}
}

/// `[section] key`'s value without a comment or surrounding spaces; `None`
/// when missing or empty.
fn value<'b>(section: &str, key: &str, buf: &'b mut [u8]) -> Option<&'b str> {
    match Rules::get(section, key, buf) {
        Ok(text) => {
            let text = text.split(';').next().unwrap_or("").trim_ascii();
            (!text.is_empty()).then_some(text)
        }
        Err(Status::NotFound) => None,
        Err(_) => stop(section, key, "the value is too long"),
    }
}

fn techno(section: &str, key: &str, name: &str) -> TypeRef {
    match TypeRef::techno_type(name) {
        Ok(ty) => ty,
        Err(_) => stop(section, key, "not a TechnoType"),
    }
}

/// An optional TechnoType key; `NONE` when missing.
fn optional_techno(section: &str, key: &str) -> TypeRef {
    let mut buf = [0u8; 64];
    match value(section, key, &mut buf) {
        Some(name) => techno(section, key, name),
        None => NONE,
    }
}

/// One `Demo.Counters` entry: parse its section, make its `Birth` trigger.
fn load_counter(record: IniEntryRecord) {
    let s = state();
    let mut name = [0u8; 64];
    let Ok(section) = record.entry_name(&mut name) else {
        stop("Demo.Counters", "entry", "a section name longer than 64 characters");
    };
    let index = s.len;
    if index == MAX_COUNTERS {
        stop("Demo.Counters", section, "more than 8 counters");
    }
    let mut counter = EMPTY;
    let mut buf = [0u8; 160];

    let Some(list) = value(section, "Count", &mut buf) else {
        stop(section, "Count", "missing");
    };
    for type_name in ini::split(list).filter(|n| !n.is_empty()) {
        if counter.count_len == MAX_TYPES {
            stop(section, "Count", "more than 8 types");
        }
        counter.count[counter.count_len] = techno(section, "Count", type_name);
        counter.count_len += 1;
    }
    if counter.count_len == 0 {
        stop(section, "Count", "no type");
    }

    counter.threshold = match value(section, "Threshold", &mut buf).map(ini::int) {
        Some(Some(n)) if n >= 1 => n,
        _ => stop(section, "Threshold", "needs a number of 1 or more"),
    };

    counter.reward = match value(section, "Reward", &mut buf) {
        Some(reward) => techno(section, "Reward", reward),
        None => stop(section, "Reward", "missing"),
    };
    counter.requires = optional_techno(section, "Requires");
    counter.factory = optional_techno(section, "Factory");

    if let Some(list) = value(section, "Houses", &mut buf) {
        for country in ini::split(list).filter(|n| !n.is_empty()) {
            if counter.countries_len == MAX_TYPES {
                stop(section, "Houses", "more than 8 countries");
            }
            let Ok(id) = lookup(NameKind::Country, country) else {
                stop(section, "Houses", "not a country");
            };
            counter.countries[counter.countries_len] = TypeRef::new(NameKind::Country, id).0;
            counter.countries_len += 1;
        }
    }

    counter.stage = match value(section, "Stage", &mut buf).map(ini::int) {
        None => 0,
        Some(Some(n)) if (0..=MAX_STAGE).contains(&n) => n,
        Some(_) => stop(section, "Stage", "needs a number from 0 to 20"),
    };

    // One trigger for the counter: `Birth` of any counted type, queueing
    // `onCounted` with the counter's index as its parameter text.
    let mut trigger = Trigger::on(Event::BIRTH);
    for ty in &counter.count[..counter.count_len] {
        trigger = trigger.bind(*ty);
    }
    if trigger
        .enqueue_with(ON_COUNTED, Target::Subject, INDEX_TEXT[index])
        .done()
        .is_err()
    {
        stop(section, "Count", "the Birth trigger could not be made");
    }

    s.counters[index] = counter;
    s.len += 1;
    Line::new()
        .text("IniCounter: counter ")
        .num(index as i64)
        .text(" [")
        .text(section)
        .text("]: every ")
        .num(counter.threshold)
        .text(" births earn a reward")
        .log();
    mark(s, MARK_ENTRY, "DEMO-OK entry");
}

/// After the `IniEntry` records: the code trigger that hears the global
/// event.
fn start() {
    let event = REWARD_GRANTED.get();
    if event.0 < 0 {
        return;
    }
    if Trigger::on_module_event(event)
        .enqueue(ON_REWARD_GRANTED)
        .done()
        .is_err()
    {
        log("IniCounter: the Demo.RewardGranted listener could not be made");
    }
}

// ----------------------------------------------------------------- Houses

/// `house`'s index, the slot of its tallies.
fn house_slot(house: House) -> Option<usize> {
    let index = get(house, Prop::Index).ok()?;
    usize::try_from(index).ok().filter(|&i| i < MAX_HOUSES)
}

/// Whether `counter` is for `house`'s country.
fn applies(counter: &Counter, house: House) -> bool {
    if counter.countries_len == 0 {
        return true;
    }
    let Ok(country) = get(house, Prop::Country) else {
        return false;
    };
    counter.countries[..counter.countries_len].contains(&country)
}

/// `house`'s first object of type `kind`, or `NO_HANDLE`.
fn owned(house: House, kind: TypeRef) -> i64 {
    Find::objects().owned_by(house).of_type(kind).first()
}

/// Whether `house` meets `counter`'s `Requires=`.
fn requirement_met(counter: &Counter, house: House) -> bool {
    counter.requires.is_none() || owned(house, counter.requires) != NO_HANDLE
}

fn first_building(house: House) -> i64 {
    Find::objects()
        .owned_by(house)
        .where_prop(Prop::Category, Op::Eq, Category::Building as i64)
        .first()
}

fn cell_of(object: i64) -> Option<Cell> {
    if object == NO_HANDLE {
        return None;
    }
    get(Techno(object), Prop::Cell).ok().map(Cell)
}

/// Creates a `kind` for `house` on the first free cell of the rings
/// `from..=to` cells around `center`.
fn place_near(kind: TypeRef, house: House, center: Cell, from: i32, to: i32) -> Option<Techno> {
    for ring in from..=to {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dy.abs() != ring {
                    continue; // Inside the ring.
                }
                let x = i32::from(center.x()) + dx;
                let y = i32::from(center.y()) + dy;
                if !(1..=511).contains(&x) || !(1..=511).contains(&y) {
                    continue;
                }
                // An occupied or off-map cell turns the object down; try on.
                if let Ok(object) = Action::create_object(Cell::at(x as u16, y as u16), kind, house)
                {
                    return Some(object);
                }
            }
        }
    }
    None
}

// --------------------------------------------------------------- Counting

/// A counted type was born: count it for its house while the requirement
/// holds, and grant the reward at the threshold.
fn on_counted(event: BirthEvent) {
    let s = state();
    let mut text = [0u8; 8];
    let index = event
        .record()
        .args_text(&mut text)
        .ok()
        .and_then(ini::int)
        .and_then(|i| usize::try_from(i).ok())
        .filter(|&i| i < s.len);
    let Some(index) = index else {
        return;
    };
    // The object may be gone by now (killed in the frame it appeared).
    let house = event.object().owner().unwrap_or(event.record().house());
    if house.is_none() {
        return;
    }
    let Some(slot) = house_slot(house) else {
        return;
    };
    let counter = s.counters[index];
    if !applies(&counter, house) || !requirement_met(&counter, house) {
        return;
    }
    s.counters[index].tally[slot] += 1;
    mark(s, MARK_COUNT, "DEMO-OK count");
    // A reward with no room stays due: the next birth tries again.
    if s.counters[index].tally[slot] >= counter.threshold && grant(s, index, house) {
        s.counters[index].tally[slot] -= counter.threshold;
    }
}

/// Creates counter `index`'s reward for `house` and raises the events;
/// `false` when there was no room for it.
fn grant(s: &mut State, index: usize, house: House) -> bool {
    let counter = s.counters[index];
    let factory = if counter.factory.is_none() {
        NO_HANDLE
    } else {
        owned(house, counter.factory)
    };
    // Out through the factory's door; this engine does not take a factory
    // as CreateObject's target yet (`Unsupported`), so a cell beside it.
    let at_door = if factory == NO_HANDLE {
        None
    } else {
        Action::create_object(Techno(factory), counter.reward, house).ok()
    };
    let reward = at_door.or_else(|| {
        let anchor = if factory == NO_HANDLE {
            first_building(house)
        } else {
            factory
        };
        place_near(counter.reward, house, cell_of(anchor)?, 2, 8)
    });
    let Some(reward) = reward else {
        return false;
    };
    mark(s, MARK_REWARD, "DEMO-OK reward");
    if s.reward_lines < 8 {
        s.reward_lines += 1;
        Line::new()
            .text("IniCounter: counter ")
            .num(index as i64)
            .text(" granted a reward to house ")
            .num(get(house, Prop::Index).unwrap_or(-1))
            .log();
    }
    let arrived = REWARD_ARRIVED.get();
    if arrived.0 >= 0 {
        let _ = Action::raise_event(arrived, reward, Techno(NO_HANDLE), index as i64, 0);
    }
    let granted = REWARD_GRANTED.get();
    if granted.0 >= 0 {
        let _ = Action::raise_event_with_house(
            granted,
            Global,
            Techno(NO_HANDLE),
            index as i64,
            0,
            house,
        );
    }
    true
}

/// The code trigger on the global `Demo.RewardGranted`.
fn on_reward_granted(event: ModuleEventRecord) {
    let s = state();
    if s.marks & MARK_EVENT == 0 {
        Line::new()
            .text("IniCounter: Demo.RewardGranted heard (counter ")
            .num(event.value())
            .text(", house ")
            .num(get(event.house(), Prop::Index).unwrap_or(-1))
            .text(")")
            .log();
    }
    mark(s, MARK_EVENT, "DEMO-OK event");
}

/// The INI trigger on the object event `Demo.RewardArrived`
/// (`rulesmd.ini`, `[DemoRewardArrived]`).
fn on_reward_arrived(_event: ModuleEventRecord) {
    mark(
        state(),
        MARK_ARRIVED,
        "IniCounter: an INI trigger heard Demo.RewardArrived on the new unit",
    );
}

// ---------------------------------------------------------------- Staging

/// Demo only (`Stage=`): gives each computer house of a counter's
/// `Houses` what the counter needs, then `Stage` counted units.
fn tick() {
    let s = state();
    let now = frame();
    if s.staging_done || !(STAGE_FROM..=STAGE_UNTIL).contains(&now) {
        return;
    }
    let mut houses = [NO_HANDLE; MAX_HOUSES];
    let Ok((found, _)) = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 0)
        .where_prop(Prop::IsDefeated, Op::Eq, 0)
        .page(&mut houses, 0)
    else {
        return;
    };
    let mut pending = false;
    for index in 0..s.len {
        let counter = s.counters[index];
        // Without `Houses=` the computer houses include the neutral ones.
        if counter.stage == 0 || counter.countries_len == 0 {
            continue;
        }
        for &house in &houses[..found] {
            let house = House(house);
            if !applies(&counter, house) {
                continue;
            }
            let Some(slot) = house_slot(house) else {
                continue;
            };
            if counter.staged[slot] {
                continue;
            }
            if stage(&counter, house, now) {
                s.counters[index].staged[slot] = true;
            } else {
                pending = true;
            }
        }
    }
    if !pending {
        s.staging_done = true;
    }
}

/// Stages `counter` for `house`; `false` while the house has nothing to
/// stage beside.
fn stage(counter: &Counter, house: House, now: i32) -> bool {
    let mut anchor = first_building(house);
    if anchor == NO_HANDLE && now >= STAGE_ANY_OBJECT_FROM {
        anchor = Find::objects().owned_by(house).first();
    }
    let Some(center) = cell_of(anchor) else {
        return false;
    };
    for building in [counter.requires, counter.factory] {
        if !building.is_none()
            && owned(house, building) == NO_HANDLE
            && place_near(building, house, center, 4, 14).is_none()
        {
            log("IniCounter: no room to stage a building");
        }
    }
    let mut made = 0;
    for _ in 0..counter.stage {
        if place_near(counter.count[0], house, center, 3, 10).is_some() {
            made += 1;
        }
    }
    Line::new()
        .text("IniCounter: staged ")
        .num(made)
        .text(" units for house ")
        .num(get(house, Prop::Index).unwrap_or(-1))
        .log();
    true
}

// ------------------------------------------------------------------- Text

/// A log line built without an allocator (`core::fmt` would pull in code
/// the engine turns down).
struct Line {
    buf: [u8; 160],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0; 160],
            len: 0,
        }
    }

    fn text(mut self, text: &str) -> Self {
        for &byte in text.as_bytes() {
            if self.len == self.buf.len() {
                break;
            }
            self.buf[self.len] = byte;
            self.len += 1;
        }
        self
    }

    fn num(self, value: i64) -> Self {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = value.unsigned_abs();
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        let line = if value < 0 { self.text("-") } else { self };
        line.text(core::str::from_utf8(&digits[at..]).unwrap_or("?"))
    }

    fn log(&self) {
        if let Ok(text) = core::str::from_utf8(&self.buf[..self.len]) {
            log(text);
        }
    }
}

vera20k_sdk::init!(init);
vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(every = 15, tick);
vera20k_sdk::on_events!(
    COUNTERS => load_counter,
    ON_COUNTED => on_counted,
    ON_REWARD_GRANTED => on_reward_granted,
    ON_REWARD_ARRIVED => on_reward_arrived,
);
