//! The actions, claims and orders, one group per scheduled frame after
//! the arena opened; each checks what it changed where that is a read.

use super::*;

/// The work of frame `t` after the arena opened.
pub(super) fn step(t: i64) {
    match t {
        6 => claim(),
        8 => stock_actions(),
        9 => release_claim(),
        10 => damage(),
        12 => yuri_attack(),
        14 => heal(),
        18 => set_property(),
        22 => detonate_warhead(),
        26 => detonate_weapon(),
        30 => raise_ping(),
        32 => raise_global(),
        34 => add_money(),
        36 => score(),
        38 => allow_type(),
        40 => animations(),
        42 => log_action(),
        44 => modify(),
        50 => put_dog(),
        60 => remove_modifiers(),
        64 => add_experience(),
        66 => ammo_and_remove(),
        68 => removed_check(),
        70 => kill(),
        72 => kill_by_damage(),
        74 => set_owner(),
        78 => retire(SWITCHED),
        90 => convert(),
        96 => convert_for(),
        100 => add_passenger(),
        105 => bind_foot(),
        106 => unbind_foot(),
        110 => filter_subject_hits(),
        120 => filter_subject_and_unbind_check(),
        124 => filter_other_hits(),
        130 => unload(),
        134 => filter_other_check(),
        140 => convert_back_check(),
        150 => clear_target(),
        160 => filter_house_check(),
        200 => {
            move_mover();
            order(Command::Move);
        }
        260 => order(Command::Guard),
        300 => clear_disguise(),
        320 => order(Command::Stop),
        380 => order(Command::Scatter),
        420 => teleport(),
        440 => order(Command::TakeProduct),
        500 => {
            retire(ORDERED_1);
            retire(ORDERED_2);
        }
        2400 => order(Command::QueueProduction),
        900 => {
            retire(YURI);
            retire(CONTROLLED);
            retire(VICTIM);
        }
        _ => {}
    }
    // Repeated until they answer.
    if (6..=400).contains(&t) && t % 3 == 0 {
        fire_weapon();
    }
    if (8..=1800).contains(&t) && have(SUPER) && ld(SUPER_GRANTED) != 0 {
        match t % 30 {
            8 => {
                let _ = Action::super_weapon_set_charge(crate::player(), kind(SUPER), 100);
            }
            10 => launch_super_weapon(),
            _ => {}
        }
    }
    if (210..=400).contains(&t) && t % 10 == 0 {
        set_mission_check();
    }
    if (430..=600).contains(&t) && t % 10 == 0 {
        teleport_check();
    }
}

// ----------------------------------------------------------------- Claims

/// Claims `SuperWeapons` (bit 3) on the computer house, which leaves its
/// production and forces to the native AI; a human house takes no claim.
fn claim() {
    let e = crate::enemy();
    if set(e, Prop::ControlDomains, 8).is_ok() && get(e, Prop::ControlDomains) == Ok(8) {
        st(CLAIMED, 1);
    } else {
        crate::bench_fail("triggers.claim", "the claim did not read back");
    }
    let p = crate::player();
    match set(p, Prop::ControlDomains, 8) {
        Err(Status::Refused) => crate::mark("triggers.claim-human"),
        Ok(()) => {
            let _ = set(p, Prop::ControlDomains, 0);
            crate::bench_fail("triggers.claim-human", "a human house took a claim");
        }
        Err(_) => crate::bench_fail("triggers.claim-human", "wrong status"),
    }
}

/// Three frames later: the claim goes again.
fn release_claim() {
    if ld(CLAIMED) == 0 {
        return;
    }
    let e = crate::enemy();
    let released = set(e, Prop::ControlDomains, 0).is_ok() && get(e, Prop::ControlDomains) == Ok(0);
    check("triggers.claim", released, "the release did not read back");
}

// ----------------------------------------------------------------- Stock

