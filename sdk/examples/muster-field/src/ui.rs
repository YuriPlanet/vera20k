//! The interface's side of the simulation (`ui.wasm`, `muster_field_data::proto`):
//! the camp published as the player's private variables, and the buttons'
//! messages turned into the orders they stand for. (Owner: lead)

use vera20k_sdk::prelude::*;

use crate::state::{self, BANNERS, CAMP_RANK, Num, PHASE, PICKS, WAITING, WAVE};
use crate::{battle, campaign, perks, shop};
use muster_field_data::proto;

vera20k_sdk::vars! {
    static PHASE_VAR: HouseVar = ("MfPhase", Private, 0);
    static WAVE_VAR: HouseVar = ("MfWave", Private, 0);
    static BANNERS_VAR: HouseVar = ("MfBanners", Private, 0);
    static RANK_VAR: HouseVar = ("MfRank", Private, 0);
    static PICKS_VAR: HouseVar = ("MfPicks", Private, 0);
    static FLAGS_VAR: HouseVar = ("MfFlags", Private, 0);
    static OFFERS_VAR: HouseVar = ("MfOffers", Private, 0);
    static CHOICES_VAR: HouseVar = ("MfChoices", Private, 0);
    static COMPANY_LOW_VAR: HouseVar = ("MfCompanyLow", Private, 0);
    static COMPANY_HIGH_VAR: HouseVar = ("MfCompanyHigh", Private, 0);
    static TAKEN_VAR: HouseVar = ("MfTaken", Private, 0);
    static BATTLE_VAR: HouseVar = ("MfBattle", Private, 0);
    static FRONT_VAR: HouseVar = ("MfFront", Private, 0);
    static LAST_VAR: HouseVar = ("MfLast", Private, 0);
}

// The names above are `proto`'s: the macro takes literals.
const _: () = {
    const fn same(a: &str, b: &str) -> bool {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut at = 0;
        while at < a.len() {
            if a[at] != b[at] {
                return false;
            }
            at += 1;
        }
        true
    }
    assert!(same(proto::PHASE, "MfPhase"));
    assert!(same(proto::WAVE, "MfWave"));
    assert!(same(proto::BANNERS, "MfBanners"));
    assert!(same(proto::RANK, "MfRank"));
    assert!(same(proto::PICKS, "MfPicks"));
    assert!(same(proto::FLAGS, "MfFlags"));
    assert!(same(proto::OFFERS, "MfOffers"));
    assert!(same(proto::CHOICES, "MfChoices"));
    assert!(same(proto::COMPANY_LOW, "MfCompanyLow"));
    assert!(same(proto::COMPANY_HIGH, "MfCompanyHigh"));
    assert!(same(proto::TAKEN, "MfTaken"));
    assert!(same(proto::BATTLE, "MfBattle"));
    assert!(same(proto::FRONT, "MfFront"));
    assert!(same(proto::LAST, "MfLast"));
};

const VARS: usize = 14;
/// What each variable was last written as, so an unchanged one costs no
/// host call.
static WRITTEN: [Num; VARS] = [const { Num::new(0) }; VARS];

/// Writes the variables that changed. Every frame once the camp is open.
pub fn publish() {
    if PHASE.get() == WAITING {
        return;
    }
    let player = state::player();
    let view = shop::ui_view();
    let (ours, theirs, arrived) = battle::standing();
    let mut taken = 0;
    for (perk, count) in perks::taken_all().into_iter().enumerate() {
        taken = proto::with_taken(taken, perk, count);
    }
    let values: [(&HouseVar, i64); VARS] = [
        (&PHASE_VAR, PHASE.get()),
        (&WAVE_VAR, WAVE.get()),
        (&BANNERS_VAR, BANNERS.get()),
        (&RANK_VAR, CAMP_RANK.get()),
        (&PICKS_VAR, PICKS.get()),
        (&FLAGS_VAR, view.flags),
        (&OFFERS_VAR, view.offers),
        (&CHOICES_VAR, view.choices),
        (&COMPANY_LOW_VAR, view.company[0]),
        (&COMPANY_HIGH_VAR, view.company[1]),
        (&TAKEN_VAR, taken),
        (&BATTLE_VAR, proto::battle(ours, theirs, arrived)),
        (&FRONT_VAR, battle::front_index()),
        (&LAST_VAR, campaign::last()),
    ];
    for ((var, value), written) in values.iter().zip(&WRITTEN) {
        if written.get() != *value && var.set(player, *value).is_ok() {
            written.set(*value);
        }
    }
}

/// A button's message: `[what, index]` from the player, in a muster. The
/// order it stands for joins the frame's orders.
fn message(sender: House, data: &[u8]) {
    let [what, index] = data else {
        return;
    };
    if sender != state::player() || PHASE.get() != state::MUSTER {
        return;
    }
    if let Some(order) = shop::ui_order(*what, usize::from(*index)) {
        crate::queue(order);
    }
}

vera20k_sdk::on_message!(message);
