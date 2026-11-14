//! L2's read-only projection, scoped to one house: what that house's player
//! sees.
//!
//! The decision layer reads the game only through this type, so its whole
//! information set is one documented surface with one owner. It is a borrow, not
//! a snapshot: nothing is copied or retained, and a decision that needs a value
//! across frames holds the value, not the view.
//!
//! The surface is `pub` and not crate-only because the layer above is the
//! consumer these queries exist for: a `pub(crate)` hook is a hook only this
//! crate's own shape keeps working, and every one of them is a read — none of
//! them moves, writes or decides anything the simulation owns.
//!
//! Each field reads the state's authoritative owner, never a presentation
//! cache. Specifically:
//!
//! - `credits` reads `HouseState::economy`, the single wallet balance, not the
//!   sidebar's smoothed `displayed_credits` (an app-layer presentation value).
//! - `power_*` reads the retained `PowerState` totals, which is what native
//!   queries, not `effective_totals()` (that one applies the trainer's full
//!   power override under `debug-tools`).
//! - `own_entities` reads `GameEntity` identity and health directly. Its
//!   `entity_id` is the entity's stable id, the same id space
//!   `Command::Move { entity_id }` and the other order variants name, so an
//!   agent can hold one across frames.
//! - `is_ground_walkable` reads `Simulation::path_grid`, the current canonical
//!   navigation projection. It is derived from terrain and blocking objects,
//!   and it says nothing about whether a cell is currently occupied: use it
//!   for reachability, not for occupancy.
//! - `owned_by_hand` reads the objects the local player has taken back — the
//!   members of the player's control groups, which live in the local view and
//!   reach this projection through the board (`with_player_groups`) — and the
//!   `TeamClass` scripts of this house. Neither is simulation state the sim
//!   owns: a control group is a promise about a unit the player made, so it
//!   belongs to the player's view, and it is handed in rather than stored.
//!
//! Everything here is integer-valued and iterates in storage order, so a
//! decision built on it stays deterministic.

use crate::map::entities::EntityCategory;
use std::collections::BTreeSet;

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseDifficulty;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::production::ProductionCategory;
use crate::sim::production::{CanBuild, can_build};
use crate::sim::world::Simulation;

/// What [`Observation::engage`] found: a yes, a not-yet, or a no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engage {
    /// A weapon answers the target and it stands inside that weapon's reach.
    Ready,
    /// A weapon answers the target, but it has to be walked to first.
    OutOfRange,
    /// Nothing this unit carries does anything to that one, ever.
    Cannot,
}

/// What one house's agent may look at, and nothing else.
pub struct Observation<'a> {
    sim: &'a Simulation,
    house: InternedId,
    /// The objects the local player has taken back: the members of the
    /// player's control groups, which live in the local view and are handed to
    /// the board each frame. The board's gate refuses a command that names
    /// one, and a policy that can see them refuses to propose one.
    player_kept: &'a BTreeSet<u64>,
}

/// What [`Observation::player_kept`] is for a house with no player behind it:
/// a headless match, a computer house, and every test that never hands the
/// board a control group.
static NO_PLAYER_GROUPS: BTreeSet<u64> = BTreeSet::new();

/// One object the observed house owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnEntity {
    /// The stable id an order command names.
    pub entity_id: u64,
    pub type_id: InternedId,
    pub cell: (u16, u16),
    pub health: i32,
    /// `effective()`: the mission the object is on, or is queued onto, now.
    pub mission: MissionId,
    /// Whether an order may be issued to it now: alive, above zero strength,
    /// out of limbo. The same predicate the order gate applies
    /// (`Simulation::order_actor_admits`), so a caller can skip a doomed
    /// attempt without restating the test.
    pub orderable: bool,
}

/// A finished building of the observed house that is waiting for a placement
/// cell — the product `Command::PlaceReadyBuilding` places. A house can hold
/// one per factory category, so `ready_buildings` yields them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyBuilding {
    pub category: ProductionCategory,
    pub type_id: InternedId,
    /// The completed object, once one exists: the factory's pending product
    /// names its type before the object is built.
    pub entity_id: Option<u64>,
}

/// An enemy object a policy may aim an order at: the id `Command::Attack`
/// names as its target, and where it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyEntity {
    pub entity_id: u64,
    pub type_id: InternedId,
    pub cell: (u16, u16),
}

/// One shot in the air that names an object, as that object reads it.
///
/// The three facts that decide whether a step aside is worth anything: where
/// the shot is committed to land, how many frames it has left, and whether it
/// follows the object. Splash is the warhead's, resolved by the caller against
/// the rules ([`IncomingShot::warhead`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncomingShot {
    pub projectile_id: u64,
    pub source_id: u64,
    /// Where the shot will land, in cells: the target position captured at
    /// fire for an unguided bullet, the live position for a guided one.
    pub impact: (i16, i16),
    /// Frames to that point along a straight line, rounded up. Zero for a
    /// shot that has already arrived (`Inviso=` bullets are placed on the
    /// target and land the same frame).
    pub frames: u32,
    /// `BulletType ROT >= 1`: the shot re-reads the live position every frame,
    /// so leaving the impact cell changes nothing.
    pub homing: bool,
    pub warhead: InternedId,
    /// `(+0x150 * +0x6C) >> 8`, the damage this impact would apply.
    pub damage: i32,
}

/// One production line's occupancy: what its factory holds, whether that build
/// is suspended, and how many copies are queued behind it.
///
/// A decision that has a ledger of the house's assets reads a line's load here
/// instead of asking the factory four times; the fields are the same ones
/// [`Observation::factory_product`] and [`Observation::finished_product`] read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactoryLine {
    /// The type being built, if any.
    pub product: Option<InternedId>,
    /// Held by the player (`FactoryClass`'s suspension) rather than running.
    pub suspended: bool,
    /// Copies queued behind the product.
    pub queued: usize,
}

/// A building that takes infantry and has room for more: the garrison click's
/// target. `room` is the carrier-side count, not a rules number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Billet {
    pub building_id: u64,
    pub cell: (u16, u16),
    pub room: u32,
}

impl<'a> Observation<'a> {
    pub fn new(sim: &'a Simulation, house: InternedId) -> Self {
        Self {
            sim,
            house,
            player_kept: &NO_PLAYER_GROUPS,
        }
    }

    /// The same view, over a house whose player has taken some of its objects
    /// back. The board calls this with the control groups it was handed;
    /// without it the view is a headless one and nothing is kept.
    ///
    /// Public, like the rest of the view: the decision layer above reads the
    /// same observation the board builds, and a `pub(crate)` hook is a hook
    /// only the crate's own luck keeps working.
    pub fn with_player_groups(mut self, groups: &'a BTreeSet<u64>) -> Self {
        self.player_kept = groups;
        self
    }

    /// The house this view is scoped to; the owner every issued command takes.
    pub fn house(&self) -> InternedId {
        self.house
    }

