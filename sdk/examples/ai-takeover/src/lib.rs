//! "AI Takeover", a demo `rules.wasm` for driving a house (RFC 0001
//! section 6i): the module claims every computer house and plays it with a
//! small scripted AI, through orders only (no `CreateObject`).
//!
//! - Claims: it writes `ControlDomains` = 15 (all four `Domain` bits) on
//!   each computer house that owns a Construction Yard or an MCV, reads it
//!   back, and checks that the same write on the human house is turned
//!   down (`Status::Refused`). With the mod's `[VeraWasm] Controller=yes`
//!   and a "mod AI" seat the house starts claimed already; the write then
//!   only confirms it.
//! - The house: deploy the MCV (`DeployMcv`), then a build order of power
//!   plant, barracks, refinery and war factory (Soviet or Allied, from the
//!   MCV or yard the house owns), each `QueueProduction` once the building
//!   factory is idle (`Factory.Building.*`) and placed with
//!   `PlaceReadyBuilding` at a cell `find` answers `Placeable=1` for, near
//!   the yard. With a barracks and a war factory it trains infantry and
//!   tanks and lets each ready one out (`TakeProduct`); once the war factory
//!   stands and the army has three or more, it sends it at the human
//!   player's base (`AttackMove`), again every 450 frames. `find` also
//!   returns the product a factory holds, off the map, so a building counts
//!   as standing only beyond the one the building factory holds.
//! - Results: a `Commanded` trigger bound to the unit types and a global
//!   `OrderRefused` trigger deliver records to `on_events`; a `Birth`
//!   trigger on the trained types tells it a unit left the factory.
//!
//! Work runs in `on_frame` every 15 frames, one claimed house per visit.
//! It logs `DEMO-OK <feature>` the first time each feature works (the
//! engine test `sim::ext_modules::tests::ini_demo_mods_reach_their_marks`
//! checks them) and a status line every 600 frames. The log never says
//! the word the test treats as a failure, so a turned-down order is
//! "order-rejected" here.
#![no_std]

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get, set};

/// Dispatch keys of the module's triggers (`on_events`).
const ON_COMMANDED: i32 = 1;
const ON_NOT_CARRIED_OUT: i32 = 2;
const ON_BIRTH: i32 = 3;

/// Every `Domain` bit: Production, Forces, Abilities, SuperWeapons.
const ALL_DOMAINS: i64 = 0b1111;
/// Houses this module drives at most.
const MAX_SEATS: usize = 4;
/// Units an attack waits for.
const ATTACK_SIZE: usize = 3;
/// Frames between attack orders.
const ATTACK_EVERY: i64 = 450;
/// Units the module trains per house at most, so the credits last.
const MAX_TRAINED: i64 = 24;
/// `RefuseReason` (appendix 8.3, enum 26) values it reads.
const REASON_CANNOT_PLACE: i64 = 7;
/// `Factory.<Kind>` indices `TakeProduct` takes.
const KIND_INFANTRY: i64 = 2;
const KIND_VEHICLE: i64 = 3;

// ------------------------------------------------------------------ Memory
// Module memory is saved and hashed with the match, so plain statics do.

fn ld(a: &AtomicI64) -> i64 {
    a.load(Relaxed)
}

fn st(a: &AtomicI64, value: i64) {
    a.store(value, Relaxed)
}

// ------------------------------------------------------------------- Sides

