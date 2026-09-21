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

/// Where every entity sits, whoever is looking. Shared by all owners.
#[derive(Debug, Default)]
struct Placements {
    /// Each entity's contribution as of the last sync.
    placed: BTreeMap<u64, Contribution>,
    units_at: BTreeMap<LayerCell, BTreeSet<u64>>,
    /// How many placed buildings block each cell.
    structure_refs: BTreeMap<(u16, u16), u32>,
}

/// Cells and keys whose contributors changed.
#[derive(Debug, Default)]
struct Dirty {
    cells: BTreeSet<(u16, u16)>,
    keys: BTreeSet<LayerCell>,
}

impl Dirty {
    fn len(&self) -> usize {
        self.cells.len() + self.keys.len()
    }
}

impl Placements {
    fn unplace(&mut self, id: u64, dirty: &mut Dirty) {
        match self.placed.remove(&id) {
            Some(Contribution::Structure(blocked)) => {
                for cell in blocked {
                    if let Some(count) = self.structure_refs.get_mut(&cell) {
                        *count -= 1;
                        if *count == 0 {
                            self.structure_refs.remove(&cell);
                        }
                    }
                    dirty.cells.insert(cell);
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
                dirty.keys.insert(key);
            }
            None => {}
        }
    }

    fn place(&mut self, id: u64, contribution: Contribution, dirty: &mut Dirty) {
        match &contribution {
            Contribution::Structure(blocked) => {
                for &cell in blocked {
                    *self.structure_refs.entry(cell).or_insert(0) += 1;
                    dirty.cells.insert(cell);
                }
            }
            Contribution::Unit(unit) => {
                let key = (unit.layer, unit.cell);
                self.units_at.entry(key).or_default().insert(id);
                dirty.keys.insert(key);
            }
        }
        self.placed.insert(id, contribution);
    }

    /// Re-derive one entity; report where it sat and where it sits if that
    /// changed.
    fn rederive(
        &mut self,
        id: u64,
        entities: &EntityStore,
        interner: &StringInterner,
        rules: Option<&RuleSet>,
        dirty: &mut Dirty,
    ) {
        let now = entities
            .get(id)
            .and_then(|entity| contribution(entity, interner, rules));
        if self.placed.get(&id) == now.as_ref() {
            return;
        }
        self.unplace(id, dirty);
        if let Some(now) = now {
            self.place(id, now, dirty);
        }
    }

    fn of_everyone(
        entities: &EntityStore,
        interner: &StringInterner,
        rules: Option<&RuleSet>,
    ) -> Self {
        let mut placements = Self::default();
        let mut dirty = Dirty::default();
        for entity in entities.values() {
            if let Some(contribution) = contribution(entity, interner, rules) {
                placements.place(entity.stable_id(), contribution, &mut dirty);
            }
        }
        placements
    }
}

/// One owner's view of the shared placements.
#[derive(Debug, Default)]
struct OwnerView {
    /// Changed since this owner's product was last brought current.
    dirty: Dirty,
    /// `are_houses_friendly(owner, other)` under the alliance graph the index
    /// last saw; the index drops every view when that graph changes.
    friendly: BTreeMap<InternedId, bool>,
    /// `None` while a movement pass holds it.
    product: Option<OwnerBlockSet>,
    /// Which loan holds the product. A pass that comes back with another
    /// number held sets this view no longer describes.
    loan: u64,
}

impl OwnerView {
    /// Rewrite the product at the dirty cells and keys from the placements.
    fn bring_current(
        &mut self,
        product: &mut OwnerBlockSet,
        placements: &Placements,
        owner: &str,
        alliances: &HouseAllianceMap,
        interner: &StringInterner,
    ) {
        let dirty = std::mem::take(&mut self.dirty);
        for cell in dirty.cells {
            if placements.structure_refs.contains_key(&cell) {
                product.0.insert(cell);
            } else {
                product.0.remove(&cell);
            }
        }
        for (layer, cell) in dirty.keys {
            product.1.remove(layer, &cell);
            product.1.remove_moving_ally(layer, &cell);
            let ids = placements
                .units_at
                .get(&(layer, cell))
                .into_iter()
                .flatten();
            for id in ids {
                let Some(Contribution::Unit(unit)) = placements.placed.get(id) else {
                    continue;
                };
                let friendly = *self.friendly.entry(unit.owner).or_insert_with(|| {
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

    /// A product from every placement: no entity is read.
    fn build(
        &mut self,
        placements: &Placements,
        owner: &str,
        alliances: &HouseAllianceMap,
        interner: &StringInterner,
    ) -> OwnerBlockSet {
        self.dirty = Dirty {
            cells: placements.structure_refs.keys().copied().collect(),
            keys: placements.units_at.keys().copied().collect(),
        };
        let mut product = OwnerBlockSet::default();
        self.bring_current(&mut product, placements, owner, alliances, interner);
        product
    }
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
    /// `None` until the first sync, and again whenever the store reports that
    /// anything may have changed or the rules are not the ones last seen.
    placements: Option<Placements>,
    owners: BTreeMap<InternedId, OwnerView>,
    /// The alliance graph the owners' friendliness answers were taken under.
    alliances_seen: Option<HouseAllianceMap>,
    /// Which rules the placements were derived under, by address: a building's
    /// blocked cells come from its type. Never dereferenced.
    rules_seen: Option<usize>,
    last_loan: u64,
    /// How many times the placements were rebuilt from every entity.
    #[cfg(test)]
    pub(crate) world_rebuilds: usize,
}

/// Sets on loan to a movement pass, with the number that returns them.
#[derive(Debug, Default)]
pub(crate) struct LentOwnerBlockSet {
    loan: u64,
    pub(crate) sets: OwnerBlockSet,
}

impl OwnerBlockIndex {
    /// Bring the shared placements to the entities' current state and tell
    /// every owner what moved.
    fn sync_placements(
        &mut self,
        entities: &mut EntityStore,
        alliances: &HouseAllianceMap,
        interner: &StringInterner,
        rules: Option<&RuleSet>,
    ) -> &Placements {
        if self.alliances_seen.as_ref() != Some(alliances) {
            self.owners.clear();
            self.alliances_seen = Some(alliances.clone());
        }
        let rules_now = rules.map(|rules| std::ptr::from_ref(rules) as usize);
        if self.rules_seen != rules_now {
            self.placements = None;
            self.rules_seen = rules_now;
        }
        let touched = entities.take_touched();
        match (self.placements.as_mut(), touched) {
            (Some(placements), TouchedEntities::Ids(ids)) => {
                let mut dirty = Dirty::default();
                // Sorted and deduplicated: each entity is re-derived once.
                for id in ids.into_iter().collect::<BTreeSet<u64>>() {
                    placements.rederive(id, entities, interner, rules, &mut dirty);
                }
                // A view that stopped being asked for would collect dirt for
                // ever; past this point building it afresh is cheaper.
                let limit = placements.units_at.len() / 2 + 256;
                self.owners.retain(|_, view| {
                    view.dirty.cells.extend(dirty.cells.iter().copied());
                    view.dirty.keys.extend(dirty.keys.iter().copied());
                    view.dirty.len() <= limit
                });
            }
            _ => {
                self.placements = Some(Placements::of_everyone(entities, interner, rules));
                self.owners.clear();
                #[cfg(test)]
                {
                    self.world_rebuilds += 1;
                }
            }
        }
        self.placements
            .as_ref()
            .expect("placements were just ensured")
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
        self.sync_placements(entities, alliances, interner, rules);
        let placements = self.placements.as_ref().expect("synced above");
        let owner_name = interner.resolve(owner);
        self.last_loan += 1;
        let loan = self.last_loan;
        let view = self.owners.entry(owner).or_default();
        let sets = match view.product.take() {
            Some(mut product) => {
                view.bring_current(&mut product, placements, owner_name, alliances, interner);
                product
            }
            // No view yet, or its product is out with a pass that will be
            // refused: build from the placements.
            None => view.build(placements, owner_name, alliances, interner),
        };
        view.loan = loan;
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
        self.sync_placements(entities, alliances, interner, rules);
        let placements = self.placements.as_ref().expect("synced above");
        let owner_name = interner.resolve(owner);
        match self.owners.get_mut(&owner) {
            Some(view) if view.product.is_none() && view.loan == lent.loan => {
                view.bring_current(&mut lent.sets, placements, owner_name, alliances, interner);
            }
            // The view was dropped or lent again since: these sets are
            // nobody's, so rebuild them and take the loan over.
            _ => {
                let view = self.owners.entry(owner).or_default();
                view.product = None;
                lent.sets = view.build(placements, owner_name, alliances, interner);
                self.last_loan += 1;
                view.loan = self.last_loan;
                lent.loan = view.loan;
            }
        }
        debug_assert_current(&lent.sets, entities, owner_name, alliances, interner, rules);
    }

    /// Take back what [`Self::lend_current`] handed out. Sets whose view was
    /// dropped or lent again in the meantime are discarded.
    pub(crate) fn give_back(&mut self, owner: InternedId, lent: LentOwnerBlockSet) {
        if let Some(view) = self.owners.get_mut(&owner)
            && view.product.is_none()
            && view.loan == lent.loan
        {
            view.product = Some(lent.sets);
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

    const OWNERS: [&str; 2] = ["Americans", "Russians"];

    fn unit(id: u64, owner: &str, cell: (u16, u16)) -> GameEntity {
        let mut entity = GameEntity::test_default(id, "MTNK", owner, cell.0, cell.1);
        entity.category = EntityCategory::Unit;
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity
    }

    fn structure(id: u64, type_id: &str, owner: &str, cell: (u16, u16)) -> GameEntity {
        let mut entity = GameEntity::test_default(id, type_id, owner, cell.0, cell.1);
        entity.category = EntityCategory::Structure;
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity
    }

    fn moving_to(next: (u16, u16), from: (u16, u16)) -> Option<MovementTarget> {
        Some(MovementTarget {
            path: vec![from, next],
            path_layers: vec![MovementLayer::Ground; 2],
            next_index: 1,
            ..MovementTarget::default()
        })
    }

    fn rules_again() -> RuleSet {
        rules()
    }

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAPOWR\n1=NABNKR\n\
             [MTNK]\nSpeed=4\n[GAPOWR]\nFoundation=2x2\n\
             [NABNKR]\nFoundation=1x1\nBunker=yes\n",
        ))
        .expect("rules")
    }

    /// Lend to both owners, compare with the whole-world build, give back.
    /// Returns how many times the placements were rebuilt from every entity.
    fn lend_and_check(
        index: &mut OwnerBlockIndex,
        entities: &mut EntityStore,
        alliances: &HouseAllianceMap,
        rules: &RuleSet,
    ) -> usize {
        let interner = test_interner();
        let before = index.world_rebuilds;
        for owner in OWNERS {
            let id = test_intern(owner);
            let lent = index.lend_current(id, entities, alliances, &interner, Some(rules));
            assert_eq!(
                lent.sets,
                build_owner_block_set(entities, owner, alliances, &interner, Some(rules)),
                "{owner}: the kept sets differ from a build of the whole world"
            );
            index.give_back(id, lent);
        }
        index.world_rebuilds - before
    }

    fn kept<'a>(index: &'a OwnerBlockIndex, owner: &str) -> &'a OwnerBlockSet {
        index.owners[&test_intern(owner)]
            .product
            .as_ref()
            .expect("the sets are back in the index")
    }

    fn code(index: &OwnerBlockIndex, owner: &str, layer: MovementLayer, cell: (u16, u16)) -> u8 {
        kept(index, owner)
            .1
            .get(layer, &cell)
            .expect("an entry at the cell")
            .cost_code
    }

    #[test]
    fn touched_entities_are_rederived_and_the_rest_is_kept() {
        let rules = rules();
        let alliances = HouseAllianceMap::new();
        let mut entities = EntityStore::new();
        entities.insert(unit(1, "Americans", (5, 5)));
        entities.insert(unit(2, "Americans", (6, 5)));
        entities.insert(unit(3, "Russians", (7, 5)));
        entities.insert(structure(4, "GAPOWR", "Russians", (20, 20)));
        let mut index = OwnerBlockIndex::default();
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            1,
            "the first loan reads everyone"
        );

        // Two occupants of one cell: the later id wins, and when it leaves the
        // earlier one returns.
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
        entities.get_mut(1).unwrap().movement_target = moving_to((5, 6), (5, 5));
        entities.get_mut(3).unwrap().dying = true;
        entities.remove(4);
        entities.insert(unit(5, "Russians", (5, 6)));
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            0
        );
        let americans = kept(&index, "Americans");
        assert!(
            americans.0.is_empty(),
            "the power plant cells were released"
        );
        let at = |cell| americans.1.get(MovementLayer::Ground, &cell);
        assert_eq!(at((5, 5)).unwrap().cost_code, 2);
        assert_eq!(at((5, 5)).unwrap().next_cell, Some((5, 6)));
        assert!(
            americans
                .1
                .moving_ally(MovementLayer::Ground, &(5, 5))
                .is_some()
        );
        assert_eq!(at((5, 6)).unwrap().cost_code, 5);
        assert!(at((7, 5)).is_none());
    }

