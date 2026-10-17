//! A computer house's economic recovery: the strategy tick's build need
//! (`HouseClass::AI_Check_Build_Need @ 0x004FD9A0`, with `AI_Can_Make_Money
//! @ 0x004F6540`) and the rebuild it asks for (`HouseClass::
//! AI_Manage_Build_Queue @ 0x004FDD10`).
//!
//! Native owner: `HouseClass`. In a nonzero game mode the strategy tick
//! (`sim::house_strategy`) asks for the need before it acts (urgency slot 1,
//! `0x004FD86C`) and runs Manage_Build_Queue for it after the missing
//! factory's sell-off (`0x004FD8E8`); neither has another caller.
//!
//! A house that can no longer make money ([`can_make_money`]) and cannot pay
//! for its next refinery, war factory or harvester ([`check_build_need`])
//! picks one to rebuild, sells buildings from the end of its base plan until
//! its balance and the refunds to come exceed the price, abandons all its
//! production and chooses again: the harvester (production mode 2), or the
//! building at its first open plan node (mode 1, through
//! `AI_Choose_Building`). A house that sells down to its first node, or
//! cannot pay even then, sends everything to hunt ([`manage_build_queue`]).
//!
//! Draws: none of their own; `AI_Choose_Building` draws as ported, and
//! All_To_Hunt's damage through ReceiveDamage. Timers: each sale starts the
//! Selling mission (`production::sell_back`), and the deleted factories'
//! step timers go with them. Detaches: each abandoned object's destruction
//! (`production::abandon_house_factories`) and a sold wall's cell
//! (`Simulation::sell_wall_at_cell`).
//!
//! Evidence: instruction reading of the three bodies and the callees named
//! on each function.
//!
//! RESIDUALS:
//! - Native null dereferences VERA defines; no retail country reaches them.
//!   Can_Make_Money reads its country's first `[General] BaseUnit=`
//!   (`0x004F65BF`), its first buildable `[AI] BuildWeapons=`
//!   (`0x004F671D`) and, without a harvester, its first buildable
//!   `BuildRefinery=` (`0x004F6794`) unchecked: VERA counts no base unit and
//!   answers that a house missing either building cannot make money.
//!   Manage_Build_Queue reads the war factory it would rebuild unchecked
//!   (`0x004FDE63`): VERA rebuilds nothing. Yuri's country has no harvester
//!   (`HarvesterUnit=HARV,CMIN`) and a refinery (YAREFN), so its houses
//!   take the arms without a harvester.
//! - Check_Build_Need reads `BuildRefinery=`'s second entry unchecked
//!   (`0x004FDA8F`); VERA matches no entry past the list. Retail lists three.
//! - The sale's upgrade arm (`0x004FDEDE..0x004FDF27`) is dormant: no retail
//!   type sets `PowersUpBuilding=`, and VERA keeps no upgrade.
//! - Dormant in retail data, where every country can build a refinery:
//!   Can_Make_Money's arms without one, Manage_Build_Queue's harvester for a
//!   house with a war factory and no buildable refinery, and its war factory
//!   rebuild with the later nodes' dedupe (`0x004FDE51`,
//!   `0x004FE136..0x004FE2BD`).
//! - Manage_Build_Queue's exit for a plan vector of capacity (`+0x570C`)
//!   below 2 (`0x004FE14D`, `0x004FE2D5`) is not ported, as VERA keeps no
//!   capacity: nodes join through the vector's Add, which grows it by 10
//!   (`0x0042E709`), so a plan the sale walked has room for 10.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_base_building::{choose_building, find_node, node_building, node_satisfied};
use crate::sim::ai_buildable::{
    first_buildable_from_array, first_owner_compatible, first_owner_compatible_harvester,
    house_country_bit,
};
use crate::sim::ai_unit_choice::UnitChoiceKind;
use crate::sim::base_plan::{BasePlanNode, unpack_base_plan_cell};
use crate::sim::credit_income::available_money;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::production::{SellOrder, can_sell_building, sell_back, techno_refund};
use crate::sim::world::{FrameEffects, Simulation};

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Observation only: the [`manage_build_queue`] calls on this thread
    /// while a test holds `Some`.
    pub(crate) static MANAGE_CALLS: std::cell::Cell<Option<u32>> =
        const { std::cell::Cell::new(None) };
}

