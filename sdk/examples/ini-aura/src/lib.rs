//! "IniAura", a demo `rules.wasm` for RFC 0001 section 6h
//! (RFC 0001 section 6h): the module supplies building
//! blocks, the modder writes every aura in INI. Nothing about a particular
//! aura is in this code.
//!
//! - Registry `[Demo.AuraTypes]`: a list of aura sections. Each holds
//!   `Radius=` (cells), `Property=` (a modifiable property the engine wires:
//!   `FirepowerMultiplier`, `SpeedMultiplier`, `ArmorMultiplier`,
//!   `ROFMultiplier`), `Op=` (`add`, `multiply`, `override`), `Value=` (a
//!   decimal such as `1.25`), `Affects=` (`allies` or `enemies`) and
//!   `Interval=` (frames between refreshes, at least 15).
//! - Key `Demo.Aura=` on a TechnoType: the aura sections the type emits,
//!   comma-separated.
//! - Object event `Demo.AuraEntered`: raised on a unit the first time an
//!   aura covers it (`value` is the aura's position in the list, `other`
//!   the emitter). INI triggers listen with `Event=Wasm:Demo.AuraEntered`.
//! - Module action `auraHello`: logs its parameter text once, for
//!   `Action1=Wasm:auraHello(Subject,welcome)`.
//!
//! Each aura refreshes every `Interval` frames, staggered: a refresh walks
//! the emitters of its types a few per frame, finds the allies or enemies
//! in range and gives each a modifier lasting a little longer than the
//! interval. The modifier group is the aura's, so a second emitter of the
//! same aura replaces the modifier instead of stacking it.
//!
//! `on_scenario_start` places a few emitters and units of the human player
//! and its enemy beside the player's first object, so the demo shows
//! without any input. The first time each step happens it logs a
//! `DEMO-OK` line (entry, key, apply, event, action).
#![no_std]

use core::cell::UnsafeCell;

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get};

// ------------------------------------------------------------------ Names

/// Dispatch keys: the module action, then the INI registrations.
vera20k_sdk::named_actions! { AURA_HELLO = "auraHello" => 1 }
const AURA_TYPES: i32 = 10;
const AURA_KEY: i32 = 11;

static AURA_ENTERED: ini::EventSlot = ini::EventSlot::new();

// The auras that have covered an object: bit `i` for aura `i`.
vera20k_sdk::vars! {
    static SEEN: ObjectVar = ("AuraSeen", Private, 0);
}

// ------------------------------------------------------------------ State

const MAX_AURAS: usize = 16;
const MAX_EMITTER_TYPES: usize = 64;
const NAME_BYTES: usize = 32;

/// Emitters handled per frame, over all auras.
const EMITTERS_PER_FRAME: usize = 8;
/// `find` pages of emitters per frame, empty ones included.
const EMITTER_PAGES_PER_FRAME: usize = 16;
/// Units one emitter reaches per refresh: pages of [`TARGET_PAGE`].
const TARGET_PAGES: usize = 2;
const TARGET_PAGE: usize = 32;
/// A modifier lasts its aura's interval plus this, so a refresh spread
/// over a few frames never leaves a gap.
const SLACK: i32 = 30;

const MIN_INTERVAL: i64 = 15;
const MAX_INTERVAL: i64 = 900;
const MAX_RADIUS: i64 = 20;
const LEPTONS_PER_CELL: i64 = 256;

/// The `DEMO-OK` marks, one bit each.
const MARK_ENTRY: u32 = 1;
const MARK_KEY: u32 = 2;
const MARK_APPLY: u32 = 4;
const MARK_EVENT: u32 = 8;
const MARK_ACTION: u32 = 16;

#[derive(Clone, Copy)]
struct Aura {
    name: [u8; NAME_BYTES],
    name_len: usize,
    prop: Prop,
    op: ModifierOp,
    value: i64,
    /// Leptons.
    radius: i32,
    enemies: bool,
    interval: i32,
    /// The frame the next refresh starts.
    next_due: i32,
    /// A refresh is walking the emitters.
    active: bool,
    /// The emitter type it is at, and the `find` position within it.
    type_cursor: usize,
    after: i64,
}

