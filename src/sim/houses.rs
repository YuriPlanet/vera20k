//! The scenario's House array: every House's state and its registration order.
//!
//! Native House ids are indices into the HouseClass array, which the House
//! constructor extends; map ordering by name id is not an equivalent
//! substitute. A House is never removed during a scenario, so registration
//! is the only structural write: lookup by name id and iteration in
//! registration order read one owner instead of a map and a separate order
//! list that every writer had to extend in step.

use std::collections::BTreeMap;
use std::collections::btree_map;

use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;

/// The House array. Iteration through [`Self::iter`] and [`Self::values`] is
/// in name-id order; [`Self::order`] and [`Self::in_order`] give the native
/// registration order.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "SavedHouses")]
pub struct Houses {
    states: BTreeMap<InternedId, HouseState>,
    /// Registration order. Holds each key of `states` exactly once.
    order: Vec<InternedId>,
}

impl Houses {
    /// Append a constructed House to the array under its own name.
    ///
    /// Panics when a House of that name is already registered: scenario
    /// construction admits distinct House names only.
    #[track_caller]
    pub fn register(&mut self, house: HouseState) -> &mut HouseState {
        let name = house.name();
        match self.states.entry(name) {
            btree_map::Entry::Vacant(slot) => {
                self.order.push(name);
                slot.insert(house)
            }
            btree_map::Entry::Occupied(_) => panic!("House {name:?} is already registered"),
        }
    }

    pub fn get(&self, name: &InternedId) -> Option<&HouseState> {
        self.states.get(name)
    }

    pub fn get_mut(&mut self, name: &InternedId) -> Option<&mut HouseState> {
        self.states.get_mut(name)
    }

    pub fn contains_key(&self, name: &InternedId) -> bool {
        self.states.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Houses in name-id order.
    pub fn iter(&self) -> btree_map::Iter<'_, InternedId, HouseState> {
        self.states.iter()
    }

    /// Houses in name-id order.
    pub fn iter_mut(&mut self) -> btree_map::IterMut<'_, InternedId, HouseState> {
        self.states.iter_mut()
    }

    pub fn keys(&self) -> btree_map::Keys<'_, InternedId, HouseState> {
        self.states.keys()
    }

    pub fn values(&self) -> btree_map::Values<'_, InternedId, HouseState> {
        self.states.values()
    }

    pub fn values_mut(&mut self) -> btree_map::ValuesMut<'_, InternedId, HouseState> {
        self.states.values_mut()
    }

    /// Name ids in native registration order; a House's index here is its
    /// native House id.
    pub fn order(&self) -> &[InternedId] {
        &self.order
    }

    /// Houses in native registration order.
    pub fn in_order(&self) -> impl Iterator<Item = &HouseState> {
        self.order.iter().map(|name| &self.states[name])
    }

    /// Houses in native registration order, mutably.
    pub fn in_order_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut HouseState> {
        let order = &self.order;
        let mut slots: Vec<(usize, &mut HouseState)> = self
            .states
            .iter_mut()
            .map(|(name, house)| {
                let index = order.iter().position(|registered| registered == name);
                (index.expect("every House is registered"), house)
            })
            .collect();
        slots.sort_unstable_by_key(|&(index, _)| index);
        slots.into_iter().map(|(_, house)| house)
    }

    /// One House, mutably, beside the registration order.
    pub fn get_mut_with_order(
        &mut self,
        name: &InternedId,
    ) -> Option<(&mut HouseState, &[InternedId])> {
        let house = self.states.get_mut(name)?;
        Some((house, &self.order))
    }

    /// The native House id: the House's index in registration order.
    pub fn index_of(&self, name: InternedId) -> Option<usize> {
        self.order.iter().position(|&registered| registered == name)
    }

    /// The House registered under `name`, registering `make()` first when
    /// the fixture has none.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_register_for_test(
        &mut self,
        name: InternedId,
        make: impl FnOnce() -> HouseState,
    ) -> &mut HouseState {
        if !self.states.contains_key(&name) {
            let house = make();
            assert_eq!(
                house.name(),
                name,
                "fixture House registered under its name"
            );
            self.register(house);
        }
        self.states.get_mut(&name).expect("registered House")
    }

    /// Register `house`, or replace the registered House of its name in
    /// place: a map insert's semantics, for fixtures that rebuild a House.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_for_test(&mut self, house: HouseState) -> Option<HouseState> {
        match self.states.get_mut(&house.name()) {
            Some(slot) => Some(std::mem::replace(slot, house)),
            None => {
                self.register(house);
                None
            }
        }
    }

    /// Replace a registered House's whole state, keeping its array slot.
    #[cfg(any(test, feature = "test-support"))]
    pub fn replace_for_test(&mut self, house: HouseState) {
        let slot = self
            .states
            .get_mut(&house.name())
            .expect("replaced House is registered");
        *slot = house;
    }

    /// Drop a House from the array, for fixtures of a missing House.
    #[cfg(any(test, feature = "test-support"))]
    pub fn remove_for_test(&mut self, name: &InternedId) -> Option<HouseState> {
        self.order.retain(|registered| registered != name);
        self.states.remove(name)
    }

    /// Re-sequence the registered Houses. `order` must name each registered
    /// House exactly once.
    #[cfg(any(test, feature = "test-support"))]
    pub fn reorder_for_test(&mut self, order: &[InternedId]) {
        let mut sorted = order.to_vec();
        sorted.sort_unstable();
        assert!(
            sorted.iter().copied().eq(self.states.keys().copied()),
            "fixture order {order:?} must name each registered House once"
        );
        self.order = order.to_vec();
    }

    /// Empty the array, for fixtures that rebuild it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_for_test(&mut self) {
        self.states.clear();
        self.order.clear();
    }

    /// Exchange two Houses' registration slots.
    #[cfg(any(test, feature = "test-support"))]
    pub fn swap_order_for_test(&mut self, a: usize, b: usize) {
        self.order.swap(a, b);
    }
}