/// The economy types of a house's country: its first buildable `[AI]
/// BuildRefinery=` and `BuildWeapons=` types (`HouseClass::
/// FirstBuildableFromArray @ 0x005051E0`) and its first `[General]
/// HarvesterUnit=` (the `Owner=` scan each body inlines).
struct EconomyTypes<'r> {
    refinery: Option<&'r ObjectType>,
    war_factory: Option<&'r ObjectType>,
    harvester: Option<&'r ObjectType>,
}

impl<'r> EconomyTypes<'r> {
    fn of(sim: &Simulation, rules: &'r RuleSet, house: &HouseState) -> Self {
        let country = sim.interner.resolve(house.house_type_id());
        let super_weapons = sim.session.game_options.super_weapons;
        Self {
            refinery: first_buildable_from_array(
                rules,
                &rules.build_refinery_types,
                country,
                house.side_index,
                super_weapons,
            ),
            war_factory: first_buildable_from_array(
                rules,
                &rules.build_weapons_types,
                country,
                house.side_index,
                super_weapons,
            ),
            harvester: first_owner_compatible_harvester(rules, house_country_bit(rules, country)),
        }
    }
}

/// `HouseClass::AI_Can_Make_Money @ 0x004F6540`: whether house `owner`
/// still earns, or can buy its way back. It does with a resource destination
/// (`+0x15C`) and a gatherer (`+0x158`) on the map. Otherwise its money
/// (`Available_Money`, IHouse vt+0x18) meets the prices (`Cost_Of`) of its
/// [`EconomyTypes`] (a missing harvester costs `0x7FFFFFFF`), and a base is a
/// Construction Yard (`+0x60`) or its country's first `[General] BaseUnit=`
/// on the map (`+0x5564`):
/// - no buildable refinery, a harvester: a gatherer; a war factory
///   (`+0x160`) and the harvester's price; or a base and the war factory's
///   and the harvester's;
/// - a buildable refinery, no harvester: a gatherer, or a base and the
///   refinery's price;
/// - both, with a destination: a war factory and the harvester's price, or a
///   base and the war factory's and the harvester's, or the refinery's;
/// - both, without one: a base and more than the refinery's price.
fn can_make_money(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let tracking = &house.tracking;
    let destinations = tracking.resource_destinations() > 0;
    let gatherers = tracking.resource_gatherers() > 0;
    let war_factories = tracking.war_factories() > 0;
    let country_bit = house_country_bit(rules, sim.interner.resolve(house.house_type_id()));
    let base = !house.build_const_order.is_empty()
        || first_owner_compatible(
            rules,
            &rules.general.base_unit_types,
            ObjectCategory::Vehicle,
            country_bit,
        )
        .and_then(|unit| sim.interner.get(&unit.id))
        .is_some_and(|unit| tracking.active_count(EntityCategory::Unit, unit) != 0);
    if destinations && gatherers {
        return true;
    }
    let money = available_money(sim, owner);
    let types = EconomyTypes::of(sim, rules, house);
    let price = |object: &ObjectType| sim.cost_of(owner, object, rules);
    let harvester = types.harvester.map_or(i32::MAX, price);
    let Some(war_factory) = types.war_factory.map(price) else {
        return false;
    };
    let war_factory_and_harvester = war_factory.wrapping_add(harvester);
    match (types.refinery.map(price), types.harvester.is_some()) {
        (None, true) => {
            gatherers
                || (war_factories && money >= harvester)
                || (base && money >= war_factory_and_harvester)
        }
        (None, false) => false,
        (Some(refinery), false) => gatherers || (base && money >= refinery),
        (Some(refinery), true) if destinations => {
            (war_factories && money >= harvester)
                || (base && (money >= war_factory_and_harvester || money >= refinery))
        }
        (Some(refinery), true) => base && money > refinery,
    }
}