/// What one side builds, by TechnoType name.
struct Side {
    name: &'static str,
    mcv: &'static str,
    yard: &'static str,
    /// Power plant, barracks, refinery, war factory: the build order.
    build: [&'static str; BUILD_STEPS],
    infantry: &'static str,
    vehicle: &'static str,
}

const BUILD_STEPS: usize = 4;
const STEP_BARRACKS: usize = 1;
const STEP_WAR_FACTORY: usize = 3;

const SIDES: [Side; 2] = [
    Side {
        name: "Soviet",
        mcv: "SMCV",
        yard: "NACNST",
        build: ["NAPOWR", "NAHAND", "NAREFN", "NAWEAP"],
        infantry: "E2",
        vehicle: "HTNK",
    },
    Side {
        name: "Allied",
        mcv: "AMCV",
        yard: "GACNST",
        build: ["GAPOWR", "GAPILE", "GAREFN", "GAWEAP"],
        infantry: "E1",
        vehicle: "MTNK",
    },
];

/// Type slots per side: MCV, yard, the build order, infantry, vehicle.
const SLOT_MCV: usize = 0;
const SLOT_YARD: usize = 1;
const SLOT_BUILD: usize = 2;
const SLOT_INFANTRY: usize = SLOT_BUILD + BUILD_STEPS;
const SLOT_VEHICLE: usize = SLOT_INFANTRY + 1;
const SLOTS: usize = SLOT_VEHICLE + 1;

/// Type references by side and slot, resolved once; 0 when the rules lack
/// the type.
static TYPES: [[AtomicI64; SLOTS]; 2] = [const { [const { AtomicI64::new(0) }; SLOTS] }; 2];

fn ty(side: usize, slot: usize) -> TypeRef {
    TypeRef(ld(&TYPES[side][slot]))
}

fn resolve_types() {
    for (side, names) in SIDES.iter().enumerate() {
        let mut all = [""; SLOTS];
        all[SLOT_MCV] = names.mcv;
        all[SLOT_YARD] = names.yard;
        all[SLOT_BUILD..SLOT_BUILD + BUILD_STEPS].copy_from_slice(&names.build);
        all[SLOT_INFANTRY] = names.infantry;
        all[SLOT_VEHICLE] = names.vehicle;
        for (slot, name) in all.iter().enumerate() {
            match TypeRef::techno_type(name) {
                Ok(found) => st(&TYPES[side][slot], found.0),
                Err(_) => Line::new()
                    .text("ai-takeover: the rules lack ")
                    .text(name)
                    .log(),
            }
        }
    }
}

// ------------------------------------------------------------------- Seats

/// Claimed houses and what the module remembers of each.
static SEATS: AtomicI64 = AtomicI64::new(0);
static HOUSE: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
static SIDE: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// Frame of the last `DeployMcv`, 0 before the first; attempts so far.
static DEPLOY_FRAME: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
static DEPLOY_TRIES: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// The building last ordered placed, and where; 0 when none waits.
static PLACING_TYPE: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
static PLACING_CELL: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// Placements the house could not make (it then tries other cells).
static PLACE_FAILS: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// Whether the infantry and vehicle factories read ready last visit.
static INFANTRY_READY: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
static VEHICLE_READY: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// Units queued so far, and whether the module queued any.
static TRAINING: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// Frame of the last attack order, 0 before the first.
static ATTACK_FRAME: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// Whether the early tank order (to be turned down) went out.
static PROVOKED: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];
/// Whether the native AI check ran.
static IDLE_CHECKED: [AtomicI64; MAX_SEATS] = [const { AtomicI64::new(0) }; MAX_SEATS];

/// Visits so far, which picks the seat of each visit.
static VISITS: AtomicI64 = AtomicI64::new(0);

fn seat_house(seat: usize) -> House {
    House(ld(&HOUSE[seat]))
}

fn seat_side(seat: usize) -> usize {
    ld(&SIDE[seat]) as usize
}

fn seat_count() -> usize {
    (ld(&SEATS) as usize).min(MAX_SEATS)
}

/// The seat of `house`, if the module drives it.
fn seat_of(house: i64) -> Option<usize> {
    (0..seat_count()).find(|&seat| ld(&HOUSE[seat]) == house)
}

/// The seat that owns `unit`.
fn seat_of_owner(unit: Techno) -> Option<usize> {
    get(unit, Prop::Owner).ok().and_then(seat_of)
}

// ------------------------------------------------------------------- Marks

static MARKS: AtomicI64 = AtomicI64::new(0);

const MARK_CLAIM: i64 = 1 << 0;
const MARK_DEPLOY: i64 = 1 << 1;
const MARK_BUILD: i64 = 1 << 2;
const MARK_PLACE: i64 = 1 << 3;
const MARK_TRAIN: i64 = 1 << 4;
const MARK_ATTACK: i64 = 1 << 5;
const MARK_REJECTED: i64 = 1 << 6;
const MARK_IDLE: i64 = 1 << 7;
const MARK_BASE: i64 = 1 << 8;
/// One-time notes that are not marks.
const NOTE_NO_CELL: i64 = 1 << 16;
const NOTE_NATIVE_BUILT: i64 = 1 << 17;
const NOTE_DEPLOY_HEARD: i64 = 1 << 18;
const NOTE_NO_SEAT: i64 = 1 << 19;
const NOTE_PLAYER_DEFEATED: i64 = 1 << 20;

