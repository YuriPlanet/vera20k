//! The area's units and triggers, made over the first frames after the
//! arena opens (a few per frame, so no frame carries all of it).

use super::*;

/// Resolves the rules names the area uses, once.
pub(super) fn lookups() {
    let types: [(usize, &str); 5] = [
        (TYPE_FV, "FV"),
        (TYPE_HTNK, "HTNK"),
        (TYPE_E1, "E1"),
        (TYPE_E2, "E2"),
        (TYPE_ENGINEER, "ENGINEER"),
    ];
    for (slot, name) in types {
        match TypeRef::techno_type(name) {
            Ok(ty) => st(slot, ty.0),
            Err(_) => crate::bench_fail("triggers.setup", "a retail TechnoType is missing"),
        }
    }
    match TypeRef::weapon("105mm") {
        Ok(ty) => st(WEAPON_105, ty.0),
        Err(_) => crate::bench_fail("triggers.setup", "weapon 105mm is missing"),
    }
    match TypeRef::warhead("AP") {
        Ok(ty) => st(WARHEAD_AP, ty.0),
        Err(_) => crate::bench_fail("triggers.setup", "warhead AP is missing"),
    }
    match TypeRef::projectile("Cannon") {
        Ok(ty) => st(PROJECTILE_CANNON, ty.0),
        Err(_) => crate::bench_fail("triggers.setup", "projectile Cannon is missing"),
    }
    match anim_id("PIFF") {
        Ok(id) if id >= 0 => st(ANIM, i64::from(id) + 1),
        _ => crate::bench_fail("triggers.setup", "animation PIFF is missing"),
    }
    match TypeRef::super_weapon("PsychicRevealSpecial") {
        Ok(ty) => st(SUPER, ty.0),
        Err(_) => crate::bench_fail("triggers.setup", "PsychicRevealSpecial is missing"),
    }
}

/// The setup work of frame `t` after the arena opened.
pub(super) fn step(t: i64) {
    match t {
        1 => first_units(),
        2 => object_triggers(),
        3 => global_triggers(),
        4 => second_units(),
        5 => trigger_properties(),
        _ => {}
    }
}

/// Creates a `name` for `house` at `dx`, `dy` from the arena's corner.
fn put(slot: usize, dx: i32, dy: i32, name: &str, house: House) {
    match crate::spawn(at(dx, dy), name, house) {
        Some(made) => st(slot, made.0),
        None => crate::bench_fail("triggers.spawn", "a unit could not be placed"),
    }
}

/// Keeps a made trigger's handle in `slot` (0 for none); a trigger the
/// engine turned down is a wrong answer.
fn keep(slot: usize, made: vera20k_sdk::Result<TriggerRef>) -> bool {
    match made {
        Ok(trigger) => {
            if slot != 0 {
                st(slot, trigger.0);
            }
            true
        }
        Err(_) => {
            crate::bench_fail("triggers.create", "trigger_create or its binding failed");
            false
        }
    }
}

/// Frame 1: the units the object triggers bind.
fn first_units() {
    let (p, e) = (crate::player(), crate::enemy());
    // The duel: a Grizzly and, three cells off, a weaponless enemy Rhino
    // with little health left.
    put(SHOOTER, 0, 0, "MTNK", p);
    put(VICTIM, 3, 0, "HTNK", e);
    if have(VICTIM) {
        let victim = unit(VICTIM);
        let _ = victim.set_health(130);
        let _ = Modifier::override_(victim, Prop::DisableWeapons, 1, -1);
    }
    // The dummy every damage, heal and modifier action works on.
    put(DUMMY, -4, -5, "HTNK", p);
    put(FOOT, -8, -8, "E1", p);
    put(KILLED, -4, 2, "E1", p);
    put(MOVER, 4, 5, "MTNK", p);
    put(CONVERTED, -2, -9, "MTNK", p);
    put(IFV, -8, -2, "FV", p);
    put(RIDER, -10, -3, "E1", p);
    put(SWITCHED, -2, 4, "E1", p);
}

