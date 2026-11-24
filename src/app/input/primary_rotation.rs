//! VERA primary rotation: several factories of one kind take turns being the
//! primary, so their units come out of each in turn instead of all from one.
//!
//! The player selects the factories and presses the Rotate Primary key. From
//! then on, each time the House finishes an object of that kind, the next
//! factory of the group is made primary with the ordinary PRIMARY event a
//! click on a factory sends (`Command::TogglePrimaryFactory`,
//! `BuildingClass` `0x00448160`). gamemd's one-primary rule stays as it is;
//! nothing here mutates simulation state, and a replay or another client sees
//! only those ordinary orders.
//!
//! The PRIMARY event clears a factory that is primary already, so a second
//! order to the same factory before the first has landed would leave the
//! kind without one. Rotation sends nothing of that kind while an admitted
//! order is on its way, even if the selection changes: until the factory it
//! names is primary or the order has surely executed ([`command_executed_by`]).
//! A full command queue or a scenario input lock leaves the turn owed.
//! The local game skips each order's "Primary building selected" line
//! ([`QuietSounds`](crate::app::match_audio::QuietSounds)).
//!
//! A group is local input state: it is not saved, the computer does not use
//! it, it ends with the match, and it ends when the key is pressed on the
//! same factories again or fewer than two of them are left.

use crate::app::AppState;
use crate::app::input::commands::{
    command_executed_by, preferred_local_owner_name, try_schedule_command,
};
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::app::match_audio::QuietSound;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectCategory};
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::world::{ControlProjection, Simulation};

/// Logic frames between two looks at the groups.
const CHECK_INTERVAL: u64 = 8;
/// The line the PRIMARY event announces for a human owner's factory
/// (`BuildingClass` `0x00448160`, `toggle_factory_primary`).
const PRIMARY_SELECTED_EVA: &str = "EVA_PrimaryBuildingSelected";

/// What a group's factories make, and whether they are naval: the same
/// grouping gamemd keeps one primary for.
type Kind = (FactoryType, bool);

/// One entry per kind. The selection can end before its admitted order does;
/// keep an empty entry until that order resolves.
#[derive(Debug)]
struct RotationGroup {
    kind: Kind,
    /// The factories that take turns, in the order they were selected.
    members: Vec<u64>,
    /// The finished-object count at activation or the last admitted turn.
    built: i64,
    /// A new selection still needs a primary if it has none.
    initial: bool,
    /// The target of an admitted order and the frame it has executed by.
    pending: Option<(u64, u64)>,
}

/// A proposed turn; neither the count nor the in-flight guard is committed
/// until the ordinary command queue admits the toggle.
struct Turn {
    kind: Kind,
    target: u64,
    built: i64,
}

/// What the ordinary command queue did with a proposed turn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Admitted {
    /// The queue took the order: the turn is spent and the guard is armed.
    Yes,
    /// The queue had no room: the turn is still owed, and the group sends
    /// it again once the queue has room.
    NoRoom,
    /// The queue refused the order for good: the turn is spent, and the
    /// group does not offer it again.
    Refused,
}

impl RotationGroup {
    fn turn(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        now: u64,
    ) -> Option<Turn> {
        if let Some((target, by)) = self.pending {
            // The target may no longer be in this selection.
            if now < by && !sim.production().is_primary_factory(target) {
                return None;
            }
            self.pending = None;
        }
        // A scenario can lock player input while its factories keep building.
        // Leave the completion count untouched until orders are allowed again.
        if self.members.len() < 2 || sim.session().input_locked() {
            return None;
        }
        let primary = self
            .members
            .iter()
            .copied()
            .find(|&id| sim.production().is_primary_factory(id));
        self.initial &= primary.is_none();
        let built = built(sim, rules, owner, self.kind);
        if !self.initial && built == self.built {
            return None;
        }
        let target = next_primary(&self.members, primary)?;
        if !self.initial && !crate::sim::production::can_select_factory_primary(sim, rules, target)
        {
            return None;
        }
        Some(Turn {
            kind: self.kind,
            target,
            built,
        })
    }
}

/// The local player's rotation groups, at most one for each kind.
#[derive(Debug, Default)]
pub(crate) struct PrimaryRotationState {
    groups: Vec<RotationGroup>,
}

/// The member that takes over from `primary`: the next in order, wrapping
/// round, or the first when the primary is not one of them. `None` when no
/// other member can.
pub(crate) fn next_primary(members: &[u64], primary: Option<u64>) -> Option<u64> {
    let next = match primary.and_then(|id| members.iter().position(|&member| member == id)) {
        Some(at) => members[(at + 1) % members.len()],
        None => *members.first()?,
    };
    (Some(next) != primary).then_some(next)
}

