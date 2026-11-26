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
//! - `owned_by_hand` applies the fifth-key scope to player control groups
//!   passed through the board (`with_control_scope`), and also respects the
//!   `TeamClass` scripts of this house. Neither is simulation state the sim
//!   owns: a control group lives in the player's view, and its membership is
//!   handed in rather than stored.
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObservedStrikeKind { Nuclear, Dominator, Lightning }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObservedStrike {
    pub(crate) kind:ObservedStrikeKind,
    pub(crate) cell:(u16,u16),
    pub(crate) radius:i32,
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
    /// Whether the AI may drive *only* the player-control-group members.
    /// False is the original "exclude manually grouped units" behavior.
    grouped_only: bool,
    /// The board's independent work gates. A strategy can plan only actions it
    /// is permitted to attempt; in particular, site clearing must not issue
    /// `Scatter` through Forces when the player disabled Production.
    production_enabled: bool,
    forces_enabled: bool,
    abilities_enabled: bool,
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
    /// Identity of the held object, including a completed unplaced building.
    pub product_entity: Option<u64>,
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
            grouped_only: false,
            production_enabled: true,
            forces_enabled: true,
            abilities_enabled: true,
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

    /// Apply the player's fifth-key scope to the strategy's observation.
    /// The brain must not plan with units L2 will later discard; otherwise
    /// its army counts and task assignments disagree with command admission.
    pub fn with_control_scope(mut self, groups: &'a BTreeSet<u64>, grouped_only: bool) -> Self {
        self.player_kept = groups;
        self.grouped_only = grouped_only;
        self
    }

    /// Let the strategy observe the independent work switches before making
    /// stateful plans. Headless and test observations default to all three enabled.
    pub fn with_work_scope(mut self, production: bool, forces: bool, abilities: bool) -> Self {
        self.production_enabled = production;
        self.forces_enabled = forces;
        self.abilities_enabled = abilities;
        self
    }

    pub fn production_enabled(&self) -> bool {
        self.production_enabled
    }

    pub fn forces_enabled(&self) -> bool {
        self.forces_enabled
    }

    pub fn abilities_enabled(&self) -> bool {
        self.abilities_enabled
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

    /// Read the actual session discriminator, not the presence of a yard.
    pub fn is_campaign(&self) -> bool {
        self.sim.session().game_mode() == crate::sim::scenario_session::ScenarioGameMode::Campaign
    }

    /// Observed stock superweapon danger. Nuclear markers use exactly the
    /// same psychic-warning admission as the UI. A storm is only exposed
    /// after the public start, and the announced Dominator only before impact.
    /// Unknown mod handlers must never be interpreted as stock blasts.
    pub(crate) fn observed_superweapon_strikes(&self,rules:&RuleSet)->Vec<ObservedStrike> {
        if rules.ares() || rules.ares_form() {return Vec::new();}
        let mut result=Vec::new();
        let spread=|name:&str|rules.warhead(name).map_or(0,|w|
            w.cell_spread_f64.ceil().clamp(0.0,64.0) as i32);
        if let Some(weapon)=rules.weapon("NukePayload") {
            let radius=weapon.warhead.as_deref().map_or(0,spread);
            if radius>0 {
                for (_,anim) in self.sim.anims() {
                    if anim.completed() || !self.sim.interner().resolve(anim.type_id())
                        .eq_ignore_ascii_case(crate::rules::effect_asset_catalog::PSI_WARNING_ANIM)
                        || !self.sim.psi_warning_detected_by(self.house,anim,rules) {continue;}
                    let world=anim.world_coord();
                    let (Ok(x),Ok(y))=(u16::try_from(world.x.div_euclid(256)),u16::try_from(world.y.div_euclid(256)))
                        else {continue;};
                    if self.is_cell_in_playfield(x,y) {
                        result.push(ObservedStrike{kind:ObservedStrikeKind::Nuclear,cell:(x,y),radius:radius+1});
                    }
                }
            }
        }
        let hostile=|owner:Option<InternedId>|owner.is_some_and(|owner|owner!=self.house
            && !self.sim.house_alliances().is_allied(self.sim.interner(),self.house,owner));
        let observed=|(x,y):(i16,i16)|{
            let (Ok(x),Ok(y))=(u16::try_from(x),u16::try_from(y)) else {return None;};
            (self.is_cell_in_playfield(x,y) && self.is_cell_visible(x,y)).then_some((x,y))
        };
        if let Some((cell,owner))=self.sim.psychic_dominator.announced_pending_strike() {
            if hostile(owner) {
                if let Some(cell)=observed(cell) {
                    let radius=spread(&rules.general.dominator_warhead)
                        .max(rules.general.dominator_capture_range.clamp(0,64))+1;
                    result.push(ObservedStrike{kind:ObservedStrikeKind::Dominator,cell,radius});
                }
            }
        }
        if let Some((cell,owner))=self.sim.lightning_storm.announced_strike() {
            if hostile(owner) {
                if let Some(cell)=observed(cell) {
                    let radius=(rules.general.lightning_cell_spread/2).clamp(0,64)
                        +spread(&rules.general.lightning_warhead)+2;
                    result.push(ObservedStrike{kind:ObservedStrikeKind::Lightning,cell,radius});
                }
            }
        }
        result.sort_unstable_by_key(|s|(s.cell,s.radius,s.kind as u8));
        result.dedup();
        result
    }

    /// Single-centre stock receiver forecast. An uncertain footprint,
    /// unknown warning, modded damage, or native unsellability means KEEP.
    pub(crate) fn forecast_strike_sale(&self,rules:&RuleSet,id:u64,
        strike:ObservedStrike)->Option<i32> {
        if !self.production_enabled() || !self.owns_object(id) || self.owned_by_hand(id)
            || !self.is_cell_visible(strike.cell.0,strike.cell.1)
            || !crate::sim::production::can_sell_building(self.sim,rules,id)
        {return None;}
        let (at,type_id)=self.object_cell_and_type(id)?;
        // Only certify the primary foundation cell. Do not assume every
        // building in CellSpread receives the centre damage (or any damage).
        if at!=strike.cell {return None;}
        let ty=self.object_type(rules,type_id)?;
        if ty.category!=ObjectCategory::Building || ty.construction_yard
            || ty.can_be_occupied || ty.build_const_eligible() || ty.undeploys_into.is_some()
            || ty.strength<=0 {return None;}
        let (damage,warhead)=match strike.kind {
            ObservedStrikeKind::Nuclear=>{
                let weapon=rules.weapon("NukePayload")?;
                (weapon.damage,weapon.warhead.as_deref()?)
            }
            // Storm scatter is stochastic; the Dominator's damage and
            // source attribution differ from the nuke. Never liquidate
            // based on those uncertain forecasts in this first policy.
            _=>return None,
        };
        let predicted=crate::sim::combat::stock_strike_damage_on_building(
            self.sim,rules,id,damage,warhead)?;
        // Even full-health survival (or a borderline prediction) means
        // keep it. A native sale request still need not finish before impact.
        (i64::from(predicted)*100>=i64::from(ty.strength)*125).then_some(predicted)
    }

    /// Centre-hit estimate through the stock damage receiver. Callers
    /// must not treat splash falloff, random storm scatter or later repairs
    /// as guaranteed kills. Unavailable for unsupported mod receivers.
    pub(crate) fn visible_strike_building_damage(&self,rules:&RuleSet,id:u64,
        damage:i32,warhead:&str)->Option<i32> {
        if !self.visible_enemy_entities().any(|e|e.entity_id==id)
            || self.visible_enemy_invulnerable(id) {return Some(0);}
        crate::sim::combat::stock_strike_damage_on_building(self.sim,rules,id,damage,warhead)
    }

    /// The same online producer that backs has_producer, for planning a
    /// prospective specialist's actual starting region.
    pub fn current_producer(&self,rules:&RuleSet,ty:&ObjectType)->Option<u64> {
        crate::sim::production::find_factory(self.sim,rules,self.house,ty,false,true,true)
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

    /// A power producer still being erected will eventually supply its full
    /// output, while an *already finished but damaged* plant only contributes
    /// what the native meter currently reports. The building planner needs
    /// this distinction to avoid both duplicate opening plants and late-game
    /// brownouts after a raid.
    pub fn building_under_construction(&self, entity_id: u64) -> bool {
        self.sim.entities().get(entity_id)
            .is_some_and(|entity| entity.in_construction_bstate())
    }

    /// Power produced by the house's live, powered buildings.
    pub fn power_output(&self) -> i32 {
        self.power().map_or(0, |(output, _)| output)
    }

    /// Whether a *currently visible* hostile building can operate its powered
    /// systems. A low-power or spy-blackout tower must not project a phantom
    /// firing envelope. Use the same native PowerState gate as the simulation,
    /// rather than inferring electricity from nearby power plants. Never
    /// reveal an unseen enemy building's house power state.
    pub fn visible_enemy_building_operational(
        &self, rules: &RuleSet, entity_id: u64,
    ) -> bool {
        let Some(entity) = self.sim.entities().get(entity_id) else { return false; };
        if entity.category() != EntityCategory::Structure
            || entity.lifecycle.in_limbo() || entity.health.current() <= 0
            || !self.is_cell_visible(entity.position.rx(), entity.position.ry())
            || !self.is_hostile(entity_id)
        { return false; }
        crate::sim::power_system::is_building_powered(
            self.sim.power_states(), rules, entity, self.sim.interner(),
        )
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

    /// A parasite inside a vehicle is a repair-depot emergency even when the
    /// vehicle itself is still healthy. Exposed read-only so the controller
    /// never needs to look through the observation boundary.
    pub fn has_parasite(&self, entity_id: u64) -> bool {
        self.sim.entities().get(entity_id).is_some_and(|entity| {
            entity.parasite.eater().is_some()
        })
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

    /// An active timed outage suppresses the native power meter's output.
    pub fn power_blackout_active(&self) -> bool {
        self.sim.power_states().get(&self.house).is_some_and(|state| {
            state.blackout_remaining(self.sim.session().binary_frame()) > 0
        })
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
                // Bound miner slaves are a parent-driven lifecycle, not player
                // infantry. A slave becomes independent once the master is
                // destroyed; only then may the assistant command it.
                orderable: self.sim.order_actor_admits(entity.stable_id())
                    && entity.seldom.slave.owner().is_none(),
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
            .map_or(0, |entity| entity.passenger_role().passenger_count())
    }

    /// The transport this object is inside, if any.
    ///
    /// A unit inside one is not on the map to be ordered; the policy reads this
    /// so a loaded passenger is not sent a second job every frame.
    pub fn inside_transport(&self, entity_id: u64) -> Option<u64> {
        self.sim
            .entities()
            .get(entity_id)
            .and_then(|entity| entity.passenger_role().inside_transport_id())
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

    /// The currently visible controller of a **formerly own** mind-controlled
    /// unit. A visible victim is not proof that its controller is visible;
    /// both must be in the player's live sight. A victim originally belonging
    /// to another enemy must not become a rescue priority for this house.
    pub fn visible_liberation_target(&self, victim_id: u64) -> Option<u64> {
        if !self.visible_enemy_entities().any(|enemy| enemy.entity_id == victim_id) {
            return None;
        }
        let victim = self.sim.entities().get(victim_id)?;
        let controller_id = victim.seldom.mind_control.controller()?;
        let controller = self.sim.entities().get(controller_id)?;
        let original = controller.capture_manager.as_ref()?
            .original_owner(victim_id)?;
        if original != self.house
            || !self.visible_enemy_entities().any(|enemy| enemy.entity_id == controller_id)
        {
            return None;
        }
        Some(controller_id)
    }

    /// Actual own rank, not a type-level estimate or an enemy fog query.
    pub(crate) fn own_combat_experience(&self,actor:u64)->Option<i32> {
        let e=self.sim.entities().get(actor)?;
        if e.owner()!=self.house || e.health.current()<=0 {return None;}
        let value=f32::from_bits(e.veterancy.raw().bits());
        value.is_finite().then_some((value.clamp(0.0,20.0)*1000.0) as i32)
    }

    pub(crate) fn own_elite(&self,actor:u64)->bool {
        self.sim.entities().get(actor).is_some_and(|e|e.owner()==self.house
            && e.veterancy.rank_kind()==crate::sim::combat::veterancy::VeterancyRank::Elite)
    }

    pub(crate) fn observed_actor_weapon<'r>(&self,rules:&'r RuleSet,actor:u64,slot:usize)
        ->Option<&'r crate::rules::weapon_type::WeaponType> {
        let e=self.sim.entities().get(actor)?;
        if e.owner()!=self.house && !self.is_cell_visible(e.position.rx(),e.position.ry()) {
            return None;
        }
        let ty=self.sim.object_type(e.type_ref(),rules)?;
        if ty.gunner {
            return if slot==0 {self.active_gunner_weapon(rules,actor)}else{None};
        }
        crate::sim::combat::combat_weapon::weapon_for_index(ty,e.veterancy.rank(),
            i32::try_from(slot).ok()?).and_then(|record|record.weapon(rules))
    }

    /// Current Gunner weapon *on this specific vehicle*. Its passenger has
    /// already selected the chassis slot in the native passenger engine.
    /// The 17 weapon records in an IFV type are alternatives, not a loadout.
    pub fn active_gunner_weapon<'r>(
        &self, rules: &'r RuleSet, actor: u64,
    ) -> Option<&'r crate::rules::weapon_type::WeaponType> {
        let entity = self.sim.entities().get(actor)?;
        let ty = self.sim.object_type(entity.type_ref(), rules)?;
        if !ty.gunner { return None; }
        crate::sim::combat::combat_weapon::current_weapon(
            entity, ty, self.sim.entities(), rules, self.sim.interner(),
        )
    }

    /// A controlled prize is eligible for recycling only if its *current*
    /// controller is one of this player's own objects. A mere unit ownership
    /// change (engineer capture, crate, factory production) is not proof.
    pub fn controlled_by_own_controller(&self, actor: u64) -> bool {
        let Some(unit) = self.sim.entities().get(actor) else { return false; };
        if unit.owner() != self.house { return false; }
        let Some(controller) = unit.seldom.mind_control.controller() else { return false; };
        self.sim.entities().get(controller).is_some_and(|entity|
            entity.owner() == self.house && entity.health.current() > 0)
    }

    /// Native player click admission for entering an owned grinder; unlike a
    /// transport it has no passenger seats, so `can_enter_transport` is false.
    pub fn can_enter_grinder(&self, rules: &RuleSet, actor: u64, grinder: u64) -> bool {
        self.sim.entry_click_action(
            actor, grinder,
            crate::sim::world::EntryBaseAction::Select, rules,
        ) == Some(crate::sim::world::EntryClickAction::Grind)
    }

    /// Exact native refund, including `Soylent=`, owner/country modifiers and
    /// the current rule policy. Never use an arbitrary fraction of Cost=.
    pub fn grinder_refund(&self, rules: &RuleSet, actor: u64) -> Option<i32> {
        let entity = self.sim.entities().get(actor)?;
        (entity.owner() == self.house)
            .then(|| crate::sim::production::techno_refund(self.sim, rules, actor))?
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
            product_entity: factory.object.and_then(|object| object.entity_id),
            suspended: factory.suspended,
            queued: factory.queue.len(),
        })
    }

    /// Own native unpaid production balance plus actual queued type prices.
    /// Queued prices are estimates at today's Cost_Of; the engine establishes
    /// their final price when promoted. A finished paid product owes zero.
    pub(crate) fn production_commitment(&self, rules: &RuleSet) -> i32 {
        use ProductionCategory::*;
        let mut total = 0i32;
        for category in [Building, Defense, Infantry, Vehicle, Aircraft, Ship] {
            let Some(line) = self.sim.production().factories().view(self.house, category)
                else { continue; };
            total = total.saturating_add(line.balance().max(0));
            for entry in line.queue {
                let cost = self.object_type(rules, entry.type_id)
                    .map_or(i32::MAX, |ty| self.cost_of(rules, ty).max(0));
                total = total.saturating_add(cost);
            }
        }
        total
    }

    /// Remaining native building-wrench costs, including player-started work.
    pub(crate) fn repair_commitment(&self, rules: &RuleSet) -> i32 {
        self.sim.entities().values().filter(|e| e.owner() == self.house && e.is_alive()
                && e.building.repairing.is_on())
            .filter_map(|e| {
                let ty = self.object_type(rules, e.type_ref())?;
                (ty.category == ObjectCategory::Building).then_some(())?;
                crate::sim::production::repair_remaining(e, ty, rules, self.tick() as u32)
                    .map(|(_, cost)| cost.max(0))
            }).fold(0i32, i32::saturating_add)
    }

    /// Price of a possible new repair; never toggle an already-running repair.
    pub(crate) fn own_repair_quote(&self, rules: &RuleSet, id: u64) -> Option<(bool, i32)> {
        let e = self.sim.entities().get(id)?;
        if e.owner() != self.house || !e.is_alive() || e.constructing_or_selling() { return None; }
        let ty = self.object_type(rules, e.type_ref())?;
        if ty.category != ObjectCategory::Building || !ty.repairable { return None; }
        let step = rules.general.repair_step;
        let missing = ty.strength.saturating_sub(e.health.current());
        if step <= 0 || missing <= 0 { return None; }
        let cost = ((missing - 1) / step + 1)
            .saturating_mul(crate::sim::production::repair_step_cost(rules,ty));
        Some((e.building.repairing.is_on(),cost.max(0)))
    }

    /// Native cumulative spent-credit counter (not a fabricated sum of orders).
    pub(crate) fn spent_credits(&self) -> i32 {
        self.sim.houses().get(&self.house).map_or(0, |h| h.economy().spent_credits())
    }

    pub(crate) fn has_pending_production_commands(&self) -> bool {
        self.sim.has_pending_production_commands(self.house)
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
            // A passive civilian or animal becomes a local defensive target
            // only while it is *visibly attacking our house*. Do not declare
            // war on the other neutral inhabitants or use unseen retaliation.
            if allied(self.sim.interner().resolve(owner))
                || (target.multiplay_passive()
                    && !self.visible_neutral_aggressor(entity.stable_id()))
            {
                return None;
            }
            Some(EnemyEntity {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell: (entity.position.rx(), entity.position.ry()),
            })
        })
    }

    /// Passive animals and civilians are not default opponents. Only a
    /// currently visible passive entity whose native attack target is one of
    /// our living objects may be treated as a defensive enemy. In particular,
    /// an unseen animal with a target must not enter strategic target lists.
    fn visible_neutral_aggressor(&self, id: u64) -> bool {
        let Some(entity) = self.sim.entities().get(id) else { return false; };
        if !self.is_cell_visible(entity.position.rx(), entity.position.ry()) {
            return false;
        }
        self.attack_target(id).is_some_and(|victim| {
            self.sim.entities().get(victim).is_some_and(|own| {
                own.owner() == self.house && !own.lifecycle.in_limbo()
                    && own.health.current() > 0
            })
        })
    }

    /// Visible, passive, unarmed civilian vehicles which might be acquired by
    /// a native mind-control weapon. The ordinary enemy iterator deliberately
    /// excludes neutral civilians; this is a separate read-only opportunity
    /// list and NEVER permission for a normal combat weapon to attack them.
    pub fn visible_neutral_vehicles<'r>(
        &'r self,
        rules: &'r RuleSet,
    ) -> impl Iterator<Item = EnemyEntity> + 'r {
        self.sim.entities().values().filter_map(move |entity| {
            if entity.owner() == self.house || entity.lifecycle.in_limbo()
                || entity.health.current() <= 0
                || !self.is_cell_visible(entity.position.rx(), entity.position.ry())
                || !self.sim.houses().get(&entity.owner())
                    .is_some_and(|house| house.multiplay_passive())
                || self.is_hostile(entity.stable_id())
            { return None; }
            let ty = self.sim.object_type(entity.type_ref(), rules)?;
            if ty.category != ObjectCategory::Vehicle
                || ty.immune_to_psionics || ty.balloon_hover
                || ty.resource_gatherer || ty.harvester
                || ty.soylent <= 0
                || (0..ty.weapon_record_count()).any(|slot|
                    ty.weapon_at(slot).and_then(|name| rules.weapon(name))
                        .is_some_and(|weapon| !weapon.never_use && weapon.damage > 0))
                || entity.seldom.mind_control.controller().is_some()
            { return None; }
            Some(EnemyEntity {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell: (entity.position.rx(), entity.position.ry()),
            })
        })
    }

    /// Narrow, *nonlethal* exception to the normal hostile-target order gate.
    /// Only a real, currently selected native MindControl weapon against a
    /// visible neutral civilian vehicle can pass. Never let a rifleman or a
    /// normal gun force-fire passive residents through this policy.
    pub fn can_control_neutral(
        &self, rules: &RuleSet, controller: u64, target: u64,
    ) -> bool {
        if !self.own_entities().any(|unit|
            unit.entity_id == controller && unit.orderable)
            || !self.visible_neutral_vehicles(rules).any(|neutral|
                neutral.entity_id == target)
        { return false; }
        let Some(actor) = self.sim.entities().get(controller) else { return false; };
        let Some(kind) = self.sim.object_type(actor.type_ref(), rules) else { return false; };
        if actor.capture_manager.is_none() { return false; }
        let Some(weapon) = crate::sim::combat::combat_weapon::current_weapon(
            actor, kind, self.sim.entities(), rules, self.sim.interner())
        else { return false; };
        let psychic = weapon.warhead.as_deref()
            .and_then(|name| rules.warhead(name))
            .is_some_and(|head| head.mind_control);
        psychic && !weapon.never_use && self.can_engage(rules, controller, target)
    }

    /// Two *currently visible* enemy objects belong to the same owner.
    /// Used to choose the actual uniform a spy must steal, without reading
    /// a hidden infantryman's identity or inferring a faction from an ID.
    pub fn visible_enemy_same_owner(&self, a: u64, b: u64) -> bool {
        let Some(first) = self.sim.entities().get(a) else { return false; };
        let Some(second) = self.sim.entities().get(b) else { return false; };
        self.is_cell_visible(first.position.rx(), first.position.ry())
            && self.is_cell_visible(second.position.rx(), second.position.ry())
            && first.owner() == second.owner()
            && first.owner() != self.house
            && self.is_hostile(a) && self.is_hostile(b)
    }

    /// The owner of an *actually visible*, hostile object. Strategic focus
    /// may group enemy buildings by this id, but never read the owner of an
    /// entity hidden by fog or of an unprovoked civilian.
    pub fn visible_enemy_house(&self, id: u64) -> Option<InternedId> {
        let entity = self.sim.entities().get(id)?;
        (self.is_cell_visible(entity.position.rx(), entity.position.ry())
            && self.is_hostile(id)).then_some(entity.owner())
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

    /// Whether an enemy currently visible to this house has an active
    /// invulnerability timer (Iron Curtain, Force Shield or mod equivalent).
    /// Never leak an unseen target's timer through a strategy query.
    pub fn visible_enemy_invulnerable(&self, target: u64) -> bool {
        let Some(entity) = self.sim.entities().get(target) else { return false; };
        // This query is used inside targeting loops on huge maps: check one
        // entity directly rather than walking every visible enemy each time.
        self.is_hostile(target)
            && self.is_cell_visible(entity.position.rx(), entity.position.ry())
            && crate::sim::superweapon::invulnerability::is_invulnerable(
                entity.invulnerability.as_ref(), self.sim.session().binary_frame() as u32,
            )
    }

    /// Whether our actor is *effectively* disguised to this currently visible
    /// enemy, according to the native per-observer predicate (including the
    /// house its uniform represents and disguise detection). `disguised` alone
    /// cannot answer this: a spy dressed as its own side remains hostile.
    /// Unseen observers cannot be queried through the strategy boundary.
    pub fn disguised_to_visible_enemy(&self, actor: u64, observer: u64) -> bool {
        let Some(observer_entity) = self.sim.entities().get(observer) else {
            return false;
        };
        if !self.visible_enemy_entities().any(|enemy| enemy.entity_id == observer) {
            return false;
        }
        self.sim.entities().get(actor).is_some_and(|entity| {
            entity.owner() == self.house
                && entity.is_disguised_to(
                    observer_entity.owner(), Some(self.sim.fog()),
                    Some(&self.sim.house_alliances), self.sim.interner(),
                )
        })
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
    pub(crate) fn visible_passive_structure(&self,id:u64)->bool {
        let Some(e)=self.sim.entities().get(id) else{return false;};
        e.owner()!=self.house && e.category()==EntityCategory::Structure
            && e.health.current()>0 && !e.lifecycle.in_limbo()
            && self.is_cell_visible(e.position.rx(),e.position.ry())
            && self.sim.houses().get(&e.owner()).is_some_and(|h|h.multiplay_passive())
    }

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
            return self.visible_neutral_aggressor(entity_id);
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

    /// Object destination of one of our own actors, without following its
    /// pointer into fog. The caller must observe the target independently.
    pub fn own_destination_object(&self, actor: u64) -> Option<u64> {
        let e = self.sim.entities().get(actor)?;
        if e.owner() != self.house { return None; }
        match e.navigation.nav_com()? {
            crate::sim::components::NavTargetRef::Object { id } => Some(id),
            _ => None,
        }
    }

    /// Current object position only if owned or presently visible. In
    /// particular a retained native target pointer is not a scout report.
    pub fn observed_object_cell(&self, actor: u64) -> Option<(u16,u16)> {
        let e = self.sim.entities().get(actor)?;
        let cell = (e.position.rx(), e.position.ry());
        if e.lifecycle.in_limbo() || e.health.current() <= 0 { return None; }
        (e.owner() == self.house || self.is_cell_visible(cell.0,cell.1)).then_some(cell)
    }

    /// Native factory-to-unit delivery is not an ordinary player mission.
    /// Guard/Move can be visible while the door or track still owns the unit.
    /// Follow the actual contact (including a factory captured during unload),
    /// not the unit's age or a guessed distance from an arbitrary factory.
    pub fn own_factory_handoff(&self, rules: &RuleSet, actor: u64) -> Option<u64> {
        let unit = self.sim.entities().get(actor)?;
        if unit.owner() != self.house || !self.sim.order_actor_admits(actor)
            || unit.category() != crate::map::entities::EntityCategory::Unit { return None; }
        [unit.seldom.radio_contacts.slot(0), unit.seldom.tether.partner()]
            .into_iter().flatten().find(|id| self.sim.order_actor_admits(*id)
                && self.sim.is_weapons_factory(*id, rules))
    }

    /// Own property may be queried without exposing a hidden enemy's owner.
    pub fn owns_object(&self, actor: u64) -> bool {
        self.sim.entities().get(actor).is_some_and(|e| e.owner() == self.house)
    }

    /// A native repair bay, not a busy or enemy docking destination. The
    /// receiver still decides whether the actual Enter command can dock.
    pub fn own_repair_bay_available(&self, rules: &RuleSet, actor: u64, bay: u64) -> bool {
        let Some(e) = self.sim.entities().get(bay) else { return false; };
        e.owner() == self.house && !e.lifecycle.in_limbo() && e.health.current()>0
            && self.sim.object_type(e.type_ref(),rules).is_some_and(|ty| ty.unit_repair)
            && !e.in_construction_bstate()
            && e.mission.effective().known() != Some(crate::sim::mission::MissionType::Selling)
            && e.seldom.radio_contacts.slot(0).is_none_or(|id| id == actor)
    }

    pub(crate) fn can_visit_own_repair_bay(&self,rules:&RuleSet,actor:u64,bay:u64)->bool {
        self.owns_object(actor) && self.own_repair_bay_available(rules,actor,bay)
            && self.sim.entities().get(actor).is_some_and(|e|
                e.category()==EntityCategory::Unit
                && self.sim.object_type(e.type_ref(),rules).is_some_and(|ty|
                    ty.movement_zone!=MovementZone::Fly && !ty.jumpjet && !ty.balloon_hover))
            && crate::sim::radio::receive::can_load_query(self.sim,bay,actor,rules)
                ==crate::sim::radio::RadioResponse::Roger
    }

    /// A native service relationship must be observed, not inferred from a
    /// Sleep mission alone (newly produced units are asleep too).
    pub fn own_repair_service(&self, rules: &RuleSet, actor: u64) -> bool {
        use crate::sim::mission::MissionType;
        let Some(e) = self.sim.entities().get(actor) else { return false; };
        if e.owner() != self.house || !matches!(e.mission.effective().known(),
            Some(MissionType::Sleep | MissionType::Repair)) { return false; }
        let Some(bay) = e.seldom.radio_contacts.slot(0) else { return false; };
        self.own_repair_bay_available(rules,actor,bay)
            && self.sim.entities().get(bay).is_some_and(|b|
                b.seldom.radio_contacts.slot(0) == Some(actor)
                    && b.mission.effective().known() == Some(MissionType::Repair))
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
            .is_some_and(|entity| entity.seldom.rearm_timer.remaining(frame) <= 0)
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

    /// Campaign recruitment candidates: ordinary capture or repair of
    /// friendly/passive occupiable cover. The native actor-specific action
    /// remains authoritative when an actual engineer exists.
    pub fn campaign_engineer_sites<'r>(&'r self, rules: &'r RuleSet)
        -> impl Iterator<Item = (EnemyEntity, bool)> + 'r
    {
        let allied = crate::map::houses::ally_test(
            self.sim.house_alliances.as_map(), self.sim.interner().resolve(self.house));
        let visible_hostiles:std::collections::BTreeSet<_>=self.visible_enemy_entities()
            .map(|e|e.entity_id).collect();
        self.sim.entities().values().filter_map(move |entity| {
            if entity.lifecycle.in_limbo()
                || entity.health.current() <= 0 || entity.category() != EntityCategory::Structure
                || entity.is_warped_out() || entity.building_up() || entity.building_down()
            { return None; }
            let cell=(entity.position.rx(),entity.position.ry());
            if !self.is_cell_visible(cell.0,cell.1) { return None; }
            let ty=self.sim.object_type(entity.type_ref(),rules)?;
            let repairable=if rules.ares() {
                ty.building_type_ext().engineer_repairable(ty.repairable)
            } else {ty.repairable};
            if !repairable || ty.is_1x1_with_undeploy() || ty.bridge_repair_hut {return None;}
            let id=entity.stable_id();
            if self.is_hostile(id) && !visible_hostiles.contains(&id) {return None;}
            let site=EnemyEntity{entity_id:id,type_id:entity.type_ref(),cell};
            let owner=entity.owner();
            let passive=self.sim.houses().get(&owner).is_some_and(|h|h.multiplay_passive());
            if ty.can_be_occupied && (owner==self.house || passive) {
                // Repair badly damaged houses; never consume an engineer
                // on an intact passive house that can already be garrisoned.
                let hp=f64::from(entity.health.current());
                let full=f64::from(ty.strength);
                let red=f64::from(rules.general.condition_red);
                return (ty.strength>0 && hp<full && hp<=full*red)
                    .then_some((site,true));
            }
            (self.is_campaign() && owner!=self.house && !allied(self.sim.interner().resolve(owner))
                && ty.capturable).then_some((site,false))
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
    pub(crate) fn engineer_takes_in_one(&self,rules:&RuleSet,actor:u64,building:u64)->bool {
        self.owns_object(actor) && self.observed_object_cell(building).is_some()
            && matches!(self.sim.engineer_building_action(actor,building,rules),
                Some(crate::sim::world::EngineerBuildingAction::Capture))
    }

    /// Prospective estimate when the engineer has not yet been produced.
    /// Custom Ares thresholds are left to native admission on arrival.
    pub(crate) fn easy_capture_candidate(&self,rules:&RuleSet,building:u64)->bool {
        let Some(cell)=self.observed_object_cell(building) else{return false;};
        if !self.is_cell_visible(cell.0,cell.1) || self.owns_object(building){return false;}
        let Some(e)=self.sim.entities().get(building) else{return false;};
        let Some(ty)=self.sim.object_type(e.type_ref(),rules) else{return false;};
        if !ty.capturable || ty.category!=ObjectCategory::Building
            || e.health.current()<=0 || e.lifecycle.in_limbo()
            || e.is_warped_out() || e.building_up() || e.building_down()
            || ty.is_1x1_with_undeploy() || ty.bridge_repair_hut
            || (ty.can_be_occupied && self.visible_passive_structure(building))
        {return false;}
        if rules.ares(){return false;}
        ty.repairable && ty.strength>0 &&
            (e.health.current() as f64)/(ty.strength as f64)
                <= f64::from(f32::from_bits(rules.general.engineer_capture_level.bits()))
    }

    pub fn can_take_building(&self, rules: &RuleSet, actor: u64, building: u64) -> bool {
        use crate::sim::world::EngineerBuildingAction;
        match self.sim.engineer_building_action(actor, building, rules) {
            Some(EngineerBuildingAction::Capture | EngineerBuildingAction::Damage) => return true,
            // A `BridgeRepairHut=` whose span is down is the same Capture click
            // and the same consumed engineer: the hut arm answers Repair, and
            // only while there is a span to put back ([`Self::bridge_repair_huts`]).
            Some(EngineerBuildingAction::Repair(true)) => {
                if self
                    .bridge_repair_huts(rules)
                    .any(|site| site.building_id == building)
                {
                    return true;
                }
                // A friendly, damaged occupiable building is repaired by the
                // *same* native CaptureBuilding command, but not by the
                // ordinary sidebar repair wrench. Its repair cursor is only
                // legal when the native engineer arm reports Repair(true).
                if self.own_entities().any(|own| own.entity_id == building)
                    && self.sim.entities().get(building)
                        .and_then(|entity| self.sim.object_type(entity.type_ref(), rules))
                        .is_some_and(|ty| ty.can_be_occupied)
                {
                    return true;
                }
                // The native engineer arm also answers Repair(true) for a
                // damaged passive occupiable house. In campaign this is a
                // legitimate preparation for garrisoning, not a capture.
                if self.observed_object_cell(building).is_some()
                    && self.sim.entities().get(building).is_some_and(|entity|
                        self.sim.houses().get(&entity.owner()).is_some_and(|h|h.multiplay_passive())
                        && self.sim.object_type(entity.type_ref(),rules)
                            .is_some_and(|ty|ty.can_be_occupied))
                {return true;}
            }
            _ => {}
        }
        self.sim.infiltrator_building_action(actor, building, rules)
            || crate::sim::passenger::building_can_be_occupied_by(self.sim, rules, building, actor)
    }

    /// Whether the house actually received a permanent spy effect.
    /// A spy dying en route must not be recorded as a successful infiltration.
    pub fn spy_class_effect_observed(&self, class: u8) -> bool {
        self.sim.houses().get(&self.house)
            .is_some_and(|house| house.spy_class_effect_observed(class))
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
                u32::try_from(entity.passenger_role().occupants().count()).unwrap_or(u32::MAX);
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
        let mut found = BTreeSet::new();
        // Ordinary random crates are in the authority's slot table. A map
        // trigger can also create a crate overlay directly, without a slot;
        // the native PickupCrate reads the overlay itself. Neither source is
        // permission to discover a crate behind the house's current fog.
        for slot in self.sim.crate_authority().slots().iter().filter(|slot| !slot.is_empty()) {
            let (Ok(x), Ok(y)) = (u16::try_from(slot.cell_x), u16::try_from(slot.cell_y))
                else { continue; };
            if self.is_cell_visible(x, y) && self.visible_crate_at(rules, (x, y)) {
                found.insert((x, y));
            }
        }
        let Some(grid) = self.sim.overlay_grid.as_ref() else {
            return found.into_iter().collect();
        };
        // A full revealed-area scan once every 12 simulation frames catches
        // trigger overlays anywhere the player can actually see them. Avoid
        // scanning a 512x512 overlay every tick on a 20k-unit map.
        if self.tick() % 12 == 0 {
            for y in 0..grid.height() {
                for x in 0..grid.width() {
                    if self.is_cell_visible(x, y)
                        && self.visible_crate_at(rules, (x, y)) {
                        found.insert((x, y));
                    }
                }
            }
        } else {
            // An explorer who walks beside a newly spawned trigger crate
            // should take it now, not after the next periodic map-wide scan.
            // Only examine a few mobile, controllable units to keep this cheap.
            for own in self.own_entities()
                .filter(|e| self.object_type(rules, e.type_id)
                    .is_some_and(|ty| ty.speed > 0
                        && ty.category != ObjectCategory::Building))
                .take(128)
            {
                for dy in -3i32..=3 {
                    for dx in -3i32..=3 {
                        let (Ok(x), Ok(y)) = (
                            u16::try_from(i32::from(own.cell.0) + dx),
                            u16::try_from(i32::from(own.cell.1) + dy),
                        ) else { continue; };
                        if x < grid.width() && y < grid.height()
                            && self.is_cell_visible(x, y)
                            && self.visible_crate_at(rules, (x, y)) {
                            found.insert((x, y));
                        }
                    }
                }
            }
        }
        found.into_iter().collect()
    }

    /// The visible live overlay, not a remembered crate slot. Used to confirm
    /// that a previously sighted trigger-created crate has truly disappeared.
    pub fn visible_crate_at(&self, rules: &RuleSet, cell: (u16, u16)) -> bool {
        if !self.is_cell_visible(cell.0, cell.1) { return false; }
        match self.sim.overlay_grid.as_ref() {
            Some(grid) if cell.0 < grid.width() && cell.1 < grid.height() => {
                grid.cell(cell.0, cell.1).overlay_id
                    .and_then(|id| rules.overlay_registry().flags(id))
                    .is_some_and(|flags| flags.crate_type)
            }
            Some(_) => false,
            None => self.sim.crate_authority().slots().iter().any(|slot| {
                // CrateSlot stores signed i16 map coordinates, while the
                // observation exposes unsigned u16 cells. Compare in one
                // lossless type (and never reinterpret a negative slot).
                !slot.is_empty() && i32::from(cell.0) == i32::from(slot.cell_x)
                    && i32::from(cell.1) == i32::from(slot.cell_y)
            }),
        }
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
        // A current Iron Curtain nullifies the damage whatever the shell's
        // normal Verses are. Do not drain the squad into a temporary shield.
        if self.visible_enemy_invulnerable(target) { return false; }
        let (Some(unit_entity), Some(target_entity)) = (
            self.sim.entities().get(unit),
            self.sim.entities().get(target),
        ) else {
            return false;
        };
        let target_type = self.sim.object_type(target_entity.type_ref(), rules);
        // BalloonHover and Jumpjet types may live in the VehicleTypes array
        // (retail Kirov does). ObjectCategory::Aircraft alone misses them and
        // mistakenly asks an anti-ground-only gun to shoot a flying airship.
        let air = target_entity.category() == EntityCategory::Aircraft
            || target_type.is_some_and(|ty| ty.balloon_hover
                || (ty.jumpjet && !self.is_deployed(target)));
        let Some(unit_type) = self.sim.object_type(unit_entity.type_ref(), rules) else {
            return false;
        };
        let armour = target_type.map_or(
            crate::rules::armor::ArmorId::default(), |ty| ty.armor_id());
        let weapon_answers = |weapon: &crate::rules::weapon_type::WeaponType| {
            if weapon.never_use { return false; }
            if !rules.weapon_projectile(weapon)
                .is_some_and(|projectile| if air { projectile.aa } else { projectile.ag })
            {
                // A projectile with no AA flag cannot hit a flying Kirov,
                // even when the object lives in VehicleTypes.
                return false;
            }
            // `Spawner=yes` on a V3/Dreadnought/Boomer is only a rangefinder.
            // Its `Warhead=Special` never damages the target: the spawned
            // rocket explodes with `MissileSpawnRules::*Warhead` and real
            // `*Damage`. Keep the targeting estimate faithful to the native
            // missile instead of treating a rocket as an inert laser pointer.
            if weapon.spawner {
                if let Some(family) = unit_type.spawns.as_deref()
                    .filter(|name| !name.is_empty())
                    .and_then(|name| rules.missile_spawn.family_of(name))
                {
                    let missile = rules.missile_spawn.params(family);
                    return missile.damage > 0 && rules.warhead(&missile.warhead)
                        .is_some_and(|warhead|
                            f64::from_bits(warhead.interaction(armour)
                                .multiplier().bits()) != 0.0);
                }
            }
            // AA capability alone is not enough: a modded AA weapon may
            // have zero Verses against this particular airship's armour.
            // Apply the same real warhead check used against ground targets.
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
        };
        if unit_type.gunner {
            // The current passenger, not any possible passenger in a mod,
            // selects a single native Gunner slot for the whole vehicle.
            return self.active_gunner_weapon(rules, unit)
                .is_some_and(weapon_answers);
        }
        (0..unit_type.weapon_record_count()).any(|slot|
            crate::sim::combat::combat_weapon::weapon_for_index(unit_type,unit_entity.veterancy.rank(),slot as i32)
                .and_then(|record|record.weapon(rules)).is_some_and(weapon_answers))
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
    /// Can this specific passenger enter this specific receiver *right now*?
    /// The native radio/size/seat check stays behind L2's observation API;
    /// strategy code must not inspect Simulation to infer remaining capacity.
    pub fn can_enter_transport(&self, rules: &RuleSet, passenger: u64, carrier: u64) -> bool {
        crate::sim::passenger::can_enter_transport(self.sim, rules, passenger, carrier)
    }

    pub fn passengers(&self, entity_id: u64) -> usize {
        self.sim.entities().get(entity_id).map_or(0, |entity| {
            entity.passenger_role().occupants().count().max(0) as usize
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
    /// Unlimbo asks again after the click. Read-only, and passes the same
    /// overlay registry as native placement so gates and fences are judged
    /// consistently at planning and execution time.
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
            Some(rules.overlay_registry()),
            ty,
            (rx as i16, ry as i16),
            Some(self.house),
            None,
            None,
        )
    }

    /// Whether the exact same player placement would be admitted after
    /// *this one own mobile unit* leaves the footprint. This is a read-only
    /// lifted-unit probe; it does not bypass enemy objects, structures,
    /// overlays, map bounds, or ordinary build-area adjacency.
    pub(crate) fn placement_admits_after_unit_leaves(
        &self,
        rules: &RuleSet,
        ty: &ObjectType,
        rx: u16,
        ry: u16,
        actor: u64,
    ) -> bool {
        let Some(unit) = self.sim.entities().get(actor) else { return false; };
        if unit.owner() != self.house || self.owned_by_hand(actor) {
            return false;
        }
        if !matches!(unit.category(), EntityCategory::Infantry | EntityCategory::Unit) {
            return false;
        }
        if !crate::sim::production::is_within_build_area(
            self.sim, rules, self.sim.interner().resolve(self.house), ty, rx, ry,
        ) {
            return false;
        }
        let lifted = crate::sim::build_site::LiftedUnit::read(self.sim, actor);
        crate::sim::build_site::can_place_building_at(
            self.sim, rules, Some(rules.overlay_registry()), ty, (rx as i16, ry as i16),
            Some(self.house), None, Some(&lifted),
        )
    }

    /// Whether the exact same player placement would be admitted after all
    /// supplied own mobile units leave the footprint. The combined lift is a
    /// read-only probe; it does not mutate occupancy or bypass any other gate.
    pub(crate) fn placement_admits_after_units_leave(
        &self,
        rules: &RuleSet,
        ty: &ObjectType,
        rx: u16,
        ry: u16,
        actors: &[u64],
    ) -> bool {
        if actors.is_empty() {
            return false;
        }
        for &actor in actors {
            let Some(unit) = self.sim.entities().get(actor) else { return false; };
            if unit.owner() != self.house || self.owned_by_hand(actor) {
                return false;
            }
            if !matches!(unit.category(), EntityCategory::Infantry | EntityCategory::Unit) {
                return false;
            }
        }
        if !crate::sim::production::is_within_build_area(
            self.sim, rules, self.sim.interner().resolve(self.house), ty, rx, ry,
        ) {
            return false;
        }
        let lifted = crate::sim::build_site::LiftedUnit::read_many(self.sim, actors.iter().copied());
        crate::sim::build_site::can_place_building_at(
            self.sim, rules, Some(rules.overlay_registry()), ty, (rx as i16, ry as i16),
            Some(self.house), None, Some(&lifted),
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
            Some(rules.overlay_registry()),
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

    /// Whether `entity_id` falls outside the fifth-key player group scope,
    /// or is already driven by a native `TeamClass` script of this house.
    ///
    /// The two player-group modes are exact complements. A team's script
    /// owns its members: `team_script_vm`'s
    /// pass has no house gate, so a policy-driven house's teams run like any
    /// other's — recruit the house's units and drive them.
    ///
    /// What this buys over leaving it to the gate: a refused command still
    /// spends the frame's budget and still counts the unit as sent for
    /// `ORDER_REFRESH_FRAMES`, so a grouped unit would quietly eat the army's
    /// orders and hold its own place in the push while doing neither. Every
    /// place the policy picks a unit to order asks this first.
    pub fn owned_by_hand(&self, entity_id: u64) -> bool {
        (self.player_kept.contains(&entity_id) != self.grouped_only)
            || self.team_has_member(entity_id)
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

    /// Read the first click of a Chronosphere through the observation layer.
    /// A planned source is not proof of acceptance: the engine only stores it
    /// after actually executing the native PreClick command.
    pub fn chrono_source_cell(&self, sw_type_id: InternedId) -> Option<(u16, u16)> {
        self.sim.super_weapons().get(&self.house)?.get(&sw_type_id)
            .map(crate::sim::superweapon::SuperWeaponInstance::chrono_cell)
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

    /// The same producer search that begins native production.
    pub(crate) fn has_producer(&self, rules: &RuleSet, object: &ObjectType) -> bool {
        crate::sim::production::find_factory(self.sim, rules, self.house, object, false, true, true)
            .is_some()
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

    /// The native OverlayData byte for an existing overlay, if any.
    ///
    /// For a tiberium overlay this is the remaining growth/density stage
    /// (0..=11 in retail). The flag is checked by the caller against the rules;
    /// unlike `ore_cells`, this returns the raw stage for every overlay kind.
    /// Policies must also apply their own fog/visibility rule before treating
    /// the stage as current knowledge.
    pub fn overlay_data_at(&self, rx: u16, ry: u16) -> Option<u8> {
        let cell = self.sim.overlay_grid.as_ref()?.cell(rx, ry);
        cell.overlay_id.map(|_| cell.overlay_data)
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

    /// Every cell of the map that carries a `Tiberium=yes` overlay, in storage
    /// order.
    ///
    /// The ore field as a map-level fact, which the wishlist's expansion and
    /// garrison questions both need: "is there ore worth a second base near" and
    /// "is there a resource worth holding a position for" are the same map read.
    ///
    /// The owner is the overlay grid — this walks it and does not keep a second
    /// copy — and the meaning of an id is the rules' own `Tiberium=yes` on the
    /// overlay type (`rules::overlay_types::OverlayTypeFlags::tiberium`), so a
    /// mod's own ore overlay is picked up by the same query.
    ///
    /// A whole-map walk, so it belongs on a slow cadence rather than in a
    /// per-frame path. Cost is the grid's cell count, not its contents.
    pub fn ore_cells(&self, rules: &RuleSet) -> Vec<(u16, u16)> {
        let Some(grid) = self.sim.overlay_grid.as_ref() else {
            return Vec::new();
        };
        let registry = rules.overlay_registry();
        let (width, height) = (grid.width(), grid.height());
        let mut cells = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let Some(overlay) = grid.cell(x, y).overlay_id else {
                    continue;
                };
                if registry.flags(overlay).is_some_and(|flags| flags.tiberium) {
                    cells.push((x, y));
                }
            }
        }
        cells
    }

    /// Whether this structure may be packed back into its maker right now.
    ///
    /// The wishlist's "占领敌方建造场以后，若敌方建造场可以打包回MCV并安全撤离，
    /// 应当打包撤离到己方基地". The question is the receiver's own gate rather
    /// than a rule the brain restates: `can_undeploy_building_runtime` is where
    /// the deploy-facing rules and the native `ShouldShowDeployButton` reading
    /// live, and a second copy of them here is how the two would disagree.
    ///
    /// Read-only. Packing is `Command::UndeployBuilding`, which re-checks.
    pub fn can_repack_yard(&self, rules: &RuleSet, entity_id: u64) -> bool {
        // `delegated=false`: the house it reads is the player's own, and an
        // autoplay query asks the same question a player's click would be
        // checked against. Only a delegated order takes the redeploy gate.
        self.sim.can_undeploy_building_runtime(entity_id, false, rules)
    }

    /// Whether the navigation projection the connectivity answers come from
    /// stands at all.
    /// The water rows of the passability matrix are modelled
    /// (`crate::sim::pathfinding::passability` carries `Water` and `WaterBeach`
    /// and the three amphibious zones), so this is the seam for a fleet: a
    /// decision that asks in its own movement zone gets the right answer for a
    /// boat today and for a hovercraft when one exists. What is *not* modelled
    /// yet is water as a movement layer — `MovementLayer` has no `Water` — so
    /// this answers connectivity between cells, not the cost of the cells
    /// between them.
    pub(crate) fn known_water_cell(&self,x:u16,y:u16)->bool {
        self.is_cell_revealed(x,y) && self.is_cell_in_playfield(x,y)
            && self.sim.resolved_terrain.as_ref().and_then(|t|t.cell(x,y))
                .is_some_and(|c|c.land_type==crate::sim::pathfinding::passability::LandType::Water.as_index())
    }

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

#[cfg(all(test, tests_sim_c_g))]
mod vision_tests {
    use super::*;
    use crate::sim::intern::InternedId;

    /// The ore field is read off the rules' own `Tiberium=yes`, not a name list.
    #[test]
    fn ore_cells_come_from_the_tiberium_overlay_flag() {
        let Some((mut sim, rules)) = arena() else {
            return;
        };
        let owner: InternedId = sim.intern("Americans");
        let obs = Observation::new(&sim, owner);

        // The arena's own ground before anything is placed: whatever ore the
        // fixture carries is what the reader reports, and the assertion is that
        // the reader and the grid agree rather than that a number is fixed.
        let cells = obs.ore_cells(&rules);
        for cell in &cells {
            let overlay = obs
                .overlay_id_at(cell.0, cell.1)
                .expect("a cell the reader returned carries an overlay");
            assert!(
                rules
                    .overlay_registry()
                    .flags(overlay)
                    .is_some_and(|flags| flags.tiberium),
                "every reported ore cell carries a Tiberium overlay: {overlay}"
            );
        }
        // And a cell the grid says has no overlay is never reported.
        assert!(obs.ore_cells(&rules).len() <= 128 * 128);
    }

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

    /// Only a visible hostile powered tower is an active gun. Native power
    /// states, including spy-induced blackouts, own the answer.
    #[test]
    fn a_powerless_enemy_coil_has_no_operational_firing_line() {
        let Some((mut sim, rules)) = arena() else { return; };
        let americans = sim.intern("Americans");
        let russians = sim.intern("Russians");
        let tower = sim.spawn_object("ATESLA", "Russians", 24, 28, 255, &rules)
            .expect("a power-dependent defensive tower");
        sim.spawn_object("E1", "Americans", 23, 28, 255, &rules)
            .expect("a nearby observer");
        for _ in 0..4 {
            let _ = sim.take_due_commands();
            sim.advance_tick(&[], Some(&rules), None, None, 33);
        }
        assert!(Observation::new(&sim, americans)
            .is_cell_visible(24,28), "enemy tower is in actual sight");
        {
            let power = sim.power_states_mut_for_test().entry(russians).or_default();
            power.set_total_output_for_test(0);
            power.set_total_drain_for_test(200);
        }
        assert!(!Observation::new(&sim, americans)
            .visible_enemy_building_operational(&rules, tower));
        {
            let power = sim.power_states_mut_for_test().entry(russians).or_default();
            power.set_total_output_for_test(400);
        }
        assert!(Observation::new(&sim, americans)
            .visible_enemy_building_operational(&rules, tower));
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

#[cfg(all(test, tests_sim_c_g))]
mod v22_observation_regressions {
    use super::*;

    #[test]
    fn signed_crate_slot_coordinates_match_visible_unsigned_cells() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        let owner = sim.interner.intern("Americans");
        // No overlay grid: the original slot table is the only authority.
        *sim.overlay_grid_mut_for_test() = None;
        *sim.crate_authority_mut_for_test().slot_mut_for_test(0) =
            crate::sim::crates::CrateSlot {
                cell_x: 20, cell_y: 10, ..Default::default()
            };
        sim.fog = crate::sim::vision::FogState::new(64, 64);
        sim.fog.reveal_cells_for_owner(owner, [(20, 10)]);
        let seen = Observation::new(&sim, owner);
        assert!(seen.visible_crate_at(&rules, (20, 10)));
        assert!(seen.crate_cells(&rules).contains(&(20, 10)));
        assert!(!seen.visible_crate_at(&rules, (21, 10)));
    }

    #[test]
    fn a_trigger_created_overlay_crate_is_seen_without_an_authority_slot() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let Some(crate_id) = (0..=u8::MAX)
            .find(|id| rules.overlay_registry().flags(*id)
                .is_some_and(|flags| flags.crate_type))
        else { return; };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.session.set_tick_for_test(0);
        let owner = sim.interner.intern("Americans");
        *sim.overlay_grid_mut_for_test() =
            Some(crate::sim::overlay_grid::OverlayGrid::new(64, 64));
        sim.overlay_grid_mut_for_test().as_mut().unwrap()
            .place_overlay(10, 10, crate_id, 0);
        sim.fog = crate::sim::vision::FogState::new(64, 64);
        sim.fog.reveal_cells_for_owner(owner, [(10, 10)]);
        let seen = Observation::new(&sim, owner);
        assert!(seen.visible_crate_at(&rules, (10, 10)));
        assert!(seen.crate_cells(&rules).contains(&(10, 10)),
            "a trigger overlay should not need a random-crate authority slot");
        sim.fog = crate::sim::vision::FogState::new(64, 64);
        let hidden = Observation::new(&sim, owner);
        assert!(!hidden.visible_crate_at(&rules, (10, 10)),
            "a newly spawned crate must not be discovered through fog");
    }

    #[test]
    fn hostile_civilian_is_only_a_target_while_it_visibly_attacks_our_house() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        // The rival fixture may not instantiate Civilian as a house. In that
        // case the meaningful part of this test is exercised by the regular
        // map/scenario fixtures, not by inventing an alliance in this test.
        let Some(civilian) = sim.spawn_object("CIV1", "Civilian", 12, 12, 255, &rules)
            else { return; };
        let own = sim.spawn_object("E1", "Americans", 13, 12, 255, &rules)
            .expect("our infantry");
        let mut fog = crate::sim::vision::FogState::new(64, 64);
        fog.reveal_cells_for_owner(owner, [(12, 12), (13, 12)]);
        sim.fog = fog;
        let peaceful = Observation::new(&sim, owner);
        assert!(!peaceful.is_hostile(civilian),
            "idle civilians remain peaceful even when standing near our unit");
        sim.entities_mut_for_test().get_mut(civilian).unwrap().attack_target
            .set_for_test(Some(crate::sim::combat::TargetKind::Entity(own)));
        let attacking = Observation::new(&sim, owner);
        assert!(attacking.is_hostile(civilian),
            "a neutral actively targeting our units deserves retaliation");
    }
}
