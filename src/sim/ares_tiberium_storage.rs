//! Ares refinery storage (`docs/ares-tiberium.md`): `Refinery.UseStorage`
//! refineries put raw tiberium into their House's silos instead of cash,
//! restored from TS by Ares 3.0.
//!
//! - Deposit: `TechnoExt::DepositTiberium @ 10044230` replaces both
//!   refinery payments, the Unit unload's (`UnitClass_Mi_Unload_Storage`,
//!   `10024BC0`, hook `0x73E4A2` in `UnitClass::Mission_Unload @
//!   0x0073D630`) and the slave's (`InfantryClass_Slave_UnloadAt_Storage`,
//!   `10024B60`, hook `0x522D75` in `0x00522D50`, through
//!   `TechnoExt::RefineTiberium @ 100461B0`).
//! - `HouseClass::GiveTiberium @ 0x004F9610` is Ares'
//!   `HouseClass_GiveTiberium_Storage @ 10024290` (hook `0x4F9610`): it
//!   fills the House's `Storage=` buildings, or pays cash.
//! - The stock storage arms Ares relies on, which VERA ports in Ares builds
//!   only because nothing fills the stores in stock YR: Spend_Money's silo
//!   drain (`0x004F97DD..0x004F9925`), Available_Money's storage value
//!   (`0x004F6990`), the store's merge in Added_To_Game (`0x00502B83`) and
//!   its withdrawal in Removed_From_Game (`0x00502801..0x00502821`), and the
//!   sale's store payout (`0x0044A232..0x0044A285`).
//! - Death: `BuildingClass_Destroy_Refinery @ 10023DA0` (hook `0x441B30` in
//!   `BuildingClass::DestructionEffects @ 0x004415F0`) removes the store and
//!   spills it with `TiberiumSpill`.
//!
//! The owner is [`AresTiberiumStorage`], kept in each House's wallet
//! ([`crate::sim::economy::Economy`]) because Spend_Money, which drains the
//! stores, is reached through the wallet alone. It holds the House store
//! (`+0x2FC`) and the store (`+0x33C`) of each of the House's in-game
//! buildings; a building outside the game (Limbo) keeps its own store in
//! its `BuildingStorage`, which the art reads through
//! [`Simulation::ares_ledger_store`] while the ledger holds it.
//!
//! `HouseClass::UpdateAllSilos @ 0x004F9970`, called by GiveTiberium and
//! Spend_Money, only marks `SiloDamage=` buildings for redraw; VERA draws
//! every frame and UpdateAnimation reads the store (`sim::building_art`).

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::tiberium_type::TiberiumTypeId;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits};

/// `OwnedTiberiumStruct::Size`: a StorageClass holds four slots, one per
/// tiberium type index.
pub(crate) const STORAGE_SLOTS: usize = 4;

const ZERO: NativeF32Bits = NativeF32Bits::POSITIVE_ZERO;

/// `amount > 0.0` as the x87 compare tests it.
fn positive(amount: NativeF32Bits) -> bool {
    X87::compare(X87::load_f32(amount), X87::load_i32(0)) == MaskedX87Ordering::Greater
}

/// `lhs - rhs` stored to a float.
fn minus(lhs: NativeF32Bits, rhs: NativeF32Bits) -> NativeF32Bits {
    X87::store_f32_masked_chop(X87::sub(X87::load_f32(lhs), X87::load_f32(rhs)))
}

/// `Game::F2I(amount * Value * IncomeMult)`: Ares' price of an amount in
/// DepositTiberium and GiveTiberium's cash arm (x87, truncating).
fn worth(amount: NativeF32Bits, value: i32, income: NativeF32Bits) -> i32 {
    X87::ftol_i32_low_masked(X87::mul(
        X87::mul(X87::load_f32(amount), X87::load_i32(value)),
        X87::load_f32(income),
    ))
}

/// An integer stored to a float (`FILD`, `FSTP dword`).
fn as_float(value: i32) -> NativeF32Bits {
    X87::store_f32_masked_chop(X87::load_i32(value))
}

/// A StorageClass: one float amount per tiberium type (Techno `+0x33C`,
/// House `+0x2FC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct TiberiumStore([NativeF32Bits; STORAGE_SLOTS]);