/// The House's finished-object counter a kind's factories feed.
fn counted_as(kind: Kind) -> Option<ObjectCategory> {
    match kind.0 {
        FactoryType::InfantryType => Some(ObjectCategory::Infantry),
        FactoryType::UnitType => Some(ObjectCategory::Vehicle),
        FactoryType::AircraftType => Some(ObjectCategory::Aircraft),
        // A construction yard's primary decides placement, not an exit.
        FactoryType::BuildingType => None,
    }
}

/// The kind of an own, live factory building that can rotate.
fn factory_kind(
    sim: &Simulation,
    control: &ControlProjection<'_>,
    rules: &RuleSet,
    id: u64,
    owner: InternedId,
) -> Option<Kind> {
    let entity = sim.entities().get(id)?;
    if !entity.is_alive()
        || !control.can_manage_primary_house(entity.owner(), owner)
        || entity.category() != EntityCategory::Structure
    {
        return None;
    }
    let object = sim.interner().object_type(entity.type_ref(), rules)?;
    let kind = (object.factory_type()?, object.naval);
    counted_as(kind).map(|_| kind)
}

/// The House's finished objects of the kind: War Factory and Naval Yard
/// products feed one counter, but each kind counts only its own.
fn built(sim: &Simulation, rules: &RuleSet, owner: InternedId, kind: Kind) -> i64 {
    counted_as(kind)
        .and_then(|category| {
            sim.houses()
                .get(&owner)
                .map(|house| house.produced_total(rules, category, kind.1))
        })
        .unwrap_or(0)
}

impl PrimaryRotationState {
    /// The key on a selection: each kind's selected factories become its
    /// group, or stop being it when they already are. Returns the factories
    /// now rotating among those kinds, and any initial turns to try.
    fn toggle(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        selected: &[u64],
        now: u64,
    ) -> Option<(usize, Vec<Turn>)> {
        let control = sim.control_projection();
        let mut kinds: Vec<(Kind, Vec<u64>)> = Vec::new();
        for &id in selected {
            let Some(kind) = factory_kind(sim, &control, rules, id, owner) else {
                continue;
            };
            match kinds.iter_mut().find(|(seen, _)| *seen == kind) {
                Some((_, members)) => members.push(id),
                None => kinds.push((kind, vec![id])),
            }
        }
        if kinds.is_empty() {
            return None;
        }
        let mut rotating = 0;
        let mut turns = Vec::new();
        for (kind, members) in kinds {
            let at = self.groups.iter().position(|group| group.kind == kind);
            let old = at.map(|at| self.groups.remove(at));
            // The same factories in any order: a band box picks them in its own.
            let same = old.as_ref().is_some_and(|group| {
                group.members.len() == members.len()
                    && members.iter().all(|id| group.members.contains(id))
            });
            if same || members.len() < 2 {
                if let Some(mut group) = old.filter(|group| group.pending.is_some()) {
                    group.members.clear();
                    self.groups.push(group);
                }
                continue;
            }
            rotating += members.len();
            let mut group = RotationGroup {
                kind,
                built: built(sim, rules, owner, kind),
                members,
                initial: true,
                pending: old.and_then(|group| group.pending),
            };
            turns.extend(group.turn(sim, rules, owner, now));
            self.groups.push(group);
        }
        Some((rotating, turns))
    }

    /// Drop lost members and try each eligible turn. A discontinued group's
    /// admitted order must still resolve before a new group of its kind turns.
    fn check(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        now: u64,
    ) -> Vec<Turn> {
        let control = sim.control_projection();
        let mut turns = Vec::new();
        self.groups.retain_mut(|group| {
            group
                .members
                .retain(|&id| factory_kind(sim, &control, rules, id, owner) == Some(group.kind));
            if group.members.len() < 2 {
                group.members.clear();
            }
            turns.extend(group.turn(sim, rules, owner, now));
            !group.members.is_empty() || group.pending.is_some()
        });
        turns
    }

    fn admit_turns(
        &mut self,
        turns: Vec<Turn>,
        until: u64,
        mut admit: impl FnMut(Command) -> Admitted,
    ) {
        for turn in turns {
            let Some(group) = self.groups.iter_mut().find(|group| group.kind == turn.kind) else {
                continue;
            };
            match admit(Command::TogglePrimaryFactory {
                entity_id: turn.target,
            }) {
                Admitted::Yes => {
                    group.built = turn.built;
                    group.initial = false;
                    group.pending = Some((turn.target, until));
                }
                Admitted::NoRoom => {
                    // The queue was full: the turn is still owed, and the
                    // group offers it again.
                }
                Admitted::Refused => {
                    // The order will never go out: the turn is spent, and
                    // the group waits for the next finished object.
                    group.built = turn.built;
                    group.initial = false;
                }
            }
        }
    }
}