/// Logs `line` the first time `bit` is marked.
fn mark(bit: i64, line: &str) {
    let marks = ld(&MARKS);
    if marks & bit == 0 {
        st(&MARKS, marks | bit);
        log(line);
    }
}

/// Detail lines about orders not carried out, kept few: the log holds the
/// latest 256 lines.
static DETAIL_LINES: AtomicI64 = AtomicI64::new(0);
const MAX_DETAIL_LINES: i64 = 8;

// ---------------------------------------------------------------- Searches

/// The first object of `ty` that `house` owns, or `NO_HANDLE`.
fn owned(house: House, ty: TypeRef) -> Techno {
    if ty.is_none() {
        return Techno(NO_HANDLE);
    }
    Techno(Find::objects().owned_by(house).of_type(ty).first())
}

/// Whether `house` has a `building` standing on the map. `find` also
/// returns the product a factory holds (it exists off the map from the
/// start of production), so the one the building factory holds is not
/// counted.
fn standing(house: House, building: TypeRef) -> bool {
    if building.is_none() {
        return false;
    }
    let mut find = Find::objects().owned_by(house).of_type(building);
    let mut page = [NO_HANDLE; 2];
    let Ok((count, _)) = find.page(&mut page, 0) else {
        return false;
    };
    let held = get(house, Prop::Factory_Building_Type).unwrap_or(0) == building.0;
    count > usize::from(held)
}

/// The house's units of `ty`, appended to `army` from `len`; the new length.
fn collect(house: House, ty: TypeRef, army: &mut [Techno], mut len: usize) -> usize {
    if ty.is_none() {
        return len;
    }
    let mut find = Find::objects().owned_by(house).of_type(ty);
    let mut page = [NO_HANDLE; 16];
    let mut after = 0;
    while len < army.len() {
        let Ok((count, next)) = find.page(&mut page, after) else {
            break;
        };
        for &unit in &page[..count] {
            if len < army.len() {
                army[len] = Techno(unit);
                len += 1;
            }
        }
        if next == 0 {
            break;
        }
        after = next;
    }
    len
}

/// The cell of the human player's base: a building of theirs, else any of
/// their objects.
fn enemy_base() -> Option<Cell> {
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    if human == NO_HANDLE {
        return None;
    }
    let human = House(human);
    let mut target = Find::objects()
        .owned_by(human)
        .where_prop(Prop::Category, Op::Eq, Category::Building as i64)
        .first();
    if target == NO_HANDLE {
        target = Find::objects().owned_by(human).first();
    }
    if target == NO_HANDLE {
        return None;
    }
    get(Techno(target), Prop::Cell).ok().map(Cell)
}

// ------------------------------------------------------------------ Claims

/// The side whose MCV or yard `house` owns.
fn side_of(house: House) -> Option<usize> {
    (0..SIDES.len()).find(|&side| {
        !owned(house, ty(side, SLOT_YARD)).is_none() || !owned(house, ty(side, SLOT_MCV)).is_none()
    })
}