impl Default for TiberiumStore {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl TiberiumStore {
    /// The constructor's store (`0x006C95E0`).
    pub(crate) const EMPTY: Self = Self([ZERO; STORAGE_SLOTS]);

    pub(crate) const fn from_amounts(amounts: [NativeF32Bits; STORAGE_SLOTS]) -> Self {
        Self(amounts)
    }

    pub(crate) const fn amounts(self) -> [NativeF32Bits; STORAGE_SLOTS] {
        self.0
    }

    fn is_empty(self) -> bool {
        self.0.iter().all(|amount| amount.is_zero())
    }

    /// `StorageClass::GetTotalAmount @ 0x006C9650`: each slot joins an
    /// integer total through `ftol`, so every slot truncates (Ares' own
    /// `OwnedTiberiumStruct::GetTotalAmount @ 10048830` truncates each slot
    /// before adding: the same for the non-negative amounts stored).
    pub(crate) fn total_amount(self) -> i32 {
        self.0.iter().fold(0, |total, &amount| {
            X87::ftol_i32_low_masked(X87::add(X87::load_i32(total), X87::load_f32(amount)))
        })
    }

    fn amount(self, slot: usize) -> NativeF32Bits {
        self.0[slot]
    }

    /// `StorageClass::AddAmount @ 0x006C9690`.
    fn add_amount(&mut self, amount: NativeF32Bits, slot: usize) {
        self.0[slot] = X87::store_f32_masked_chop(X87::add(
            X87::load_f32(amount),
            X87::load_f32(self.0[slot]),
        ));
    }

    /// `StorageClass::RemoveAmount @ 0x006C96B0`: at most the slot's
    /// amount leaves; returns what left.
    fn remove_amount(&mut self, amount: NativeF32Bits, slot: usize) -> NativeF32Bits {
        let held = self.0[slot];
        let removed = if X87::compare(X87::load_f32(held), X87::load_f32(amount))
            == MaskedX87Ordering::Less
        {
            held
        } else {
            amount
        };
        self.0[slot] = minus(held, removed);
        removed
    }

    /// `StorageClass::FindFirstNonEmptySlot @ 0x006C9820`.
    fn first_non_empty(self) -> Option<usize> {
        (0..STORAGE_SLOTS).find(|&slot| positive(self.0[slot]))
    }

    /// `StorageClass::AddFrom @ 0x006C9740` (`operator+=`).
    fn add_from(&mut self, other: Self) {
        for slot in 0..STORAGE_SLOTS {
            self.add_amount(other.0[slot], slot);
        }
    }

    /// `StorageClass::SubtractFrom` (`operator-=`), Removed_From_Game's
    /// flag-0 arm.
    fn subtract_from(&mut self, other: Self) {
        for slot in 0..STORAGE_SLOTS {
            self.0[slot] = minus(self.0[slot], other.0[slot]);
        }
    }

    /// `StorageClass::GetTotalAsInt @ 0x006C9600`: each positive slot adds
    /// `ftol(Value * amount + total)`.
    fn total_value(self, values: &[i32; STORAGE_SLOTS]) -> i32 {
        (0..STORAGE_SLOTS).fold(0, |total, slot| {
            if !positive(self.0[slot]) {
                return total;
            }
            X87::ftol_i32_low_masked(X87::add(
                X87::mul(X87::load_i32(values[slot]), X87::load_f32(self.0[slot])),
                X87::load_i32(total),
            ))
        })
    }
}

/// What prices a store: each tiberium's `Value=` and the HouseType's
/// `IncomeMult` (`+0x148`). The wallet's Spend_Money and Available_Money
/// read them without the rules, so every entry into the House store
/// (GiveTiberium, the Added_To_Game merge) retains them first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct StoragePricing {
    values: [i32; STORAGE_SLOTS],
    income: NativeF32Bits,
}

impl StoragePricing {
    const UNSET: Self = Self {
        values: [0; STORAGE_SLOTS],
        income: ZERO,
    };

    fn value(&self, slot: usize) -> i32 {
        self.values[slot]
    }
}