/// Frame 2: triggers bound to the first units.
fn object_triggers() {
    let p = crate::player();
    let dummy = unit(DUMMY);
    if have(DUMMY) {
        let made = Trigger::on(Event::TAKE_DAMAGE)
            .bind(dummy)
            .enqueue(K_TAKE_DAMAGE)
            .done();
        if keep(0, made) {
            crate::mark("triggers.create");
        }
        // Timer beside an object event: a period, then the next hit.
        keep(
            0,
            Trigger::every(10)
                .and(Event::TAKE_DAMAGE)
                .bind(dummy)
                .once()
                .enqueue(K_TIMER_OBJECT)
                .done(),
        );
        for (event, key) in [
            (Event::MODIFIER_START, K_MOD_START),
            (Event::MODIFIER_STACKS, K_MOD_STACKS),
            (Event::MODIFIER_EXPIRE, K_MOD_EXPIRE),
        ] {
            keep(0, Trigger::on(event).bind(dummy).enqueue(key).done());
        }
        keep(
            0,
            Trigger::on_property(Prop::Health, Op::Lt, 300, 3)
                .bind(dummy)
                .once()
                .enqueue(K_POLL_PROPERTY)
                .done(),
        );
        keep(
            0,
            Trigger::on_modifier_count(5, Op::Ge, 1, 3)
                .bind(dummy)
                .once()
                .enqueue(K_POLL_MOD_COUNT)
                .done(),
        );
        // Filter clauses: Strength above 300 (the Rhino, not the GI); an
        // infantry source.
        let mut subject = Trigger::on(Event::TAKE_DAMAGE)
            .where_prop(Prop::Strength, Op::Gt, 300)
            .bind(dummy);
        if have(FOOT) {
            subject = subject.bind(unit(FOOT));
        }
        keep(0, subject.enqueue(K_FILTER_SUBJECT).done());
        keep(
            0,
            Trigger::on(Event::TAKE_DAMAGE)
                .where_other(Prop::Category, Op::Eq, Category::Infantry as i64)
                .bind(dummy)
                .enqueue(K_FILTER_OTHER)
                .done(),
        );
        // `bind` and `unbind` add and take away the GI later.
        keep(
            TR_UNBIND,
            Trigger::on(Event::TAKE_DAMAGE)
                .bind(dummy)
                .enqueue(K_UNBIND)
                .done(),
        );
        // A module event with immediate actions, one of which raises
        // another module event: the chain.
        let (ping, pong) = (PING.get(), PONG.get());
        if ping.0 >= 0 && pong.0 >= 0 {
            keep(
                0,
                Trigger::on_module_event(ping)
                    .bind(dummy)
                    .immediate(ActionDef::heal(Target::Subject, 30))
                    .immediate(ActionDef::add_money_to(p, 7))
                    .immediate(ActionDef::raise_event(Target::Subject, pong, 5, 0))
                    .enqueue(K_PING)
                    .done(),
            );
            keep(
                0,
                Trigger::on_module_event(pong)
                    .bind(dummy)
                    .enqueue(K_PONG)
                    .done(),
            );
        }
    }
    if have(KILLED) || have(VICTIM) {
        let mut killed = Trigger::on(Event::KILLED);
        for slot in [KILLED, VICTIM] {
            if have(slot) {
                killed = killed.bind(unit(slot));
            }
        }
        keep(0, killed.enqueue(K_KILLED).done());
    }
    if have(SHOOTER) {
        keep(
            0,
            Trigger::on(Event::FIRE)
                .bind(unit(SHOOTER))
                .enqueue(K_FIRE)
                .done(),
        );
    }
    if have(VICTIM) {
        let victim = unit(VICTIM);
        for (event, key) in [
            (Event::ATTACKED_BY, K_ATTACKED_BY),
            (Event::ATTACKED_BY_ANY_HOUSE, K_STOCK_ATTACKED),
            (Event::DESTROYED_BY_ANY_HOUSE, K_STOCK_DESTROYED),
        ] {
            keep(0, Trigger::on(event).bind(victim).enqueue(key).done());
        }
    }
    if have(RIDER) && have(IFV) {
        let (rider, ifv) = (unit(RIDER), unit(IFV));
        for (event, on, key) in [
            (Event::TRANSPORT_ENTERED, rider, K_ENTERED),
            (Event::PASSENGER_ADDED, ifv, K_ADDED),
            (Event::TRANSPORT_EXITED, rider, K_EXITED),
            (Event::PASSENGER_REMOVED, ifv, K_REMOVED),
        ] {
            keep(0, Trigger::on(event).bind(on).enqueue(key).done());
        }
    }
    if have(SWITCHED) {
        keep(
            TR_TRANSFERRED,
            Trigger::on(Event::HOUSE_TRANSFERRED)
                .bind(unit(SWITCHED))
                .enqueue(K_TRANSFERRED)
                .done(),
        );
    }
    if have(MOVER) {
        // Where it stands (a ring cell when its own was taken).
        let start = get(unit(MOVER), Prop::Cell).map_or(at(4, 5), Cell);
        keep(
            0,
            Trigger::on_distance(start, Op::Gt, 512, 5)
                .bind(unit(MOVER))
                .once()
                .enqueue(K_POLL_DISTANCE)
                .done(),
        );
    }
    // Stock cell events: a foot arriving on the row (25) or the column (26)
    // of a bound cell, of any house (−1). The mover crosses both.
    let (row, column) = (at(0, 7), at(6, 0));
    st(CELL_ROW, row.0);
    st(CELL_COLUMN, column.0);
    let rows = keep(
        0,
        Trigger::on(Event::CROSSES_HORIZONTAL_LINE)
            .value(-1)
            .bind(row)
            .once()
            .enqueue(K_CELL_ROW)
            .done(),
    );
    let columns = keep(
        0,
        Trigger::on(Event::CROSSES_VERTICAL_LINE)
            .value(-1)
            .bind(column)
            .once()
            .enqueue(K_CELL_COLUMN)
            .done(),
    );
    if rows && columns {
        crate::mark("triggers.bind-cell");
    }
}