/// Stock actions through `action`: the mission timer (27 set, 23 start, 24
/// stop), a local variable (56 set, 57 clear), and 16 (reveal the map).
fn stock_actions() {
    let started = Action::timer_set(60).is_ok() && Action::timer_start().is_ok();
    let running = Global::mission_timer_running().unwrap_or(false);
    let stopped = Action::timer_stop().is_ok();
    let idle = !Global::mission_timer_running().unwrap_or(true);
    check(
        "triggers.stock-timer",
        started && running && stopped && idle,
        "the mission timer did not run and stop",
    );
    let local = Var::local(88);
    let set_ok = Action::set_local(88).is_ok() && local.get() == 1;
    let cleared = Action::clear_local(88).is_ok() && local.get() == 0;
    check(
        "triggers.stock-local",
        set_ok && cleared,
        "local 88 did not read back",
    );
    check(
        "triggers.stock-reveal",
        Action::reveal_all().is_ok(),
        "stock 16 answered an error",
    );
}

// ---------------------------------------------------- The dummy's actions

fn damage() {
    if !have(DUMMY) || !have(WARHEAD_AP) {
        return;
    }
    let dummy = unit(DUMMY);
    let before = health(dummy);
    let ok = Action::damage(dummy, 150, kind(WARHEAD_AP)).is_ok();
    let after = health(dummy);
    check(
        "triggers.act-damage",
        ok && after >= 0 && after < before,
        "health did not drop",
    );
}

fn heal() {
    let dummy = unit(DUMMY);
    if !have(DUMMY) || !crate::alive(dummy) {
        return;
    }
    let _ = dummy.set_health(200);
    let ok = Action::heal(dummy, 50).is_ok();
    check(
        "triggers.act-heal",
        ok && health(dummy) == 250,
        "health did not rise by the amount",
    );
}

fn set_property() {
    let dummy = unit(DUMMY);
    if !have(DUMMY) || !crate::alive(dummy) {
        return;
    }
    let ok = Action::set_property(dummy, Prop::Health, 350).is_ok();
    check(
        "triggers.act-set-property",
        ok && health(dummy) == 350,
        "Health did not read back",
    );
}

fn detonate_warhead() {
    let dummy = unit(DUMMY);
    if !have(DUMMY) || !have(WARHEAD_AP) || !crate::alive(dummy) {
        return;
    }
    let before = health(dummy);
    let ok = Action::detonate_warhead(dummy, kind(WARHEAD_AP), 60).is_ok();
    check(
        "triggers.act-detonate-warhead",
        ok && health(dummy) < before,
        "the warhead did not hurt the dummy",
    );
}

fn detonate_weapon() {
    let dummy = unit(DUMMY);
    if !have(DUMMY) || !have(WEAPON_105) || !crate::alive(dummy) {
        return;
    }
    let _ = dummy.set_health(380);
    let ok = Action::detonate_weapon(dummy, kind(WEAPON_105), Techno(NO_HANDLE)).is_ok();
    check(
        "triggers.act-detonate-weapon",
        ok && health(dummy) < 380,
        "the weapon did not hurt the dummy",
    );
}

/// Raises the object module event the dummy's chain trigger hears; the
/// trigger heals it by 30 and pays the player 7 at once (`records`).
fn raise_ping() {
    let (dummy, ping) = (unit(DUMMY), PING.get());
    if !have(DUMMY) || ping.0 < 0 || !crate::alive(dummy) {
        return;
    }
    let _ = dummy.set_health(300);
    st(PING_HEALTH, health(dummy));
    st(PING_MONEY, money());
    check(
        "triggers.act-raise-event",
        Action::raise_event(ping, dummy, Techno(NO_HANDLE), 1, 2).is_ok(),
        "RaiseEvent answered an error",
    );
}

/// Raises the global module event.
fn raise_global() {
    let event = GLOBAL_EVENT.get();
    if event.0 >= 0 && Action::raise_event(event, Global, Techno(NO_HANDLE), 3, 4).is_err() {
        crate::bench_fail("triggers.ev-module-global", "RaiseEvent answered an error");
    }
}

fn add_money() {
    let before = money();
    let ok = Action::add_money(crate::player(), 100).is_ok();
    check(
        "triggers.act-add-money",
        ok && before >= 0 && money() == before + 100,
        "Money did not rise by 100",
    );
}