    #[test]
    fn layer_owner_bunker_and_reused_ids_follow_the_entities() {
        let rules = rules();
        let alliances = HouseAllianceMap::new();
        let mut entities = EntityStore::new();
        entities.insert(unit(1, "Americans", (5, 5)));
        entities.insert(unit(2, "Russians", (5, 5)));
        entities.insert(structure(3, "NABNKR", "Russians", (9, 9)));
        let mut index = OwnerBlockIndex::default();
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            1
        );

        // Onto the bridge deck above the same cell: another list, another key.
        entities.get_mut(2).unwrap().on_bridge = true;
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            0
        );
        assert_eq!(code(&index, "Americans", MovementLayer::Ground, (5, 5)), 6);
        assert_eq!(code(&index, "Americans", MovementLayer::Bridge, (5, 5)), 5);

        // The store changes its owner; the bunker takes an occupant; an id
        // comes back as another kind of thing somewhere else.
        entities.change_owner(2, test_intern("Americans"));
        entities.get_mut(3).unwrap().bunker_occupant = Some(1);
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            0
        );
        assert_eq!(code(&index, "Americans", MovementLayer::Bridge, (5, 5)), 6);
        entities.remove(1);
        entities.insert(structure(1, "GAPOWR", "Americans", (30, 30)));
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &alliances, &rules),
            0
        );
        assert!(kept(&index, "Russians").0.contains(&(31, 31)));
    }

    #[test]
    fn alliances_rules_and_whole_store_walks_rebuild_what_they_must() {
        let rules = rules();
        let mut entities = EntityStore::new();
        entities.insert(unit(1, "Americans", (5, 5)));
        entities.insert(unit(2, "Russians", (5, 6)));
        let mut index = OwnerBlockIndex::default();
        let enemies = HouseAllianceMap::new();
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &enemies, &rules),
            1
        );
        assert_eq!(code(&index, "Americans", MovementLayer::Ground, (5, 6)), 5);

        // A changed alliance graph rebuilds the views from the kept placements.
        let mut allied = HouseAllianceMap::new();
        allied.insert("AMERICANS".into(), ["RUSSIANS".to_string()].into());
        allied.insert("RUSSIANS".into(), ["AMERICANS".to_string()].into());
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &allied, &rules),
            0
        );
        assert_eq!(code(&index, "Americans", MovementLayer::Ground, (5, 6)), 6);

        // An all-entity mutable walk, and rules at another address, rebuild
        // the placements.
        for entity in entities.values_mut() {
            entity.position.ry += 1;
        }
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &allied, &rules),
            1
        );
        let other_rules = rules_again();
        assert_eq!(
            lend_and_check(&mut index, &mut entities, &allied, &other_rules),
            1
        );
    }

    /// Random edits through the store's own interface, each batch followed by
    /// a comparison of both owners' sets with a whole-world build.
    #[test]
    fn random_edits_never_leave_the_sets_behind_the_world() {
        let rules = rules();
        let alliances = HouseAllianceMap::new();
        let mut entities = EntityStore::new();
        let mut index = OwnerBlockIndex::default();
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound
        };
        let mut rebuilds = 0;
        for _ in 0..600 {
            let id = 1 + next(14);
            let cell = (4 + next(4) as u16, 4 + next(4) as u16);
            let owner = OWNERS[next(2) as usize];
            match next(9) {
                0 => {
                    entities.remove(id);
                }
                1 => {
                    entities.insert(unit(id, owner, cell));
                }
                2 => {
                    let kind = if next(2) == 0 { "GAPOWR" } else { "NABNKR" };
                    entities.insert(structure(id, kind, owner, cell));
                }
                3 => entities.change_owner(id, test_intern(owner)),
                edit => {
                    if let Some(entity) = entities.get_mut(id) {
                        match edit {
                            4 => (entity.position.rx, entity.position.ry) = cell,
                            5 => entity.on_bridge = !entity.on_bridge,
                            6 => entity.movement_target = moving_to(cell, (0, 0)),
                            7 => entity.dying = !entity.dying,
                            _ => {
                                entity.foot_occupation_enabled = !entity.foot_occupation_enabled;
                                entity.bunker_occupant = entity.bunker_occupant.xor(Some(1));
                            }
                        }
                    }
                }
            }
            if next(3) == 0 {
                rebuilds += lend_and_check(&mut index, &mut entities, &alliances, &rules);
            }
        }
        assert_eq!(rebuilds, 1, "only the first loan read every entity");
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
        let lend = |index: &mut OwnerBlockIndex, entities: &mut EntityStore| {
            index.lend_current(owner, entities, &alliances, &interner, Some(&rules))
        };
        let first = lend(&mut index, &mut entities);
        // A second pass asks while the first still holds the sets.
        entities.get_mut(1).unwrap().position.rx = 8;
        let second = lend(&mut index, &mut entities);
        index.give_back(owner, first);
        assert!(
            index.owners[&owner].product.is_none(),
            "the older loan is refused"
        );
        index.give_back(owner, second);
        assert_eq!(code(&index, "Americans", MovementLayer::Ground, (8, 5)), 6);

        // A pass holding refused sets is rebuilt on its next refresh.
        let mut lent = lend(&mut index, &mut entities);
        let _newer = lend(&mut index, &mut entities);
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