/// Claims every computer house with a base, once some exist.
fn claim_seats() {
    let mut houses = [NO_HANDLE; 16];
    let mut find = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 0);
    let mut after = 0;
    let mut all_read_back = true;
    loop {
        let Ok((count, next)) = find.page(&mut houses, after) else {
            break;
        };
        for &raw in &houses[..count] {
            let house = House(raw);
            let seats = seat_count();
            if seats == MAX_SEATS || seat_of(raw).is_some() {
                continue;
            }
            // Civilian and neutral houses own neither an MCV nor a yard.
            let Some(side) = side_of(house) else {
                continue;
            };
            if set(house, Prop::ControlDomains, ALL_DOMAINS).is_err() {
                all_read_back = false;
                continue;
            }
            let held = get(house, Prop::ControlDomains).unwrap_or(0);
            if held != ALL_DOMAINS {
                all_read_back = false;
                continue;
            }
            st(&HOUSE[seats], raw);
            st(&SIDE[seats], side as i64);
            st(&SEATS, seats as i64 + 1);
            Line::new()
                .text("ai-takeover: playing house ")
                .num(get(house, Prop::Index).unwrap_or(-1))
                .text(" (")
                .text(SIDES[side].name)
                .text("), domains ")
                .num(held)
                .log();
        }
        if next == 0 {
            break;
        }
        after = next;
    }
    if seat_count() == 0 {
        return;
    }
    // A player's house takes no claim.
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    let human_declined = human != NO_HANDLE
        && match set(House(human), Prop::ControlDomains, ALL_DOMAINS) {
            Err(Status::Refused) => true,
            Ok(()) => {
                // Not expected: give it back at once.
                let _ = set(House(human), Prop::ControlDomains, 0);
                log("ai-takeover: the human house took the claim");
                false
            }
            Err(_) => false,
        };
    if all_read_back && human_declined {
        mark(MARK_CLAIM, "DEMO-OK claim");
    }
}

// ---------------------------------------------------------------- Triggers

fn make_triggers() {
    // `Commanded` is object-bound: bind the unit types the module orders.
    let mut commanded = Trigger::on(Event::COMMANDED).enqueue(ON_COMMANDED);
    let mut birth = Trigger::on(Event::BIRTH).enqueue(ON_BIRTH);
    for side in 0..SIDES.len() {
        for slot in [SLOT_MCV, SLOT_INFANTRY, SLOT_VEHICLE] {
            let unit = ty(side, slot);
            if unit.is_none() {
                continue;
            }
            commanded = commanded.bind(unit);
            if slot != SLOT_MCV {
                birth = birth.bind(unit);
            }
        }
    }
    let made = [
        commanded.done().is_ok(),
        birth.done().is_ok(),
        Trigger::on(Event::ORDER_REFUSED)
            .enqueue(ON_NOT_CARRIED_OUT)
            .done()
            .is_ok(),
    ];
    if made.contains(&false) {
        log("ai-takeover: a trigger could not be made");
    }
}

/// A unit took an order.
fn on_commanded(r: &EventRecord) {
    let Some(seat) = seat_of_owner(r.subject()) else {
        return;
    };
    if r.value == Command::AttackMove as i64 && ld(&ATTACK_FRAME[seat]) != 0 {
        mark(MARK_ATTACK, "DEMO-OK attack");
    } else if r.value == Command::DeployMcv as i64 {
        mark(
            NOTE_DEPLOY_HEARD,
            "ai-takeover: the MCV took the deploy order",
        );
    }
}

/// An order of a claimed house was not carried out.
fn on_not_carried_out(r: &EventRecord) {
    let Some(seat) = seat_of(r.house) else {
        return;
    };
    mark(MARK_REJECTED, "DEMO-OK order-rejected");
    if r.value == Command::PlaceReadyBuilding as i64 && r.value2 == REASON_CANNOT_PLACE {
        st(&PLACE_FAILS[seat], ld(&PLACE_FAILS[seat]) + 1);
        st(&PLACING_TYPE[seat], 0);
    }
    let lines = ld(&DETAIL_LINES);
    if lines < MAX_DETAIL_LINES {
        st(&DETAIL_LINES, lines + 1);
        Line::new()
            .text("ai-takeover: frame ")
            .num(i64::from(r.frame))
            .text(" order ")
            .num(r.value)
            .text(" not carried out: ")
            .text(reason_name(r.value2))
            .log();
    }
}

/// A unit left a factory.
fn on_birth(r: &EventRecord) {
    let unit = r.subject();
    let Some(seat) = seat_of_owner(unit) else {
        return;
    };
    if ld(&TRAINING[seat]) == 0 {
        return;
    }
    let side = seat_side(seat);
    let kind = get(unit, Prop::Type).unwrap_or(0);
    if kind == ty(side, SLOT_INFANTRY).0 || kind == ty(side, SLOT_VEHICLE).0 {
        mark(MARK_TRAIN, "DEMO-OK train");
    }
}

