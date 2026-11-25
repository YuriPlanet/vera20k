//! The records the area's triggers deliver: each marks its event once its
//! fields are what the scene made.

use super::*;

/// One record of this area's range.
pub(super) fn dispatch(r: &EventRecord) {
    match r.key {
        K_TAKE_DAMAGE => {
            if have(WARHEAD_AP) && r.subject == ld(DUMMY) && r.value > 0 {
                check(
                    "triggers.ev-take-damage",
                    r.id == kind(WARHEAD_AP).id(),
                    "the warhead is not AP",
                );
            }
        }
        K_KILLED => {
            if have(WARHEAD_AP) && r.id == kind(WARHEAD_AP).id() {
                crate::mark("triggers.ev-killed-warhead");
            } else if r.subject == ld(KILLED) {
                crate::bench_fail("triggers.ev-killed-warhead", "no AP warhead on the kill");
            }
        }
        K_FIRE => {
            if r.subject == ld(SHOOTER) && have(WEAPON_105) {
                check(
                    "triggers.ev-fire",
                    r.id == kind(WEAPON_105).id(),
                    "the weapon is not 105mm",
                );
            }
        }
        K_TIMER => timer(r),
        // Both events latch on the dummy's instance; whichever comes second
        // completes it and is the record (a hit before the period's end
        // leaves the Timer to complete it).
        K_TIMER_OBJECT => {
            if (r.kind == Event::TAKE_DAMAGE || r.kind == Event::TIMER)
                && r.subject == ld(DUMMY)
                && r.holder == ld(DUMMY)
            {
                crate::mark("triggers.ev-timer-object");
            }
        }
        K_ATTACKED_BY => {
            if r.subject == ld(VICTIM) && r.other != NO_HANDLE {
                crate::mark("triggers.ev-attacked-by");
            }
        }
        K_BIRTH => {
            if have(BAIT) && r.subject == ld(BAIT) {
                crate::mark("triggers.ev-birth");
                retire(BAIT);
            }
        }
        K_ENTERED => transport(r, RIDER, IFV, "triggers.ev-transport-entered"),
        K_ADDED => transport(r, IFV, RIDER, "triggers.ev-passenger-added"),
        K_EXITED => transport(r, RIDER, IFV, "triggers.ev-transport-exited"),
        K_REMOVED => transport(r, IFV, RIDER, "triggers.ev-passenger-removed"),
        K_TRANSFERRED => transferred(r),
        K_SW_LAUNCHED => {
            if r.house == crate::player().0 && have(SUPER) {
                check(
                    "triggers.ev-sw-launched",
                    r.id == kind(SUPER).id(),
                    "the superweapon type is wrong",
                );
            }
        }
        K_MOD_START => {
            if have(MODIFIER) && r.modifier == ld(MODIFIER) {
                check(
                    "triggers.ev-mod-start",
                    r.value == i64::from(Prop::FirepowerMultiplier.0),
                    "the property is wrong",
                );
            }
        }
        K_MOD_STACKS => {
            if r.subject == ld(DUMMY) {
                check(
                    "triggers.ev-mod-stacks",
                    r.value >= 1,
                    "the group count is wrong",
                );
            }
        }
        K_MOD_EXPIRE => {
            if have(MODIFIER) && r.modifier == ld(MODIFIER) {
                crate::mark("triggers.ev-mod-expire");
            }
        }
        K_PING => ping(r),
        K_PONG => {
            if r.subject == ld(DUMMY) {
                check("triggers.chain", r.value == 5, "the chained value is wrong");
            }
        }
        K_GLOBAL_EVENT => check(
            "triggers.ev-module-global",
            r.value == 3 && r.value2 == 4,
            "the values are wrong",
        ),
        K_STOCK_ATTACKED => {
            if r.subject == ld(VICTIM) {
                crate::mark("triggers.ev-stock-attacked");
            }
        }
        K_STOCK_DESTROYED => {
            if r.subject == ld(VICTIM) {
                crate::mark("triggers.ev-stock-destroyed");
            }
        }
        K_STOCK_HOUSE => check(
            "triggers.ev-stock-house",
            r.frame % 8 == 0 && r.house == crate::player().0,
            "not the player on an eighth frame",
        ),
        K_STOCK_LOGIC => crate::mark("triggers.ev-stock-logic"),
        K_CELL_ROW => check(
            "triggers.ev-cell-row",
            r.holder == ld(CELL_ROW),
            "not the bound cell",
        ),
        K_CELL_COLUMN => check(
            "triggers.ev-cell-column",
            r.holder == ld(CELL_COLUMN),
            "not the bound cell",
        ),
        K_POLL_PROPERTY => check(
            "triggers.poll-property",
            r.value < 300,
            "the reading is not below 300",
        ),
        K_POLL_VARIABLE => check(
            "triggers.poll-variable",
            r.value == 1,
            "the reading is not 1",
        ),
        K_POLL_POWER => crate::mark("triggers.poll-power"),
        K_POLL_MOD_COUNT => check(
            "triggers.poll-modifier-count",
            r.value >= 1,
            "the count is below 1",
        ),
        K_POLL_DISTANCE => check(
            "triggers.poll-distance",
            r.value > 512,
            "the reading is not past 512",
        ),
        K_POLL_MONEY => check(
            "triggers.poll-money",
            r.value >= 0,
            "the reading is negative",
        ),
        K_FILTER_SUBJECT => {
            if r.subject == ld(DUMMY) {
                st(SUBJECT_YES, 1);
            } else if r.subject == ld(FOOT) {
                st(SUBJECT_NO, 1);
                crate::bench_fail("triggers.filter-subject", "the clause let the GI through");
            }
        }
        K_FILTER_OTHER => {
            if r.other == ld(FOOT) {
                st(OTHER_YES, 1);
            } else if r.other == ld(DUMMY) {
                st(OTHER_NO, 1);
                crate::bench_fail("triggers.filter-other", "the clause let a vehicle through");
            }
        }
        K_FILTER_HOUSE_YES => st(HOUSE_YES, 1),
        K_FILTER_HOUSE_NO => {
            st(HOUSE_NO, 1);
            crate::bench_fail(
                "triggers.filter-house",
                "the clause let a computer house through",
            );
        }
        K_UNBIND => {
            if r.subject == ld(DUMMY) {
                st(UNBIND_YES, 1);
            } else if r.subject == ld(FOOT) {
                st(UNBIND_NO, 1);
                crate::bench_fail("triggers.unbind", "an unbound object raised the event");
            }
        }
        K_SET_VAR => check(
            "triggers.act-set-var",
            Var::local(87).get() == 1,
            "local 87 did not read back",
        ),
        K_FIRE_WEAPON_TYPE => typed_fire(r, TR_WEAPON, "triggers.bind-weapon"),
        K_FIRE_WARHEAD_TYPE => typed_fire(r, TR_WARHEAD, "triggers.bind-warhead"),
        K_FIRE_PROJECTILE_TYPE => typed_fire(r, TR_PROJECTILE, "triggers.bind-projectile"),
        K_COMMANDED => commanded(r),
        K_ORDER_REFUSED => {
            if r.house == crate::enemy().0 && r.value == Command::TakeProduct as i64 {
                crate::mark("triggers.order-rejected");
            }
        }
        INI_DOG => ini_dog(r),
        K_BIRTH_TEAM | K_BIRTH_CHRONO | K_BIRTH_BUILDING => super::teams::record(r),
        _ => {}
    }
}

