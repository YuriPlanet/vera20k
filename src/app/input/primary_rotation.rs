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
//! A group is local input state: it is not saved, the computer does not use
//! it, and it ends when the key is pressed on the same factories again or
//! fewer than two of them are left.

use crate::app::AppState;
use crate::app::input::commands::{preferred_local_owner_name, try_schedule_command};
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectCategory};
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// Logic frames between two looks at the groups.
const CHECK_INTERVAL: u64 = 8;

/// What a group's factories make, and whether they are naval: the same
/// grouping gamemd keeps one primary for.
type Kind = (FactoryType, bool);

/// The factories of one kind that take turns, in the order they were
/// selected.
#[derive(Debug, Clone)]
struct RotationGroup {
    kind: Kind,
    members: Vec<u64>,
    /// The House's count of finished objects of this kind at the last look.
    built: i64,
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

fn built(sim: &Simulation, owner: InternedId, kind: Kind) -> i64 {
    counted_as(kind)
        .and_then(|category| {
            sim.houses()
                .get(&owner)
                .map(|house| house.produced_total(category))
        })
        .unwrap_or(0)
}

impl PrimaryRotationState {
    /// The key on a selection: each kind's selected factories become its
    /// group, or stop being it when they already are. Returns the factories
    /// now rotating among those kinds, and the order that makes a group's
    /// first member primary where none of its members is.
    fn toggle(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        selected: &[u64],
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
            let same = self
                .groups
                .iter()
                .any(|group| group.kind == kind && group.members == members);
            self.groups.retain(|group| group.kind != kind);
            if same || members.len() < 2 {
                continue;
            }
            if !members
                .iter()
                .any(|&id| sim.production().is_primary_factory(id))
            {
                orders.push(Command::TogglePrimaryFactory {
                    entity_id: members[0],
                });
            }
            rotating += members.len();
            self.groups.push(RotationGroup {
                kind,
                members,
                built: built(sim, owner, kind),
            });
        }
        Some((rotating, orders))
    }

    /// One look: for each group whose kind finished an object since the last
    /// look, the order that makes its next factory primary. Groups with
    /// fewer than two live members are dropped.
    fn check(&mut self, sim: &Simulation, rules: &RuleSet, owner: InternedId) -> Vec<Command> {
        let mut orders = Vec::new();
        self.groups.retain_mut(|group| {
            group
                .members
                .retain(|&id| factory_kind(sim, rules, id, owner) == Some(group.kind));
            if group.members.len() < 2 {
                return false;
            }
            let now = built(sim, owner, group.kind);
            if now == group.built {
                return true;
            }
            group.built = now;
            let primary = group
                .members
                .iter()
                .copied()
                .find(|&id| sim.production().is_primary_factory(id));
            if let Some(next) = next_primary(&group.members, primary)
                && crate::sim::production::can_select_factory_primary(sim, rules, next)
            {
                orders.push(Command::TogglePrimaryFactory { entity_id: next });
            }
            true
        });
        orders
    }
}

/// The Rotate Primary key.
pub(crate) fn hotkey_pressed(state: &mut AppState) {
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let mut rotation = std::mem::take(&mut state.match_state.input.primary_rotation);
    let result = state.match_state.sim_runtime.as_ref().and_then(|runtime| {
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let owner = sim.interner().get(&owner_name)?;
        let selected = selected_stable_ids_in_order(
            Some(sim),
            Some(rules),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        rotation.toggle(sim, rules, owner, &selected)
    });
    state.match_state.input.primary_rotation = rotation;
    let Some((rotating, orders)) = result else {
        return;
    };
    for order in orders {
        let _ = try_schedule_command(state, &owner_name, order);
    }
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
    let orders = match state.match_state.sim_runtime.as_ref() {
        Some(runtime) => {
            let sim = &runtime.simulation;
            match sim.interner().get(&owner_name) {
                Some(owner) if sim.session().tick() % CHECK_INTERVAL == 0 => {
                    rotation.check(sim, &runtime.resources.rules, owner)
                }
                _ => Vec::new(),
            }
        }
        None => Vec::new(),
    };
    state.match_state.input.primary_rotation = rotation;
    for order in orders {
        // An order the queue cannot take now is not repeated: the next
        // finished object turns the group again.
        let _ = try_schedule_command(state, &owner_name, order);
    }
}

#[cfg(test)]
mod tests {
    use super::next_primary;

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
}