/// `RefuseReason` names (appendix 8.3, enum 26).
fn reason_name(reason: i64) -> &'static str {
    match reason {
        1 => "gone",
        2 => "ignores orders",
        3 => "illegal target",
        4 => "prerequisite",
        5 => "cannot afford",
        6 => "queue full",
        7 => "cannot place",
        8 => "not ready",
        9 => "vetoed",
        10 => "rate limited",
        _ => "other",
    }
}

// ------------------------------------------------------------- The driver

fn start() {
    resolve_types();
    make_triggers();
    claim_seats();
}

fn tick() {
    let frame = i64::from(frame());
    let seats = seat_count();
    if seats == 0 {
        // The houses may not hold their MCVs yet; look again for a while.
        if frame <= 300 {
            claim_seats();
        } else {
            mark(NOTE_NO_SEAT, "ai-takeover: no computer house to play");
        }
        return;
    }
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    if human != NO_HANDLE && get(House(human), Prop::IsDefeated).unwrap_or(0) != 0 {
        mark(
            NOTE_PLAYER_DEFEATED,
            "ai-takeover: the player's house is defeated",
        );
    }
    let visit = ld(&VISITS);
    st(&VISITS, visit + 1);
    let seat = (visit as usize) % seats;
    drive(seat, frame);
    if frame >= 900 && ld(&IDLE_CHECKED[seat]) == 0 {
        check_native_idle(seat);
    }
    if frame % 600 == 0 {
        status(seat, frame);
    }
}

/// One visit of a claimed house.
fn drive(seat: usize, frame: i64) {
    let house = seat_house(seat);
    if get(house, Prop::IsDefeated).unwrap_or(1) != 0 {
        return;
    }
    let side = seat_side(seat);
    let yard = owned(house, ty(side, SLOT_YARD));
    if yard.is_none() {
        deploy(seat, house, side, frame);
        return;
    }
    if ld(&DEPLOY_FRAME[seat]) != 0 {
        mark(MARK_DEPLOY, "DEMO-OK deploy");
    }
    let Ok(yard_cell) = get(yard, Prop::Cell).map(Cell) else {
        return;
    };
    // An order it cannot carry out yet, on purpose: a tank before any war
    // factory. The execution step answers `OrderRefused` (Prerequisite).
    if ld(&PROVOKED[seat]) == 0 && !standing(house, ty(side, SLOT_BUILD + STEP_WAR_FACTORY)) {
        let tank = ty(side, SLOT_VEHICLE);
        if !tank.is_none() && house.order(&Order::queue_production(tank)).is_ok() {
            st(&PROVOKED[seat], 1);
        }
    }
    build(seat, house, side, yard_cell);
    train(seat, house, side);
    attack(seat, house, side, frame);
}

/// Deploys the house's MCV, moving it a little when deploying keeps
/// failing where it stands.
fn deploy(seat: usize, house: House, side: usize, frame: i64) {
    let mcv = owned(house, ty(side, SLOT_MCV));
    if mcv.is_none() {
        return;
    }
    let last = ld(&DEPLOY_FRAME[seat]);
    if last != 0 && frame - last < 60 {
        return;
    }
    let tries = ld(&DEPLOY_TRIES[seat]);
    st(&DEPLOY_TRIES[seat], tries + 1);
    let units = [mcv];
    if tries > 0 && tries % 3 == 0 {
        // Three tries where it stands: try a few cells away.
        if let Ok(at) = get(mcv, Prop::Cell).map(Cell) {
            let step = 3 * (tries / 3) as u16;
            let to = if (tries / 3) % 2 == 1 {
                Cell::at(at.x().saturating_add(step), at.y().saturating_add(step))
            } else {
                Cell::at(at.x().saturating_sub(step), at.y().saturating_sub(step))
            };
            let _ = house.order(&Order::move_to(&units, to));
            st(&DEPLOY_FRAME[seat], frame.max(1));
            return;
        }
    }
    if house.order(&Order::deploy_mcv(&units)).is_ok() {
        st(&DEPLOY_FRAME[seat], frame.max(1));
    }
}