    /// The world behind the projection.
    ///
    /// Crate-internal, and only for another L2 helper that needs the whole
    /// world rather than a projection of it — today the native placement
    /// search (`sim::ai_base_site`), which takes `&Simulation`. A decision
    /// layer must not use it: it would bypass this surface, and
    /// `architecture_guards.rs` fails the tests if the strategy module does.
    pub(crate) fn simulation(&self) -> &'a Simulation {
        self.sim
    }

    /// The tick commands issued now are stamped with.
    pub fn tick(&self) -> u64 {
        self.sim.session().tick()
    }

    /// The house's wallet: `HouseState::economy`, the authoritative balance.
    pub fn credits(&self) -> i32 {
        self.sim
            .houses()
            .get(&self.house)
            .map_or(0, |house| house.economy().credits())
    }

    /// Everything the house has taken in: the wallet plus everything
    /// `Spend_Money` has taken out of it (`HouseClass::Spent_Credits`, House
    /// `+0x2DC`).
    ///
    /// Production charges as it goes, so the wallet alone reads a house that
    /// is building as a house that earns nothing. A decision that asks how
    /// fast the house is earning reads this and takes two samples of it
    /// instead — the difference is income, whatever was paid out meanwhile.
    pub fn earned(&self) -> i32 {
        self.sim.houses().get(&self.house).map_or(0, |house| {
            let economy = house.economy();
            economy.credits().wrapping_add(economy.spent_credits())
        })
    }

    /// Power produced by the house's live, powered buildings.
    pub fn power_output(&self) -> i32 {
        self.power().map_or(0, |(output, _)| output)
    }

    /// Whether `entity_id` is deployed: the state the D key toggles
    /// (`TechnoClass::Is_Deployed`; infantry `Doing` 27..30, which covers the
    /// deploy animation and the deployed pose).
    ///
    /// A decision that tells a soldier to deploy has to know whether it
    /// already has, because the command toggles: sent twice, it walks the
    /// soldier back out of the position it just took.
    pub fn is_deployed(&self, entity_id: u64) -> bool {
        self.sim
            .entities()
            .get(entity_id)
            .is_some_and(|entity| entity.is_deployed())
    }

    /// Whether `rx, ry` is no longer under the house's shroud: the cell has
    /// been revealed to it, or to an ally it shares sight with
    /// (`FogState::is_cell_revealed`, `src/sim/vision/mod.rs:1752`; the grid is
    /// per-house, `mod.rs:686`).
    ///
    /// A scout is sent where the house has not been, and that is the shroud's
    /// own answer. Kept here, next to the rest of the projection, so nothing
    /// that wants to send one reaches into the fog itself.
    pub fn is_cell_revealed(&self, rx: u16, ry: u16) -> bool {
        self.sim.fog().is_cell_revealed(self.house, rx, ry)
    }

    /// Whether the house sees `rx, ry` right now, in line of sight
    /// (`FogState::is_cell_visible`, the L2 layer): revealed is what the house
    /// has seen at some point, this is what it sees this frame.
    pub fn is_cell_visible(&self, rx: u16, ry: u16) -> bool {
        self.sim.fog().is_cell_visible(self.house, rx, ry)
    }

    /// The map's extent in cells, as the fog grid measures it: what a scan of
    /// the map is bounded by.
    ///
    /// This is the **buffer**, not the ground. A policy that asks "how big is this
    /// map" to scale a distance wants [`Self::ground_extent`]: `fog` is padded, and
    /// every map reports the same padded figure. Three changes this session
    /// misread it that way — a scout's far corner landed at `(255, 255)`, and a
    /// far-target threshold became fifty-one cells on two maps whose bases are a
    /// hundred and thirty cells apart.
    pub fn map_extent(&self) -> (u16, u16) {
        (self.sim.fog().width(), self.sim.fog().height())
    }

    /// The walkable ground's extent in cells.
    ///
    /// The map's own declared playfield, which is what a distance meant "relative
    /// to the map" has to be measured against. `None` only when the scenario has
    /// not declared one.
    pub fn ground_extent(&self) -> Option<(u16, u16)> {
        let bounds = self.sim.playfield_bounds()?;
        let width = u16::try_from(bounds.off_104).ok()?;
        let height = u16::try_from(bounds.off_108).ok()?;
        Some((width, height))
    }

    /// Credits of damage this house's objects have dealt, by attacker type name.
    ///
    /// The other half of what a unit is worth: `own_entities` and the price say
    /// what an army costs, and this says what it bought. A two-thousand credit
    /// building knocked from full to nine tenths pays two hundred to whatever
    /// knocked it, so a unit that does most of the work is paid even when it never
    /// lands the last hit.
    ///
    /// Empty for a house that has not dealt damage yet, and VERA-only: gamemd
    /// keeps no such ledger, so nothing in play reads it.
    pub fn damage_value(&self) -> std::collections::BTreeMap<String, i64> {
        let mut out = std::collections::BTreeMap::new();
        let Some(house) = self.sim.houses().get(&self.house) else {
            return out;
        };
        for (type_id, value) in house.tracking().damage_value() {
            out.insert(self.sim.interner().resolve(*type_id).to_string(), *value);
        }
        out
    }

    /// Whether `rx, ry` is ground a unit can stand on, rather than the fog grid's
    /// frame or the map buffer's padding.
    ///
    /// The two extents are not the same thing. The fog grid covers the whole map
    /// buffer — 256x256 on Death.mmx — while the playfield a unit may walk on is
    /// smaller and offset inside it. A destination taken from the fog grid alone
    /// is a cell the pathfinder can never reach: three tanks ordered to
    /// `Cell((0, 0))` stood there for the rest of the match, and the same at
    /// `Cell((16, 16))`. A decision that sends a unit somewhere asks this first.
    ///
    /// `true` when the simulation has no map authority at all, so a fixture
    /// without a playfield keeps its old behaviour instead of refusing every
    /// destination.
    pub fn is_cell_in_playfield(&self, rx: u16, ry: u16) -> bool {
        let Some(bounds) = self.sim.playfield_bounds() else {
            return true;
        };
        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(rx), i32::from(ry)),
            Some(bounds),
            self.sim.resolved_terrain.as_ref(),
        )
    }

    /// Power the house's buildings consume.
    pub fn power_drain(&self) -> i32 {
        self.power().map_or(0, |(_, drain)| drain)
    }

    /// Output minus drain: negative means a brownout.
    pub fn power_balance(&self) -> i32 {
        let (output, drain) = self.power().unwrap_or((0, 0));
        output.wrapping_sub(drain)
    }

    fn power(&self) -> Option<(i32, i32)> {
        let state = self.sim.power_states().get(&self.house)?;
        Some((state.total_output(), state.total_drain()))
    }

    /// The house's own objects, in entity-storage order.
    ///
    /// The store also holds objects that are in limbo, riding in a transport
    /// or awaiting deletion, so "mine" does not imply "orderable": the host's
    /// gate applies `order_actor_admits` before a command is queued.
    pub fn own_entities(&self) -> impl Iterator<Item = OwnEntity> + '_ {
        let house = self.house;
        self.sim
            .entities()
            .values()
            .filter(move |entity| entity.owner() == house)
            .map(|entity| OwnEntity {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell: (entity.position.rx(), entity.position.ry()),
                health: entity.health.current(),
                mission: entity.mission.effective(),
                orderable: self.sim.order_actor_admits(entity.stable_id()),
            })
    }

    /// Where one object stands and what it is, by id.
    ///
    /// The order gate asks this to reach the object's movement zone, which is
    /// the key the zone map answers a destination by; `own_entities` walks the
    /// whole store, and a gate that runs once per order cannot.
    pub fn object_cell_and_type(&self, entity_id: u64) -> Option<((u16, u16), InternedId)> {
        let entity = self.sim.entities().get(entity_id)?;
        Some((
            (entity.position.rx(), entity.position.ry()),
            entity.type_ref(),
        ))
    }

    /// How many passengers the object carries. Zero for anything that is not a
    /// transport, and for a transport standing empty.
    ///
    /// The policy asks this before it loads or unloads: a transport with no
    /// room is not a destination, and a transport that is already full is not
    /// one either.
    pub fn passenger_count(&self, entity_id: u64) -> u32 {
        self.sim
            .entities()
            .get(entity_id)
            .map_or(0, |entity| entity.passenger_role.passenger_count())
    }

    /// The transport this object is inside, if any.
    ///
    /// A unit inside one is not on the map to be ordered; the policy reads this
    /// so a loaded passenger is not sent a second job every frame.
    pub fn inside_transport(&self, entity_id: u64) -> Option<u64> {
        self.sim
            .entities()
            .get(entity_id)
            .and_then(|entity| entity.passenger_role.inside_transport_id())
    }

    /// The bales in a harvester's hold.
    ///
    /// A harvester that is filling its hold changes this every frame; one that
    /// is standing still with a full hold does not. That difference is the
    /// only way a policy can tell a truck at work from a truck that stopped,
    /// and it is the engine's own number, not an opinion.
    pub fn miner_cargo_bales(&self, entity_id: u64) -> Option<u32> {
        self.sim
            .entities()
            .get(entity_id)
            .and_then(|entity| entity.miner.as_ref())
            .map(|miner| miner.cargo().len() as u32)
    }

    /// Whether a harvester's hold is full: it should be on its way to a
    /// refinery, not standing where it filled.
    pub fn miner_cargo_full(&self, entity_id: u64) -> Option<bool> {
        self.sim
            .entities()
            .get(entity_id)
            .and_then(|entity| entity.miner.as_ref())
            .map(|miner| miner.is_full())
    }

    /// Full strength of a type, from the frozen rules an object was built with.
    pub fn max_health(&self, rules: &RuleSet, type_id: InternedId) -> Option<i32> {
        self.sim.object_type(type_id, rules).map(|ty| ty.strength)
    }

    /// Current strength of any object by stable id, the house's or an enemy's.
    ///
    /// The projection already carries health for the house's own objects
    /// (`OwnEntity::health`). An assault job needs the same fact about the thing
    /// it is attacking: whether the attack is doing anything is a question about
    /// the target's health, and the design's `Ineffective` outcome is exactly
    /// that question (`design.md`, section 4). A dead or missing object answers
    /// `None`, which is also how a caller tells "finished" from "not yet".
    pub fn health(&self, entity_id: u64) -> Option<i32> {
        self.sim
            .entities()
            .get(entity_id)
            .filter(|entity| entity.health.current() > 0)
            .map(|entity| entity.health.current())
    }

    /// Finished buildings waiting to be placed on the map.
    pub fn ready_buildings(&self) -> impl Iterator<Item = ReadyBuilding> + '_ {
        let house = self.house;
        [ProductionCategory::Building, ProductionCategory::Defense]
            .into_iter()
            .filter_map(move |category| {
                let pending = self
                    .sim
                    .production()
                    .factories()
                    .view(house, category)?
                    .complete_object()?;
                Some(ReadyBuilding {
                    category,
                    type_id: pending.type_id,
                    entity_id: pending.entity_id,
                })
            })
    }

    /// Whether the factory that makes this type still holds it: the object
    /// being built, or a copy queued behind it. `false` once a building's
    /// product waits for placement, which [`Self::ready_buildings`] reports.
    pub fn in_production(&self, rules: &RuleSet, type_id: InternedId) -> bool {
        let Some(ty) = self.sim.object_type(type_id, rules) else {
            return false;
        };
        let category = crate::sim::production::category_for_object(ty);
        let Some(factory) = self.sim.production().factories().view(self.house, category) else {
            return false;
        };
        factory
            .object
            .is_some_and(|object| object.type_id == type_id)
            || factory.queue.iter().any(|entry| entry.type_id == type_id)
    }

    /// How far the factory has got on this type, in production steps — the
    /// progress bar the sidebar draws. `None` when the factory does not hold
    /// this type.
    pub fn production_progress(&self, rules: &RuleSet, type_id: InternedId) -> Option<u16> {
        let ty = self.sim.object_type(type_id, rules)?;
        let category = crate::sim::production::category_for_object(ty);
        let factory = self
            .sim
            .production()
            .factories()
            .view(self.house, category)?;
        let object = factory.object?;
        (object.type_id == type_id).then_some(factory.progress)
    }

    /// The type the building factory holds right now, and whether its build is
    /// on hold (`FactoryClass`'s suspension).
    ///
    /// A budget that puts the yard on hold needs the type to resume it:
    /// `HouseClass::Begin_Production` resumes a suspended product and would
    /// queue a second copy of anything else.
    pub fn building_in_production(&self) -> Option<(InternedId, bool)> {
        self.factory_product(ProductionCategory::Building)
    }

    /// The type the house's factory of `category` holds right now, and whether
    /// its build is on hold — `None` when the factory is idle or the house has
    /// none of that category.
    pub fn factory_product(&self, category: ProductionCategory) -> Option<(InternedId, bool)> {
        let factory = self
            .sim
            .production()
            .factories()
            .view(self.house, category)?;
        factory
            .object
            .map(|object| (object.type_id, factory.suspended))
    }

    /// What the house's factory of `category` holds right now, in one read.
    ///
    /// `None` when the house has no factory of that category. This is the
    /// per-category form of [`Self::factory_product`] plus the queue length,
    /// for a caller keeping a ledger of every line rather than asking about one
    /// type: the answers are the factory's own fields, not a second rule.
    pub fn factory_line(&self, category: ProductionCategory) -> Option<FactoryLine> {
        let factory = self
            .sim
            .production()
            .factories()
            .view(self.house, category)?;
        Some(FactoryLine {
            product: factory.object.map(|object| object.type_id),
            suspended: factory.suspended,
            queued: factory.queue.len(),
        })
    }

    /// Enemy objects an order may name as its target, in storage order.
    ///
    /// Natively the computer AI is omniscient, and its teams pick targets from
    /// the whole map; a policy reusing its decisions inherits that. Fog honesty
    /// is a recorded open question, not a decision this accessor makes.
    ///
    /// Enmity is the House ally graph's complement (`sim::house_diplomacy`,
    /// `HouseClass+0x5788`): an ally is never a target, and neither is a
    /// `MultiplayPassive` house — the neutral and civilian houses that own the
    /// map's tech buildings, garrisonable houses and parked cars, which the
    /// computer AI leaves alone and turns on only when they are made enemies.
    pub fn enemy_entities(&self) -> impl Iterator<Item = EnemyEntity> + '_ {
        let house = self.house;
        let houses = self.sim.houses();
        let allied = crate::map::houses::ally_test(
            self.sim.house_alliances.as_map(),
            self.sim.interner().resolve(house),
        );
        self.sim.entities().values().filter_map(move |entity| {
            let owner = entity.owner();
            if owner == house || entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
                return None;
            }
            let target = houses.get(&owner)?;
            if target.multiplay_passive() || allied(self.sim.interner().resolve(owner)) {
                return None;
            }
            Some(EnemyEntity {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell: (entity.position.rx(), entity.position.ry()),
            })
        })
    }

    /// The enemy objects [`Self::enemy_entities`] returns narrowed to what the
    /// house sees this frame (`FogState::is_cell_visible`, the L2 layer).
    ///
    /// The list above is the omniscient read the native computer AI uses, and the
    /// spec forbids a policy from aiming at what the player could not have
    /// clicked (spec 2.5, "no real-time enemy information outside vision"). A
    /// policy that wants a legal target asks for this one; memory of what has
    /// been seen (L1) is a further step, not this accessor's business.
    pub fn visible_enemy_entities(&self) -> impl Iterator<Item = EnemyEntity> + '_ {
        let house = self.house;
        let fog = self.sim.fog();
        self.enemy_entities()
            .filter(move |enemy| fog.is_cell_visible(house, enemy.cell.0, enemy.cell.1))
    }

    /// Buildings owned by a `MultiplayPassive` house — the neutral and civilian
    /// structures that stand on the map and belong to nobody, in storage order.
    ///
    /// Kept apart from [`Self::enemy_entities`], which excludes them on purpose:
    /// the computer AI does not go to war with the map. It does shoot what
    /// stands in its way, and what stands in its way is exactly this: a
    /// building that is neither the house's nor an enemy's, so no target list
    /// holds it. The caller decides what counts as in the way; this only says
    /// which objects are nobody's.
    pub fn neutral_structures(&self) -> impl Iterator<Item = EnemyEntity> + '_ {
        let houses = self.sim.houses();
        self.sim.entities().values().filter_map(move |entity| {
            if entity.lifecycle.in_limbo()
                || entity.health.current() <= 0
                || entity.category() != EntityCategory::Structure
            {
                return None;
            }
            let owner = entity.owner();
            if owner == self.house {
                return None;
            }
            let house = houses.get(&owner)?;
            if !house.multiplay_passive() {
                return None;
            }
            Some(EnemyEntity {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell: (entity.position.rx(), entity.position.ry()),
            })
        })
    }

    /// Every cell a live ground object stands on — units, infantry and the whole
    /// foundation of every building.
    ///
    /// A plan that names a cell for a new object has to know this before it
    /// names it: the placement is refused at the other end while anything
    /// stands there, and a plan that keeps naming the same occupied cell never
    /// finishes — which is what a wall ring looked like when a rifleman
    /// happened to be standing on it.
    pub fn ground_object_cells(&self, rules: &RuleSet) -> std::collections::BTreeSet<(u16, u16)> {
        let mut cells = std::collections::BTreeSet::new();
        for entity in self.sim.entities().values() {
            if entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
                continue;
            }
            let (rx, ry) = (entity.position.rx(), entity.position.ry());
            let foundation = self
                .sim
                .object_type(entity.type_ref(), rules)
                .map_or((1u16, 1u16), |ty| ty.foundation.dimensions());
            for dy in 0..i32::from(foundation.1.max(1)) {
                for dx in 0..i32::from(foundation.0.max(1)) {
                    let (Ok(x), Ok(y)) = (
                        u16::try_from(i32::from(rx) + dx),
                        u16::try_from(i32::from(ry) + dy),
                    ) else {
                        continue;
                    };
                    cells.insert((x, y));
                }
            }
        }
        cells
    }

    /// Every shot in the air that names `entity_id`, in projectile order.
    ///
    /// What a player sees as a shell crossing the screen is here as the three
    /// facts that decide whether stepping aside is worth anything: where the
    /// shot is committed to land, how many frames it needs to get there, and
    /// whether it follows the object or its launch-time aim. A homing bullet
    /// (`BulletType ROT >= 1`, `tracks_target`) re-reads the live position, so
    /// walking out of it does nothing; a plain one keeps the point it was fired
    /// at (`BulletClass::Fire 0x00468A80`, `initial_target_position`), which is
    /// why a target that has stepped away is not there when it lands.
    ///
    /// An `Inviso=` bullet is placed on its target at fire time with no speed
    /// and detonates the same frame: `frames` is zero and nothing can be done
    /// about it. That is most small-arms fire, and it is not a gap — it is what
    /// the bullet type says.
    pub fn incoming_shots(&self, entity_id: u64) -> impl Iterator<Item = IncomingShot> + '_ {
        self.shots_in_flight()
            .filter(move |(target, _)| *target == entity_id)
            .map(|(_, shot)| shot)
    }

    /// Every shot in the air, with the object it names, in projectile order.
    ///
    /// The whole-frame form of [`Self::incoming_shots`], for a pass that asks
    /// about the whole army once: one walk over the projectiles instead of one
    /// walk per unit.
    pub fn shots_in_flight(&self) -> impl Iterator<Item = (u64, IncomingShot)> + '_ {
        let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
        self.sim
            .projectiles()
            .iter()
            .filter_map(move |(id, projectile)| {
                let target = projectile.target().object_id()?;
                let aim = projectile.last_target_position();
                let speed = i32::from(projectile.speed_leptons_per_frame()).max(0);
                let dx = aim.x - projectile.position().x;
                let dy = aim.y - projectile.position().y;
                let distance = isqrt(dx * dx + dy * dy);
                let frames = if speed <= 0 {
                    0
                } else {
                    u32::try_from((distance + speed - 1) / speed).unwrap_or(u32::MAX)
                };
                let payload = projectile.payload();
                Some((
                    target,
                    IncomingShot {
                        projectile_id: *id,
                        source_id: projectile.source_id(),
                        impact: (
                            i16::try_from(aim.x.div_euclid(per_cell)).unwrap_or(0),
                            i16::try_from(aim.y.div_euclid(per_cell)).unwrap_or(0),
                        ),
                        frames,
                        homing: projectile.tracks_target(),
                        warhead: payload.warhead,
                        damage: payload.area_damage(),
                    },
                ))
            })
    }

    /// Whether `entity_id` is still something this house may shoot at.
    ///
    /// The question an order door has to ask before it lets an attack stand:
    /// ownership changes under a running attack, and an engineer, a mind
    /// control or a capture can turn the thing being shot at into the house's
    /// own building while the gunner never re-reads it.
    ///
    /// True only when the object exists, is alive, is not this house's, is not
    /// this house's ally's, and does not belong to a `MultiplayPassive=` house
    /// — the same three exclusions [`Self::enemy_entities`] applies, so an
    /// order that survives here is one the targeting layer would also have
    /// offered. Reading it per frame is the point: a cached answer is exactly
    /// the stale target this exists to catch.
    pub fn is_hostile(&self, entity_id: u64) -> bool {
        let Some(entity) = self.sim.entities().get(entity_id) else {
            return false;
        };
        if entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
            return false;
        }
        let owner = entity.owner();
        if owner == self.house {
            return false;
        }
        let Some(house) = self.sim.houses().get(&owner) else {
            return false;
        };
        if house.multiplay_passive() {
            return false;
        }
        let allied = crate::map::houses::ally_test(
            self.sim.house_alliances.as_map(),
            self.sim.interner().resolve(self.house),
        );
        !allied(self.sim.interner().resolve(owner))
    }

    /// How many houses this house is at war with: alive, not itself, not an
    /// ally, not a `MultiplayPassive=` bystander.
    ///
    /// The denominator a superweapon reading needs. A count of strike sites says
    /// nothing by itself — twenty-nine of them across twenty-nine houses is one
    /// per opponent, and one against one house is the same war — so any rule
    /// that compares the two divides by this. It is read from the houses rather
    /// than fixed, because a mod decides how many there are.
    pub fn enemy_house_count(&self) -> usize {
        let allied = crate::map::houses::ally_test(
            self.sim.house_alliances.as_map(),
            self.sim.interner().resolve(self.house),
        );
        self.sim
            .houses()
            .iter()
            .filter(|(house, state)| {
                **house != self.house
                    && !state.is_defeated()
                    && !state.multiplay_passive()
                    && !allied(self.sim.interner().resolve(**house))
            })
            .count()
    }

    /// The object `entity_id` is shooting at, when it has one.
    ///
    /// The native mission AI picks it (`TechnoClass::Greatest_Threat` behind
    /// the fire loop), and a brain that wants to move a unit around a fight
    /// reads the choice here rather than picking a second one: the unit is
    /// already aimed at this, and a second opinion is what makes a soldier
    /// turn away from what it is shooting.
    ///
    /// It answers *what the gun is pointed at*, not whether that is still an
    /// enemy: the object can change hands under the order, and
    /// [`Self::is_hostile`] is the second half of the question.
    pub fn attack_target(&self, entity_id: u64) -> Option<u64> {
        match self.sim.entities().get(entity_id)?.attack_target.target() {
            Some(crate::sim::combat::TargetKind::Entity(id)) => Some(id),
            _ => None,
        }
    }

    /// Where `entity_id` is walking to, in cells, when it is walking; `None`
    /// when it stands still or its nav target is an object rather than a cell.
    ///
    /// The order door asks this before it re-sends a march: `Set_Destination`
    /// re-runs the path, so a destination a unit already holds is not one to
    /// send again — a path re-run every frame is what drift looks like.
    pub fn destination(&self, entity_id: u64) -> Option<(u16, u16)> {
        match self.sim.entities().get(entity_id)?.navigation.nav_com()? {
            crate::sim::components::NavTargetRef::Cell { rx, ry } => Some((rx, ry)),
            crate::sim::components::NavTargetRef::Object { .. } => None,
        }
    }

    /// Whether `entity_id`'s weapon may fire this frame: the rearm timer has
    /// run out (`TechnoClass+0x2E4`, `sim::combat::rof`).
    ///
    /// The reload is the whole clock of a fire-and-move cycle: a unit steps out
    /// while it is reloading and back in when it is not, so the answer is read
    /// per frame rather than remembered.
    pub fn weapon_ready(&self, entity_id: u64) -> bool {
        let frame = self.sim.session().binary_frame() as i32;
        self.sim
            .entities()
            .get(entity_id)
            .is_some_and(|entity| entity.rearm_timer.remaining(frame) <= 0)
    }

    /// Whether `entity_id` has emptied its rack: it carries an ammo count and
    /// that count is zero.
    ///
    /// Only a type with `Ammo=` has one, and retail sets the key on aircraft
    /// (`[ORCA]`, `[ASW]`, `[BPLN]`, `[SPYP]`, `[CAAIRP]`) — the rules' own
    /// note is "aircraft are hard wired to require ammo". A negative count is
    /// the rules' "unlimited", so the test is exactly zero.
    ///
    /// The answer a caller needs is not "cannot shoot right now": it is that
    /// the object has its own way home to reload, and an `Attack` issued over
    /// that is the take-off, turnaround, take-off loop a play test reports.
    pub fn out_of_ammo(&self, entity_id: u64) -> bool {
        self.sim
            .entities()
            .get(entity_id)
            .and_then(|entity| entity.ammo())
            .is_some_and(|ammo| ammo.current() == 0)
    }

    /// The blast radius of a warhead, in whole cells: `CellSpread=` floored, so
    /// a `0.4` shell hurts only the cell it lands in and a `2.0` one reaches two
    /// cells out.
    pub fn warhead_spread_cells(&self, rules: &RuleSet, warhead: InternedId) -> i32 {
        rules
            .warhead(self.sim.interner().resolve(warhead))
            .map_or(0, |warhead| warhead.cell_spread_f64.floor().max(0.0) as i32)
    }

    /// Candidate buildings for an engineer: alive, not this house's or its
    /// ally's, and `Capturable=`. Neutral tech buildings and civilian
    /// structures qualify, which is exactly the difference between capturing
    /// one and shooting it.
    ///
    /// A candidate list, not the click's answer. Whether one engineer may take
    /// one building is [`Self::can_take_building`]: an ally's building is a
    /// Repair click, and one whose type answers no action at all is no click.
    /// An ally is left out here because no engineer ever captures one — the
    /// native arm answers Repair before it looks at `Capturable=`.
    pub fn capturable_buildings<'r>(
        &'r self,
        rules: &'r RuleSet,
    ) -> impl Iterator<Item = EnemyEntity> + 'r {
        let house = self.house;
        let allied = crate::map::houses::ally_test(
            self.sim.house_alliances.as_map(),
            self.sim.interner().resolve(house),
        );
        self.sim.entities().values().filter_map(move |entity| {
            let owner = entity.owner();
            if owner == house || entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
                return None;
            }
            if allied(self.sim.interner().resolve(owner)) {
                return None;
            }
            let ty = self.sim.object_type(entity.type_ref(), rules)?;
            if !ty.capturable || ty.category != ObjectCategory::Building {
                return None;
            }
            Some(EnemyEntity {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell: (entity.position.rx(), entity.position.ry()),
            })
        })
    }

    /// Whether `actor` may be told to take `building`: the click's own answer,
    /// from the three `InfantryClass::What_Action_OnObject` arms that answer
    /// action 9 (`Capture`).
    ///
    /// An engineer's arm ([`Simulation::engineer_building_action`]) answers
    /// Capture — or the Damage it does to a building under
    /// `EngineerCaptureLevel`, which is the same click's other outcome — only
    /// for a building its house is not allied to, that answers an action at all
    /// (`is_1x1_with_undeploy`, `Repairable=`) and carries `Capturable=`. A
    /// spy's arm ([`Simulation::infiltrator_building_action`]) answers for the
    /// `Infiltrate=` flags. A rifleman's arm is the garrison: `Foot
    /// Class::ClickedAction` (`0x004D7716..0x004D773A`) sends the same
    /// `Capture8` for an `Occupier=` infantryman over a `CanBeOccupied=`
    /// building, and [`Self::can_garrison`] is `CanBeOccupiedBy`'s answer.
    ///
    /// Everything else — an ally's plant, an MCV-type hulk, a
    /// `Capturable=no` structure — is an order the object refuses on arrival:
    /// the specialist walks the whole way there and stands at the wall, which
    /// reads as a stuck unit and is not one.
    pub fn can_take_building(&self, rules: &RuleSet, actor: u64, building: u64) -> bool {
        use crate::sim::world::EngineerBuildingAction;
        match self.sim.engineer_building_action(actor, building, rules) {
            Some(EngineerBuildingAction::Capture | EngineerBuildingAction::Damage) => return true,
            // A `BridgeRepairHut=` whose span is down is the same Capture click
            // and the same consumed engineer: the hut arm answers Repair, and
            // only while there is a span to put back ([`Self::bridge_repair_huts`]).
            Some(EngineerBuildingAction::Repair(_)) => {
                if self
                    .bridge_repair_huts(rules)
                    .any(|site| site.building_id == building)
                {
                    return true;
                }
            }
            _ => {}
        }
        self.sim.infiltrator_building_action(actor, building, rules)
            || crate::sim::passenger::building_can_be_occupied_by(self.sim, rules, building, actor)
    }

    /// Buildings a spy may walk into: alive, not this house's, and `Spyable=`.
    ///
    /// `InfantryClass::What_Action_OnObject`'s infiltration arm
    /// (`0x0051EE3B..0x0051EEEF`) turns a spy's Attack into Capture only over a
    /// `Spyable=` building — `Capturable=` is the flag every other infiltrator
    /// asks for — so the engineer's list is the wrong one for a spy, and a spy
    /// sent at a merely-`Capturable=` building walks into nothing.
    ///
    /// Enmity is [`Self::enemy_entities`]' own: an ally's building and a
    /// `MultiplayPassive` house's are both left out, because there is no
    /// treasury behind either to steal from.
    pub fn spyable_buildings<'r>(
        &'r self,
        rules: &'r RuleSet,
    ) -> impl Iterator<Item = EnemyEntity> + 'r {
        let house = self.house;
        let houses = self.sim.houses();
        let allied = crate::map::houses::ally_test(
            self.sim.house_alliances.as_map(),
            self.sim.interner().resolve(house),
        );
        self.sim.entities().values().filter_map(move |entity| {
            let owner = entity.owner();
            if owner == house || entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
                return None;
            }
            let target = houses.get(&owner)?;
            if target.multiplay_passive() || allied(self.sim.interner().resolve(owner)) {
                return None;
            }
            let ty = self.sim.object_type(entity.type_ref(), rules)?;
            if !ty.spyable || ty.category != ObjectCategory::Building {
                return None;
            }
            Some(EnemyEntity {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell: (entity.position.rx(), entity.position.ry()),
            })
        })
    }

    /// Buildings that take infantry and have room for more, whoever owns them.
    ///
    /// Whether a given infantryman may walk into one is not this accessor's
    /// question: `BuildingClass::CanBeOccupiedBy` owns it, and the policy asks
    /// it per candidate. Any owner is listed because that owner also refuses
    /// the ones a house may not enter.
    pub fn occupiable_buildings<'r>(
        &'r self,
        rules: &'r RuleSet,
    ) -> impl Iterator<Item = Billet> + 'r {
        self.sim.entities().values().filter_map(move |entity| {
            if entity.category() != EntityCategory::Structure
                || entity.lifecycle.in_limbo()
                || entity.health.current() <= 0
            {
                return None;
            }
            let ty = self.sim.object_type(entity.type_ref(), rules)?;
            if !ty.can_be_occupied {
                return None;
            }
            // A structure holds garrison occupants, not transport cargo:
            // `passenger_role.cargo()` is the transport's `Passengers=` list and
            // is absent on a building, which would leave every bunker off the
            // list. The room here is `MaxOccupants=` minus who is inside.
            let taken =
                u32::try_from(entity.passenger_role.occupants().count()).unwrap_or(u32::MAX);
            let room = ty.max_number_occupants.saturating_sub(taken);
            if room == 0 {
                return None;
            }
            Some(Billet {
                building_id: entity.stable_id(),
                cell: (entity.position.rx(), entity.position.ry()),
                room,
            })
        })
    }

    /// `BridgeRepairHut=` buildings whose bridge is actually down.
    ///
    /// Walking an engineer into one is the only thing that rebuilds a bridge:
    /// the hut has no timer, no `Capturable=` and `Immune=yes`, so it never
    /// appears in [`Self::capturable_buildings`] and no other command reaches
    /// it. The scan of the damaged span is `bridge_hut_can_repair`'s own;
    /// repeating it here would be a second answer to the same question.
    pub fn bridge_repair_huts<'r>(
        &'r self,
        rules: &'r RuleSet,
    ) -> impl Iterator<Item = Billet> + 'r {
        self.sim.entities().values().filter_map(move |entity| {
            if entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
                return None;
            }
            let ty = self.sim.object_type(entity.type_ref(), rules)?;
            if !ty.bridge_repair_hut {
                return None;
            }
            // `[CABHUT]` is 1x1 (`artmd.ini:4143`), so its cell is the centre
            // the native scan is keyed by.
            let cell = (entity.position.rx(), entity.position.ry());
            if !crate::sim::world::bridge_orchestrator::bridge_hut_can_repair(self.sim, cell) {
                return None;
            }
            Some(Billet {
                building_id: entity.stable_id(),
                cell,
                room: 1,
            })
        })
    }

    /// The cells the map currently holds a crate on.
    ///
    /// The crate is an overlay on the map and the pickup reads that overlay
    /// (`CellClass::PickupCrate`), so the answer is the overlay's: the
    /// authority's slot says where a crate was placed, and the overlay says
    /// whether it is still there. A slot whose overlay is gone -- tiberium over
    /// it, a structure on it -- is not a crate to walk to, and a house that
    /// walks to it walks to nothing.
    ///
    /// Without an overlay grid there is no overlay to read, and the slots are
    /// the whole of what is known.
    pub fn crate_cells(&self, rules: &RuleSet) -> Vec<(u16, u16)> {
        let registry = rules.overlay_registry();
        self.sim
            .crate_authority()
            .slots()
            .iter()
            .filter(|slot| !slot.is_empty())
            .filter_map(|slot| {
                let cell = (
                    u16::try_from(slot.cell_x).ok()?,
                    u16::try_from(slot.cell_y).ok()?,
                );
                let crate_here = match self.sim.overlay_grid.as_ref() {
                    Some(grid) => grid
                        .cell(cell.0, cell.1)
                        .overlay_id
                        .and_then(|id| registry.flags(id))
                        .is_some_and(|flags| flags.crate_type),
                    None => true,
                };
                crate_here.then_some(cell)
            })
            .collect()
    }

    /// Whether `firer` may be sent at `target` at all:
    /// `TechnoClass::GetFireError` (`src/sim/combat/fire_error.rs`), the one
    /// native answer to "can this object do anything to that one".
    ///
    /// Public, and the only door to it, because the brain used to send every
    /// combat unit at the army's one target: a terror drone was ordered to
    /// attack a building its weapon has no answer to, the order was refused
    /// every time, and the refusals ate the frame's whole command budget.
    ///
    /// Reach is the caller's question — a unit out of reach walks — so it is
    /// not part of this answer.
    pub fn can_engage(&self, rules: &RuleSet, firer: u64, target: u64) -> bool {
        !matches!(
            crate::sim::combat::fire_error_world::fire_error(self.sim, rules, firer, target),
            None | Some(crate::sim::combat::fire_error::FireError::Illegal)
                | Some(crate::sim::combat::fire_error::FireError::Cant)
        )
    }

    /// Whether "can this unit do anything to that" is a yes, a not-yet or a no.
    ///
    /// [`Self::can_engage`] answers the first two the same way, and for "may
    /// this unit be sent at that one" that is right: a target out of reach is
    /// still a target. The layer choosing *which* order to send needs the
    /// difference. An `Attack` on a target out of reach is a mission the engine
    /// restarts every time it is re-issued (`Queue_Mission`), so a brain that
    /// re-sends it each frame keeps the unit in the opening state and it never
    /// walks closer.
    pub fn engage(&self, rules: &RuleSet, firer: u64, target: u64) -> Engage {
        match crate::sim::combat::fire_error_world::fire_error(self.sim, rules, firer, target) {
            None
            | Some(crate::sim::combat::fire_error::FireError::Illegal)
            | Some(crate::sim::combat::fire_error::FireError::Cant) => Engage::Cannot,
            // The magazine empty or the turret still turning is "not this
            // instant", and both clear on their own: the question left is only
            // whether the target is inside the weapon's reach.
            _ => match crate::sim::combat::fire_error_world::fire_error_in_range(
                self.sim, rules, firer, target,
            ) {
                Some(crate::sim::combat::fire_error::FireError::Range) => Engage::OutOfRange,
                _ => Engage::Ready,
            },
        }
    }

    /// Whether any weapon `unit` actually fires does anything to `target`: the
    /// armour floor the native target scan applies to a candidate
    /// (`Evaluate_Candidate` `0x006F7D19..0x006F7D4F`), asked of the unit's own
    /// slots.
    ///
    /// [`Self::can_engage`] is not this answer. A dog carries a
    /// `NeverUse=yes` scanner beside its jaw: the jaw's `ParasiteDog` does
    /// nothing to armour, the scanner does, and `GetFireError` therefore admits
    /// a tank that the dog can never hurt. The scan's own rule is the armour
    /// multiplier, so a slot is only counted when it is one the unit would fire
    /// (`NeverUse=` takes the scanner out) and its warhead's Verses against the
    /// target's armour are not zero.
    ///
    /// One owner for the question, in the rules the object is built from: a mod
    /// that gives a dog an anti-tank jaw, or a tank a warhead that cannot dent
    /// another, is answered without another list here.
    ///
    /// An aircraft is the same question asked of a different key. It used to be
    /// answered `true` for every firer, on the grounds that `AA=` is a projectile
    /// flag and high-flying targets are the fire error's business — but the two
    /// callers here are the *targeting* gates (`orders::admits` and `micro`'s
    /// in-reach test), and an unconditional yes is what let a rifleman be told it
    /// could hurt a Harrier. The wishlist's "一群无法对空的单位追着飞机跑".
    ///
    /// The flag read is the projectile's own `AA=`, the same one
    /// `combat_weapon::weapon_class_bits` reads for the sim's threat scan, so a
    /// mod that gives a rifle an AA-capable weapon is answered by the same key
    /// the sim will fire on.
    pub fn can_hurt(&self, rules: &RuleSet, unit: u64, target: u64) -> bool {
        let (Some(unit_entity), Some(target_entity)) = (
            self.sim.entities().get(unit),
            self.sim.entities().get(target),
        ) else {
            return false;
        };
        let air = target_entity.category() == EntityCategory::Aircraft;
        let (Some(unit_type), target_type) = (
            self.sim.object_type(unit_entity.type_ref(), rules),
            if air {
                None
            } else {
                self.sim.object_type(target_entity.type_ref(), rules)
            },
        ) else {
            return false;
        };
        let armour =
            target_type.map_or(crate::rules::armor::ArmorId::default(), |ty| ty.armor_id());
        (0..unit_type.weapon_record_count()).any(|slot| {
            let Some(name) = unit_type.weapon_at(slot) else {
                return false;
            };
            let Some(weapon) = rules.weapon(name) else {
                return false;
            };
            if weapon.never_use {
                return false;
            }
            if air {
                // A missing projectile answers `AG` in the sim's own reading
                // (`combat_weapon::weapon_class_bits`), so it does not answer
                // air here either.
                return rules
                    .weapon_projectile(weapon)
                    .is_some_and(|projectile| projectile.aa);
            }
            let Some(warhead) = weapon
                .warhead
                .as_deref()
                .and_then(|name| rules.warhead(name))
            else {
                return false;
            };
            // The native floor is a masked x87 compare against `0.02f` for
            // acquisition and plain zero for fire; a weapon that does no
            // damage is not a weapon for this target either way.
            f64::from_bits(warhead.interaction(armour).multiplier().bits()) != 0.0
        })
    }

    /// Why `firer` may not fire at `target` right now, when it may not. `None`
    /// when it may, or when either object is gone.
    ///
    /// The brain asks this where "not yet" and "never" are different answers:
    /// an empty magazine means wait for the reload, an `Illegal` means pick
    /// another target.
    pub fn engage_error(
        &self,
        rules: &RuleSet,
        firer: u64,
        target: u64,
    ) -> Option<crate::sim::combat::fire_error::FireError> {
        match crate::sim::combat::fire_error_world::fire_error(self.sim, rules, firer, target) {
            Some(crate::sim::combat::fire_error::FireError::Ok) => None,
            other => other,
        }
    }

    /// Whether `infantry_id` may garrison `building_id`:
    /// `BuildingClass::CanBeOccupiedBy`'s answer for this pair.
    pub fn can_garrison(&self, rules: &RuleSet, infantry_id: u64, building_id: u64) -> bool {
        crate::sim::passenger::building_can_be_occupied_by(
            self.sim,
            rules,
            building_id,
            infantry_id,
        )
    }

    /// How many objects this one is carrying: a transport's cargo, or a
    /// building's garrison. The Transport pilot needs it to tell a loaded
    /// carrier from an empty one.
    pub fn passengers(&self, entity_id: u64) -> usize {
        self.sim.entities().get(entity_id).map_or(0, |entity| {
            entity.passenger_role.occupants().count().max(0) as usize
        })
    }

    /// The rules' type of one object, for the questions that read it directly
    /// rather than through the ledger.
    pub fn entity_type<'r>(&self, rules: &'r RuleSet, entity_id: u64) -> Option<&'r ObjectType> {
        let entity = self.sim.entities().get(entity_id)?;
        self.object_type(rules, entity.type_ref())
    }

    /// Whether `attacker` flattens `target` by driving onto its cell: the
    /// attacker's `Crusher=`/`OmniCrusher=` against the victim's
    /// `ObjectClass::Is_Crushable_By @ 0x005F6CD0` (allies and an Iron
    /// Curtain refused).
    ///
    /// The native approach arm (`UnitClass::Approach_Target`, `0x007414E0`)
    /// is a computer house's — `0x0073856D` turns a human one away — so a
    /// human house's tank only crushes what it actually drives over. A policy
    /// driving that house has to order the walk itself, and this is the two
    /// facts it asks before it does.
    pub fn crushes(&self, rules: &RuleSet, attacker: u64, target: u64) -> bool {
        let Some(ty) = self.entity_type(rules, attacker) else {
            return false;
        };
        (ty.crusher || ty.omni_crusher)
            && self.sim.object_crushable_by_unit(attacker, target, rules)
    }

    /// The rules' type for `type_id`: a policy reads the type's own fields
    /// (foundation, `Adjacent=`, `DeploysInto=`, `BuildConst=` membership).
    pub fn object_type<'r>(
        &self,
        rules: &'r RuleSet,
        type_id: InternedId,
    ) -> Option<&'r ObjectType> {
        self.sim.object_type(type_id, rules)
    }

    /// What the house's own choice has a factory of `category` make next:
    /// `HouseClass::Suggest_New_Object @ 0x004FBD80`, read for a policy driving
    /// a human house. `HouseClass::Update` fills that choice every eight frames
    /// from what the house's forming teams still need (`TeamClass::
    /// Get_Needed_Types`), which is where an engineer, a spy or a transport
    /// comes from — needs no combat ranking states. The native factory reads
    /// the slot itself (`BuildingClass::Factory_AI`), and a human house's
    /// factory never runs that body, so the policy is the reader here.
    ///
    /// Unchecked, as the native reader is: the caller still owns `CanBuild`
    /// and the wallet.
    pub fn chosen_product<'r>(
        &self,
        rules: &'r RuleSet,
        category: ObjectCategory,
    ) -> Option<(&'r ObjectType, InternedId)> {
        let ty =
            crate::sim::ai_base_building::suggested_product(self.sim, rules, self.house, category)?;
        let type_id = self.type_id(&ty.id)?;
        Some((ty, type_id))
    }

    /// Whether the unit `entity_id` could deploy into `yard` where it stands:
    /// the placement test `UnitClass::Deploy` applies (`0x00739422..
    /// 0x00739565`), answered by the receiver's own owner without touching
    /// the unit.
    ///
    /// A brain that presses D on a blocked cell gets nothing back — the
    /// receiver plays `CannotDeployHere`, clears the deploy latch and queues
    /// Guard (`0x007394E0..0x0073957A`) — so the decision "drive somewhere
    /// else first" needs this answer before the order, not after it.
    pub fn deploy_site_admits(&self, rules: &RuleSet, entity_id: u64, yard: &ObjectType) -> bool {
        self.sim.deploy_site_admits(entity_id, rules, yard)
    }

    /// Whether a deploy would land if `entity_id` walked to `cell` first.
    ///
    /// The search for a second base ranks *cells*, and a deploy's origin is
    /// the foundation's corner rather than the unit's cell, so a cell whose
    /// corner is clear can be a cell whose centre is not. Asked here, the
    /// answer is about the cell the unit would stand on, which is the one the
    /// walk aims at.
    pub fn deploy_admits_at(
        &self,
        rules: &RuleSet,
        entity_id: u64,
        yard: &ObjectType,
        cell: (u16, u16),
    ) -> bool {
        self.sim.deploy_site_admits_at(entity_id, rules, yard, cell)
    }

    /// Whether `HouseClass::Place_Production` would accept `ty` with its
    /// top-left cell here: the sidebar's build-area adjacency and the
    /// building's own `CanPlaceAt` clear-to-build, the two predicates that
    /// Unlimbo asks again after the click. Read-only; walls need the overlay
    /// registry and so are not admitted here.
    pub(crate) fn placement_admits(
        &self,
        rules: &RuleSet,
        ty: &ObjectType,
        rx: u16,
        ry: u16,
    ) -> bool {
        crate::sim::production::is_within_build_area(
            self.sim,
            rules,
            self.sim.interner().resolve(self.house),
            ty,
            rx,
            ry,
        ) && crate::sim::build_site::can_place_building_at(
            self.sim,
            rules,
            None,
            ty,
            (rx as i16, ry as i16),
            Some(self.house),
            None,
            None,
        )
    }

    /// Whether a building of `ty` may stand with its top-left cell here,
    /// ignoring the sidebar's build-area adjacency.
    ///
    /// This is the question an MCV's deploy asks, not the one a factory's
    /// placement asks: a second base is by definition outside the first base's
    /// build area, and `placement_admits` is false for every cell of it. The
    /// clear-to-build predicate is what is left, and it is the same one
    /// `deploy_site_admits` runs for the cell the maker already stands on.
    pub(crate) fn clear_for_building(
        &self,
        rules: &RuleSet,
        ty: &ObjectType,
        rx: u16,
        ry: u16,
    ) -> bool {
        crate::sim::build_site::can_place_building_at(
            self.sim,
            rules,
            None,
            ty,
            (rx as i16, ry as i16),
            Some(self.house),
            None,
            None,
        )
    }

    /// Whether a wall of `ty` may stand with its cell at `(rx, ry)`.
    ///
    /// The same clear-to-build test the placement asks, and the same overlay
    /// registry it asks with: a cell that carries a tiberium overlay refuses a
    /// wall, and a plan that cannot ask that question keeps naming the same
    /// ore cell and losing every wall it builds. The sidebar's build-area
    /// adjacency is deliberately not part of it — a wall is built at the edge
    /// of the base, which is outside that area by definition.
    pub(crate) fn wall_site_admits(
        &self,
        rules: &RuleSet,
        ty: &ObjectType,
        rx: u16,
        ry: u16,
    ) -> bool {
        crate::sim::build_site::can_place_building_at(
            self.sim,
            rules,
            Some(rules.overlay_registry()),
            ty,
            (rx as i16, ry as i16),
            Some(self.house),
            None,
            None,
        )
    }

    /// Whether the ground takes a yard deployed from `cell`.
    ///
    /// The deploy's own origin (the foundation's corner, shifted a cell
    /// north-west on a foundation wider than two) and the clear-to-build test
    /// at that origin: the same question the placement asks, asked about a
    /// cell the maker has not reached yet.
    pub(crate) fn clear_for_deploy(
        &self,
        rules: &RuleSet,
        ty: &ObjectType,
        cell: (u16, u16),
    ) -> bool {
        let origin = crate::sim::world::deploy_origin(cell, ty);
        crate::sim::build_site::can_place_building_at(
            self.sim,
            rules,
            Some(rules.overlay_registry()),
            ty,
            origin,
            Some(self.house),
            None,
            None,
        )
    }

    /// Whether a hand other than this policy already owns `entity_id`, so the
    /// policy orders it nothing at all: a unit in one of the player's control
    /// groups, or a unit a `TeamClass` script of this house has recruited.
    ///
    /// Both are owners the policy would otherwise fight. A control group is a
    /// promise that the unit is still where the player put it, and the local
    /// view keeps it. A team's script owns its members: `team_script_vm`'s
    /// pass has no house gate, so a policy-driven house's teams run like any
    /// other's — recruit the house's units and drive them.
    ///
    /// What this buys over leaving it to the gate: a refused command still
    /// spends the frame's budget and still counts the unit as sent for
    /// `ORDER_REFRESH_FRAMES`, so a grouped unit would quietly eat the army's
    /// orders and hold its own place in the push while doing neither. Every
    /// place the policy picks a unit to order asks this first.
    pub fn owned_by_hand(&self, entity_id: u64) -> bool {
        self.player_kept.contains(&entity_id) || self.team_has_member(entity_id)
    }

    /// Whether a team of this house has recruited `entity_id`.
    ///
    /// A team's script owns its members: `team_script_vm`'s pass has no house
    /// gate, so a policy-driven house's teams run like any other's — recruit
    /// the house's units and drive them. The policy does not order over a
    /// member, or two issuers would give the same unit its next step.
    pub fn team_has_member(&self, entity_id: u64) -> bool {
        self.sim
            .team_script_vm
            .team_for_member(entity_id)
            .is_some_and(|(team_id, _)| {
                self.sim
                    .team_script_vm
                    .team(team_id)
                    .is_some_and(|team| team.owner() == self.house)
            })
    }

    /// The house's country, spelled the way the rules name it. `None` when the
    /// house has none or the id is unknown.
    pub fn country(&self) -> Option<String> {
        let id = self.sim.houses().get(&self.house)?.country()?;
        Some(self.sim.interner().resolve(id).to_owned())
    }

    /// The house's side index, as the rules carry it.
    pub fn side_index(&self) -> Option<i32> {
        self.sim
            .houses()
            .get(&self.house)
            .map(|house| house.side_index())
    }

    pub fn difficulty(&self) -> Option<HouseDifficulty> {
        self.sim
            .houses()
            .get(&self.house)
            .map(|house| house.difficulty())
    }

    pub fn tech_level(&self) -> Option<i32> {
        self.sim
            .houses()
            .get(&self.house)
            .map(|house| house.tech_level())
    }

    /// `[GameOptions] SuperWeapons=`, which the native Recalc reads.
    pub fn super_weapons(&self) -> bool {
        self.sim.session().game_options().super_weapons
    }

    /// The house's own supers that may be fired now, in the rules'
    /// `[SuperWeaponTypes]` order: the buttons a sidebar would light up.
    ///
    /// `HouseClass` holds one Super per entry from its constructor; a button is
    /// live when it is granted and charged, which is
    /// [`SuperWeaponInstance::ready_to_fire`] — the same pair `ClickFire` tests
    /// before it launches. The list is the house's, not the rules': a type the
    /// house was never granted is not in it.
    pub fn ready_super_weapons(&self, rules: &RuleSet) -> Vec<InternedId> {
        let Some(held) = self.sim.super_weapons().get(&self.house) else {
            return Vec::new();
        };
        rules
            .super_weapon_order
            .iter()
            .filter_map(|name| {
                let super_weapon = rules.super_weapon(name)?;
                let type_id = self.sim.interner().get(name)?;
                held.get(&type_id)
                    .filter(|instance| instance.ready_to_fire(super_weapon))
                    .map(|_| type_id)
            })
            .collect()
    }

    /// The id the rules registered a type under, for a policy holding a name.
    pub fn type_id(&self, name: &str) -> Option<InternedId> {
        self.sim.interner().get(name)
    }

    /// `HouseClass::CanBuild` for this house, over the same inputs production
    /// uses. `count_in_production` decides whether a type already queued counts
    /// against the limit, which is what a policy wants when it is about to ask
    /// for another one.
    ///
    /// This is the production path's own test, read through the projection: a
    /// decision asks the same question the sidebar's build button answers, so
    /// it cannot talk itself into a build the house would refuse, and it needs
    /// no second copy of the rules it checks (tech level, prerequisites,
    /// owner limits, in-production counts).
    pub fn can_build(
        &self,
        rules: &RuleSet,
        object: &ObjectType,
        count_in_production: bool,
    ) -> CanBuild {
        can_build(
            self.sim,
            rules,
            self.house,
            object,
            false,
            count_in_production,
        )
    }

    /// The finished product `category`'s factory still holds: the cameo a
    /// player's sidebar shows as done.
    ///
    /// A factory does not hand its object over on its own. The house has to
    /// take it — the click on the done cameo, `Place_Production` — and until
    /// it does the object is built, paid for and counted, the category starts
    /// nothing else, and the house's money buys nothing new there. A decision
    /// layer that queues builds and never takes them stalls itself.
    pub fn finished_product(&self, category: ProductionCategory) -> Option<InternedId> {
        self.sim
            .production()
            .factories()
            .view(self.house, category)?
            .complete_object()
            .map(|object| object.type_id)
    }

    /// What the house pays for one `object` now: `HouseClass::Cost_Of`, the
    /// price production charges when it starts the build.
    ///
    /// Read from the simulation rather than from the rules' `Cost=`: the price
    /// carries the house's country cost multiplier, which the rules alone do
    /// not. A decision that budgets credits against this cannot misprice a
    /// build for the house it is playing.
    pub fn cost_of(&self, rules: &RuleSet, object: &ObjectType) -> i32 {
        self.sim.cost_of(self.house, object, rules)
    }

    /// Whether the navigation projection this file's ground answers come from
    /// stands at all.
    ///
    /// "Not walkable" is the projection's answer about a cell, and it is only
    /// an answer when the projection exists: a map loaded without one gives
    /// the same `false` for every cell, which is a map made of walls. A
    /// decision that reads `is_ground_walkable` to tell a blocked cell from an
    /// open one has to know the difference.
    pub fn has_ground_grid(&self) -> bool {
        self.sim.path_grid().is_some()
    }

    /// Whether the ground at a cell is walkable in the current path projection.
    /// Out of bounds answers false. This is not an occupancy query.
    pub fn is_ground_walkable(&self, rx: u16, ry: u16) -> bool {
        self.sim
            .path_grid()
            .is_some_and(|grid| grid.is_walkable(rx, ry))
    }

    /// Whether a wall stands on the cell, whoever owns it.
    ///
    /// A wall is an overlay, so it is not an object this file can see; and the
    /// passability it takes away is the navigation projection's
    /// ([`Self::is_ground_walkable`]), which says "closed" without saying why.
    /// What this adds is the reason: a closed cell that is a wall is one a shot
    /// can open, and a closed cell that is a cliff is not.
    pub fn wall_at(&self, rules: &RuleSet, rx: u16, ry: u16) -> bool {
        self.sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.cell(rx, ry).overlay_id)
            .and_then(|id| rules.overlay_registry().flags(id))
            .is_some_and(|flags| flags.wall)
    }

    /// The overlay standing on the cell, by its internal `[OverlayTypes]` id.
    ///
    /// [`Self::overlay_at`] answers "is something there"; this answers *what*,
    /// which is the only way to tell ore from a wall or a fence without a second
    /// grid. The id is meaningful to the rules, so the caller that wants a
    /// meaning reads it through the overlay registry (`Tiberium=`) rather than
    /// comparing a number here.
    pub fn overlay_id_at(&self, rx: u16, ry: u16) -> Option<u8> {
        self.sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.cell(rx, ry).overlay_id)
    }

    /// The overlay standing on the cell, wall or not.
    ///
    /// A wall is an overlay whose flags say so ([Self::wall_at]); a gate, a
    /// fence or a mod's own barricade is an overlay too and closes the cell
    /// without being a wall. The forced cell click is the same click for both.
    pub fn overlay_at(&self, rx: u16, ry: u16) -> bool {
        self.sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.cell(rx, ry).overlay_id)
            .is_some()
    }

    /// Whether the navigation projection the connectivity answers come from
    /// stands at all.
    ///
    /// The water rows of the passability matrix are modelled
    /// (`crate::sim::pathfinding::passability` carries `Water` and `WaterBeach`
    /// and the three amphibious zones), so this is the seam for a fleet: a
    /// decision that asks in its own movement zone gets the right answer for a
    /// boat today and for a hovercraft when one exists. What is *not* modelled
    /// yet is water as a movement layer — `MovementLayer` has no `Water` — so
    /// this answers connectivity between cells, not the cost of the cells
    /// between them.
    pub fn has_zone_grid(&self) -> bool {
        self.sim.zone_grid().is_some()
    }

    /// Whether a walker of movement zone `zone` can get from `from` to `to` at
    /// all: one lookup in the zone map, so this is cheap enough to ask per
    /// candidate cell.
    ///
    /// `is_ground_walkable` answers "is this one cell open"; this answers the
    /// question a destination has to pass — the cell can be open and still be
    /// on the far side of a wall, an island or a cliff, where a walk ordered
    /// to it never arrives. `true` when there is no zone grid: a decision that
    /// has no map is not told that everywhere is unreachable, and the caller
    /// that cares asks [`Self::has_zone_grid`] first.
    pub fn can_reach(&self, zone: MovementZone, from: (u16, u16), to: (u16, u16)) -> bool {
        self.sim.zone_grid().is_none_or(|grid| {
            grid.can_reach(zone, from, MovementLayer::Ground, to, MovementLayer::Ground)
        })
    }

    /// The structure standing whose foundation covers `cell`, as `(id, mine)`.
    ///
    /// A structure is several cells of closed ground and it is not an overlay,
    /// so the wall's answer does not reach it: the ground is closed and the
    /// reason is an object, which is the one thing that says "it can be shot".
    /// `mine` is what keeps a house from shelling its own power plant.
    ///
    /// It is a scan of the entity store, so it is for the question a walk asks
    /// about the one cell in front of it, not for a sweep.
    pub fn structure_at(&self, rules: &RuleSet, cell: (u16, u16)) -> Option<(u64, bool)> {
        for entity in self.sim.entities().values() {
            if entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
                continue;
            }
            let Some(ty) = self.sim.object_type(entity.type_ref(), rules) else {
                continue;
            };
            if ty.category != ObjectCategory::Building {
                continue;
            }
            let (width, height) = ty.foundation.dimensions();
            let (x0, y0) = (entity.position.rx(), entity.position.ry());
            let covers = {
                let (w, h) = (i32::from(width.max(1)), i32::from(height.max(1)));
                let dx = i32::from(cell.0) - i32::from(x0);
                let dy = i32::from(cell.1) - i32::from(y0);
                dx >= 0 && dx < w && dy >= 0 && dy < h
            };
            if covers {
                return Some((entity.stable_id(), entity.owner() == self.house));
            }
        }
        None
    }

    /// The owner of the wall standing on a cell, if one stands there.
    ///
    /// A wall is not an object the house owns. `[GAWALL] Wall=yes` leaves its
    /// overlay on the cell and uninitializes the transient Building
    /// (`sim::world::world_spawn::wall_unlimbo`), so [`Self::own_entities`]
    /// cannot answer "is this cell already walled" — the grid can, and it is
    /// also where a wall's owner is authoritative.
    ///
    /// The grid keeps the last owner of a cell whose wall was removed, for its
    /// retained neighbour counts, so the owner is only an answer together with
    /// the overlay: a sold cell reads `None`.
    pub fn wall_owner_at(&self, rx: u16, ry: u16) -> Option<InternedId> {
        let cell = self.sim.overlay_grid.as_ref()?.cell(rx, ry);
        if cell.overlay_id.is_none() {
            return None;
        }
        cell.wall_owner
    }
}

