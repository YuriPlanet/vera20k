//! Production owns the global enqueue order and the factories it orders.
use super::{EnqueueOutcome, FactoryEdit, FactoryRegistry, ProductionCategory};
use crate::sim::intern::InternedId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Player production state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionState {
    // Building+3D3, one byte per retained building. Building448070/448160
    // group peers by the raw Factory value and Naval, not by sidebar queue,
    // and clear only matching non-limbo peers, so a limbo peer can keep its
    // flag beside the chosen on-map factory. This is the sole primary-state
    // owner; the House FactoryClass queue slots remain in `factories`.
    primary_factories: BTreeSet<u64>,
    /// The monotonic temporal stamp the next accepted enqueue takes.
    next_enqueue_order: u64,
    /// House queues and factories attached to individual buildings. Each
    /// factory retains its active product and ordered queue; the global
    /// factory update charges the owning House before the House update.
    factories: FactoryRegistry,
}

impl ProductionState {
    /// The next global production order, retained across House and building factories.
    pub fn next_enqueue_order(&self) -> u64 {
        self.next_enqueue_order
    }

    pub fn factories(&self) -> &FactoryRegistry {
        &self.factories
    }

    pub(in crate::sim::production) fn edit_factories(&mut self) -> FactoryEdit<'_> {
        self.factories.edit()
    }

    /// House Begin_Production (0x004FA350) and forced StartNext (0x004CA60A).
    /// An accepted request spends one order even when it re-arms an idle
    /// factory and its original construction order stays in place.
    pub(in crate::sim::production) fn enqueue(
        &mut self,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        cost: i32,
        max_queued: i32,
        at_build_limit: bool,
        force: bool,
    ) -> EnqueueOutcome {
        let outcome = self.factories.enqueue(
            owner,
            category,
            type_id,
            self.next_enqueue_order,
            cost,
            max_queued,
            at_build_limit,
            force,
        );
        if outcome != EnqueueOutcome::AppendRefused {
            self.next_enqueue_order = self.next_enqueue_order.saturating_add(1);
        }
        outcome
    }

    /// Building Factory_AI publishes the Factory (0x00450387) before
    /// starting its product (0x0045039A). A failed start still spent its order.
    pub(in crate::sim::production) fn create_building_factory(
        &mut self,
        building: u64,
        owner: InternedId,
        category: ProductionCategory,
        type_id: InternedId,
        cost: i32,
    ) {
        let order = self.next_enqueue_order;
        self.next_enqueue_order = order.saturating_add(1);
        self.factories
            .create_building_factory(building, owner, category, type_id, order, cost);
    }

    /// FactoryClass::PointerExpired (0x004CA580) only drops a matching product.
    pub(in crate::sim) fn factory_product_expired(&mut self, id: u64) -> bool {
        self.factories.pointer_expired(id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn factories_mut(&mut self) -> &mut FactoryRegistry {
        self.factories_mut_for_test()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_next_enqueue_order_for_test(&mut self, value: u64) {
        self.next_enqueue_order = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn factories_mut_for_test(&mut self) -> &mut FactoryRegistry {
        &mut self.factories
    }

    /// Set the native primary byte. Building448070/448160 own which peers
    /// clear first; this store keeps no group of its own.
    pub(in crate::sim::production) fn set_primary_factory(&mut self, id: u64) {
        self.primary_factories.insert(id);
    }
    /// Clear the native primary byte without choosing a replacement. Unlimbo
    /// and ChangeOwner write it; destruction removes its retained identity.
    /// Limbo preserves the byte, while FindFactory filters the limbo object.
    pub(crate) fn clear_primary_factory(&mut self, id: u64) {
        self.primary_factories.remove(&id);
    }
    /// Whether this retained building has its native primary flag
    /// (`TechnoClass+0x3D3`). Limbo preserves that flag; production eligibility
    /// and display admission remain with their respective consumers.
    pub fn is_primary_factory(&self, id: u64) -> bool {
        self.primary_factories.contains(&id)
    }
    pub(crate) fn retain_primary_factory_links(&mut self, live: &BTreeSet<u64>) {
        self.primary_factories.retain(|id| live.contains(id));
    }
    /// Every retained primary flag, in id order; no flag folds nothing.
    pub(crate) fn hash_primary_factories(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        for id in &self.primary_factories {
            id.hash(hasher);
        }
    }
    /// Fixtures outside this module set one byte; they clear any other
    /// flag they placed themselves.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_primary_factory_for_test(&mut self, id: u64) {
        self.set_primary_factory(id);
    }
}

impl Default for ProductionState {
    fn default() -> Self {
        Self {
            primary_factories: BTreeSet::new(),
            next_enqueue_order: 1,
            factories: FactoryRegistry::default(),
        }
    }
}
