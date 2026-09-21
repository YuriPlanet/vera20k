//! HouseClass strategy-state substrate.
//!
//! This module intentionally does not schedule Strategy or execute its object
//! callbacks yet. Native order is AI-hate -> synchronous AI superweapon -> this
//! emergency block -> no-factory priority -> build-need/Manage -> reschedule.
//! Activating only the middle of that chain would perturb object and Scenario
//! RNG order. The pure state machine and candidate-bias decision can land
//! independently without inventing such a scheduler.
//! The independently active anger writer and House-update decay also live here
//! so future Strategy code reuses one score/reselection authority.

use std::collections::BTreeMap;

use crate::map::houses::HouseAllianceMap;
use crate::sim::house_state::HouseState;
#[cfg(test)]
use crate::sim::house_state::HouseStrategyEmergencyState;
use crate::sim::intern::{InternedId, StringInterner};

#[cfg(test)]
const LOW_WALLET_THRESHOLD: i32 = 25;
#[cfg(test)]
const ATTACK_SUPPRESSION_FRAMES: i32 = 900;
const ANGER_DECAY_PERIOD_FRAMES: i32 = 100;

/// Apply one signed anger delta and recompute the designated enemy.
///
/// gamemd-derived: `HouseClass__UpdateAngerNodes @ 0x00504790`. Native updates
/// only a constructor-registered peer node, then forward-scans every peer with
/// a strict-positive/strict-greater winner rule. Rust's sparse map represents
/// untouched constructor-zero nodes without materializing serialized state.
pub(crate) fn update_anger_nodes(
    houses: &mut BTreeMap<InternedId, HouseState>,
    house_order: &[InternedId],
    alliances: &HouseAllianceMap,
    interner: &StringInterner,
    owner: InternedId,
    peer: InternedId,
    delta: i32,
) {
    let peer_is_registered = peer != owner
        && house_order.iter().any(|&candidate| candidate == peer)
        && houses.contains_key(&peer);
    if peer_is_registered && let Some(house) = houses.get_mut(&owner) {
        if delta != 0 || house.grudge_scores.contains_key(&peer) {
            let score = house.grudge_scores.entry(peer).or_insert(0);
            *score = score.wrapping_add(delta);
        }
    }

    let Some(house) = houses.get(&owner) else {
        return;
    };
    let mut best_score = 0;
    let mut best_house = None;
    for &candidate_id in house_order {
        if candidate_id == owner {
            continue;
        }
        let Some(candidate) = houses.get(&candidate_id) else {
            continue;
        };
        let score = house.grudge_scores.get(&candidate_id).copied().unwrap_or(0);
        if score > best_score
            && !candidate.is_defeated
            && !crate::map::houses::is_allied_with(
                alliances,
                interner.resolve(owner),
                interner.resolve(candidate_id),
            )
        {
            best_score = score;
            best_house = Some(candidate_id);
        }
    }
    if let Some(house) = houses.get_mut(&owner) {
        house.enemy_house = best_house;
    }
}

/// Apply the unconditional House-update anger decay without enemy reselection.
///
/// gamemd-derived: `HouseClass__Update @ 0x004F8440`. On exact signed frame
/// multiples of 100, native forward-walks the registered peer vector and
/// decrements only scores strictly greater than one.
pub(crate) fn decay_anger_scores(
    house: &mut HouseState,
    house_order: &[InternedId],
    current_frame: i32,
) {
    if current_frame % ANGER_DECAY_PERIOD_FRAMES != 0 {
        return;
    }
    for &peer in house_order {
        if peer == house.name {
            continue;
        }
        if let Some(score) = house.grudge_scores.get_mut(&peer)
            && *score > 1
        {
            *score = score.wrapping_sub(1);
        }
    }
}

/// Ordered callbacks requested by the direct state-four block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmergencyAction {
    FireSale,
    AllToHunt,
}

/// Advance only `House+0x250`'s native state block.
///
/// `available_wallet` is deliberately a callback: state zero can transition to
/// one and immediately query a second time in the same invocation. Collapsing
/// this to a single sampled value would erase observable call count/order.
#[cfg(test)]
pub(crate) fn advance_emergency_state(
    state: &mut HouseStrategyEmergencyState,
    current_frame: i32,
    mut available_wallet: impl FnMut() -> i32,
) -> Vec<EmergencyAction> {
    if state.mode == 4 {
        return vec![EmergencyAction::FireSale, EmergencyAction::AllToHunt];
    }

    if state.mode == 0 && available_wallet() < LOW_WALLET_THRESHOLD {
        state.mode = 1;
    }
    if state.mode == 1 && available_wallet() >= LOW_WALLET_THRESHOLD {
        state.mode = 0;
    }

    let deadline = state
        .last_building_attack_frame
        .wrapping_add(ATTACK_SUPPRESSION_FRAMES);
    if state.mode == 3 {
        if deadline < current_frame {
            state.mode = 0;
        }
    } else if current_frame < deadline {
        state.mode = 3;
    }

    Vec::new()
}