fn score() {
    let p = crate::player();
    check(
        "triggers.act-set-score",
        Action::set_score(p, 10).is_ok(),
        "SetScore answered an error",
    );
    check(
        "triggers.act-add-score",
        Action::add_score(p, 5).is_ok(),
        "AddScore answered an error",
    );
}

/// Forbids the GI to the player, then lifts the forbid.
fn allow_type() {
    if !have(TYPE_E1) {
        return;
    }
    let p = crate::player();
    let forbid = Action::allow_type(p, kind(TYPE_E1), false).is_ok();
    let allow = Action::allow_type(p, kind(TYPE_E1), true).is_ok();
    check(
        "triggers.act-allow-type",
        forbid && allow,
        "AllowType answered an error",
    );
}

fn animations() {
    if !have(ANIM) {
        return;
    }
    let anim = (ld(ANIM) - 1) as i32;
    check(
        "triggers.act-spawn-anim",
        Action::spawn_anim(at(0, -3), anim).is_ok(),
        "SpawnAnim answered an error",
    );
    if have(DUMMY) && crate::alive(unit(DUMMY)) {
        check(
            "triggers.act-attach-anim",
            Action::attach_anim(unit(DUMMY), anim).is_ok(),
            "AttachAnim answered an error",
        );
    }
}

fn log_action() {
    check(
        "triggers.act-log",
        Action::log("Control Bench triggers: the Log action").is_ok(),
        "Log answered an error",
    );
}

/// A grouped modifier (group 5) on the dummy: `ModifierStart`,
/// `ModifierStacks` and the `ModifierCountPoll` follow.
fn modify() {
    let dummy = unit(DUMMY);
    if !have(DUMMY) || !crate::alive(dummy) {
        return;
    }
    match Action::modify(
        dummy,
        Prop::FirepowerMultiplier,
        ModifierOp::Multiply,
        FIXED_ONE,
        900,
        5,
    ) {
        Ok(modifier) => {
            st(MODIFIER, modifier.0);
            check(
                "triggers.act-modify",
                get(modifier, Prop::Remaining).is_ok_and(|left| left > 0),
                "the modifier did not read back",
            );
        }
        Err(_) => crate::bench_fail("triggers.act-modify", "Modify answered an error"),
    }
}

/// Group 5 goes: the modifier reads as gone, `ModifierExpire` follows.
fn remove_modifiers() {
    if !have(MODIFIER) {
        return;
    }
    let ok = Action::remove_modifiers(unit(DUMMY), 5, None).is_ok();
    check(
        "triggers.act-remove-modifiers",
        ok && get(Modifier(ld(MODIFIER)), Prop::Remaining).is_err(),
        "the modifier still reads",
    );
}

fn add_experience() {
    let dummy = unit(DUMMY);
    if !have(DUMMY) || !crate::alive(dummy) {
        return;
    }
    let before = dummy.experience().unwrap_or(-1);
    let ok = Action::add_experience(dummy, 1000).is_ok();
    let after = dummy.experience().unwrap_or(-1);
    check(
        "triggers.act-add-experience",
        ok && before >= 0 && after > before,
        "Experience did not rise",
    );
}

/// A Harrier (`Ammo=1`) made for one call: ammo down and up again, then
/// removed. Without one a GI takes the removal.
fn ammo_and_remove() {
    let p = crate::player();
    let made = match crate::spawn(at(2, -12), "ORCA", p) {
        Some(plane) => {
            let full = plane.ammo().unwrap_or(-1);
            let down = Action::add_ammo(plane, -1).is_ok() && plane.ammo() == Ok(full - 1);
            let up = Action::add_ammo(plane, 1).is_ok() && plane.ammo() == Ok(full);
            check(
                "triggers.act-add-ammo",
                full >= 1 && down && up,
                "Ammo did not read back",
            );
            Some(plane)
        }
        None => {
            crate::bench_fail("triggers.act-add-ammo", "no Harrier could be placed");
            crate::spawn(at(2, -12), "E1", p)
        }
    };
    // Remove queues the object's deletion (UnInit's pending delete, as the
    // native one does): it reads as gone from the next frame on.
    if let Some(made) = made {
        if Action::remove(made).is_ok() {
            st(REMOVED, made.0);
        } else {
            crate::bench_fail("triggers.act-remove", "Remove answered an error");
        }
    }
}