/// The building factory: place what is ready, else start the next step of
/// the build order the house does not own yet.
fn build(seat: usize, house: House, side: usize, yard_cell: Cell) {
    let ready = get(house, Prop::Factory_Building_Ready).unwrap_or(0);
    let queued = get(house, Prop::Factory_Building_Queued).unwrap_or(0);
    if ready != 0 {
        mark(MARK_BUILD, "DEMO-OK build");
        let product = TypeRef(get(house, Prop::Factory_Building_Type).unwrap_or(0));
        if !product.is_none() {
            place(seat, house, product, yard_cell);
        }
        return;
    }
    let placing = TypeRef(ld(&PLACING_TYPE[seat]));
    if !placing.is_none() {
        let at = Cell(ld(&PLACING_CELL[seat]));
        let placed = Find::objects()
            .owned_by(house)
            .of_type(placing)
            .near(at, 6 * 256)
            .first();
        if placed != NO_HANDLE {
            mark(MARK_PLACE, "DEMO-OK place");
        }
        st(&PLACING_TYPE[seat], 0);
    }
    if queued != 0 {
        return;
    }
    for step in 0..BUILD_STEPS {
        let building = ty(side, SLOT_BUILD + step);
        if building.is_none() || standing(house, building) {
            continue;
        }
        let _ = house.order(&Order::queue_production(building));
        return;
    }
    mark(MARK_BASE, "DEMO-OK base");
}

/// Places the finished `building` at a `Placeable` cell near the yard:
/// about sqrt(`target`) cells out, further after each failed placement.
fn place(seat: usize, house: House, building: TypeRef, yard_cell: Cell) {
    let fails = ld(&PLACE_FAILS[seat]).min(8);
    let target = 20 + 12 * fails;
    let mut find = Find::cells()
        .near(yard_cell, 12 * 256)
        .of_type(building)
        .seen_by(house)
        .where_prop(Prop::Placeable, Op::Eq, 1);
    let mut cells = [NO_HANDLE; 64];
    let mut after = 0;
    let mut best = NO_HANDLE;
    let mut best_score = i64::MAX;
    for _ in 0..8 {
        let Ok((count, next)) = find.page(&mut cells, after) else {
            break;
        };
        for &raw in &cells[..count] {
            let cell = Cell(raw);
            let dx = i64::from(cell.x()) - i64::from(yard_cell.x());
            let dy = i64::from(cell.y()) - i64::from(yard_cell.y());
            let score = (dx * dx + dy * dy - target).abs();
            if score < best_score {
                best = raw;
                best_score = score;
            }
        }
        if next == 0 {
            break;
        }
        after = next;
    }
    if best == NO_HANDLE {
        mark(NOTE_NO_CELL, "ai-takeover: no cell to place a building yet");
        return;
    }
    if house
        .order(&Order::place_ready_building(building, Cell(best)))
        .is_ok()
    {
        st(&PLACING_TYPE[seat], building.0);
        st(&PLACING_CELL[seat], best);
    }
}

/// Infantry from the barracks, tanks from the war factory; a ready unit is
/// let out when it is still waiting on the next visit.
fn train(seat: usize, house: House, side: usize) {
    let lines = [
        (
            STEP_BARRACKS,
            SLOT_INFANTRY,
            KIND_INFANTRY,
            Prop::Factory_Infantry_Queued,
            Prop::Factory_Infantry_Ready,
            &INFANTRY_READY[seat],
            2,
        ),
        (
            STEP_WAR_FACTORY,
            SLOT_VEHICLE,
            KIND_VEHICLE,
            Prop::Factory_Vehicle_Queued,
            Prop::Factory_Vehicle_Ready,
            &VEHICLE_READY[seat],
            1,
        ),
    ];
    for (step, slot, kind, queued_prop, ready_prop, was_ready, depth) in lines {
        if !standing(house, ty(side, SLOT_BUILD + step)) {
            continue;
        }
        let ready = get(house, ready_prop).unwrap_or(0) != 0;
        if ready && ld(was_ready) != 0 {
            let _ = house.order(&Order::take_product(kind));
            st(was_ready, 0);
        } else {
            st(was_ready, i64::from(ready));
        }
        let queued = get(house, queued_prop).unwrap_or(0);
        let trained = ld(&TRAINING[seat]);
        let unit = ty(side, slot);
        if queued < depth
            && trained < MAX_TRAINED
            && !unit.is_none()
            && house.order(&Order::queue_production(unit)).is_ok()
        {
            st(&TRAINING[seat], trained + 1);
        }
    }
}

