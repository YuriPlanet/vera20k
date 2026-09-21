//! Owner block sets kept current across object turns.
//!
//! A moving object's turn needs, for its owner, the cells buildings block and
//! the soft-block entry of every occupied cell: the inputs of A*'s code-2/5/6
//! edge costs (`AStar_compute_edge_cost 0x00429830`) and of the Drive selection
//! gate. Building them walks every entity. Production runs one object per
//! movement pass, so that walk ran once per moving object per frame.
//!
//! The sets are a pure function of the entities (plus rules, which a match does
//! not change, and the alliance graph). This index keeps each owner's sets
//! between passes and re-derives only the entities the store handed out
//! mutably since (`EntityStore::take_touched`), at exactly the points where the
//! sets used to be rebuilt, so a pass sees what a fresh build would give it.
//! Debug builds make that fresh build and compare.
//!
//! One rule says what an entity contributes ([`contribution`]) and one insert
//! says what an occupant does to its cell's entry ([`insert_unit`]); the
//! whole-world build (`bump_crush::build_entity_block_sets`) applies the same
//! two to everyone in id order.
//!
//! ## Dependency rules
//! - Part of sim/movement: depends on sim/entity_store, sim/game_entity,
//!   sim/pathfinding, sim/production (foundation cells), map/houses, rules.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::{EntityStore, TouchedEntities};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::{EntityBlockEntry, LayeredEntityBlockMap, MovingAllyOccupant};

/// One owner's product: the cells buildings block, and the per-cell soft
/// blockers as that owner's movers see them.
pub(crate) type OwnerBlockSet = (BTreeSet<(u16, u16)>, LayeredEntityBlockMap);

type LayerCell = (MovementLayer, (u16, u16));