impl Default for StoragePricing {
    fn default() -> Self {
        Self::UNSET
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct StoredBuilding {
    id: u64,
    store: TiberiumStore,
}

/// A House's tiberium storage in Ares builds: its store (`+0x2FC`) and the
/// stores of its in-game buildings whose type has `Storage=` (or which
/// arrived holding tiberium), from Added_To_Game to Removed_From_Game. The
/// buildings keep House+68's order: both join at Unlimbo and ChangeOwner
/// and leave stably at Limbo, so GiveTiberium and Spend_Money walk them in
/// the native order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresTiberiumStorage {
    house: TiberiumStore,
    buildings: Vec<StoredBuilding>,
    pricing: StoragePricing,
}

impl Default for AresTiberiumStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl AresTiberiumStorage {
    pub(crate) const fn new() -> Self {
        Self {
            house: TiberiumStore::EMPTY,
            buildings: Vec::new(),
            pricing: StoragePricing::UNSET,
        }
    }

    /// House `+0x2FC`.
    pub(crate) fn house_store(&self) -> TiberiumStore {
        self.house
    }

    /// The `+0x33C` store of a building the House holds in game.
    pub(crate) fn building_store(&self, id: u64) -> Option<TiberiumStore> {
        self.buildings
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.store)
    }