/// The global `Timer`: its record names the previous period's frame; then
/// the trigger is removed (`trigger_remove`) and no longer reads.
fn timer(r: &EventRecord) {
    check(
        "triggers.ev-timer",
        r.value < i64::from(r.frame),
        "the last frame is not before this one",
    );
    if marked_remove() || !have(TR_TIMER) {
        return;
    }
    let timer = trigger(TR_TIMER);
    let removed = timer.remove().is_ok() && get(timer, Prop::Enabled).is_err();
    check(
        "triggers.remove",
        removed,
        "the removed trigger still reads",
    );
}

fn marked_remove() -> bool {
    crate::marked("triggers.remove")
}

/// A transport record on `subject` naming `other`.
fn transport(r: &EventRecord, subject: usize, other: usize, mark: &str) {
    if have(subject) && r.subject == ld(subject) {
        check(mark, r.other == ld(other), "the other object is wrong");
    }
}

/// `HouseTransferred`: the GI given away (value 0, old house the player),
/// then the engineer the Yuri took (value 1, old house the player),
/// released at once. The release is a mind-control transfer too (FreeUnit's
/// ChangeOwner, value 1, old house the computer's), which is left alone.
fn transferred(r: &EventRecord) {
    if have(SWITCHED) && r.subject == ld(SWITCHED) {
        check(
            "triggers.ev-house-transferred",
            r.value == 0 && r.house == crate::player().0,
            "the cause or old house is wrong",
        );
    } else if have(CONTROLLED)
        && r.subject == ld(CONTROLLED)
        && r.value == 1
        && r.house == crate::player().0
    {
        crate::mark("triggers.ev-mind-control");
        let controlled = unit(CONTROLLED);
        let freed = Action::clear_mind_control(controlled).is_ok()
            && controlled.owner() == Ok(crate::player());
        check(
            "triggers.act-clear-mind-control",
            freed,
            "the engineer did not return",
        );
        retire(YURI);
        retire(CONTROLLED);
    }
}