impl Aura {
    const EMPTY: Self = Self {
        name: [0; NAME_BYTES],
        name_len: 0,
        prop: Prop(0),
        op: ModifierOp::Multiply,
        value: 0,
        radius: 0,
        enemies: false,
        interval: 0,
        next_due: 0,
        active: false,
        type_cursor: 0,
        after: 0,
    };

    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

#[derive(Clone, Copy)]
struct EmitterType {
    ty: TypeRef,
    /// Bit `i`: emits aura `i`.
    mask: u32,
}

struct State {
    auras: [Aura; MAX_AURAS],
    aura_count: usize,
    emitters: [EmitterType; MAX_EMITTER_TYPES],
    emitter_count: usize,
    marks: u32,
}

/// Module memory; a module runs on one thread and nothing re-enters it
/// during a host call, so one `&mut` at a time is safe. Saves keep it.
struct Shared(UnsafeCell<State>);

// SAFETY: a Wasm module has one thread.
unsafe impl Sync for Shared {}

static STATE: Shared = Shared(UnsafeCell::new(State {
    auras: [Aura::EMPTY; MAX_AURAS],
    aura_count: 0,
    emitters: [EmitterType {
        ty: TypeRef(0),
        mask: 0,
    }; MAX_EMITTER_TYPES],
    emitter_count: 0,
    marks: 0,
}));

/// The state; callers drop the borrow before taking it again.
fn state() -> &'static mut State {
    // SAFETY: see `Shared`; no caller keeps two borrows alive at once.
    unsafe { &mut *STATE.0.get() }
}

/// Whether `mark` is new; records it.
fn first(mark: u32) -> bool {
    let st = state();
    let new = st.marks & mark == 0;
    st.marks |= mark;
    new
}

// ------------------------------------------------------------------- Init

fn init() {
    if ini::registry("Demo.AuraTypes", AURA_TYPES).is_err() {
        log("ini-aura: Demo.AuraTypes is already taken");
    }
    if ini::key("Demo.Aura", IniSection::TechnoType, AURA_KEY).is_err() {
        log("ini-aura: Demo.Aura is already taken");
    }
    match ini::event("Demo.AuraEntered", EventScope::Object) {
        Ok(event) => AURA_ENTERED.set(event),
        Err(_) => log("ini-aura: Demo.AuraEntered is already taken"),
    }
}

// ------------------------------------------------------- INI: aura types

/// One `[Demo.AuraTypes]` entry: reads and checks its section.
fn load_aura(record: IniEntryRecord) {
    let mut section_buf = [0u8; 64];
    let Ok(section) = record.entry_name(&mut section_buf) else {
        stop(Line::new().text("ini-aura: [Demo.AuraTypes] an entry name is too long"));
    };
    if section.len() > NAME_BYTES {
        stop(bad(section, "", "the section name is over 32 characters"));
    }
    if state().aura_count == MAX_AURAS {
        stop(bad(section, "", "more than 16 aura types"));
    }

    let mut buf = [0u8; 64];
    let radius = match Rules::get(section, "Radius", &mut buf)
        .ok()
        .and_then(ini::int)
    {
        Some(cells) if (1..=MAX_RADIUS).contains(&cells) => cells,
        _ => stop(bad(
            section,
            "Radius",
            "needs a whole number of cells, 1 to 20",
        )),
    };

    let prop = match Rules::get(section, "Property", &mut buf) {
        Ok(name) => match Prop::named(name.trim_ascii()) {
            Ok(prop) if WIRED.contains(&prop) => prop,
            _ => stop(bad(
                section,
                "Property",
                "needs FirepowerMultiplier, SpeedMultiplier, ArmorMultiplier or ROFMultiplier",
            )),
        },
        Err(_) => stop(bad(section, "Property", "is missing")),
    };

    let op = match Rules::get(section, "Op", &mut buf) {
        Err(_) => ModifierOp::Multiply,
        Ok(text) => {
            let text = text.trim_ascii();
            if text.is_empty() || text.eq_ignore_ascii_case("multiply") {
                ModifierOp::Multiply
            } else if text.eq_ignore_ascii_case("add") {
                ModifierOp::Add
            } else if text.eq_ignore_ascii_case("override") {
                ModifierOp::Override
            } else {
                stop(bad(section, "Op", "needs add, multiply or override"))
            }
        }
    };

    let value = match Rules::get(section, "Value", &mut buf)
        .ok()
        .and_then(parse_fixed)
    {
        Some(value) => value,
        None => stop(bad(section, "Value", "needs a decimal number such as 1.25")),
    };

    let enemies = match Rules::get(section, "Affects", &mut buf) {
        Err(_) => false,
        Ok(text) => {
            let text = text.trim_ascii();
            if text.is_empty() || text.eq_ignore_ascii_case("allies") {
                false
            } else if text.eq_ignore_ascii_case("enemies") {
                true
            } else {
                stop(bad(section, "Affects", "needs allies or enemies"))
            }
        }
    };

    let interval = match Rules::get(section, "Interval", &mut buf) {
        Err(_) => 30,
        Ok(text) => match ini::int(text) {
            Some(frames) if (MIN_INTERVAL..=MAX_INTERVAL).contains(&frames) => frames,
            _ => stop(bad(section, "Interval", "needs 15 to 900 frames")),
        },
    };

    let st = state();
    let mut aura = Aura::EMPTY;
    aura.name[..section.len()].copy_from_slice(section.as_bytes());
    aura.name_len = section.len();
    aura.prop = prop;
    aura.op = op;
    aura.value = value;
    aura.radius = (radius * LEPTONS_PER_CELL) as i32;
    aura.enemies = enemies;
    aura.interval = interval as i32;
    st.auras[st.aura_count] = aura;
    st.aura_count += 1;

    if first(MARK_ENTRY) {
        Line::new()
            .text("DEMO-OK entry ")
            .text(section)
            .text(" radius ")
            .num(radius)
            .text(" interval ")
            .num(interval)
            .log();
    }
}