/// Two frames after `Remove`: the unit reads as gone.
fn removed_check() {
    if have(REMOVED) {
        check(
            "triggers.act-remove",
            unit(REMOVED).health().is_err(),
            "the removed unit still reads",
        );
    }
}

/// A GI made to be killed (`C4Warhead=`, defenses ignored).
fn kill() {
    let Some(victim) = crate::spawn(at(-6, 2), "E1", crate::player()) else {
        crate::bench_fail("triggers.act-kill", "no GI could be placed");
        return;
    };
    crate::mark("triggers.act-create");
    let ok = Action::kill(victim, Techno(NO_HANDLE)).is_ok();
    check(
        "triggers.act-kill",
        ok && !crate::alive(victim),
        "the GI still lives",
    );
}

/// Kills the `Killed` trigger's GI with the AP warhead.
fn kill_by_damage() {
    if have(KILLED) && have(WARHEAD_AP) && crate::alive(unit(KILLED)) {
        let _ = Action::damage(unit(KILLED), 1000, kind(WARHEAD_AP));
    }
}

/// Gives a GI to the computer house; `HouseTransferred` follows.
fn set_owner() {
    let switched = unit(SWITCHED);
    if !have(SWITCHED) || !crate::alive(switched) {
        return;
    }
    let e = crate::enemy();
    let ok = Action::set_owner(switched, e).is_ok();
    check(
        "triggers.act-set-owner",
        ok && switched.owner() == Ok(e),
        "Owner did not read back",
    );
}

/// The computer house's Yuri takes the player's engineer (`records`
/// releases it).
fn yuri_attack() {
    if have(YURI) && have(CONTROLLED) {
        let units = [unit(YURI)];
        let order = Order::attack(&units, unit(CONTROLLED));
        if crate::enemy().order(&order).is_err() {
            crate::bench_fail("triggers.order-attack", "the order was turned down");
        }
    }
}

// ------------------------------------------------------------- Conversion

fn convert() {
    let converted = unit(CONVERTED);
    if !have(CONVERTED) || !have(TYPE_FV) || !crate::alive(converted) {
        return;
    }
    let ok = Action::convert(converted, kind(TYPE_FV)).is_ok();
    check(
        "triggers.act-convert",
        ok && converted.type_ref() == Ok(kind(TYPE_FV)),
        "Type did not read back",
    );
}

/// For 30 frames a Rhino, then the IFV again.
fn convert_for() {
    let converted = unit(CONVERTED);
    if !have(CONVERTED) || !have(TYPE_HTNK) || !crate::alive(converted) {
        return;
    }
    let ok = Action::convert_for(converted, kind(TYPE_HTNK), 30).is_ok();
    if !ok || converted.type_ref() != Ok(kind(TYPE_HTNK)) {
        crate::bench_fail("triggers.act-convert-timed", "Type did not read back");
    }
}

fn convert_back_check() {
    let converted = unit(CONVERTED);
    if have(CONVERTED) && have(TYPE_FV) && crate::alive(converted) {
        check(
            "triggers.act-convert-timed",
            converted.type_ref() == Ok(kind(TYPE_FV)),
            "the conversion did not revert",
        );
    }
}

// -------------------------------------------------------------- Transport

fn add_passenger() {
    let (ifv, rider) = (unit(IFV), unit(RIDER));
    if !have(IFV) || !have(RIDER) || !crate::alive(ifv) || !crate::alive(rider) {
        return;
    }
    let ok = Action::add_passenger(ifv, rider).is_ok();
    check(
        "triggers.act-add-passenger",
        ok && rider.transport() == Ok(ifv),
        "Transport did not read back",
    );
}

/// The IFV's Unload mission lets the GI out (`TransportExited`,
/// `PassengerRemoved`).
fn unload() {
    if have(IFV) && crate::alive(unit(IFV)) {
        let _ = Action::set_mission(unit(IFV), Mission::Unload, Cell(NO_HANDLE));
    }
}