/// Send the groups' orders; each one admitted leaves its "Primary building
/// selected" line unheard on this game until frame `until`.
fn send(
    state: &mut AppState,
    rotation: &mut PrimaryRotationState,
    owner_name: &str,
    owner: InternedId,
    turns: Vec<Turn>,
    now: u64,
) {
    let until = command_executed_by(state, now);
    rotation.admit_turns(turns, until, |order| {
        // A full local queue is backpressure: the order goes out later, and
        // the turn is still owed. Any other refusal is permanent, and the
        // group does not offer the turn again.
        let no_room = state
            .match_state
            .sim_runtime
            .runtime()
            .is_some_and(|runtime| runtime.simulation.available_local_command_slots() == 0);
        if try_schedule_command(state, owner_name, order).is_none() {
            return if no_room {
                Admitted::NoRoom
            } else {
                Admitted::Refused
            };
        }
        state.match_state.match_audio.quiet.expect(
            QuietSound::HouseEva {
                owner,
                event: PRIMARY_SELECTED_EVA,
            },
            now,
            until,
        );
        Admitted::Yes
    });
}

/// The Rotate Primary key.
pub(crate) fn hotkey_pressed(state: &mut AppState) {
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let mut rotation = std::mem::take(&mut state.match_state.input.primary_rotation);
    let result = state.match_state.sim_runtime.runtime().and_then(|runtime| {
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let owner = sim.interner().get(&owner_name)?;
        let now = sim.session().tick();
        let selected = selected_stable_ids_in_order(
            Some(sim),
            Some(rules),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        let (rotating, turns) = rotation.toggle(sim, rules, owner, &selected, now)?;
        Some((owner, now, rotating, turns))
    });
    let rotating = result.map(|(owner, now, rotating, turns)| {
        send(state, &mut rotation, &owner_name, owner, turns, now);
        rotating
    });
    state.match_state.input.primary_rotation = rotation;
    if let Some(rotating) = rotating {
        crate::app::input::messages::post_primary_rotation(state, rotating);
    }
}

/// Once per logic frame: every [`CHECK_INTERVAL`] frames look at the groups
/// and send their orders.
pub(crate) fn frame_pass(state: &mut AppState) {
    if state.match_state.input.primary_rotation.groups.is_empty() {
        return;
    }
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let mut rotation = std::mem::take(&mut state.match_state.input.primary_rotation);
    let result = state.match_state.sim_runtime.runtime().and_then(|runtime| {
        let sim = &runtime.simulation;
        let now = sim.session().tick();
        if now % CHECK_INTERVAL != 0 {
            return None;
        }
        let owner = sim.interner().get(&owner_name)?;
        let turns = rotation.check(sim, &runtime.resources.rules, owner, now);
        Some((owner, now, turns))
    });
    if let Some((owner, now, turns)) = result {
        send(state, &mut rotation, &owner_name, owner, turns, now);
    }
    state.match_state.input.primary_rotation = rotation;
}

#[cfg(all(test, tests_app_input_d_z))]
mod tests {
    use super::*;
    use crate::app::input::commands::schedule_rules_command_in_sim;
    use crate::sim::command::CommandEnvelope;

    const OWNER: &str = "Americans";
    /// E1's index in the synthetic Infantry type array.
    const E1_INDEX: i32 = 0;

    /// Four barracks of one kind: the House's first is its primary.
    fn factories() -> (Simulation, RuleSet, InternedId, [u64; 4]) {
        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n[Sides]\nGDI=Americans\n\
             [BuildingTypes]\n0=GAPILE\n[InfantryTypes]\n0=E1\n\
             [GAPILE]\nFactory=InfantryType\nStrength=100\nOwner=Americans\n\
             [E1]\nCost=200\nStrength=100\nSpeed=4\nTechLevel=1\nOwner=Americans\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
        ))
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0044_8160);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.intern(OWNER);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let ids = [(4, 4), (8, 4), (12, 4), (16, 4)]
            .map(|(x, y)| sim.spawn_object("GAPILE", OWNER, x, y, 0, &rules).unwrap());
        assert!(sim.production().is_primary_factory(ids[0]));
        assert!(!sim.production().is_primary_factory(ids[1]));
        (sim, rules, owner, ids)
    }

    /// Fill the ordinary local command queue, so the next order is refused.
    fn fill_queue(sim: &mut Simulation, owner: InternedId) {
        let slots = sim.available_local_command_slots();
        let stop = sim.encode_stop_record(owner, 0).unwrap();
        let envelope = CommandEnvelope::synchronized(stop, sim.session().tick());
        assert!(sim.issue_local_commands(std::iter::repeat_n(envelope, slots)));
        assert_eq!(sim.available_local_command_slots(), 0);
    }

    /// The House's finished-object counter: one infantry finished.
    fn finish_infantry(sim: &mut Simulation, owner: InternedId) {
        set_infantry_count(sim, owner, 1);
    }

    /// The House's finished-object counter, at the given count.
    fn set_infantry_count(sim: &mut Simulation, owner: InternedId, count: i32) {
        sim.houses_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .set_produced_count_for_test(ObjectCategory::Infantry, E1_INDEX, count);
    }

    /// Offer the group's turns to the ordinary command queue.
    fn admit(
        rotation: &mut PrimaryRotationState,
        sim: &mut Simulation,
        rules: &RuleSet,
        turns: Vec<Turn>,
    ) {
        rotation.admit_turns(turns, 100, |command| {
            if schedule_rules_command_in_sim(sim, rules, OWNER, command).is_some() {
                Admitted::Yes
            } else if sim.available_local_command_slots() == 0 {
                Admitted::NoRoom
            } else {
                Admitted::Refused
            }
        });
    }

    /// The one command the queue holds, executed.
    fn execute_only_toggle(sim: &mut Simulation, rules: &RuleSet, target: u64) {
        let due = sim.take_frame_commands_for_test();
        assert_eq!(due.len(), 1);
        assert_eq!(
            due[0].payload,
            Command::TogglePrimaryFactory { entity_id: target }
        );
        assert!(sim.apply_command(OWNER, &due[0].payload, Some(rules)));
        assert!(sim.production().is_primary_factory(target));
    }

    /// The primary passes to the next member in the order they were
    /// selected and wraps round; a primary outside the group hands over to
    /// the first member, and a lone member has no one to hand to.
    #[test]
    fn the_primary_passes_round_the_group_in_order() {
        let group = [7, 3, 9];
        assert_eq!(next_primary(&group, Some(7)), Some(3));
        assert_eq!(next_primary(&group, Some(9)), Some(7));
        assert_eq!(next_primary(&group, Some(42)), Some(7));
        assert_eq!(next_primary(&group, None), Some(7));
        assert_eq!(next_primary(&[5], Some(5)), None);
    }

    /// A turn the command queue refused is still due: the group keeps it and
    /// sends it again, without waiting for another finished object.
    #[test]
    fn a_refused_completion_turn_retries_without_another_completion() {
        let (mut sim, rules, owner, [a, b, ..]) = factories();
        let mut rotation = PrimaryRotationState::default();
        // A member of the group is primary: the group waits for a finish.
        let (_, turns) = rotation.toggle(&sim, &rules, owner, &[a, b], 0).unwrap();
        assert!(turns.is_empty());
        finish_infantry(&mut sim, owner);

        fill_queue(&mut sim, owner);
        let turns = rotation.check(&sim, &rules, owner, 8);
        assert_eq!(turns.len(), 1);
        admit(&mut rotation, &mut sim, &rules, turns);
        assert!(
            sim.take_frame_commands_for_test()
                .iter()
                .all(|envelope| !matches!(envelope.payload, Command::TogglePrimaryFactory { .. })),
            "the full queue refused the turn"
        );

        // The same turn is still due, and the freed queue admits it.
        let turns = rotation.check(&sim, &rules, owner, 16);
        assert_eq!(turns.len(), 1);
        admit(&mut rotation, &mut sim, &rules, turns);
        execute_only_toggle(&mut sim, &rules, b);
    }

    #[test]
    fn a_completion_during_an_input_lock_rotates_after_unlock() {
        let (mut sim, rules, owner, [a, b, ..]) = factories();
        let mut rotation = PrimaryRotationState::default();
        rotation.toggle(&sim, &rules, owner, &[a, b], 0).unwrap();
        sim.session_mut_for_test().set_input_locked_for_test(true);
        finish_infantry(&mut sim, owner);
        let turns = rotation.check(&sim, &rules, owner, 8);
        admit(&mut rotation, &mut sim, &rules, turns);
        assert_eq!(sim.pending_local_command_count(), 0);

        sim.session_mut_for_test().set_input_locked_for_test(false);
        let turns = rotation.check(&sim, &rules, owner, 16);
        admit(&mut rotation, &mut sim, &rules, turns);
        execute_only_toggle(&mut sim, &rules, b);
    }

    /// The first order of a group makes one of its members primary. A
    /// refused one is sent again, without waiting for a finished object.
    #[test]
    fn a_refused_initial_turn_retries_without_a_completion() {
        let (mut sim, rules, owner, [_, a, b, _]) = factories();
        let mut rotation = PrimaryRotationState::default();
        // The House's primary is outside the selection: the group needs one.
        fill_queue(&mut sim, owner);
        let (_, turns) = rotation.toggle(&sim, &rules, owner, &[a, b], 0).unwrap();
        assert_eq!(turns.len(), 1);
        admit(&mut rotation, &mut sim, &rules, turns);
        assert!(
            sim.take_frame_commands_for_test()
                .iter()
                .all(|envelope| !matches!(envelope.payload, Command::TogglePrimaryFactory { .. })),
            "the full queue refused the first order"
        );

        let turns = rotation.check(&sim, &rules, owner, 8);
        assert_eq!(turns.len(), 1);
        admit(&mut rotation, &mut sim, &rules, turns);
        execute_only_toggle(&mut sim, &rules, a);
    }

    /// One admitted order at a time: a new selection of the kind waits for
    /// the order on its way, and stopping the group does not forget it.
    #[test]
    fn replacing_or_disabling_a_group_cannot_repeat_an_admitted_toggle() {
        let (mut sim, rules, owner, [primary, a, b, c]) = factories();
        let mut rotation = PrimaryRotationState::default();
        // The selection holds no primary: its first member needs the order.
        let (_, turns) = rotation.toggle(&sim, &rules, owner, &[a, b], 0).unwrap();
        assert_eq!(turns.len(), 1);
        admit(&mut rotation, &mut sim, &rules, turns);
        assert_eq!(sim.pending_local_command_count(), 1);

        // A new selection of the kind, one factory alone, then a new pair.
        for selection in [&[a, c][..], &[c][..], &[a, c][..]] {
            let (_, turns) = rotation.toggle(&sim, &rules, owner, selection, 0).unwrap();
            assert!(
                turns.is_empty(),
                "no second order while the first is on its way"
            );
            admit(&mut rotation, &mut sim, &rules, turns);
            assert_eq!(sim.pending_local_command_count(), 1);
        }
        execute_only_toggle(&mut sim, &rules, a);
        assert!(!sim.production().is_primary_factory(primary));
        assert!(rotation.check(&sim, &rules, owner, 8).is_empty());
    }

    /// A group whose order never landed carries on once that order's time
    /// is up: the in-flight guard holds the group until then, and not
    /// after.
    #[test]
    fn a_group_carries_on_when_its_order_never_landed() {
        let (mut sim, rules, owner, [a, b, ..]) = factories();
        let mut rotation = PrimaryRotationState::default();
        rotation.toggle(&sim, &rules, owner, &[a, b], 0).unwrap();
        finish_infantry(&mut sim, owner);

        let turns = rotation.check(&sim, &rules, owner, 8);
        assert_eq!(turns.len(), 1);
        admit(&mut rotation, &mut sim, &rules, turns);

        // The order is on its way: the group waits for it, even for a new
        // finished object.
        set_infantry_count(&mut sim, owner, 2);
        assert!(
            rotation.check(&sim, &rules, owner, 16).is_empty(),
            "the order on its way holds the group"
        );

        // Its time is up at frame 100; the group carries on.
        let turns = rotation.check(&sim, &rules, owner, 120);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].target, b);
    }

    /// An order the queue refuses for good is spent: the group does not
    /// offer it again, and waits for the next finished object.
    #[test]
    fn a_permanently_refused_turn_is_not_offered_again() {
        let (mut sim, rules, owner, [_a, b, ..]) = factories();
        let mut rotation = PrimaryRotationState::default();
        rotation.toggle(&sim, &rules, owner, &[_a, b], 0).unwrap();
        finish_infantry(&mut sim, owner);

        // A queue with room, and an order it will not take.
        let turns = rotation.check(&sim, &rules, owner, 8);
        assert_eq!(turns.len(), 1);
        rotation.admit_turns(turns, 100, |_command| Admitted::Refused);

        assert!(
            rotation.check(&sim, &rules, owner, 16).is_empty(),
            "a refused order is not offered again"
        );
        set_infantry_count(&mut sim, owner, 2);
        let turns = rotation.check(&sim, &rules, owner, 24);
        assert_eq!(turns.len(), 1, "the next finished object turns again");
        assert_eq!(turns[0].target, b);
    }
}