    fn building_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.buildings.iter().map(|entry| entry.id)
    }

    fn entry_mut(&mut self, id: u64) -> Option<&mut StoredBuilding> {
        self.buildings.iter_mut().find(|entry| entry.id == id)
    }

    /// Hashes the retained stores once any holds tiberium, so Houses
    /// without storage hash as before.
    pub(crate) fn hash_retained(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if self.house.is_empty() && self.buildings.iter().all(|entry| entry.store.is_empty()) {
            return;
        }
        b"ares-tiberium-storage-v1".hash(hasher);
        self.house.hash(hasher);
        for entry in self
            .buildings
            .iter()
            .filter(|entry| !entry.store.is_empty())
        {
            entry.hash(hasher);
        }
        self.pricing.hash(hasher);
    }

    /// `HouseClass::Available_Money @ 0x004F6990`: `ftol(GetTotalAsInt(+0x2FC)
    /// * IncomeMult + balance)`. An empty store values 0, which leaves the
    /// balance itself for a finite IncomeMult.
    pub(crate) fn available_money(&self, credits: i32) -> i32 {
        if self.house.is_empty() {
            return credits;
        }
        let value = self.house.total_value(&self.pricing.values);
        X87::ftol_i32_low_masked(X87::add(
            X87::mul(X87::load_i32(value), X87::load_f32(self.pricing.income)),
            X87::load_i32(credits),
        ))
    }

    /// `HouseClass::Spend_Money @ 0x004F97DD..0x004F9925`, the silo drain of
    /// what the balance left `remaining`: while the House store holds any,
    /// each building in order gives single units (`RemoveAmount(1.0)`) from
    /// its first non-empty slot, each worth `ftol(Value * IncomeMult *
    /// unit)`, until the spend is met; the last unit's excess comes back as
    /// change. Returns `(taken, change)`: what joins the spending total and
    /// the new balance.
    pub(crate) fn drain_for_spend(&mut self, mut remaining: i32) -> (i32, i32) {
        let (mut taken, mut change) = (0_i32, 0_i32);
        if remaining <= 0 || self.house.total_amount() <= 0 {
            return (taken, change);
        }
        let Self {
            house,
            buildings,
            pricing,
        } = self;
        for entry in buildings.iter_mut() {
            let store = &mut entry.store;
            if store.total_amount() > 0 {
                loop {
                    if remaining < 1 {
                        break;
                    }
                    let Some(slot) = store.first_non_empty() else {
                        break;
                    };
                    while positive(store.amount(slot)) {
                        let unit = store.remove_amount(NativeF32Bits::ONE, slot);
                        house.remove_amount(unit, slot);
                        let value = X87::ftol_i32_low_masked(X87::mul(
                            X87::mul(
                                X87::load_i32(pricing.value(slot)),
                                X87::load_f32(pricing.income),
                            ),
                            X87::load_f32(unit),
                        ));
                        remaining = remaining.wrapping_sub(value);
                        taken = taken.wrapping_add(value);
                        if remaining < 0 {
                            taken = taken.wrapping_add(remaining);
                            change = change.wrapping_sub(remaining);
                            remaining = 0;
                            break;
                        }
                        if remaining <= 0 {
                            break;
                        }
                    }
                    if store.total_amount() <= 0 {
                        break;
                    }
                }
            }
            if remaining == 0 {
                break;
            }
        }
        (taken, change)
    }

    /// The storage arm of Ares `10024290`: `amount` is cut to the free
    /// capacity (`capacity` House `+0x310` minus the stored total), then
    /// fills each `silos` building (in order, on the map with its type's
    /// `Storage`) up to that Storage, in the building and the House store.
    /// What finds no room is lost.
    fn give(
        &mut self,
        amount: NativeF32Bits,
        slot: usize,
        silos: &[(u64, i32)],
        capacity: i32,
        pricing: StoragePricing,
    ) {
        self.pricing = pricing;
        let free = capacity.wrapping_sub(self.house.total_amount());
        let mut amount = amount;
        if X87::compare(X87::load_f32(amount), X87::load_i32(free)) == MaskedX87Ordering::Greater {
            amount = as_float(free);
        }
        for &(id, storage) in silos {
            if !positive(amount) {
                break;
            }
            let Some(entry) = self.entry_mut(id) else {
                continue;
            };
            let room = as_float(storage.wrapping_sub(entry.store.total_amount()));
            if !positive(room) {
                continue;
            }
            let room = if X87::compare(X87::load_f32(room), X87::load_f32(amount))
                == MaskedX87Ordering::Greater
            {
                amount
            } else {
                room
            };
            entry.store.add_amount(room, slot);
            self.house.add_amount(room, slot);
            amount = minus(amount, room);
        }
    }

    /// `HouseClass::Added_To_Game @ 0x00502B83`: a building's store joins
    /// the House store (`StorageClass::AddFrom`), and the House holds it.
    fn merge_building(&mut self, id: u64, store: TiberiumStore, pricing: StoragePricing) {
        self.pricing = pricing;
        self.house.add_from(store);
        self.buildings.push(StoredBuilding { id, store });
    }

    /// `HouseClass::Removed_From_Game @ 0x00502801..0x00502821` with
    /// `keepTiberium` false (every Limbo and ChangeOwner caller): the
    /// building's store leaves the House store and goes back with it.
    fn withdraw_building(&mut self, id: u64) -> Option<TiberiumStore> {
        let index = self.buildings.iter().position(|entry| entry.id == id)?;
        let store = self.buildings.remove(index).store;
        self.house.subtract_from(store);
        Some(store)
    }

    /// The removal of Ares `10023DA0`: a building whose store totals at
    /// least 1 loses `ceil(amount)` of each slot, from its store and from
    /// the House store (each clamped to what it holds). Returns the
    /// removed amounts, the bales the spill places.
    fn destroy_building_store(&mut self, id: u64) -> [i32; STORAGE_SLOTS] {
        let mut removed = [0; STORAGE_SLOTS];
        let Some(index) = self.buildings.iter().position(|entry| entry.id == id) else {
            return removed;
        };
        if self.buildings[index].store.total_amount() < 1 {
            return removed;
        }
        for (slot, bales) in removed.iter_mut().enumerate() {
            // std::ceil on the float's double (FUN_10083BC0); the ceiling
            // of a float is itself a float.
            let amount = f64::from(f32::from_bits(
                self.buildings[index].store.amount(slot).bits(),
            ))
            .ceil() as f32;
            if amount > 0.0 {
                let amount = NativeF32Bits::from_bits(amount.to_bits());
                self.buildings[index].store.remove_amount(amount, slot);
                self.house.remove_amount(amount, slot);
                *bales = X87::ftol_i32_low_masked(X87::load_f32(amount));
            }
        }
        removed
    }
}

/// A tiberium type's `Value=` (0 for a slot no type fills).
fn tiberium_value(rules: &RuleSet, slot: usize) -> i32 {
    u8::try_from(slot)
        .ok()
        .and_then(|slot| rules.tiberium_types.get(TiberiumTypeId(slot)))
        .map_or(0, |tiberium| tiberium.value)
}

impl Simulation {
    /// The prices `owner`'s store is valued at ([`StoragePricing`]).
    fn ares_storage_pricing(&self, owner: InternedId, rules: &RuleSet) -> StoragePricing {
        StoragePricing {
            values: std::array::from_fn(|slot| tiberium_value(rules, slot)),
            income: self.ares_income_mult(owner, rules),
        }
    }