/// Sends the army at the human player's base once the war factory stands
/// and the army is large enough, and again every [`ATTACK_EVERY`] frames.
fn attack(seat: usize, house: House, side: usize, frame: i64) {
    let last = ld(&ATTACK_FRAME[seat]);
    if last != 0 && frame - last < ATTACK_EVERY {
        return;
    }
    if !standing(house, ty(side, SLOT_BUILD + STEP_WAR_FACTORY)) {
        return;
    }
    let mut army = [Techno(NO_HANDLE); vera20k_sdk::ORDER_UNITS];
    let len = collect(house, ty(side, SLOT_INFANTRY), &mut army, 0);
    let len = collect(house, ty(side, SLOT_VEHICLE), &mut army, len);
    if len < ATTACK_SIZE {
        return;
    }
    let Some(target) = enemy_base() else {
        return;
    };
    if house
        .order(&Order::attack_move(&army[..len], target))
        .is_ok()
    {
        if last == 0 {
            Line::new()
                .text("ai-takeover: attack-move with ")
                .num(len as i64)
                .text(" units to ")
                .num(i64::from(target.x()))
                .text(",")
                .num(i64::from(target.y()))
                .log();
        }
        st(&ATTACK_FRAME[seat], frame.max(1));
    }
}

/// Evidence that the native AI stood down: every building the house owns
/// is its yard or a step of the module's build order.
fn check_native_idle(seat: usize) {
    let house = seat_house(seat);
    let side = seat_side(seat);
    if owned(house, ty(side, SLOT_YARD)).is_none() {
        return;
    }
    st(&IDLE_CHECKED[seat], 1);
    let mut find = Find::objects().owned_by(house).where_prop(
        Prop::Category,
        Op::Eq,
        Category::Building as i64,
    );
    let mut page = [NO_HANDLE; 32];
    let mut after = 0;
    let mut buildings = 0;
    let mut foreign = 0;
    loop {
        let Ok((count, next)) = find.page(&mut page, after) else {
            return;
        };
        for &building in &page[..count] {
            buildings += 1;
            let kind = get(Techno(building), Prop::Type).unwrap_or(0);
            let ordered = kind == ty(side, SLOT_YARD).0
                || (0..BUILD_STEPS).any(|step| kind == ty(side, SLOT_BUILD + step).0);
            if !ordered {
                foreign += 1;
            }
        }
        if next == 0 {
            break;
        }
        after = next;
    }
    if foreign == 0 && buildings > 0 {
        mark(MARK_IDLE, "DEMO-OK native-idle");
    } else if foreign > 0 {
        mark(
            NOTE_NATIVE_BUILT,
            "ai-takeover: the house owns a building the module did not order",
        );
    }
}

fn status(seat: usize, frame: i64) {
    let house = seat_house(seat);
    let side = seat_side(seat);
    let mut buildings = Find::objects().owned_by(house).where_prop(
        Prop::Category,
        Op::Eq,
        Category::Building as i64,
    );
    let mut page = [NO_HANDLE; 32];
    let built = match buildings.page(&mut page, 0) {
        Ok((count, _)) => count as i64,
        Err(_) => -1,
    };
    let mut army = [Techno(NO_HANDLE); vera20k_sdk::ORDER_UNITS];
    let len = collect(house, ty(side, SLOT_INFANTRY), &mut army, 0);
    let len = collect(house, ty(side, SLOT_VEHICLE), &mut army, len);
    Line::new()
        .text("ai-takeover: frame ")
        .num(frame)
        .text(" seat ")
        .num(seat as i64)
        .text(" buildings ")
        .num(built)
        .text(" army ")
        .num(len as i64)
        .text(" credits ")
        .num(get(house, Prop::Money).unwrap_or(-1))
        .log();
}

// -------------------------------------------------------------------- Text

/// A log line without an allocator: `core::fmt` pulls in code the engine
/// does not load.
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
        let mut end = text.len().min(self.buf.len() - self.len);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.buf[self.len..self.len + end].copy_from_slice(&text.as_bytes()[..end]);
        self.len += end;
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

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(every = 15, tick);
vera20k_sdk::on_events!(
    ON_COMMANDED => on_commanded,
    ON_NOT_CARRIED_OUT => on_not_carried_out,
    ON_BIRTH => on_birth,
);