/// `HouseClass::AI_Check_Build_Need @ 0x004FD9A0`: whether house `owner`, no
/// human's and unable to make money ([`can_make_money`]), must rebuild
/// ([`manage_build_queue`]). Its money (`Available_Money`) decides:
/// - With a resource destination: not while one of the first `+0x78`
///   buildings of BuildingClass::Array, whoever owns them, is a
///   `BuildRefinery=` first or second type on Construction
///   ([`refinery_under_construction`]); always without a harvester type;
///   while the Unit choice (`+0x5650`) is another type, when the harvester's
///   price exceeds it; otherwise when the Balance still owed by the house's
///   first factory holding a `ResourceGatherer=` Unit does
///   ([`balance_owed`]), or without one.
/// - Without one: for its first buildable refinery, else war factory (always
///   without both): when that is the building choice (`+0x564C`), as for the
///   harvester with the factory holding that building; otherwise when its
///   price exceeds the money.
pub(crate) fn check_build_need(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    if house.is_controlled_by_human(sim.session.game_mode_nonzero)
        || can_make_money(sim, rules, owner)
    {
        return false;
    }
    let money = available_money(sim, owner);
    let types = EconomyTypes::of(sim, rules, house);
    if house.tracking.resource_destinations() > 0 {
        if refinery_under_construction(sim, rules, house) {
            return false;
        }
        let Some(harvester) = types.harvester else {
            return true;
        };
        let chosen = rules.type_array_index(ObjectCategory::Vehicle, &harvester.id)
            == Some(house.ai_unit_choices.get(UnitChoiceKind::Unit));
        if !chosen {
            return money < sim.cost_of(owner, harvester, rules);
        }
        return balance_owed(sim, rules, owner, |object| {
            object.category == ObjectCategory::Vehicle && object.resource_gatherer
        })
        .is_none_or(|owed| money < owed);
    }
    let Some(building) = types.refinery.or(types.war_factory) else {
        return true;
    };
    if house.ai_production.building_choice() == building.base_plan_type_index {
        return balance_owed(sim, rules, owner, |object| {
            object.category == ObjectCategory::Building && object.id == building.id
        })
        .is_none_or(|owed| money < owed);
    }
    money < sim.cost_of(owner, building, rules)
}

/// `0x004FDA56..0x004FDAA7`: among the first buildings of BuildingClass::Array
/// (construction order here), as many as the house has in its own list
/// (`+0x78`) and whoever owns them, one whose type is `[AI]
/// BuildRefinery=`'s first or second and whose mission (`+0xAC`) is
/// Construction.
fn refinery_under_construction(sim: &Simulation, rules: &RuleSet, house: &HouseState) -> bool {
    let refineries: Vec<InternedId> = rules
        .build_refinery_types
        .iter()
        .take(2)
        .filter_map(|id| sim.interner.get(id))
        .collect();
    let construction = MissionId::from_known(MissionType::Construction);
    sim.substrate
        .entities
        .values()
        .filter(|entity| entity.category() == EntityCategory::Structure)
        .take(house.base_projection.buildings().len())
        .any(|building| {
            building.mission.current() == construction && refineries.contains(&building.type_ref())
        })
}

/// The Balance (`Factory+0x60`) still owed by house `owner`'s first factory
/// in FactoryClass::Array order whose object (`FactoryClass::GetObject @
/// 0x004CA160`) `holds` accepts by its type (`0x004FDB0E..0x004FDB79`,
/// `0x004FDC0C..0x004FDC70`).
fn balance_owed(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    holds: impl Fn(&ObjectType) -> bool,
) -> Option<i32> {
    sim.production
        .factories
        .holders_insertion_ordered()
        .into_iter()
        .map(|(_, factory)| factory)
        .filter(|factory| factory.owner == owner)
        .find(|factory| {
            factory
                .object
                .as_ref()
                .and_then(|object| sim.object_type(object.type_id, rules))
                .is_some_and(&holds)
        })
        .map(|factory| factory.balance)
}