/// What one entity adds to the block sets, before the viewing owner's
/// friendliness turns it into a code.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Contribution {
    /// A building: the cells it blocks for movement.
    Structure(Vec<(u16, u16)>),
    Unit(UnitContribution),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnitContribution {
    layer: MovementLayer,
    cell: (u16, u16),
    owner: InternedId,
    infantry: bool,
    moving: Option<MovingContribution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MovingContribution {
    next_cell: (u16, u16),
    ally: MovingAllyOccupant,
    /// False when the occupant is skipped by the locomotor answer, so an ally
    /// raises no code here and the occupation mask arm decides.
    raises_code_2: bool,
}

/// The one rule for what an entity contributes.
fn contribution(
    entity: &GameEntity,
    interner: &StringInterner,
    rules: Option<&RuleSet>,
) -> Option<Contribution> {
    // A Dying corpse is off the occupancy grid (uninit unmarked it); exclude
    // it here too so movers don't path around a building that no longer
    // exists.
    if entity.dying || !entity.lifecycle.cell_marked {
        return None;
    }
    // Entities inside transports don't occupy cells.
    if entity.passenger_role.is_inside_transport() {
        return None;
    }
    let layer = entity.occupancy_list_layer()?;
    let cell = (entity.position.rx, entity.position.ry);
    // Buildings always block (they never move). Always ground layer.
    // With rules, expand to the full foundation so A* sees every occupied
    // cell; without it, only the anchor blocks (legacy behavior).
    if entity.category == EntityCategory::Structure {
        let Some(obj) = rules.and_then(|r| r.object(interner.resolve(entity.type_ref()))) else {
            return Some(Contribution::Structure(vec![cell]));
        };
        let foundation_cells =
            crate::sim::production::building_base_foundation_cells(cell.0, cell.1, &obj.foundation);
        let is_bunker_occupied = obj.bunker
            && (entity.bunker_occupant.is_some()
                || entity
                    .passenger_role
                    .cargo()
                    .is_some_and(|cargo| cargo.count() > 0));
        return Some(Contribution::Structure(
            crate::sim::production::building_movement_blocking_cells_for_state(
                &foundation_cells,
                cell.0,
                obj.bib,
                obj.number_impassable_rows,
                obj.bunker,
                is_bunker_occupied,
                false,
            ),
        ));
    }
    let infantry = entity.category == EntityCategory::Infantry;
    // Friendly moving units: code-2 chain walk entry.
    //
    // `UnitClass::Can_Enter_Cell 0x0073FA2C..FA7C`: an occupant in transit
    // (`Foot+0x6B6 == 0`) or an infantryman is first asked on its locomotor
    // slot `+0xA4` (Drive/Ship `Can_Use_Track`, every other class false);
    // a false answer skips it (no entry, the occupation mask arm decides)
    // and only a true one raises the running code to 2. See
    // `cell_entry::classify_blocker` for the same arm on the live walk.
    let moving = entity
        .movement_target
        .as_ref()
        .and_then(|target| target.path.get(target.next_index).copied())
        .filter(|&next_cell| next_cell != cell)
        .map(|next_cell| {
            let in_transit = !entity.foot_occupation_enabled;
            MovingContribution {
                next_cell,
                // Recorded for every moving ally, code or not: the head-on
                // exit runs before the locomotor question and is answered
                // per mover by the Drive selection lane.
                ally: MovingAllyOccupant {
                    facing: entity.facing,
                    world: crate::sim::pathfinding::cell_entry::entity_world_leptons(entity),
                },
                raises_code_2: !((in_transit || infantry)
                    && !super::drive_track::occupant_slot_a4_answers_true(entity)),
            }
        });
    Some(Contribution::Unit(UnitContribution {
        layer,
        cell,
        owner: entity.owner(),
        infantry,
        moving,
    }))
}

/// Insert one occupant into the owner's map. Applied in ascending id order,
/// a later occupant of the same cell overwrites the entry of an earlier one,
/// except a moving ally the locomotor answer skips, which leaves it alone.
fn insert_unit(map: &mut LayeredEntityBlockMap, unit: &UnitContribution, friendly: bool) {
    let entry = |next_cell, cost_code| EntityBlockEntry {
        next_cell,
        cost_code,
        blocker_is_infantry: unit.infantry,
    };
    if !friendly {
        // Enemy units: soft-block with code 5 (cost 20x).
        map.insert(unit.layer, unit.cell, entry(None, 5));
    } else if let Some(moving) = unit.moving {
        map.insert_moving_ally(unit.layer, unit.cell, moving.ally);
        if moving.raises_code_2 {
            map.insert(unit.layer, unit.cell, entry(Some(moving.next_cell), 2));
        }
    } else {
        // Stationary friendly: soft-block with code 6 (cost 8x).
        map.insert(unit.layer, unit.cell, entry(None, 6));
    }
}

/// Everything behind one owner's product.
#[derive(Debug, Default)]
struct OwnerBlockState {
    /// Each entity's contribution as the product last saw it.
    placed: BTreeMap<u64, Contribution>,
    units_at: BTreeMap<LayerCell, BTreeSet<u64>>,
    /// How many placed buildings block each cell.
    structure_refs: BTreeMap<(u16, u16), u32>,
    /// Touched since this owner's product was last brought current.
    pending: BTreeSet<u64>,
    /// `are_houses_friendly(owner, other)` under the alliance graph the index
    /// last saw; the index drops every state when that graph changes.
    friendly: BTreeMap<InternedId, bool>,
    /// `None` while a movement pass holds it.
    product: Option<OwnerBlockSet>,
    /// Which loan holds the product. A pass that comes back with another
    /// number held sets this state no longer describes.
    loan: u64,
}

impl OwnerBlockState {
    fn unplace(
        &mut self,
        id: u64,
        cells: &mut BTreeSet<(u16, u16)>,
        keys: &mut BTreeSet<LayerCell>,
    ) {
        match self.placed.remove(&id) {
            Some(Contribution::Structure(blocked)) => {
                for cell in blocked {
                    if let Some(count) = self.structure_refs.get_mut(&cell) {
                        *count -= 1;
                        if *count == 0 {
                            self.structure_refs.remove(&cell);
                        }
                    }
                    cells.insert(cell);
                }
            }
            Some(Contribution::Unit(unit)) => {
                let key = (unit.layer, unit.cell);
                if let Some(ids) = self.units_at.get_mut(&key) {
                    ids.remove(&id);
                    if ids.is_empty() {
                        self.units_at.remove(&key);
                    }
                }
                keys.insert(key);
            }
            None => {}
        }
    }

    fn place(
        &mut self,
        id: u64,
        contribution: Contribution,
        cells: &mut BTreeSet<(u16, u16)>,
        keys: &mut BTreeSet<LayerCell>,
    ) {
        match &contribution {
            Contribution::Structure(blocked) => {
                for &cell in blocked {
                    *self.structure_refs.entry(cell).or_insert(0) += 1;
                    cells.insert(cell);
                }
            }
            Contribution::Unit(unit) => {
                let key = (unit.layer, unit.cell);
                self.units_at.entry(key).or_default().insert(id);
                keys.insert(key);
            }
        }
        self.placed.insert(id, contribution);
    }

    /// Rewrite the product at the cells whose contributors changed.
    fn rewrite(
        &mut self,
        product: &mut OwnerBlockSet,
        cells: &BTreeSet<(u16, u16)>,
        keys: &BTreeSet<LayerCell>,
        owner: &str,
        alliances: &HouseAllianceMap,
        interner: &StringInterner,
    ) {
        for &cell in cells {
            if self.structure_refs.contains_key(&cell) {
                product.0.insert(cell);
            } else {
                product.0.remove(&cell);
            }
        }
        for &(layer, cell) in keys {
            let placed = &self.placed;
            let friendly = &mut self.friendly;
            let units = self
                .units_at
                .get(&(layer, cell))
                .into_iter()
                .flatten()
                .filter_map(|id| match placed.get(id) {
                    Some(Contribution::Unit(unit)) => Some(unit),
                    _ => None,
                });
            product.1.remove(layer, &cell);
            product.1.remove_moving_ally(layer, &cell);
            for unit in units {
                let friendly = *friendly.entry(unit.owner).or_insert_with(|| {
                    crate::map::houses::are_houses_friendly(
                        alliances,
                        owner,
                        interner.resolve(unit.owner),
                    )
                });
                insert_unit(&mut product.1, unit, friendly);
            }
        }
    }

    /// Re-derive the pending entities and rewrite the product where they sit
    /// or sat.
    fn bring_current(
        &mut self,
        product: &mut OwnerBlockSet,
        entities: &EntityStore,
        owner: &str,
        alliances: &HouseAllianceMap,
        interner: &StringInterner,
        rules: Option<&RuleSet>,
    ) {
        let mut cells = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for id in std::mem::take(&mut self.pending) {
            let now = entities
                .get(id)
                .and_then(|entity| contribution(entity, interner, rules));
            if self.placed.get(&id) == now.as_ref() {
                continue;
            }
            self.unplace(id, &mut cells, &mut keys);
            if let Some(now) = now {
                self.place(id, now, &mut cells, &mut keys);
            }
        }
        self.rewrite(product, &cells, &keys, owner, alliances, interner);
    }
}

/// The whole-world build: every entity placed, every cell folded.
fn build_state(
    entities: &EntityStore,
    owner: &str,
    alliances: &HouseAllianceMap,
    interner: &StringInterner,
    rules: Option<&RuleSet>,
) -> (OwnerBlockState, OwnerBlockSet) {
    let mut state = OwnerBlockState::default();
    let mut cells = BTreeSet::new();
    let mut keys = BTreeSet::new();
    for entity in entities.values() {
        if let Some(contribution) = contribution(entity, interner, rules) {
            state.place(entity.stable_id(), contribution, &mut cells, &mut keys);
        }
    }
    let mut product = OwnerBlockSet::default();
    state.rewrite(&mut product, &cells, &keys, owner, alliances, interner);
    (state, product)
}

/// The owner block sets built from every entity, in id order, with none of
/// the index's bookkeeping. O(entities): path searches outside a movement pass,
/// tests, and the debug-build check of the index.
pub(crate) fn build_owner_block_set(
    entities: &EntityStore,
    owner: &str,
    alliances: &HouseAllianceMap,
    interner: &StringInterner,
    rules: Option<&RuleSet>,
) -> OwnerBlockSet {
    let mut product = OwnerBlockSet::default();
    let mut friendly_to: BTreeMap<InternedId, bool> = BTreeMap::new();
    for entity in entities.values() {
        match contribution(entity, interner, rules) {
            Some(Contribution::Structure(blocked)) => product.0.extend(blocked),
            Some(Contribution::Unit(unit)) => {
                let friendly = *friendly_to.entry(unit.owner).or_insert_with(|| {
                    crate::map::houses::are_houses_friendly(
                        alliances,
                        owner,
                        interner.resolve(unit.owner),
                    )
                });
                insert_unit(&mut product.1, &unit, friendly);
            }
            None => {}
        }
    }
    product
}

/// Each owner's block sets, kept between movement passes.
#[derive(Debug, Default)]
pub(crate) struct OwnerBlockIndex {
    owners: BTreeMap<InternedId, OwnerBlockState>,
    /// The alliance graph the owners' friendliness answers were taken under.
    alliances_seen: Option<HouseAllianceMap>,
    last_loan: u64,
    /// How many owner states were built from every entity.
    #[cfg(test)]
    rebuilds: usize,
}

/// Sets on loan to a movement pass, with the number that returns them.
#[derive(Debug, Default)]
pub(crate) struct LentOwnerBlockSet {
    loan: u64,
    pub(crate) sets: OwnerBlockSet,
}

impl OwnerBlockIndex {
    /// Move the store's touch log onto every owner's pending list.
    fn take_touched(&mut self, entities: &mut EntityStore, alliances: &HouseAllianceMap) {
        if self.alliances_seen.as_ref() != Some(alliances) {
            self.owners.clear();
            self.alliances_seen = Some(alliances.clone());
        }
        match entities.take_touched() {
            TouchedEntities::All => self.owners.clear(),
            TouchedEntities::Ids(ids) => {
                for state in self.owners.values_mut() {
                    state.pending.extend(ids.iter().copied());
                }
                // An owner that stopped moving would collect ids for ever;
                // past this point a rebuild is cheaper than the backlog.
                self.owners
                    .retain(|_, state| state.pending.len() <= state.placed.len() / 2 + 256);
            }
        }
    }

    /// The owner's sets as a build from the entities would give them now,
    /// handed to the movement pass that asked. [`Self::give_back`] returns them.
    pub(crate) fn lend_current(
        &mut self,
        owner: InternedId,
        entities: &mut EntityStore,
        alliances: &HouseAllianceMap,
        interner: &StringInterner,
        rules: Option<&RuleSet>,
    ) -> LentOwnerBlockSet {
        self.take_touched(entities, alliances);
        let owner_name = interner.resolve(owner);
        self.last_loan += 1;
        let loan = self.last_loan;
        let kept = self
            .owners
            .get_mut(&owner)
            .and_then(|state| state.product.take().map(|product| (state, product)));
        let sets = match kept {
            Some((state, mut product)) => {
                state.bring_current(
                    &mut product,
                    entities,
                    owner_name,
                    alliances,
                    interner,
                    rules,
                );
                state.loan = loan;
                product
            }
            None => {
                let (mut state, product) =
                    build_state(entities, owner_name, alliances, interner, rules);
                #[cfg(test)]
                {
                    self.rebuilds += 1;
                }
                state.loan = loan;
                self.owners.insert(owner, state);
                product
            }
        };
        debug_assert_current(&sets, entities, owner_name, alliances, interner, rules);
        LentOwnerBlockSet { loan, sets }
    }

    /// Bring sets a pass already holds to the entities' current state.
    pub(crate) fn refresh_lent(
        &mut self,
        owner: InternedId,
        lent: &mut LentOwnerBlockSet,
        entities: &mut EntityStore,
        alliances: &HouseAllianceMap,
        interner: &StringInterner,
        rules: Option<&RuleSet>,
    ) {
        self.take_touched(entities, alliances);
        let owner_name = interner.resolve(owner);
        match self.owners.get_mut(&owner) {
            Some(state) if state.product.is_none() && state.loan == lent.loan => {
                state.bring_current(
                    &mut lent.sets,
                    entities,
                    owner_name,
                    alliances,
                    interner,
                    rules,
                );
            }
            _ => {
                let (mut state, built) =
                    build_state(entities, owner_name, alliances, interner, rules);
                #[cfg(test)]
                {
                    self.rebuilds += 1;
                }
                self.last_loan += 1;
                state.loan = self.last_loan;
                lent.loan = state.loan;
                lent.sets = built;
                self.owners.insert(owner, state);
            }
        }
        debug_assert_current(&lent.sets, entities, owner_name, alliances, interner, rules);
    }

    /// Take back what [`Self::lend_current`] handed out. Sets whose state was
    /// dropped in the meantime are discarded; the next loan rebuilds.
    pub(crate) fn give_back(&mut self, owner: InternedId, lent: LentOwnerBlockSet) {
        if let Some(state) = self.owners.get_mut(&owner)
            && state.product.is_none()
            && state.loan == lent.loan
        {
            state.product = Some(lent.sets);
        }
    }
}

fn debug_assert_current(
    product: &OwnerBlockSet,
    entities: &EntityStore,
    owner: &str,
    alliances: &HouseAllianceMap,
    interner: &StringInterner,
    rules: Option<&RuleSet>,
) {
    debug_assert!(
        !super::movement_occupancy::live_read_check_enabled()
            || *product == build_owner_block_set(entities, owner, alliances, interner, rules),
        "owner block sets for {owner} diverged from a build of the whole world"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::MovementTarget;
    use crate::sim::intern::{test_intern, test_interner};

    fn unit(id: u64, owner: &str, cell: (u16, u16)) -> GameEntity {
        let mut entity = GameEntity::test_default(id, "MTNK", owner, cell.0, cell.1);
        entity.category = EntityCategory::Unit;
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity
    }

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAPOWR\n\
             [MTNK]\nSpeed=4\n[GAPOWR]\nFoundation=2x2\n",
        ))
        .expect("rules")
    }

    /// Lend, compare with the whole-world build for both viewing owners, give
    /// back. Returns how many states were rebuilt from scratch to do it.
    fn lend_and_check(
        index: &mut OwnerBlockIndex,
        entities: &mut EntityStore,
        alliances: &HouseAllianceMap,
        rules: &RuleSet,
    ) -> usize {
        let interner = test_interner();
        let before = index.rebuilds;
        for owner in ["Americans", "Russians"] {
            let id = test_intern(owner);
            let lent = index.lend_current(id, entities, alliances, &interner, Some(rules));
            assert_eq!(
                lent.sets,
                build_owner_block_set(entities, owner, alliances, &interner, Some(rules)),
                "{owner}'s sets differ from a build of the whole world"
            );
            index.give_back(id, lent);
        }
        index.rebuilds - before
    }

    #[test]
    fn touched_entities_are_rederived_and_the_rest_is_kept() {
        let rules = rules();
        let alliances = HouseAllianceMap::new();
        let mut entities = EntityStore::new();
        entities.insert(unit(1, "Americans", (5, 5)));
        entities.insert(unit(2, "Americans", (6, 5)));
        entities.insert(unit(3, "Russians", (7, 5)));
        let mut power = unit(4, "Russians", (20, 20));
        power.category = EntityCategory::Structure;
        power.type_ref = test_intern("GAPOWR");
        entities.insert(power);
        let mut index = OwnerBlockIndex::default();
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            2
        );

        // Two occupants of one cell: the later id's entry wins, and when it
        // leaves the earlier one's returns.
        entities.get_mut(2).unwrap().position.rx = 5;
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            0
        );
        entities.get_mut(2).unwrap().position.rx = 9;
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            0
        );

        // A unit starts moving (code 2 with a next cell, and a moving-ally
        // record), an enemy dies in place, a building goes, a unit arrives.
        entities.get_mut(1).unwrap().movement_target = Some(MovementTarget {
            path: vec![(5, 5), (5, 6)],
            path_layers: vec![MovementLayer::Ground; 2],
            next_index: 1,
            ..MovementTarget::default()
        });
        entities.get_mut(3).unwrap().dying = true;
        entities.remove(4);
        entities.insert(unit(5, "Russians", (5, 6)));
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            0
        );
        let americans = &index.owners[&test_intern("Americans")];
        let product = americans.product.as_ref().unwrap();
        assert!(
            product.0.is_empty(),
            "the power plant's cells were released"
        );
        assert_eq!(
            product
                .1
                .get(MovementLayer::Ground, &(5, 5))
                .unwrap()
                .cost_code,
            2
        );
        assert!(
            product
                .1
                .moving_ally(MovementLayer::Ground, &(5, 5))
                .is_some()
        );
        assert_eq!(
            product
                .1
                .get(MovementLayer::Ground, &(5, 6))
                .unwrap()
                .cost_code,
            5
        );
        assert!(product.1.get(MovementLayer::Ground, &(7, 5)).is_none());

        // A changed alliance graph and an all-entity walk both force a rebuild.
        let mut allied = HouseAllianceMap::new();
        allied.insert("Americans".into(), ["Russians".to_string()].into());
        allied.insert("Russians".into(), ["Americans".to_string()].into());
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &allied, &rules),
            2
        );
        for entity in entities.values_mut() {
            entity.position.ry += 1;
        }
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &allied, &rules),
            2
        );
    }

    #[test]
    fn a_stale_loan_is_not_taken_back() {
        let rules = rules();
        let alliances = HouseAllianceMap::new();
        let owner = test_intern("Americans");
        let mut entities = EntityStore::new();
        entities.insert(unit(1, "Americans", (5, 5)));
        // After the fixture, so it can resolve what the fixture interned.
        let interner = test_interner();
        let mut index = OwnerBlockIndex::default();
        let first = index.lend_current(owner, &mut entities, &alliances, &interner, Some(&rules));
        // A second pass asks while the first still holds the sets.
        entities.get_mut(1).unwrap().position.rx = 8;
        let second = index.lend_current(owner, &mut entities, &alliances, &interner, Some(&rules));
        index.give_back(owner, first);
        assert!(
            index.owners[&owner].product.is_none(),
            "the older loan is refused"
        );
        index.give_back(owner, second);
        let kept = index.owners[&owner].product.as_ref().unwrap();
        assert!(kept.1.contains_key(MovementLayer::Ground, &(8, 5)));

        // A pass holding refused sets is rebuilt on its next refresh.
        let mut lent =
            index.lend_current(owner, &mut entities, &alliances, &interner, Some(&rules));
        let _newer = index.lend_current(owner, &mut entities, &alliances, &interner, Some(&rules));
        entities.get_mut(1).unwrap().position.rx = 9;
        index.refresh_lent(
            owner,
            &mut lent,
            &mut entities,
            &alliances,
            &interner,
            Some(&rules),
        );
        assert!(lent.sets.1.contains_key(MovementLayer::Ground, &(9, 5)));
    }
}