/// Frame 3: global triggers and the type-bound ones.
fn global_triggers() {
    let (p, e) = (crate::player(), crate::enemy());
    keep(TR_TIMER, Trigger::every(30).enqueue(K_TIMER).done());
    keep(
        0,
        Trigger::on(Event::SUPER_WEAPON_LAUNCHED)
            .enqueue(K_SW_LAUNCHED)
            .done(),
    );
    let global = GLOBAL_EVENT.get();
    if global.0 >= 0 {
        keep(
            0,
            Trigger::on_module_event(global)
                .enqueue(K_GLOBAL_EVENT)
                .done(),
        );
    }
    // Stock House event (polled every eighth frame) and Logic event.
    keep(
        0,
        Trigger::on(Event::CREDITS_EXCEED)
            .value(0)
            .house(p)
            .once()
            .enqueue(K_STOCK_HOUSE)
            .done(),
    );
    keep(
        0,
        Trigger::on(Event::ELAPSED_TIME)
            .value(1)
            .once()
            .enqueue(K_STOCK_LOGIC)
            .done(),
    );
    // Polls: a Red Alert 2 local variable a trigger's SetVar sets, the
    // player's power and money.
    keep(
        0,
        Trigger::on_variable(Var::local(87), Op::Eq, 1, 5)
            .house(p)
            .once()
            .enqueue(K_POLL_VARIABLE)
            .done(),
    );
    keep(
        TR_SET_VAR,
        Trigger::every(40)
            .once()
            .immediate(ActionDef::set_var(Var::local(87), 1))
            .enqueue(K_SET_VAR)
            .done(),
    );
    keep(
        0,
        Trigger::on_house_power(Op::Ge, -1_000_000, 15)
            .house(p)
            .once()
            .enqueue(K_POLL_POWER)
            .done(),
    );
    keep(
        0,
        Trigger::on_property(Prop::Money, Op::Ge, 0, 15)
            .house(p)
            .once()
            .enqueue(K_POLL_MONEY)
            .done(),
    );
    // A house clause: true for the human player's house only.
    keep(
        0,
        Trigger::every(20)
            .house(p)
            .where_house(Prop::IsHuman, Op::Eq, 1)
            .once()
            .enqueue(K_FILTER_HOUSE_YES)
            .done(),
    );
    keep(
        TR_HOUSE_NO,
        Trigger::every(20)
            .house(e)
            .where_house(Prop::IsHuman, Op::Eq, 1)
            .enqueue(K_FILTER_HOUSE_NO)
            .done(),
    );
    keep(
        0,
        Trigger::on(Event::ORDER_REFUSED)
            .enqueue(K_ORDER_REFUSED)
            .done(),
    );
    // Birth bound to a type (the engineer the area creates next frame).
    if have(TYPE_ENGINEER)
        && keep(
            0,
            Trigger::on(Event::BIRTH)
                .bind(kind(TYPE_ENGINEER))
                .enqueue(K_BIRTH)
                .done(),
        )
    {
        crate::mark("triggers.bind-type");
    }
    // `Fire` bound to the Grizzly's weapon, its warhead and its projectile
    // type; each is removed after its first record.
    for (slot, ty, key) in [
        (TR_WEAPON, WEAPON_105, K_FIRE_WEAPON_TYPE),
        (TR_WARHEAD, WARHEAD_AP, K_FIRE_WARHEAD_TYPE),
        (TR_PROJECTILE, PROJECTILE_CANNON, K_FIRE_PROJECTILE_TYPE),
    ] {
        if have(ty) {
            keep(
                slot,
                Trigger::on(Event::FIRE).bind(kind(ty)).enqueue(key).done(),
            );
        }
    }
}