/// A saved array before its invariant is checked.
#[derive(serde::Deserialize)]
struct SavedHouses {
    states: BTreeMap<InternedId, HouseState>,
    order: Vec<InternedId>,
}

impl TryFrom<SavedHouses> for Houses {
    type Error = String;

    fn try_from(saved: SavedHouses) -> Result<Self, String> {
        let mut seen = std::collections::BTreeSet::new();
        for name in &saved.order {
            if !seen.insert(*name) || !saved.states.contains_key(name) {
                return Err(format!(
                    "House array order names {name:?} twice or without a House"
                ));
            }
        }
        if seen.len() != saved.states.len() {
            return Err("House array order omits a registered House".to_owned());
        }
        if let Some((key, _)) = saved
            .states
            .iter()
            .find(|(key, house)| **key != house.name())
        {
            return Err(format!("House {key:?} is saved under another House's name"));
        }
        Ok(Self {
            states: saved.states,
            order: saved.order,
        })
    }
}

impl std::ops::Index<&InternedId> for Houses {
    type Output = HouseState;

    fn index(&self, name: &InternedId) -> &HouseState {
        &self.states[name]
    }
}

/// Registers each House in iteration order.
impl FromIterator<HouseState> for Houses {
    fn from_iter<I: IntoIterator<Item = HouseState>>(houses: I) -> Self {
        let mut array = Self::default();
        for house in houses {
            array.register(house);
        }
        array
    }
}

impl<'a> IntoIterator for &'a Houses {
    type Item = (&'a InternedId, &'a HouseState);
    type IntoIter = btree_map::Iter<'a, InternedId, HouseState>;

    fn into_iter(self) -> Self::IntoIter {
        self.states.iter()
    }
}

impl<'a> IntoIterator for &'a mut Houses {
    type Item = (&'a InternedId, &'a mut HouseState);
    type IntoIter = btree_map::IterMut<'a, InternedId, HouseState>;

    fn into_iter(self) -> Self::IntoIter {
        self.states.iter_mut()
    }
}