// --------------------------------------------------------- Filter clauses

/// `trigger_bind` adds the GI to the unbind trigger...
fn bind_foot() {
    if have(TR_UNBIND) && have(FOOT) && trigger(TR_UNBIND).bind(unit(FOOT)).is_err() {
        crate::bench_fail("triggers.unbind", "trigger_bind failed");
    }
}

/// ...and `trigger_unbind` takes it away again.
fn unbind_foot() {
    if have(TR_UNBIND) && have(FOOT) && trigger(TR_UNBIND).unbind(unit(FOOT)).is_err() {
        crate::bench_fail("triggers.unbind", "trigger_unbind failed");
    }
}

/// The GI (Strength 125) and the Rhino (400) take a hit: the subject
/// clause lets only the Rhino's through, and the unbound GI raises nothing
/// on the unbind trigger.
fn filter_subject_hits() {
    if !have(WARHEAD_AP) {
        return;
    }
    for slot in [FOOT, DUMMY] {
        if have(slot) && crate::alive(unit(slot)) {
            let _ = Action::damage(unit(slot), 5, kind(WARHEAD_AP));
        }
    }
}

fn filter_subject_and_unbind_check() {
    if ld(SUBJECT_YES) != 0 && ld(SUBJECT_NO) == 0 {
        crate::mark("triggers.filter-subject");
    }
    if ld(UNBIND_YES) != 0 && ld(UNBIND_NO) == 0 {
        crate::mark("triggers.unbind");
    }
}

/// The Rhino takes a hit from itself (a vehicle) and from the GI: the
/// clause on the other object lets only the GI's through.
fn filter_other_hits() {
    let dummy = unit(DUMMY);
    if !have(DUMMY) || !have(FOOT) || !have(WARHEAD_AP) || !crate::alive(dummy) {
        return;
    }
    let _ = Action::damage_from(dummy, 5, kind(WARHEAD_AP), dummy);
    let _ = Action::damage_from(dummy, 5, kind(WARHEAD_AP), unit(FOOT));
}

fn filter_other_check() {
    if ld(OTHER_YES) != 0 && ld(OTHER_NO) == 0 {
        crate::mark("triggers.filter-other");
    }
}

/// The player's house passes the house clause, the computer's never does.
fn filter_house_check() {
    if ld(HOUSE_YES) != 0 && ld(HOUSE_NO) == 0 {
        crate::mark("triggers.filter-house");
    }
    if have(TR_HOUSE_NO) {
        let _ = trigger(TR_HOUSE_NO).remove();
    }
}

// ------------------------------------------------------------- The duel

/// The Grizzly's target goes; it is sent at the Rhino again.
fn clear_target() {
    let shooter = unit(SHOOTER);
    if !have(SHOOTER) || !crate::alive(shooter) {
        return;
    }
    let ok = Action::clear_target(shooter).is_ok();
    check(
        "triggers.act-clear-target",
        ok && shooter.target() == Ok(NO_HANDLE),
        "Target did not clear",
    );
    if have(VICTIM) && crate::alive(unit(VICTIM)) {
        let _ = Action::set_mission_on(shooter, Mission::Attack, unit(VICTIM));
    }
}

/// A GI (infantry fire asks no facing) fires at the ground two cells off
/// once it is ready; the Grizzly at its current target is the other try.
fn fire_weapon() {
    if marked("triggers.act-fire-weapon") {
        return;
    }
    let foot = have(FOOT)
        && crate::alive(unit(FOOT))
        && Action::fire_weapon_at_cell(unit(FOOT), 0, at(-10, -8)).is_ok();
    let shooter = !foot
        && have(SHOOTER)
        && crate::alive(unit(SHOOTER))
        && Action::fire_weapon(unit(SHOOTER), 0).is_ok();
    if foot || shooter {
        crate::mark("triggers.act-fire-weapon");
    }
}

fn marked(name: &str) -> bool {
    crate::marked(name)
}

/// The player's one-time Psychic Reveal, charged, at a cell north of the
/// arena.
fn launch_super_weapon() {
    if marked("triggers.act-launch-sw") {
        return;
    }
    if Action::launch_super_weapon(crate::player(), kind(SUPER), at(0, -14)).is_ok() {
        crate::mark("triggers.act-launch-sw");
    }
}

