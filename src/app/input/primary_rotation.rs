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
//! kind without one. A group therefore sends nothing while an order is on its
//! way: until the factory it names is primary, or the order has surely
//! executed ([`command_executed_by`]). The turns are quiet on this game: the
//! "Primary building selected" line each order makes is skipped
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
use crate::sim::world::Simulation;

/// Logic frames between two looks at the groups.
const CHECK_INTERVAL: u64 = 8;
/// The line the PRIMARY event announces for a human owner's factory
/// (`BuildingClass` `0x00448160`, `toggle_factory_primary`).
const PRIMARY_SELECTED_EVA: &str = "EVA_PrimaryBuildingSelected";

/// What a group's factories make, and whether they are naval: the same
/// grouping gamemd keeps one primary for.
type Kind = (FactoryType, bool);

/// The factories of one kind that take turns, in the order they were
/// selected.
#[derive(Debug, Clone)]
struct RotationGroup {
    kind: Kind,
    members: Vec<u64>,
    /// The House's count of finished objects of this kind at the last turn.
    built: i64,
    /// The member an order was sent to make primary, and the frame that
    /// order has surely executed by.
    pending: Option<(u64, u64)>,
}

impl RotationGroup {
    /// One look, with `built` the House's finished objects of this kind and
    /// `primary` the member that is primary now: the member to order
    /// primary, if this kind finished an object since the last turn. Nothing
    /// while an order is on its way; a finish meanwhile turns the group once
    /// it has landed. An order goes out until frame `until`.
    fn turn(
        &mut self,
        built: i64,
        primary: Option<u64>,
        now: u64,
        until: u64,
        can_select: impl Fn(u64) -> bool,
    ) -> Option<u64> {
        if let Some((target, by)) = self.pending {
            if primary != Some(target) && now < by {
                return None;
            }
            self.pending = None;
        }
        if built == self.built {
            return None;
        }
        self.built = built;
        let next = next_primary(&self.members, primary).filter(|&next| can_select(next))?;
        self.pending = Some((next, until));
        Some(next)
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
fn factory_kind(sim: &Simulation, rules: &RuleSet, id: u64, owner: InternedId) -> Option<Kind> {
    let entity = sim.entities().get(id)?;
    if !entity.is_alive()
        || entity.owner() != owner
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
    /// now rotating among those kinds, and the order that makes a group's
    /// first member primary where none of its members is; that order goes
    /// out until frame `until`.
    fn toggle(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        selected: &[u64],
        until: u64,
    ) -> Option<(usize, Vec<Command>)> {
        let mut kinds: Vec<(Kind, Vec<u64>)> = Vec::new();
        for &id in selected {
            let Some(kind) = factory_kind(sim, rules, id, owner) else {
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
        let mut orders = Vec::new();
        for (kind, members) in kinds {
            // The same factories in any order: a band box picks them in its own.
            let same = self.groups.iter().any(|group| {
                group.kind == kind
                    && group.members.len() == members.len()
                    && members.iter().all(|id| group.members.contains(id))
            });
            self.groups.retain(|group| group.kind != kind);
            if same || members.len() < 2 {
                continue;
            }
            let mut pending = None;
            if !members
                .iter()
                .any(|&id| sim.production().is_primary_factory(id))
            {
                orders.push(Command::TogglePrimaryFactory {
                    entity_id: members[0],
                });
                pending = Some((members[0], until));
            }
            rotating += members.len();
            self.groups.push(RotationGroup {
                kind,
                built: built(sim, rules, owner, kind),
                members,
                pending,
            });
        }
        Some((rotating, orders))
    }

    /// One look: for each group whose kind finished an object since its last
    /// turn, the order that makes its next factory primary; each goes out
    /// until frame `until`. Groups with fewer than two live members are
    /// dropped.
    fn check(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        now: u64,
        until: u64,
    ) -> Vec<Command> {
        let mut orders = Vec::new();
        self.groups.retain_mut(|group| {
            group
                .members
                .retain(|&id| factory_kind(sim, rules, id, owner) == Some(group.kind));
            if group.members.len() < 2 {
                return false;
            }
            let primary = group
                .members
                .iter()
                .copied()
                .find(|&id| sim.production().is_primary_factory(id));
            if let Some(next) = group.turn(
                built(sim, rules, owner, group.kind),
                primary,
                now,
                until,
                |id| crate::sim::production::can_select_factory_primary(sim, rules, id),
            ) {
                orders.push(Command::TogglePrimaryFactory { entity_id: next });
            }
            true
        });
        orders
    }
}

/// Send the groups' orders; each one sent leaves its "Primary building
/// selected" line unheard on this game until frame `until`.
fn send(
    state: &mut AppState,
    owner_name: &str,
    owner: InternedId,
    orders: Vec<Command>,
    now: u64,
    until: u64,
) {
    for order in orders {
        // An order the queue cannot take now is not repeated: the next
        // finished object turns the group again.
        if try_schedule_command(state, owner_name, order).is_some() {
            state.match_state.match_audio.quiet.expect(
                QuietSound::HouseEva {
                    owner,
                    event: PRIMARY_SELECTED_EVA,
                },
                now,
                until,
            );
        }
    }
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
        let until = command_executed_by(state, now);
        let selected = selected_stable_ids_in_order(
            Some(sim),
            Some(rules),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        let (rotating, orders) = rotation.toggle(sim, rules, owner, &selected, until)?;
        Some((owner, now, until, rotating, orders))
    });
    state.match_state.input.primary_rotation = rotation;
    let Some((owner, now, until, rotating, orders)) = result else {
        return;
    };
    send(state, &owner_name, owner, orders, now, until);
    crate::app::input::messages::post_primary_rotation(state, rotating);
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
        let until = command_executed_by(state, now);
        let orders = rotation.check(sim, &runtime.resources.rules, owner, now, until);
        Some((owner, now, until, orders))
    });
    state.match_state.input.primary_rotation = rotation;
    if let Some((owner, now, until, orders)) = result {
        send(state, &owner_name, owner, orders, now, until);
    }
}

#[cfg(test)]
mod tests {
    use super::{RotationGroup, next_primary};
    use crate::rules::object_type::FactoryType;

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

    /// A second finish while the first turn's order is on its way sends
    /// nothing: a second order to the same factory would clear it again.
    /// The group turns once the order has landed, or its time is up.
    #[test]
    fn no_turn_while_the_last_order_is_on_its_way() {
        let mut group = RotationGroup {
            kind: (FactoryType::UnitType, false),
            members: vec![7, 3, 9],
            built: 0,
            pending: None,
        };
        let any = |_| true;
        assert_eq!(group.turn(1, Some(7), 8, 100, any), Some(3));
        // Not landed yet: 7 is still primary.
        assert_eq!(group.turn(2, Some(7), 16, 108, any), None);
        // Landed: the finish meanwhile turns the group on.
        assert_eq!(group.turn(2, Some(3), 24, 116, any), Some(9));
        // Never seen landing: the group carries on once its time is up.
        assert_eq!(group.turn(3, Some(3), 115, 207, any), None);
        assert_eq!(group.turn(3, Some(3), 116, 208, any), Some(9));
    }
}