/// Exact `House+0x249` decision consumed by native
/// `TechnoClass__Evaluate_Candidate @ 0x006F8765`.
///
/// VERA's current acquisition ranking is explicitly not that native evaluator,
/// so this helper remains disconnected until the expanding-ring score path is
/// implemented. Translating native score `1` into the current nearest-first
/// tuple would be an approximation, not parity.
#[cfg(test)]
pub(crate) fn all_to_hunt_score_override(
    attacker_house: &HouseState,
    candidate_owner: InternedId,
) -> Option<i32> {
    if attacker_house.strategy_emergency.all_to_hunt_bias
        && attacker_house
            .enemy_house
            .is_some_and(|enemy| candidate_owner != enemy)
    {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use crate::sim::house_state::HouseState;

    fn state(mode: i32, last_attack: i32) -> HouseStrategyEmergencyState {
        HouseStrategyEmergencyState {
            mode,
            all_to_hunt_bias: false,
            last_building_attack_frame: last_attack,
            last_attacker_house_index: -1,
        }
    }

    fn house(name: InternedId, side: u8) -> HouseState {
        HouseState::new(name, side, None, false, 0, 10)
    }

    #[test]
    fn gsi_04_05_update_anger_nodes_wraps_and_selects_first_positive_eligible_peer() {
        let mut interner = StringInterner::new();
        let owner = interner.intern("OWNER");
        let allied = interner.intern("ALLIED");
        let defeated = interner.intern("DEFEATED");
        let missing = interner.intern("MISSING");
        let second = interner.intern("SECOND");
        let first = interner.intern("FIRST");
        let unregistered = interner.intern("UNREGISTERED");
        let house_order = [owner, allied, defeated, missing, first, second];
        assert!(
            second < first,
            "fixture intern order must oppose the equal-score House order"
        );
        let mut defeated_house = house(defeated, 2);
        defeated_house.is_defeated = true;
        let mut houses = BTreeMap::from([
            (owner, house(owner, 0)),
            (allied, house(allied, 1)),
            (defeated, defeated_house),
            (first, house(first, 3)),
            (second, house(second, 4)),
        ]);
        let mut alliances = HouseAllianceMap::new();
        alliances.insert("OWNER".to_string(), BTreeSet::from(["ALLIED".to_string()]));
        {
            let anger = &mut houses.get_mut(&owner).unwrap().grudge_scores;
            anger.insert(allied, 99);
            anger.insert(defeated, 98);
            anger.insert(missing, 97);
            anger.insert(first, 7);
            anger.insert(second, 7);
        }

        update_anger_nodes(
            &mut houses,
            &house_order,
            &alliances,
            &interner,
            owner,
            first,
            0,
        );
        assert_eq!(houses[&owner].enemy_house, Some(first));

        houses.get_mut(&owner).unwrap().enemy_house = None;
        let missing_score = houses[&owner].grudge_scores[&missing];
        update_anger_nodes(
            &mut houses,
            &house_order,
            &alliances,
            &interner,
            owner,
            missing,
            5,
        );
        assert_eq!(houses[&owner].grudge_scores[&missing], missing_score);
        assert_eq!(
            houses[&owner].enemy_house,
            Some(first),
            "a rejected in-order null peer still triggers the full ordered rescan"
        );

        houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores
            .insert(first, i32::MAX);
        houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores
            .insert(second, 1);
        update_anger_nodes(
            &mut houses,
            &house_order,
            &alliances,
            &interner,
            owner,
            first,
            1,
        );
        assert_eq!(houses[&owner].grudge_scores[&first], i32::MIN);
        assert_eq!(houses[&owner].enemy_house, Some(second));

        update_anger_nodes(
            &mut houses,
            &house_order,
            &alliances,
            &interner,
            owner,
            unregistered,
            5,
        );
        assert!(!houses[&owner].grudge_scores.contains_key(&unregistered));
        assert_eq!(houses[&owner].enemy_house, Some(second));
        houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores
            .insert(second, 0);
        update_anger_nodes(
            &mut houses,
            &house_order,
            &alliances,
            &interner,
            owner,
            owner,
            5,
        );
        assert!(!houses[&owner].grudge_scores.contains_key(&owner));
        assert_eq!(houses[&owner].enemy_house, None);
    }

    #[test]
    fn gsi_04_05_sparse_zero_updates_preserve_representation_and_rescan() {
        let mut interner = StringInterner::new();
        let owner = interner.intern("OWNER");
        let first = interner.intern("FIRST");
        let second = interner.intern("SECOND");
        let house_order = [owner, first, second];
        let mut houses = BTreeMap::from([
            (owner, house(owner, 0)),
            (first, house(first, 1)),
            (second, house(second, 2)),
        ]);
        houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores
            .insert(second, 5);
        houses.get_mut(&owner).unwrap().enemy_house = Some(first);

        update_anger_nodes(
            &mut houses,
            &house_order,
            &HouseAllianceMap::new(),
            &interner,
            owner,
            first,
            0,
        );
        assert!(!houses[&owner].grudge_scores.contains_key(&first));
        assert_eq!(houses[&owner].enemy_house, Some(second));

        houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores
            .insert(first, 5);
        houses.get_mut(&owner).unwrap().enemy_house = None;
        update_anger_nodes(
            &mut houses,
            &house_order,
            &HouseAllianceMap::new(),
            &interner,
            owner,
            first,
            -5,
        );
        assert_eq!(houses[&owner].grudge_scores.get(&first), Some(&0));
        assert_eq!(houses[&owner].enemy_house, Some(second));
    }

    #[test]
    fn gsi_04_05_anger_decay_uses_signed_frame_boundaries_and_strict_score_gate() {
        let mut interner = StringInterner::new();
        let owner = interner.intern("OWNER");
        let minimum = interner.intern("MINIMUM");
        let zero = interner.intern("ZERO");
        let one = interner.intern("ONE");
        let two = interner.intern("TWO");
        let house_order = [owner, minimum, zero, one, two];
        let mut base = house(owner, 0);
        base.grudge_scores.insert(minimum, i32::MIN);
        base.grudge_scores.insert(zero, 0);
        base.grudge_scores.insert(one, 1);
        base.grudge_scores.insert(two, 2);

        for frame in [99, 101] {
            let mut unchanged = base.clone();
            decay_anger_scores(&mut unchanged, &house_order, frame);
            assert_eq!(unchanged.grudge_scores, base.grudge_scores);
        }
        for frame in [100, -100, 0] {
            let mut decayed = base.clone();
            decay_anger_scores(&mut decayed, &house_order, frame);
            assert_eq!(decayed.grudge_scores[&minimum], i32::MIN);
            assert_eq!(decayed.grudge_scores[&zero], 0);
            assert_eq!(decayed.grudge_scores[&one], 1);
            assert_eq!(decayed.grudge_scores[&two], 1);
        }
    }

    #[test]
    fn gsi_04_05_anger_decay_does_not_reselect_enemy() {
        let mut interner = StringInterner::new();
        let owner = interner.intern("OWNER");
        let selected = interner.intern("SELECTED");
        let stronger = interner.intern("STRONGER");
        let house_order = [owner, selected, stronger];
        let mut owner_house = house(owner, 0);
        owner_house.grudge_scores.insert(selected, 2);
        owner_house.grudge_scores.insert(stronger, 5);
        owner_house.enemy_house = Some(selected);

        decay_anger_scores(&mut owner_house, &house_order, 100);

        assert_eq!(owner_house.grudge_scores[&selected], 1);
        assert_eq!(owner_house.grudge_scores[&stronger], 4);
        assert_eq!(owner_house.enemy_house, Some(selected));
    }

    #[test]
    fn state_zero_below_25_requeries_immediately() {
        let mut emergency = state(0, -900);
        let mut calls = 0;
        let actions = advance_emergency_state(&mut emergency, 1, || {
            calls += 1;
            if calls == 1 { 24 } else { 25 }
        });
        assert_eq!(calls, 2);
        assert_eq!(emergency.mode(), 0);
        assert!(actions.is_empty());
    }

    #[test]
    fn wallet_threshold_hysteresis_is_signed_24_25() {
        let mut low = state(1, -900);
        advance_emergency_state(&mut low, 1, || 24);
        assert_eq!(low.mode(), 1);

        let mut recovered = state(1, -900);
        advance_emergency_state(&mut recovered, 1, || 25);
        assert_eq!(recovered.mode(), 0);
    }

    #[test]
    fn state_three_deadline_equality_is_asymmetric() {
        let mut before = state(0, 100);
        advance_emergency_state(&mut before, 999, || 25);
        assert_eq!(before.mode(), 3);

        let mut armed_at_equal = state(3, 100);
        advance_emergency_state(&mut armed_at_equal, 1000, || 25);
        assert_eq!(armed_at_equal.mode(), 3);

        let mut unarmed_at_equal = state(0, 100);
        advance_emergency_state(&mut unarmed_at_equal, 1000, || 25);
        assert_eq!(unarmed_at_equal.mode(), 0);

        let mut after = state(3, 100);
        advance_emergency_state(&mut after, 1001, || 25);
        assert_eq!(after.mode(), 0);
    }

    #[test]
    fn attack_deadline_uses_wrapping_signed_addition() {
        let last_attack = i32::MAX - 100;
        let wrapped_deadline = last_attack.wrapping_add(ATTACK_SUPPRESSION_FRAMES);
        assert!(wrapped_deadline < 0);

        let mut emergency = state(3, last_attack);
        advance_emergency_state(&mut emergency, wrapped_deadline, || 25);
        assert_eq!(emergency.mode(), 3, "equality retains an existing state 3");
        advance_emergency_state(&mut emergency, wrapped_deadline.wrapping_add(1), || 25);
        assert_eq!(emergency.mode(), 0);
    }

    #[test]
    fn state_four_emits_ordered_actions_without_clearing() {
        let mut emergency = state(4, 0);
        let mut wallet_called = false;
        let actions = advance_emergency_state(&mut emergency, 10_000, || {
            wallet_called = true;
            0
        });
        assert_eq!(
            actions,
            vec![EmergencyAction::FireSale, EmergencyAction::AllToHunt]
        );
        assert_eq!(emergency.mode(), 4);
        assert!(!wallet_called);
    }

    #[test]
    fn all_to_hunt_override_is_persistent_and_follows_designated_enemy() {
        let owner = InternedId::from_index(1);
        let first_enemy = InternedId::from_index(2);
        let second_enemy = InternedId::from_index(3);
        let bystander = InternedId::from_index(4);
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);

        house.enemy_house = Some(first_enemy);
        assert_eq!(all_to_hunt_score_override(&house, bystander), None);

        house.strategy_emergency.set_all_to_hunt_bias();
        assert_eq!(all_to_hunt_score_override(&house, first_enemy), None);
        assert_eq!(all_to_hunt_score_override(&house, bystander), Some(1));

        house.enemy_house = Some(second_enemy);
        assert_eq!(all_to_hunt_score_override(&house, first_enemy), Some(1));
        assert_eq!(all_to_hunt_score_override(&house, second_enemy), None);

        house.enemy_house = None;
        assert_eq!(all_to_hunt_score_override(&house, bystander), None);
        assert!(house.strategy_emergency.all_to_hunt_bias());
    }

    #[test]
    fn native_entry_writers_change_only_their_owned_fields() {
        let mut emergency = state(-7, 123);
        emergency.set_state_four();
        assert_eq!(emergency.mode(), 4);
        assert_eq!(emergency.last_building_attack_frame(), 123);
        assert!(!emergency.all_to_hunt_bias());

        emergency.note_building_attack(-55);
        assert_eq!(emergency.last_building_attack_frame(), -55);
        assert_eq!(emergency.mode(), 4);
    }

    #[test]
    fn non_bincode_missing_house_field_uses_native_constructor_defaults() {
        let owner = InternedId::from_index(1);
        let house = HouseState::new(owner, 0, None, false, 0, 10);
        let mut value = serde_json::to_value(house).expect("HouseState serializes to JSON");
        value
            .as_object_mut()
            .expect("HouseState JSON is an object")
            .remove("strategy_emergency");

        let restored: HouseState =
            serde_json::from_value(value).expect("serde default fills the absent field");
        assert_eq!(
            restored.strategy_emergency,
            HouseStrategyEmergencyState::default()
        );
        assert_eq!(restored.strategy_emergency.mode(), 0);
        assert!(!restored.strategy_emergency.all_to_hunt_bias());
        assert_eq!(restored.strategy_emergency.last_building_attack_frame(), 0);
    }
}