// ----------------------------------------------------------------- Motion

/// The mover drives three cells on; `DistancePoll` and the stock cell
/// events follow.
fn move_mover() {
    let mover = unit(MOVER);
    if have(MOVER) && crate::alive(mover) {
        st(MOVER_START, get(mover, Prop::Cell).unwrap_or(NO_HANDLE));
        let _ = Action::set_mission(mover, Mission::Move, at(7, 8));
    }
}

fn set_mission_check() {
    let mover = unit(MOVER);
    if marked("triggers.act-set-mission") || !have(MOVER) || !crate::alive(mover) {
        return;
    }
    let moved = get(mover, Prop::Cell).is_ok_and(|cell| cell != ld(MOVER_START));
    if moved || mover.mission() == Ok(Mission::Move as i64) {
        crate::mark("triggers.act-set-mission");
    }
}

fn teleport() {
    let mover = unit(MOVER);
    if have(MOVER) && crate::alive(mover) {
        if Action::teleport(mover, at(6, 2), Blocked::Nearby).is_err() {
            crate::bench_fail("triggers.act-teleport", "Teleport answered an error");
        }
    }
}

/// The mover stands within two cells of the teleport's cell.
fn teleport_check() {
    let mover = unit(MOVER);
    if marked("triggers.act-teleport") || !have(MOVER) || !crate::alive(mover) {
        return;
    }
    let Ok(cell) = get(mover, Prop::Cell).map(Cell) else {
        return;
    };
    let goal = at(6, 2);
    let near = (i32::from(cell.x()) - i32::from(goal.x())).abs() <= 2
        && (i32::from(cell.y()) - i32::from(goal.y())).abs() <= 2;
    if near {
        crate::mark("triggers.act-teleport");
    }
}

/// The Mirage tank's disguise (if it wears one by now) goes.
fn clear_disguise() {
    let mirage = unit(MIRAGE);
    if !have(MIRAGE) || !crate::alive(mirage) {
        return;
    }
    let ok = Action::clear_disguise(mirage).is_ok();
    let bare = mirage.disguise_type().map_or(true, |ty| ty.is_none());
    check(
        "triggers.act-clear-disguise",
        ok && bare,
        "the disguise stayed",
    );
}

// ---------------------------------------------------------------- Orders

/// House orders for the computer house on the area's two Rhinos (its
/// `Commanded` and `OrderRefused` records are read in `records`).
fn order(command: Command) {
    let units = [unit(ORDERED_1), unit(ORDERED_2)];
    let alive = have(ORDERED_1) && have(ORDERED_2) && units.iter().all(|u| crate::alive(*u));
    let second = [unit(ORDERED_2)];
    let order = match command {
        Command::Move if alive => Order::move_to(&units, at(8, -9)),
        Command::Guard if alive => Order::new(Command::Guard, at(8, -9).0, 0, &units),
        Command::Stop if alive => Order::stop(&units),
        Command::Scatter if alive => Order::new(Command::Scatter, 0, 0, &second),
        // No naval factory has a product ready: refused (`NotReady`).
        Command::TakeProduct => Order::take_product(4),
        Command::QueueProduction if have(TYPE_E2) => Order::queue_production(kind(TYPE_E2)),
        _ => return,
    };
    match crate::enemy().order(&order) {
        Ok(()) if command == Command::QueueProduction => crate::mark("triggers.order-queue"),
        Ok(()) => {}
        Err(_) => crate::bench_fail("triggers.order", "an order was turned down at once"),
    }
}

// -------------------------------------------------------------------- INI

/// An Allied attack dog: its `Birth` reaches the INI trigger of the area's
/// `rulesmd.ini` block, whose `Wasm:benchIniDog` action `records` hears.
fn put_dog() {
    match crate::spawn(at(-11, -12), "ADOG", crate::player()) {
        Some(dog) => st(DOG, dog.0),
        None => crate::bench_fail("triggers.ini-trigger", "no dog could be placed"),
    }
}