/// Frame 4: the units of the orders, the mind control, the disguise and
/// the `Birth`; the duel starts and the player gets a superweapon.
fn second_units() {
    let (p, e) = (crate::player(), crate::enemy());
    put(BAIT, -4, -12, "ENGINEER", p);
    put(ORDERED_1, 8, -12, "HTNK", e);
    put(ORDERED_2, 10, -12, "HTNK", e);
    // Ten cells apart, beyond the Yuri's reach, until its attack order.
    put(YURI, 14, 8, "YURI", e);
    put(CONTROLLED, 14, -2, "SENGINEER", p);
    put(MIRAGE, -8, -12, "MGTK", p);
    let mut commanded = Trigger::on(Event::COMMANDED);
    for slot in [ORDERED_1, ORDERED_2, YURI] {
        if have(slot) {
            commanded = commanded.bind(unit(slot));
        }
    }
    keep(0, commanded.enqueue(K_COMMANDED).done());
    // `trigger_bind` on a made trigger: the mind control's victim.
    if have(TR_TRANSFERRED) && have(CONTROLLED) {
        check(
            "triggers.bind",
            trigger(TR_TRANSFERRED).bind(unit(CONTROLLED)).is_ok(),
            "trigger_bind failed",
        );
    }
    if have(SHOOTER) && have(VICTIM) {
        let _ = Action::set_mission_on(unit(SHOOTER), Mission::Attack, unit(VICTIM));
    }
    if have(SUPER) && Action::one_time_super_weapon(p, kind(SUPER)).is_ok() {
        st(SUPER_GRANTED, 1);
    }
}

/// Frame 5: the trigger properties `Enabled` (read and write), `House` and
/// `Repeat` (read; a write answers `Refused`).
fn trigger_properties() {
    let no = trigger(TR_HOUSE_NO);
    if have(TR_HOUSE_NO) {
        let off = set(no, Prop::Enabled, 0).is_ok() && get(no, Prop::Enabled) == Ok(0);
        let on = set(no, Prop::Enabled, 1).is_ok() && get(no, Prop::Enabled) == Ok(1);
        check(
            "triggers.prop-enabled",
            off && on,
            "Enabled did not read back",
        );
        let enemy = crate::enemy().0;
        let house = get(no, Prop::House) == Ok(enemy)
            && set(no, Prop::House, crate::player().0) == Err(Status::Refused)
            && get(no, Prop::House) == Ok(enemy);
        check("triggers.prop-house", house, "House read wrong");
    }
    if have(TR_TIMER) && have(TR_SET_VAR) {
        let timer = trigger(TR_TIMER);
        let repeat = get(timer, Prop::Repeat) == Ok(2)
            && get(trigger(TR_SET_VAR), Prop::Repeat) == Ok(0)
            && set(timer, Prop::Repeat, 0) == Err(Status::Refused)
            && get(timer, Prop::Repeat) == Ok(2);
        check("triggers.prop-repeat", repeat, "Repeat read wrong");
    }
}