    /// The owner's HouseType `IncomeMult` (`+0x148`).
    fn ares_income_mult(&self, owner: InternedId, rules: &RuleSet) -> NativeF32Bits {
        crate::sim::house_state::income_mult_for_owner(
            &self.houses,
            &self.interner,
            rules,
            self.interner.resolve(owner),
        )
    }

    /// The `+0x33C` store the owner's ledger holds for building `id`, which
    /// the art reads instead of the building's own while it is in game.
    pub(crate) fn ares_ledger_store(&self, id: u64) -> Option<TiberiumStore> {
        let owner = self.substrate.entities.get(id)?.owner();
        self.houses
            .get(&owner)?
            .economy()
            .ares_storage()
            .building_store(id)
    }

    /// `TechnoExt::DepositTiberium @ 10044230` on the receiving `building`
    /// (a refinery, or a slave's master): the purifier `bonus` always pays
    /// cash; the raw `amount` pays cash too unless the building's type sets
    /// `Refinery.UseStorage`, which gives it to the owner's House
    /// ([`Self::ares_give_tiberium`]). The cash, each part
    /// `ftol(amount * Value * IncomeMult)`, goes in one Add_Credits when
    /// positive; it adds no score. `TechnoExt::RefineTiberium @ 100461B0`
    /// (the slave's) computes the same bonus first.
    pub(crate) fn ares_deposit_tiberium(
        &mut self,
        building: u64,
        slot: TiberiumTypeId,
        amount: NativeF32Bits,
        bonus: NativeF32Bits,
        rules: &RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(building) else {
            return;
        };
        let owner = entity.owner();
        let use_storage = self
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.techno_combat().tiberium().refinery_use_storage());
        let slot = usize::from(slot.0);
        let (price, income) = (
            tiberium_value(rules, slot),
            self.ares_income_mult(owner, rules),
        );
        let mut value = 0_i32;
        if positive(bonus) {
            value = value.wrapping_add(worth(bonus, price, income));
        }
        if positive(amount) {
            if use_storage {
                self.ares_give_tiberium(owner, amount, slot, rules);
            } else {
                value = value.wrapping_add(worth(amount, price, income));
            }
        }
        if value > 0 {
            crate::sim::credit_income::add_credits(&mut self.houses, owner, value);
        }
    }

    /// `HouseClass::GiveTiberium @ 0x004F9610` as Ares 3.0
    /// `HouseClass_GiveTiberium_Storage @ 10024290` replaces it: the score
    /// (`+0x54E8`) gains `ftol(amount * 5)`; a campaign House or a human one
    /// stores the amount in its buildings ([`AresTiberiumStorage`]), any
    /// other is paid `ftol(amount * Value * IncomeMult)` in cash.
    pub(crate) fn ares_give_tiberium(
        &mut self,
        owner: InternedId,
        amount: NativeF32Bits,
        slot: usize,
        rules: &RuleSet,
    ) {
        let pricing = self.ares_storage_pricing(owner, rules);
        let campaign = !self.session.game_mode_nonzero();
        let Some(house) = self.houses.get(&owner) else {
            return;
        };
        let stores = campaign || house.is_human();
        let capacity = house.tracking().storage_capacity();
        let silos: Vec<(u64, i32)> = if stores {
            house
                .economy()
                .ares_storage()
                .building_ids()
                .filter_map(|id| {
                    let entity = self.substrate.entities.get(id)?;
                    if entity.in_limbo() {
                        return None;
                    }
                    let storage = self.object_type(entity.type_ref(), rules)?.storage;
                    (storage > 0).then_some((id, storage))
                })
                .collect()
        } else {
            Vec::new()
        };
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        let economy = house.economy_mut();
        economy.add_score(X87::ftol_i32_low_masked(X87::mul(
            X87::load_f32(amount),
            X87::load_i32(5),
        )));
        if stores {
            // A fifth type's slot lies past the four-slot store natively;
            // VERA stores none of it.
            if slot < STORAGE_SLOTS {
                economy
                    .ares_storage_mut()
                    .give(amount, slot, &silos, capacity, pricing);
            }
        } else {
            economy.add_credits(worth(amount, tiberium_value(rules, slot), pricing.income));
        }
    }

    /// `HouseClass::Added_To_Game @ 0x00502B83` for a building: its store
    /// (zero unless it left another House holding tiberium) joins its
    /// owner's store, which then holds it.
    pub(crate) fn ares_storage_added_to_game(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure {
            return;
        }
        let owner = entity.owner();
        let storage = self
            .object_type(entity.type_ref(), rules)
            .map_or(0, |object| object.storage);
        if !self.houses.contains_key(&owner) {
            return;
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let store = TiberiumStore::from_amounts(entity.building_art.take_storage_amounts());
        if storage <= 0 && store.is_empty() {
            return;
        }
        let pricing = self.ares_storage_pricing(owner, rules);
        if let Some(house) = self.houses.get_mut(&owner) {
            house
                .economy_mut()
                .ares_storage_mut()
                .merge_building(id, store, pricing);
        }
    }

    /// `HouseClass::Removed_From_Game @ 0x00502801..0x00502821` for a
    /// building: its store leaves the receiving House's (`owner`, pinned by
    /// the caller) and stays with the building.
    pub(crate) fn ares_storage_removed_from_game(&mut self, id: u64, owner: InternedId) {
        let Some(store) = self
            .houses
            .get_mut(&owner)
            .and_then(|house| house.economy_mut().ares_storage_mut().withdraw_building(id))
        else {
            return;
        };
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.building_art.restore_storage_amounts(store.amounts());
        }
    }

    /// Ares 3.0 `BuildingClass_Destroy_Refinery @ 10023DA0` (hook
    /// `0x441B30`, after the Explodes= fires of `DestructionEffects`): the
    /// destroyed building's store leaves it and its House a whole unit up
    /// per slot ([`AresTiberiumStorage`]); with its type's `TiberiumSpill`
    /// each unit becomes one tiberium of its slot in the cell of a point
    /// `RandomRanged(256, 768)` leptons from its Location in a random
    /// direction (`MapClass::GetRandomCoordsNear @ 0x0049F420`, cell
    /// centred: one more draw), through `CellClass::IncreaseTiberium`.
    pub(crate) fn ares_destroy_refinery_storage(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let owner = entity.owner();
        let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let spill = self
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.techno_combat().tiberium().tiberium_spill());
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        let removed = house
            .economy_mut()
            .ares_storage_mut()
            .destroy_building_store(id);
        if !spill || removed.iter().all(|&bales| bales <= 0) {
            return;
        }
        let grid = self.cell_grid();
        self.increase_tiberium_cells(rules, rules.overlay_registry(), |context| {
            let mut placements = Vec::new();
            for (slot, bales) in removed.into_iter().enumerate() {
                for _ in 0..bales.max(0) {
                    let distance = context.rng.next_range_i32_inclusive(256, 768);
                    let (x, y) = crate::sim::combat::inviso_scatter::random_direction_coord(
                        grid,
                        context.rng,
                        location.x,
                        location.y,
                        distance,
                        crate::sim::combat::inviso_scatter::RandomDirectionSnap::CellCenter,
                    );
                    let cell = (
                        crate::util::lepton::lepton_to_cell(x)
                            .try_into()
                            .unwrap_or(u16::MAX),
                        crate::util::lepton::lepton_to_cell(y)
                            .try_into()
                            .unwrap_or(u16::MAX),
                    );
                    placements.extend(crate::sim::tiberium::place_tiberium(
                        context,
                        cell,
                        TiberiumTypeId(slot as u8),
                        1,
                    ));
                }
            }
            placements
        });
    }

    /// The sale's store payout (`BuildingClass::Mission_Selling
    /// 0x0044A232..0x0044A285`, after its Limbo): each non-empty slot of
    /// the sold building's store empties into GiveTiberium as
    /// `(float)ftol(amount)`, which Ares makes [`Self::ares_give_tiberium`]:
    /// a storing House moves it into its other silos, as room allows.
    pub(crate) fn ares_sale_storage_payout(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let owner = entity.owner();
        let mut store = TiberiumStore::from_amounts(entity.building_art.take_storage_amounts());
        while let Some(slot) = store.first_non_empty() {
            let removed = store.remove_amount(store.amount(slot), slot);
            let amount = as_float(X87::ftol_i32_low_masked(X87::load_f32(removed)));
            self.ares_give_tiberium(owner, amount, slot, rules);
        }
    }
}

#[cfg(test)]
#[path = "ares_tiberium_storage_tests.rs"]
mod tests;