/// The ping trigger's immediate actions ran before its module action:
/// the dummy healed by 30, the player paid 7.
fn ping(r: &EventRecord) {
    if r.subject != ld(DUMMY) {
        return;
    }
    check(
        "triggers.ev-module-event",
        r.value == 1 && r.value2 == 2,
        "the raised values are wrong",
    );
    check(
        "triggers.imm-heal",
        health(unit(DUMMY)) == ld(PING_HEALTH) + 30,
        "Heal did not add 30",
    );
    check(
        "triggers.imm-add-money",
        money() == ld(PING_MONEY) + 7,
        "AddMoney did not pay 7",
    );
}

/// A `Fire` record of a weapon, warhead or projectile binding: marked,
/// then that trigger is removed (every shooter of the type raises it).
fn typed_fire(r: &EventRecord, slot: usize, mark: &str) {
    if r.subject != NO_HANDLE {
        crate::mark(mark);
    }
    if have(slot) {
        let _ = trigger(slot).remove();
        st(slot, 0);
    }
}

/// `Commanded` on the ordered Rhinos and the Yuri, by command.
fn commanded(r: &EventRecord) {
    let ours = [ORDERED_1, ORDERED_2, YURI]
        .iter()
        .any(|&slot| have(slot) && r.subject == ld(slot));
    if !ours {
        return;
    }
    let mark = match r.value {
        v if v == Command::Move as i64 => "triggers.order-move",
        v if v == Command::Guard as i64 => "triggers.order-guard",
        v if v == Command::Attack as i64 => "triggers.order-attack",
        v if v == Command::Stop as i64 => "triggers.order-stop",
        v if v == Command::Scatter as i64 => "triggers.order-scatter",
        _ => return,
    };
    crate::mark(mark);
}

/// The INI trigger's `Wasm:benchIniDog(Subject,dog)` action.
fn ini_dog(r: &EventRecord) {
    let mut buf = [0u8; 16];
    check(
        "triggers.ini-trigger",
        r.args_text(&mut buf) == Ok("dog"),
        "the action text is wrong",
    );
    if have(DOG) && r.subject == ld(DOG) {
        retire(DOG);
    }
}