/// The properties whose owners read modifiers (status document, "Wired").
const WIRED: [Prop; 4] = [
    Prop::FirepowerMultiplier,
    Prop::SpeedMultiplier,
    Prop::ArmorMultiplier,
    Prop::ROFMultiplier,
];

/// A decimal such as `1.25` or `-0.5` as a `fixed` value (16 fraction
/// bits), rounded; integer arithmetic only.
fn parse_fixed(text: &str) -> Option<i64> {
    let text = text.trim_ascii();
    let (negative, rest) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let (whole, fraction) = rest.split_once('.').unwrap_or((rest, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let mut int: i64 = 0;
    for byte in whole.bytes() {
        let digit = byte.checked_sub(b'0').filter(|d| *d <= 9)?;
        int = int * 10 + i64::from(digit);
        if int > 32767 {
            return None;
        }
    }
    let (mut num, mut den) = (0i64, 1i64);
    for byte in fraction.bytes() {
        let digit = byte.checked_sub(b'0').filter(|d| *d <= 9)?;
        // Six digits are finer than 1/65536; the rest are checked only.
        if den < 1_000_000 {
            num = num * 10 + i64::from(digit);
            den *= 10;
        }
    }
    let value = int * FIXED_ONE + (num * FIXED_ONE + den / 2) / den;
    Some(if negative { -value } else { value })
}

// ------------------------------------------------------ INI: Demo.Aura=

/// One TechnoType with `Demo.Aura=`: the auras it emits.
fn load_emitter(record: IniKeyRecord) {
    let mut value_buf = [0u8; 256];
    let mut section_buf = [0u8; 64];
    let Ok(value) = record.value(&mut value_buf) else {
        let section = record.section_name(&mut section_buf).unwrap_or("?");
        stop(bad(section, "Demo.Aura", "is over 256 characters"));
    };
    let mut mask = 0u32;
    for part in ini::split(value).filter(|part| !part.is_empty()) {
        let st = state();
        let found = st.auras[..st.aura_count]
            .iter()
            .position(|aura| aura.name().eq_ignore_ascii_case(part));
        match found {
            Some(index) => mask |= 1 << index,
            None => {
                let section = record.section_name(&mut section_buf).unwrap_or("?");
                stop(
                    Line::new()
                        .text("ini-aura: [")
                        .text(section)
                        .text("] Demo.Aura: ")
                        .text(part)
                        .text(" is not listed in [Demo.AuraTypes]"),
                );
            }
        }
    }
    if mask == 0 {
        return;
    }
    let st = state();
    if st.emitter_count == MAX_EMITTER_TYPES {
        let section = record.section_name(&mut section_buf).unwrap_or("?");
        stop(bad(section, "Demo.Aura", "more than 64 types emit auras"));
    }
    st.emitters[st.emitter_count] = EmitterType {
        ty: record.type_ref(),
        mask,
    };
    st.emitter_count += 1;

    if first(MARK_KEY) {
        let section = record.section_name(&mut section_buf).unwrap_or("?");
        Line::new()
            .text("DEMO-OK key ")
            .text(section)
            .text(" emits ")
            .text(value)
            .log();
    }
}

// ------------------------------------------------------------------ Start

fn start() {
    let now = frame();
    let st = state();
    for (i, aura) in st.auras[..st.aura_count].iter_mut().enumerate() {
        // Staggered, so the auras do not all refresh on one frame.
        aura.next_due = now + 1 + 7 * i as i32;
    }
    Line::new()
        .text("ini-aura: ")
        .num(st.aura_count as i64)
        .text(" aura types, ")
        .num(st.emitter_count as i64)
        .text(" emitting types")
        .log();
    stage();
}

/// What `stage` places: type, the enemy's (else the player's), cells to
/// the side of the player's first object.
const STAGED: [(&str, bool, i32); 6] = [
    ("MTNK", false, 3),
    ("E1", false, 3),
    ("E1", false, 3),
    ("HTNK", true, 6),
    ("E2", true, 6),
    ("E2", true, 6),
];

/// Cells tried around a cluster's center, nearest first.
const OFFSETS: [(i32, i32); 21] = [
    (0, 0),
    (1, 0),
    (0, 1),
    (-1, 0),
    (0, -1),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
    (2, 0),
    (0, 2),
    (-2, 0),
    (0, -2),
    (2, 1),
    (1, 2),
    (-2, -1),
    (-1, -2),
    (2, 2),
    (-2, 2),
    (2, -2),
    (-2, -2),
];

/// Places a few emitters and units of the human player and its enemy close
/// enough for both auras to reach across.
fn stage() {
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    if human == NO_HANDLE {
        log("ini-aura: no human player, nothing staged");
        return;
    }
    let human = House(human);
    let enemy = enemy_of(human);
    let anchor = Find::objects().owned_by(human).first();
    let Ok(at) = get(Techno(anchor), Prop::Cell) else {
        log("ini-aura: the player has no object, nothing staged");
        return;
    };
    let at = Cell(at);
    let (x, y) = (i32::from(at.x()), i32::from(at.y()));
    let mut placed = 0;
    for (name, for_enemy, dx) in STAGED {
        let house = if for_enemy { enemy } else { human };
        if house.is_none() {
            continue;
        }
        let Ok(ty) = TypeRef::techno_type(name) else {
            continue;
        };
        // To the right of the anchor, else to its left.
        if place(ty, house, x + dx, y + 1) || place(ty, house, x - dx, y + 1) {
            placed += 1;
        }
    }
    Line::new()
        .text("ini-aura: staged ")
        .num(placed)
        .text(" units beside the player")
        .log();
}

/// The first undefeated enemy house of `human` that owns an object.
fn enemy_of(human: House) -> House {
    let mut find = Find::houses()
        .seen_by(human)
        .where_prop(Prop::Relation, Op::Eq, Relation::Enemy as i64)
        .where_prop(Prop::IsDefeated, Op::Eq, 0);
    let mut buf = [NO_HANDLE; 8];
    let mut after = 0;
    for _ in 0..8 {
        let Ok((count, next)) = find.page(&mut buf, after) else {
            break;
        };
        for &house in &buf[..count] {
            if Find::objects().owned_by(House(house)).first() != NO_HANDLE {
                return House(house);
            }
        }
        if next == 0 {
            break;
        }
        after = next;
    }
    House(NO_HANDLE)
}

/// Creates `ty` for `house` on the first free cell around (`cx`, `cy`).
fn place(ty: TypeRef, house: House, cx: i32, cy: i32) -> bool {
    for (dx, dy) in OFFSETS {
        let (x, y) = (cx + dx, cy + dy);
        if !(0..=i32::from(u16::MAX)).contains(&x) || !(0..=i32::from(u16::MAX)).contains(&y) {
            continue;
        }
        if Action::create_object(Cell::at(x as u16, y as u16), ty, house).is_ok() {
            return true;
        }
    }
    false
}

// ------------------------------------------------------------------ Frame

fn tick() {
    let now = frame();
    let count = state().aura_count;
    if count == 0 || state().emitter_count == 0 {
        return;
    }
    let mut emitters_left = EMITTERS_PER_FRAME;
    let mut pages_left = EMITTER_PAGES_PER_FRAME;
    let start = now as usize % count;
    for k in 0..count {
        let i = (start + k) % count;
        {
            let aura = &mut state().auras[i];
            if !aura.active && now >= aura.next_due {
                aura.active = true;
                aura.type_cursor = 0;
                aura.after = 0;
                aura.next_due = now + aura.interval;
            }
        }
        while state().auras[i].active && emitters_left > 0 && pages_left > 0 {
            pages_left -= 1;
            emitters_left -= refresh_page(i, emitters_left);
        }
    }
}

/// One page of aura `i`'s emitters, at most `room`; returns how many.
fn refresh_page(i: usize, room: usize) -> usize {
    let st = state();
    let bit = 1u32 << i;
    let mut cursor = st.auras[i].type_cursor;
    while cursor < st.emitter_count && st.emitters[cursor].mask & bit == 0 {
        cursor += 1;
    }
    if cursor >= st.emitter_count {
        st.auras[i].active = false;
        return 0;
    }
    let mut buf = [NO_HANDLE; EMITTERS_PER_FRAME];
    let want = room.min(buf.len());
    let mut find = Find::objects().of_type(st.emitters[cursor].ty);
    let (count, next) = find
        .page(&mut buf[..want], st.auras[i].after)
        .unwrap_or((0, 0));
    let aura = &mut st.auras[i];
    if next == 0 {
        aura.type_cursor = cursor + 1;
        aura.after = 0;
    } else {
        aura.type_cursor = cursor;
        aura.after = next;
    }
    let aura = *aura;
    for &emitter in &buf[..count] {
        cover(i, &aura, Techno(emitter));
    }
    count
}

/// Gives every unit aura `i` of `emitter` reaches its modifier.
fn cover(i: usize, aura: &Aura, emitter: Techno) {
    let Ok(owner) = emitter.owner() else { return };
    let Ok(at) = get(emitter, Prop::Cell) else {
        return;
    };
    // Self (0) and allies (1), or enemies (2); never neutral houses.
    let (op, relation) = if aura.enemies {
        (Op::Eq, Relation::Enemy)
    } else {
        (Op::Le, Relation::Ally)
    };
    let mut find = Find::objects()
        .near(Cell(at), aura.radius)
        .seen_by(owner)
        .where_prop(Prop::Category, Op::Le, Category::Vehicle as i64)
        .where_prop(Prop::Relation, op, relation as i64);
    let mut buf = [NO_HANDLE; TARGET_PAGE];
    let mut after = 0;
    for _ in 0..TARGET_PAGES {
        let Ok((count, next)) = find.page(&mut buf, after) else {
            return;
        };
        for &unit in &buf[..count] {
            touch(i, aura, emitter, Techno(unit));
        }
        if next == 0 {
            return;
        }
        after = next;
    }
}

/// Applies aura `i` to `unit`; on the first cover, raises
/// `Demo.AuraEntered` on it.
fn touch(i: usize, aura: &Aura, emitter: Techno, unit: Techno) {
    // The group is the aura's: re-applying replaces, it never stacks.
    let applied = Modifier::new(
        unit,
        aura.prop,
        aura.op,
        aura.value,
        aura.interval + SLACK,
        emitter.0,
        i as i32 + 1,
    );
    if applied.is_ok() && first(MARK_APPLY) {
        Line::new().text("DEMO-OK apply ").text(aura.name()).log();
    }
    let event = AURA_ENTERED.get();
    if event.0 < 0 {
        // Registration failed in init (logged there): an unknown event ID
        // would stop the module.
        return;
    }
    let bit = 1i64 << i;
    let Ok(seen) = SEEN.get(unit) else { return };
    if seen & bit != 0 || SEEN.set(unit, seen | bit).is_err() {
        return;
    }
    let raised = Action::raise_event(event, unit, emitter, i as i64, 0);
    if raised.is_ok() && first(MARK_EVENT) {
        Line::new()
            .text("DEMO-OK event Demo.AuraEntered for ")
            .text(aura.name())
            .log();
    }
}

// ----------------------------------------------------------------- Action

/// `Wasm:auraHello(target,text)`: logs the parameter text the first time.
fn aura_hello(record: &EventRecord) {
    let mut buf = [0u8; 64];
    let text = record.args_text(&mut buf).unwrap_or("");
    if !text.is_empty() && first(MARK_ACTION) {
        Line::new()
            .text("DEMO-OK action auraHello(")
            .text(text)
            .text(")")
            .log();
    }
}

// ---------------------------------------------------------------- Helpers

/// A message naming `[section] key` for a bad value.
fn bad(section: &str, key: &str, why: &str) -> Line {
    let line = Line::new().text("ini-aura: [").text(section).text("] ");
    let line = if key.is_empty() {
        line
    } else {
        line.text(key).text(" ")
    };
    line.text(why)
}

/// Logs `line` and stops the module: a wrong INI value stops it, as the
/// RFC 0001 section 6h asks.
fn stop(line: Line) -> ! {
    line.log();
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    loop {}
}

/// A log line built without `core::fmt`, which pulls in code the engine
/// refuses to load.
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
vera20k_sdk::on_frame!(tick);
vera20k_sdk::on_events!(
    AURA_HELLO => aura_hello,
    AURA_TYPES => load_aura,
    AURA_KEY => load_emitter,
);