/// Integer square root of a non-negative lepton distance.
///
/// A flight time is a decision input, so it is computed in integers: `f64`
/// `sqrt` is correctly rounded on every platform VERA runs on, but an exact
/// integer answer costs one small loop and cannot drift with a libm change.
fn isqrt(value: i32) -> i32 {
    if value <= 0 {
        return 0;
    }
    let value = i64::from(value);
    // 46 341 is the largest root whose square still fits in an i32.
    let (mut low, mut high) = (0_i64, 46_341_i64);
    while low < high {
        let mid = (low + high + 1) / 2;
        if mid * mid <= value {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    i32::try_from(low).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod vision_tests {
    use super::*;
    use crate::sim::intern::InternedId;

    fn arena() -> Option<(Simulation, RuleSet)> {
        let retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
        let rules = retail.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    /// A policy may aim at what the house sees, and not at what it does not.
    ///
    /// The projection can answer both: `enemy_entities` is the world's list, and
    /// it is the read the native computer AI uses. If this accessor ever stopped
    /// filtering, the first assertion below would still pass and the second
    /// would not.
    #[test]
    fn an_enemy_behind_the_shroud_is_not_a_target() {
        let Some((mut sim, rules)) = arena() else {
            return;
        };
        let americans: InternedId = sim.intern("Americans");
        let near = sim
            .spawn_object("E1", "Russians", 28, 28, 0, &rules)
            .expect("a rifleman next to the house's own");
        let far = sim
            .spawn_object("E1", "Russians", 4, 4, 0, &rules)
            .expect("a rifleman across the map");
        let scout = sim
            .spawn_object("E1", "Americans", 27, 28, 0, &rules)
            .expect("the house's own rifleman");
        assert!(scout > 0);
        for _ in 0..4 {
            let _ = sim.take_due_commands();
            sim.advance_tick(&[], Some(&rules), None, None, 33);
        }

        let obs = Observation::new(&sim, americans);
        let seen: Vec<u64> = obs
            .visible_enemy_entities()
            .map(|enemy| enemy.entity_id)
            .collect();
        let all: Vec<u64> = obs.enemy_entities().map(|enemy| enemy.entity_id).collect();
        assert!(
            all.contains(&near) && all.contains(&far),
            "the world knows both"
        );
        assert!(
            seen.contains(&near),
            "the rifleman in line of sight is a target"
        );
        assert!(!seen.contains(&far), "the one across the map is not");
    }

    /// The playfield is not the fog grid, and a decision that sends a unit
    /// somewhere has to know the difference: the grid covers the whole map
    /// buffer, so its far corner reads as ground while nothing can walk there.
    ///
    /// The arena fixture has a playfield (`OPEN_PLAYFIELD`), so both answers are
    /// available here: a cell the fixture's ground covers, and one past its edge
    /// but still inside the grid.
    #[test]
    fn a_cell_outside_the_playfield_is_not_ground() {
        let Some((mut sim, _rules)) = arena() else {
            return;
        };
        let americans: InternedId = sim.intern("Americans");
        let obs = Observation::new(&sim, americans);

        // The arena's own ground: 32x32 cells at the origin.
        assert!(
            obs.is_cell_in_playfield(10, 10),
            "the arena's ground is playfield"
        );
        // Far past the arena's ground. The query reads the map's own bounds, not
        // the arena's size, so this asserts what the shipped maps need: a cell
        // the playfield does not cover is not ground, whatever the fog grid says
        // about it.
        assert!(
            !obs.is_cell_in_playfield(20_000, 20_000),
            "a cell far outside every playfield is not ground"
        );
        // A cell at the origin is ground either way: with playfield authority it
        // is inside the arena, and with none the answer must be permissive or
        // every fixture without a map would refuse every destination.
        assert!(
            obs.is_cell_in_playfield(0, 0),
            "the origin is ground with or without a playfield"
        );
    }

    /// A ground unit that cannot answer air is not told it can hurt an aircraft.
    ///
    /// The wishlist's "一群无法对空的单位追着飞机跑": the targeting gates ask this
    /// predicate, and it answered `true` for every firer whenever the target was
    /// an aircraft — so a rifleman was a valid answer to a Harrier and the layer
    /// walked it after one. The flag is the projectile's own `AA=`, which is what
    /// the sim fires on, so a mod that puts an AA weapon on a rifle changes the
    /// answer with no list here.
    #[test]
    fn a_ground_unit_that_cannot_answer_air_is_not_told_it_can() {
        let Some((mut sim, rules)) = arena() else {
            return;
        };
        let americans: InternedId = sim.intern("Americans");
        let gi = sim
            .spawn_object("E1", "Americans", 20, 20, 255, &rules)
            .expect("a rifleman");
        let harrier = sim
            .spawn_object("ORCA", "Russians", 24, 20, 255, &rules)
            .expect("a Harrier");
        // The IFV carries `RedEye2` on its passenger slot and `HoverMissile` on
        // its own, so it is the answer the rifleman is not.
        let ifv = sim
            .spawn_object("FV", "Americans", 22, 22, 255, &rules)
            .expect("an IFV");
        let obs = Observation::new(&sim, americans);
        assert!(
            obs.can_engage(&rules, gi, harrier),
            "the fire error still admits it: that is the sim's own answer"
        );
        assert!(
            !obs.can_hurt(&rules, gi, harrier),
            "and the targeting gate refuses it: a rifleman cannot answer air"
        );
        assert!(
            obs.can_hurt(&rules, ifv, harrier),
            "the IFV's own weapon answers air"
        );
    }
}