/// What Manage_Build_Queue rebuilds (`0x004FDD3F..0x004FDE96`).
#[derive(Clone, Copy)]
enum Rebuild<'r> {
    Harvester(&'r ObjectType),
    Refinery(&'r ObjectType),
    WarFactory(&'r ObjectType),
}

/// `HouseClass::AI_Manage_Build_Queue @ 0x004FDD10` for house `owner`. It
/// rebuilds, at its price (`Cost_Of`), the harvester while the house has a
/// resource destination (`+0x15C`) and a war factory (`+0x160`), or has a
/// war factory and no buildable refinery; else the refinery; else the war
/// factory while it has none; else nothing.
///
/// From the last base node down (`0x004FDE9A..0x004FE04A`) it sells each
/// node's building ([`sell_node`]) until its Balance (`+0x30C`) and the
/// refunds to come exceed the price. Then ([`restart_production`]) it
/// abandons all its production and chooses again. When the money never
/// suffices, or suffices only at the first node, everything hunts
/// (All_To_Hunt, IHouse vt+0x38, `0x004FE3BA..0x004FE3C9`).
pub(crate) fn manage_build_queue(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    #[cfg(any(test, feature = "test-support"))]
    MANAGE_CALLS.with(|calls| calls.set(calls.get().map(|count| count + 1)));
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    let types = EconomyTypes::of(sim, rules, house);
    let war_factories = house.tracking.war_factories();
    let refinery_and_factory = house.tracking.resource_destinations() > 0 && war_factories > 0;
    let rebuild = match (types.harvester, types.refinery) {
        (Some(harvester), refinery)
            if refinery_and_factory || (refinery.is_none() && war_factories > 0) =>
        {
            Rebuild::Harvester(harvester)
        }
        (_, Some(refinery)) => Rebuild::Refinery(refinery),
        (_, None) if war_factories == 0 => match types.war_factory {
            Some(war_factory) => Rebuild::WarFactory(war_factory),
            None => return,
        },
        // `0x004FDE4B`.
        (_, None) => return,
    };
    let (Rebuild::Harvester(object) | Rebuild::Refinery(object) | Rebuild::WarFactory(object)) =
        rebuild;
    let price = sim.cost_of(owner, object, rules);

    let nodes = sim
        .houses
        .get(&owner)
        .map_or(0, |house| house.base_plan.nodes.len());
    let mut index = nodes as i32 - 1;
    let mut sale = Sale::default();
    let paid = loop {
        let Ok(at) = usize::try_from(index) else {
            break false;
        };
        sell_node(sim, rules, owner, at, &mut sale, registry, effects);
        let balance = sim
            .houses
            .get(&owner)
            .map_or(0, |house| house.economy.credits());
        if balance.wrapping_add(sale.refunds) > price {
            break true;
        }
        index -= 1;
    };
    if paid
        && !restart_production(
            sim,
            rules,
            owner,
            rebuild,
            sale.slot_zero,
            registry,
            effects,
        )
    {
        crate::sim::house_strategy::all_to_hunt(sim, rules, owner, registry, effects);
        return;
    }
    if index <= 0 {
        crate::sim::house_strategy::all_to_hunt(sim, rules, owner, registry, effects);
    }
}

/// What Manage_Build_Queue's sale has gathered.
#[derive(Default)]
struct Sale {
    /// The refunds to come (`[esp+0x1C]`).
    refunds: i32,
    /// Slot 0's bytes once the sale empties the plan: the vector's delete
    /// moves nothing past the count, so they stay the node last removed.
    slot_zero: Option<BasePlanNode>,
}

/// One node of Manage_Build_Queue's sale (`0x004FDEB4..0x004FE01C`), at
/// `index`. A node building (`0x0042E820`) that may be sold (CanSell, vt+0x98)
/// gets the computer's sale (`Sell_Back(1)`, vt+0x1A0), its `Gate=` node goes
/// (`Type+0x16B7`) and its refund ([`techno_refund`]) joins the sale's,
/// even when the sale stood down. Otherwise a built node (`0x0042E780`) has
/// its cell's wall sold (`Sell_Building_At_Cell(cell, 0)`) and goes.
fn sell_node(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    index: usize,
    sale: &mut Sale,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let node = sim.houses[&owner].base_plan.nodes[index];
    let building = node_building(sim, owner, &node).filter(|&id| can_sell_building(sim, rules, id));
    if let Some(id) = building {
        sell_back(sim, rules, id, SellOrder::Computer, registry, effects);
        let gate = sim
            .substrate
            .entities
            .get(id)
            .and_then(|building| sim.object_type(building.type_ref(), rules))
            .map(|object| object.gate);
        let (Some(gate), Some(refund)) = (gate, techno_refund(sim, rules, id)) else {
            return;
        };
        if gate {
            remove_node(sim, owner, index, sale);
        }
        sale.refunds = sale.refunds.wrapping_add(refund);
        return;
    }
    if !node_satisfied(sim, rules, owner, &node, registry) {
        return;
    }
    if let Some(overlays) = registry {
        let (x, y) = unpack_base_plan_cell(node.packed_cell);
        let house = sim.interner.resolve(owner).to_string();
        sim.sell_wall_at_cell(&house, x, y, rules, overlays, effects);
    }
    remove_node(sim, owner, index, sale);
}

/// The ordered removal of node `index`.
fn remove_node(sim: &mut Simulation, owner: InternedId, index: usize, sale: &mut Sale) {
    let Some(house) = sim.houses.get_mut(&owner) else {
        return;
    };
    let node = house.base_plan.nodes.get(index).copied();
    house.base_plan.remove(index);
    if house.base_plan.nodes.is_empty() {
        sale.slot_zero = node;
    }
}

/// Manage_Build_Queue once the money suffices (`0x004FE04F..0x004FE3B5`):
/// every factory of the house is abandoned
/// ([`crate::sim::production::abandon_house_factories`]) and its four
/// choices cleared (`+0x564C..+0x5658`). A harvester becomes the Unit choice
/// in production mode 2. A building takes a new node (no cell) at the first
/// open node ([`find_node`]), or at 1 without one, and the house, in mode 1,
/// makes its building choice ([`choose_building`]). Returns false when the
/// first open node is the first node (`0x004FE143`, `0x004FE2CB`), where
/// native sends everything to hunt and stops.
fn restart_production(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    rebuild: Rebuild<'_>,
    slot_zero: Option<BasePlanNode>,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    crate::sim::production::abandon_house_factories(sim, rules, owner, effects);
    let Some(house) = sim.houses.get_mut(&owner) else {
        return true;
    };
    house.ai_unit_choices = Default::default();
    house.ai_production.clear_building_choice();
    let building = match rebuild {
        Rebuild::Harvester(harvester) => {
            if let Some(index) = rules.type_array_index(ObjectCategory::Vehicle, &harvester.id) {
                house.ai_unit_choices.set(UnitChoiceKind::Unit, index);
            }
            house.ai_production.set_mode(2);
            return true;
        }
        Rebuild::Refinery(building) | Rebuild::WarFactory(building) => building,
    };
    let index = match find_node(sim, rules, owner, -1, registry) {
        Some(0) => return false,
        Some(index) => index,
        // Native first tests the vector's capacity (`+0x570C`) against 2;
        // with a node at entry it has grown by 10 (`0x0042E709`), so it goes
        // on at 1.
        None => 1,
    };
    let node = BasePlanNode {
        type_or_control: building.base_plan_type_index,
        packed_cell: 0,
        filled: false,
        retry_count: 0,
    };
    let Some(house) = sim.houses.get_mut(&owner) else {
        return true;
    };
    let nodes = &mut house.base_plan.nodes;
    if index > nodes.len() {
        // At 1 on a plan the sale emptied, native writes the slot past the
        // count, which grows to take in slot 0: the node last removed.
        nodes.extend(slot_zero);
    } else {
        nodes.insert(index, node);
        // `0x004FE226..0x004FE2BD`: later nodes of the war factory's type
        // go, but only when `BuildRefinery=`'s first type is that type too.
        let refinery_first = rules
            .build_refinery_types
            .first()
            .and_then(|id| rules.object_in_category(ObjectCategory::Building, id))
            .map(|refinery| refinery.base_plan_type_index);
        if matches!(rebuild, Rebuild::WarFactory(_)) && refinery_first == Some(node.type_or_control)
        {
            let later = nodes.split_off(index + 1);
            nodes.extend(
                later
                    .into_iter()
                    .filter(|later| later.type_or_control != node.type_or_control),
            );
        }
    }
    house.ai_production.set_mode(1);
    choose_building(sim, rules, owner, registry);
    true
}

#[cfg(test)]
#[path = "ai_build_need_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ai_build_need_retail_tests.rs"]
mod retail_tests;
