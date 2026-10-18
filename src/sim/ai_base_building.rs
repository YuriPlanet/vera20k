//! The computer's production choices, its building choice, the BasePlan
//! node it builds from and the Construction Yard's placement of the finished
//! building.
//!
//! Native owners: `HouseClass` holds the production mode (`+0x1E4`), the
//! BuildingType its Construction Yard makes next (`+0x564C`) and the naval
//! latch (`+0x1F0`); its update makes its choices every eight frames
//! (`HouseClass::Update @ 0x004F9038..0x004F9265`,
//! [`update_production_choices`]: `AI_Choose_Building @ 0x004FE3E0` and the
//! unit choosers of `sim::ai_unit_choice`, by mode) and the exit of a
//! produced object steps the mode (`EconomyStateMachine @ 0x00509700`). The
//! node lookup is `BaseClass`'s (`0x0042EB50` with `0x0042E780`,
//! `0x0042E820` and `0x0050CAD0`); a `-1` node walls in a
//! `ProtectWithWall=` building ([`build_walls`]) or becomes a base defense
//! (`sim::ai_base_defense`). The yard
//! (`production::factory_ai`) makes the choice (`Suggest_New_Object @
//! 0x004FBD80`), places it (`BuildingClass::Exit_Object @ 0x00443C60`'s
//! building case, with the site search in `sim::ai_base_site` and the site
//! clearing in `sim::build_site`) and records it (`Record_Last_Built @
//! 0x004FB6B0`, owned by `production::factory_lifecycle`, which also takes
//! the player's placements and every delivered unit).
//!
//! Evidence: instruction reading of the bodies named on each function; the
//! draws, the mode table and the walls are executed by
//! `tools/ai_base_building_oracle.py` (see the tests in
//! `ai_base_building_tests.rs`), whose chooser rows run the walls and answer
//! the defense choice with failure.
//!
//! The strategy tick's economic recovery (`AI_Manage_Build_Queue @
//! 0x004FDD10`, `sim::ai_build_need`) also writes the mode, clears the
//! choices, sells and splices nodes and makes the building choice.
//!
//! RESIDUALS:
//! - Native reads VERA defines: the wall percent is read unchecked at the
//!   House difficulty (VERA takes 0, no walls, past the vector's end, which
//!   retail's three values never reach).
//! - A `-3` node is removed without `0x005082C0`'s perimeter scan; only a map
//!   plan can hold one.
//! - Dormant in retail data: the `PowersUpBuilding=` arms of `0x0042E820` and
//!   `AI_Choose_Building` (`0x004FE953..0x004FEA3A`); no retail type sets the
//!   key and VERA reads no upgrade (as `production_sell` and
//!   `building_missions` record). The exit's `FirestormWall=` placement
//!   after Unlimbo (`0x00445355..0x004453AE`, `0x00588570`); retail
//!   `rulesmd.ini` sets the key on no type.
//! - `Record_Last_Built` and Building discovery retain class-local type indices
//!   through HouseState for built-type triggers19..22. The remaining `+0x246`
//!   notification restarts a90-frame House timer (`0x004F8DF6..0x004F8E21`);
//!   no gameplay consumer of that retained timer has been established.
//!   The shared production owner now requests the existing House+1FC recheck
//!   after accounting and the produced type's CreateSound/class fallback
//!   (4FB7CA..4FB804). General retained-cameo admission has its separate
//!   residual in production_tech; this does not close the whole House tail.
//! - Native crash or out-of-range reads that VERA defines: a node index past
//!   the BuildingType array or a negative control other than -1/-2/-3 reads
//!   memory past the array (`0x004FE537`, `0x004FE6F6`); a null power plant
//!   slot is dereferenced (`0x004FE8A9`, `0x00505368`). VERA makes no choice
//!   for such a node and splices nothing for a missing plant.

use crate::rules::object_type::{FactoryType, ObjectCategory, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_base_site::{SiteKey, find_base_building_site, reserved_near};
use crate::sim::ai_unit_choice::{self, UnitChoiceKind};
use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell, unpack_base_plan_cell};
use crate::sim::build_site::{Flush, can_place_building_at, flush_for_placement};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::production::find_factory;
use crate::sim::world::FrameEffects;
use crate::sim::world::{PlacementEvidence, Simulation};

/// The computer's production state on its House.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseAiProduction {
    /// `HouseClass+0x1E4`, the mode `EconomyStateMachine` steps and the
    /// chooser block dispatches on.
    mode: i32,
    /// `HouseClass+0x564C`, the BuildingType index the yard's next factory
    /// makes (`Suggest_New_Object @ 0x004FBD80`, `0x004FBDDC`); -1 is none.
    building_choice: i32,
    /// `HouseClass+0x1F0`: while clear, the choice skips naval nodes. A naval
    /// building the yard could not place clears it (`0x00450219..0x00450220`);
    /// nothing sets it again (the other clearing writer, `0x005048A0`, has no
    /// caller).
    naval_allowed: bool,
}

impl Default for HouseAiProduction {
    /// `HouseClass::Constructor`: mode 0 (`0x004F56DF`, EBX), choice -1
    /// (`0x004F5BC0`, ESI), naval 1 (`0x004F56FD`).
    fn default() -> Self {
        Self {
            mode: 0,
            building_choice: -1,
            naval_allowed: true,
        }
    }
}

impl HouseAiProduction {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn mode(&self) -> i32 {
        self.mode
    }

    pub(crate) const fn building_choice(&self) -> i32 {
        self.building_choice
    }

    /// The building exit (`0x00444F3F`, `0x0044531F`) and a computer house's
    /// abandoned building (`FactoryClass::AbandonProduction`, `0x004CA0DD`).
    pub(crate) fn clear_building_choice(&mut self) {
        self.building_choice = -1;
    }

    /// `BuildingClass::Factory_AI`'s refused naval building (`0x00450220`).
    pub(crate) fn forbid_naval(&mut self) {
        self.naval_allowed = false;
    }

    /// `AI_Manage_Build_Queue`'s mode: 2 for a harvester (`0x004FE109`), 1
    /// for a building (`0x004FE3AB`).
    pub(crate) fn set_mode(&mut self, mode: i32) {
        self.mode = mode;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, mode: i32, building_choice: i32, naval_allowed: bool) {
        self.mode = mode;
        self.building_choice = building_choice;
        self.naval_allowed = naval_allowed;
    }
}

/// `HouseClass::AI_EconomyStateMachine @ 0x00509700`, from a produced
/// object's exit: `building_exit` is the Building exit's kind 6
/// (`0x00444F34`); the unit and infantry exit passes the object's kind
/// (`0x00444102`), the aircraft exit 2 (`0x00443CBC`).
///
/// A house no human controls in a skirmish steps its mode against
/// `AIAlternateProductionCreditCutoff=`: below it, mode 0 moves to 2 for a
/// building and 1 otherwise, mode 1 moves to 2 for a building; mode 2 stays
/// only if it owns a barracks and a war factory, has enough power and draws
/// 1 from `RandomRanged(0,1)` (`0x00509863`), else a non-building exit moves
/// it to 1; at or above it, modes 1 and 2 return to 0, and mode 3 only above.
pub(crate) fn economy_state_machine(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    building_exit: bool,
) {
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if house.is_controlled_by_human(game_mode_nonzero) || !game_mode_nonzero {
        return;
    }
    let mode = house.ai_production.mode;
    let money = crate::sim::credit_income::available_money(sim, owner);
    let cutoff = rules.general.ai_alternate_production_credit_cutoff;
    let next = match mode {
        0 => (money < cutoff).then_some(if building_exit { 2 } else { 1 }),
        1 if money >= cutoff => Some(0),
        1 => building_exit.then_some(2),
        2 if money >= cutoff => Some(0),
        2 => {
            // `0x005097A2..0x0050983A`: one owned (on-map, `+0x5550`) type
            // from each list.
            let owns_any = |types: &[String]| {
                types.iter().any(|name| {
                    sim.interner.get(name).is_some_and(|id| {
                        house
                            .tracking
                            .active_count(crate::map::entities::EntityCategory::Structure, id)
                            > 0
                    })
                })
            };
            let (output, drain) = sim
                .power_states
                .get(&owner)
                .map_or((0, 0), |power| (power.total_output, power.total_drain));
            let may_stay = owns_any(&rules.build_barracks_types)
                && owns_any(&rules.build_weapons_types)
                && drain <= output;
            if may_stay && sim.scenario_rng.next_range_i32_inclusive(0, 1) != 0 {
                None
            } else {
                (!building_exit).then_some(1)
            }
        }
        3 => (money > cutoff).then_some(0),
        // Modes above 3 (unsigned, `0x00509742`) change nothing.
        _ => None,
    };
    if let Some(next) = next
        && let Some(house) = sim.houses.get_mut(&owner)
    {
        house.ai_production.mode = next;
    }
}

/// `HouseClass::Update`'s chooser block (`0x004F9038..0x004F9265`): a house
/// no human controls, whose HouseType is not `MultiplayPassive=`, on every
/// eighth (signed) frame, by its production mode (`+0x1E4`):
/// - mode 0, and every mode in a campaign: the building choice, then the
///   Unit, Infantry and Aircraft choosers (`sim::ai_unit_choice`);
/// - mode 1: the building choice, then the three unit choosers unless it
///   holds a choice some factory of the house can build (`FindFactory`,
///   `vt+0x94`, with all three flags: [`find_factory`]);
/// - mode 2: the Unit chooser, then the Infantry and Aircraft choosers
///   unless the Unit choice is the house's harvester (the first `[General]
///   HarvesterUnit=` its country owns, `0x004F90F7..0x004F9194`); then the
///   building choice when all three choices are -1 or a chosen type has no
///   such factory (`0x004F91A4..0x004F9242`);
/// - other modes: nothing.
pub(crate) fn update_production_choices(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&OverlayTypeRegistry>,
) {
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if house.is_controlled_by_human(game_mode_nonzero) || house.multiplay_passive {
        return;
    }
    if (sim.session.binary_frame as i32) % 8 != 0 {
        return;
    }
    let choose_units = |sim: &mut Simulation, kinds: &[UnitChoiceKind]| {
        for &kind in kinds {
            ai_unit_choice::choose(sim, rules, owner, kind);
        }
    };
    const ALL: [UnitChoiceKind; 3] = [
        UnitChoiceKind::Unit,
        UnitChoiceKind::Infantry,
        UnitChoiceKind::Aircraft,
    ];
    // A campaign runs mode 0's choices (`0x004F9099..0x004F90A1`).
    let mode = if game_mode_nonzero {
        house.ai_production.mode
    } else {
        0
    };
    match mode {
        0 => {
            choose_building(sim, rules, owner, registry);
            choose_units(sim, &ALL);
        }
        1 => {
            choose_building(sim, rules, owner, registry);
            let choice = sim
                .houses
                .get(&owner)
                .map_or(-1, |house| house.ai_production.building_choice);
            let buildable = rules
                .building_type_at(choice)
                .is_some_and(|ty| find_factory(sim, rules, owner, ty, true, true, true).is_some());
            if !buildable {
                choose_units(sim, &ALL);
            }
        }
        2 => {
            choose_units(sim, &[UnitChoiceKind::Unit]);
            if !unit_choice_is_harvester(sim, rules, owner) {
                choose_units(sim, &[UnitChoiceKind::Infantry, UnitChoiceKind::Aircraft]);
            }
            let Some(choices) = sim.houses.get(&owner).map(|house| house.ai_unit_choices) else {
                return;
            };
            let none_chosen = ALL.iter().all(|&kind| choices.get(kind) == -1);
            let unbuildable = ALL.iter().any(|&kind| {
                let index = choices.get(kind);
                index != -1
                    && !rules
                        .type_array_at(kind.category(), index)
                        .is_some_and(|ty| {
                            find_factory(sim, rules, owner, ty, true, true, true).is_some()
                        })
            });
            if none_chosen || unbuildable {
                choose_building(sim, rules, owner, registry);
            }
        }
        _ => {}
    }
}

/// Whether the house's Unit choice is its first owner-compatible `[General]
/// HarvesterUnit=` type (`0x004F90F7..0x004F9194`, the type's array index
/// `+0xDF8`).
fn unit_choice_is_harvester(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let country_bit = crate::sim::ai_buildable::house_country_bit(
        rules,
        sim.interner.resolve(house.house_type_id()),
    );
    crate::sim::ai_buildable::first_owner_compatible_harvester(rules, country_bit)
        .and_then(|harvester| rules.type_array_index(ObjectCategory::Vehicle, &harvester.id))
        .is_some_and(|index| index == house.ai_unit_choices.get(UnitChoiceKind::Unit))
}

/// `HouseClass::AI_Choose_Building @ 0x004FE3E0`.
pub(crate) fn choose_building(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&OverlayTypeRegistry>,
) {
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    // `0x004FE3E9..0x004FE416`: a choice already waits, the house has no
    // Construction Yard (`+0x60`, the ConYards count), or no node is open.
    if house.ai_production.building_choice != -1 || house.build_const_order.is_empty() {
        return;
    }
    let naval_allowed = house.ai_production.naval_allowed;
    let Some(mut index) = find_node(sim, rules, owner, -1, registry) else {
        return;
    };
    // `0x004FE41C..0x004FE4A4`: a naval node while naval building is off is
    // removed, and the lookup runs once more.
    let node_type = plan_node(sim, owner, index).type_or_control;
    if !naval_allowed && rules.building_type_at(node_type).is_some_and(|ty| ty.naval) {
        remove_node(sim, owner, index);
        let Some(next) = find_node(sim, rules, owner, -1, registry) else {
            return;
        };
        index = next;
    }
    let mut node = plan_node(sim, owner, index);
    // `0x004FE4B0..0x004FE518`: remove a perimeter node (its `0x005082C0`
    // scan is a residual).
    if node.type_or_control == -3 {
        remove_node(sim, owner, index);
        return;
    }
    let wall_tower = rules.building_type_at(node.type_or_control).filter(|ty| {
        rules
            .general
            .building_types
            .wall_tower
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&ty.id))
    });
    if node.type_or_control == -1 || (wall_tower.is_some() && node.packed_cell == 0) {
        // `0x004FE58C..0x004FE5B8`: below the difficulty's
        // `AIPickWallDefensePercent=` the house first tries walls; when they
        // go in, the node (found again by value) goes.
        let draw = sim.scenario_rng.next_range_i32_inclusive(0, 99);
        let percent = sim.houses.get(&owner).map_or(0, |house| {
            house.difficulty_value(&rules.general.ai_pick_wall_defense_percent)
        });
        if draw < percent && build_walls(sim, rules, owner, index) {
            if let Some(house) = sim.houses.get_mut(&owner) {
                house.base_plan.remove_equal(node);
            }
            return;
        }
        if !crate::sim::ai_base_defense::choose_next_production(sim, rules, owner, index, registry)
        {
            // `0x004FE640..0x004FE6D2`: a WallTower node goes first, then
            // the node at its index.
            if wall_tower.is_some() {
                remove_node(sim, owner, index);
            }
            remove_node(sim, owner, index);
            return;
        }
        // `0x004FE6E1`: the node now holds the defense (or the WallTower its
        // cell) and takes the ordinary path.
        node = plan_node(sim, owner, index);
    }
    if node.type_or_control == -2 {
        return;
    }
    let Some(ty) = rules.building_type_at(node.type_or_control) else {
        return;
    };
    if let Some(plant) = power_plant_to_splice(sim, rules, owner, ty) {
        // `0x004FE893..0x004FE943`: the plant takes the node's index, the
        // node and the rest shift up.
        if let Some(house) = sim.houses.get_mut(&owner) {
            house.base_plan.nodes.insert(
                index,
                BasePlanNode {
                    type_or_control: plant,
                    packed_cell: 0,
                    filled: false,
                    retry_count: 0,
                },
            );
        }
        return;
    }
    // `0x004FEA3C..0x004FEA42`.
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.ai_production.building_choice = node.type_or_control;
    }
}

/// The power splice gates of `AI_Choose_Building` (`0x004FE6F6..0x004FE8A6`):
/// the building's drain would exceed the house's output (less `+0x160B4`,
/// which only the constructor writes, zero), it is not a `BuildConst=` type,
/// it drains power, no blackout remains and no powered building is being
/// drained (`+0x577B`). Returns the plant's BuildingType index: the side 0
/// house's `GDIPowerPlant=`, the side 2 house's `ThirdPowerPlant=`, any other
/// side's `NodAdvancedPower=` when `0x00505360` admits it, else
/// `NodRegularPower=`.
fn power_plant_to_splice(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    ty: &ObjectType,
) -> Option<i32> {
    let house = sim.houses.get(&owner)?;
    let power = sim.power_states.get(&owner).cloned().unwrap_or_default();
    let drain = crate::sim::power_system::native_building_power_drain(ty.power);
    let short = power.total_drain.wrapping_add(drain) > power.total_output;
    if !short
        || ty.build_const_eligible
        || drain <= 0
        || power.blackout_remaining(sim.session.binary_frame) > 0
        || power.has_drained_power_source
    {
        return None;
    }
    let plants = &rules.general.building_types;
    let index = |name: Option<&str>| name.and_then(|name| rules.building_type_index(name));
    // A missing plant is dereferenced natively (module residual): no splice.
    match house.side_index {
        0 => index(plants.gdi_power_plant.as_deref()),
        2 => index(plants.third_power_plant.as_deref()),
        // `0x004FE7A7..0x004FE85D`: the list handed to `0x00505360` holds the
        // house's BuildingClass objects (`+0x6C`, reversed) where the helper
        // compares BuildingTypes, so a prerequisite never matches and the
        // advanced plant passes only when it has no prerequisite (`+0x648`,
        // `0x00505368..0x00505379`). Retail NANRCT needs NATECH and NACNST,
        // so every such house splices NAPOWR.
        _ => index(plants.nod_advanced_power.as_deref())
            .filter(|&advanced| {
                rules
                    .building_type_at(advanced)
                    .is_some_and(|advanced| advanced.prerequisite.is_empty())
            })
            .or_else(|| index(plants.nod_regular_power.as_deref())),
    }
}

/// What the Construction Yard's exit answers `Factory_AI`
/// (`BuildingClass::Exit_Object @ 0x00443C60`'s return value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildingExit {
    /// 0: nothing was placed; the yard abandons the object.
    Failed,
    /// 1: units were told to clear the site; the yard tries again later.
    TryLater,
    /// 2: the building stands, or its wall is stamped.
    Placed,
}

/// The building case of `BuildingClass::Exit_Object @ 0x00443C60`
/// (`0x00444F19..0x00445696`): yard `factory` places its finished building
/// `product`. The prelude that marks the product a playfield member
/// (`0x00443C81`) is the caller's (`production_queue::exit_produced_object`);
/// a placement's Unlimbo then sets it exactly.
///
/// A human's yard places nothing. Otherwise the exit steps the production
/// mode, forgets the building choice and looks up the product's open node
/// ([`find_node`]). The site is the node's cell while a reservation of the
/// house lies near it, else a new one that the node keeps ([`building_site`]).
/// Units in the way are told to leave (`Flush_For_Placement`): the yard tries
/// later and the node counts a failure. Anything else in the way, or a refused
/// placement, fails ([`forget_failed_site`]).
pub(crate) fn exit_building(
    sim: &mut Simulation,
    rules: &RuleSet,
    factory: u64,
    product: u64,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> BuildingExit {
    let Some(owner) = sim.substrate.entities.get(factory).map(|yard| yard.owner()) else {
        return BuildingExit::Failed;
    };
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    // `0x00444F19..0x00444F2A` (`0x0050B730`).
    if sim
        .houses
        .get(&owner)
        .is_none_or(|house| house.is_controlled_by_human(game_mode_nonzero))
    {
        return BuildingExit::Failed;
    }
    let Some(ty) = sim
        .substrate
        .entities
        .get(product)
        .and_then(|object| sim.object_type(object.type_ref(), rules))
    else {
        return BuildingExit::Failed;
    };
    economy_state_machine(sim, rules, owner, true);
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.ai_production.clear_building_choice();
    }
    // `0x0042EB20` with the product type's index (`+0xDF8`): the node or none.
    let node = rules
        .building_type_index(&ty.id)
        .and_then(|index| find_node(sim, rules, owner, index, registry));
    let site = building_site(sim, rules, owner, ty, node, registry);
    match flush_for_placement(sim, rules, registry, ty, site, owner, effects) {
        Flush::Scattered => {
            // `0x00445237..0x004452C3`.
            if let Some(index) = node
                && let Some(house) = sim.houses.get_mut(&owner)
            {
                house.base_plan.count_placement_failure(
                    index,
                    game_mode_nonzero,
                    rules.general.maximum_building_placement_failures,
                );
            }
            BuildingExit::TryLater
        }
        Flush::Blocked => {
            forget_failed_site(sim, rules, owner, node, site);
            BuildingExit::Failed
        }
        Flush::Clear => {
            if !place_building(sim, rules, owner, ty, product, site, registry, effects) {
                forget_failed_site(sim, rules, owner, node, site);
                return BuildingExit::Failed;
            }
            carry_wall_tower_site(sim, rules, owner, ty, node, site);
            log::info!(
                "{} placed {} at ({}, {})",
                sim.interner.resolve(owner),
                ty.id,
                site.0,
                site.1
            );
            BuildingExit::Placed
        }
    }
}

/// `0x00444F65..0x004451AA`: the node's cell when it has one and
/// [`reserved_near`] admits it (`0x00444F9B`; the `PowersUpBuilding=` arm is
/// dormant), else a new site (`0x00444FE1`, `0x004450BD`) that the node, if
/// any, keeps (`0x00445055`, `0x0044519F`). The site's coordinate is its
/// centre at height 0; no ordinary site answers the null coordinate that
/// would take the upgrade arm at `0x00445614`, so a site of (0, 0) goes on to
/// be refused.
fn building_site(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    ty: &ObjectType,
    node: Option<usize>,
    registry: Option<&OverlayTypeRegistry>,
) -> (i16, i16) {
    let node_cell = node
        .map(|index| plan_node(sim, owner, index).packed_cell)
        .filter(|&packed| packed != 0);
    if let Some(packed) = node_cell {
        let cell = unpack_base_plan_cell(packed);
        if reserved_near(sim, rules, owner, ty, cell) {
            return cell;
        }
    }
    let site = find_base_building_site(sim, rules, owner, ty, SiteKey::Ordinary, registry);
    if let Some(index) = node
        && let Some(house) = sim.houses.get_mut(&owner)
    {
        house.base_plan.nodes[index].packed_cell =
            pack_base_plan_cell(i32::from(site.0), i32::from(site.1));
    }
    site
}

/// The product's `Unlimbo(coord, 0)` at `site` (`0x004452D7`) and what follows
/// it up to the wall fill. `BuildingClass::Unlimbo` admits the site through
/// its `Can_Enter_Cell` (`sim::build_site`). A wall stamps its overlay and
/// deletes the object (`0x00440774..0x00440865`), then fills towards the
/// house's walls (`0x00445408`). Any other building stands and runs its
/// Unlimbo's grant pass (`0x0050B1D0`); its Unlimbo queued Construction
/// (`Simulation::building_enter_idle_mode`), which keeps its owner's
/// discovery from opening it; its slave manager takes the hand-off
/// (`0x004452F0..0x004452FA`) and the queued Construction commences with no
/// readiness check (`0x00445329..0x0044533F`). A computer house's yard stands
/// in the Logic vector, which is re-read, so the new building's first Update
/// follows later in the same frame.
fn place_building(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    ty: &ObjectType,
    product: u64,
    site: (i16, i16),
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    if !can_place_building_at(sim, rules, registry, ty, site, Some(owner), None) {
        return false;
    }
    let (Ok(rx), Ok(ry)) = (u16::try_from(site.0), u16::try_from(site.1)) else {
        return false;
    };
    if ty.wall {
        let Some(registry) = registry else {
            return false;
        };
        if sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                product,
                crate::sim::components::DriveCoord {
                    x: i32::from(rx) * 256 + 128,
                    y: i32::from(ry) * 256 + 128,
                    z: 0,
                },
                0,
                PlacementEvidence::EvaluateMark,
                rules,
                Some(registry),
                effects,
            )
            .is_none()
        {
            return false;
        }
        crate::sim::production::autofill_walls(sim, rules, registry, ty, (rx, ry), owner, effects);
        return true;
    }
    let z = sim.terrain_cell_level(rx, ry).unwrap_or(0);
    if sim
        .reveal_constructed_object_at_height(
            product,
            rx,
            ry,
            0,
            z,
            PlacementEvidence::EvaluateMark,
            rules,
            effects,
        )
        .is_none()
    {
        return false;
    }
    sim.mission_spawned_entities = true;
    sim.slave_manager_hand_off(product, rules, effects);
    // The choice clear at `0x0044531F` compares the product with the choice
    // the exit cleared at its start; nothing in between sets it.
    let now = sim.session.binary_frame;
    if let Some(building) = sim.substrate.entities.get_mut(product)
        && building.mission.current() == MissionId::NONE
        && building.mission.queued() == MissionId::from_known(MissionType::Construction)
    {
        crate::sim::mission::authority::commence_entity_mission(building, now);
    }
    true
}

/// `0x004454E6..0x00445611`: after a failure, a wall or gate node
/// (`+0x1571`, `+0x16B7`) is removed; any other node's site is forgotten by
/// every node that holds it. Without a node nothing changes.
fn forget_failed_site(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    node: Option<usize>,
    site: (i16, i16),
) {
    let Some(index) = node else {
        return;
    };
    let node_type = plan_node(sim, owner, index).type_or_control;
    if rules
        .building_type_at(node_type)
        .is_some_and(|ty| ty.wall || ty.gate)
    {
        remove_node(sim, owner, index);
    } else if let Some(house) = sim.houses.get_mut(&owner) {
        house
            .base_plan
            .clear_failed_site(pack_base_plan_cell(i32::from(site.0), i32::from(site.1)));
    }
}

/// `0x0044540D..0x004454D4`: a placed `WallTower=` building hands its cell to
/// the first base defense node (`+0x1706`) after its own.
fn carry_wall_tower_site(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    ty: &ObjectType,
    node: Option<usize>,
    site: (i16, i16),
) {
    let wall_tower = rules
        .general
        .building_types
        .wall_tower
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(&ty.id));
    // Without a node the index lookup (`vt+0x14`, `0x0042F3D0`) maps the null
    // pointer to `(0 - items) >> 4`, past the count, so no node takes the
    // cell.
    let (true, Some(index)) = (wall_tower, node) else {
        return;
    };
    let Some(house) = sim.houses.get_mut(&owner) else {
        return;
    };
    let defense = house
        .base_plan
        .nodes
        .iter_mut()
        .skip(index + 1)
        .find(|next| {
            rules
                .building_type_at(next.type_or_control)
                .is_some_and(|ty| ty.is_base_defense)
        });
    if let Some(defense) = defense {
        defense.packed_cell = pack_base_plan_cell(i32::from(site.0), i32::from(site.1));
    }
}

/// `HouseClass::Suggest_New_Object @ 0x004FBD80` for a factory that makes
/// `factory_rtti`: the house's choice of that kind, unchecked: the building
/// choice (`+0x564C`, `0x004FBDDC`) or a choice of `sim::ai_unit_choice`
/// (`+0x5650`, `+0x5654`, `+0x5658`).
pub(crate) fn suggest_new_object<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    owner: InternedId,
    factory_rtti: i32,
) -> Option<&'r ObjectType> {
    let house = sim.houses.get(&owner)?;
    let unit_choice = |kind: UnitChoiceKind| {
        rules.type_array_at(kind.category(), house.ai_unit_choices.get(kind))
    };
    // Steam3e81a617..08600, two-level tables4FBE0C/4FBDF8. This one
    // consumer and factory accounting accept object/type aliases, unlike
    // FindFactory's exact class comparison. Other RTTIs return null.
    match FactoryType::for_production_rtti(factory_rtti)? {
        FactoryType::BuildingType => rules.building_type_at(house.ai_production.building_choice),
        FactoryType::UnitType => unit_choice(UnitChoiceKind::Unit),
        FactoryType::InfantryType => unit_choice(UnitChoiceKind::Infantry),
        FactoryType::AircraftType => unit_choice(UnitChoiceKind::Aircraft),
    }
}

/// `BaseClass::Find_Node @ 0x0042EB50` for BuildingType index `wanted`, or
/// the first open node when `wanted` is -1. A node is open when
/// [`node_satisfied`] rejects it; for -1 in a skirmish a filled node is open
/// only if [`node_may_replan`] admits it, and then its cell is cleared
/// (`0x0042EBC3..0x0042EBD1`, the zeroed static `0x0089C310`).
pub(crate) fn find_node(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    wanted: i32,
    registry: Option<&OverlayTypeRegistry>,
) -> Option<usize> {
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    let mut index = 0;
    // The count is re-read every step (`0x0042EBAE`).
    while index < sim.houses.get(&owner)?.base_plan.nodes.len() {
        let node = plan_node(sim, owner, index);
        if wanted == -1 {
            if !node_satisfied(sim, rules, owner, &node, registry) {
                if !game_mode_nonzero || !node.filled {
                    return Some(index);
                }
                if node_may_replan(sim, rules, owner, &node) {
                    if let Some(house) = sim.houses.get_mut(&owner) {
                        house.base_plan.nodes[index].packed_cell = 0;
                    }
                    return Some(index);
                }
            }
        } else if node.type_or_control == wanted
            && !node_satisfied(sim, rules, owner, &node, registry)
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// `BaseClass @ 0x0042E780`: the node's building stands ([`node_building`]),
/// or a wall node's cell holds its wall overlay or any building.
pub(crate) fn node_satisfied(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    node: &BasePlanNode,
    registry: Option<&OverlayTypeRegistry>,
) -> bool {
    if node_building(sim, owner, node).is_some() {
        return true;
    }
    let Some(ty) = rules.building_type_at(node.type_or_control) else {
        return false;
    };
    if !ty.wall {
        return false;
    }
    let Some((rx, ry)) = map_cell(node.packed_cell) else {
        return false;
    };
    // `cell+0x44 == ToOverlay(+0xE54)->ArrayIndex` (`0x0042E7CA..0x0042E7D9`).
    let wall_overlay = ty
        .to_overlay
        .as_deref()
        .and_then(|name| registry?.id_for_name(name));
    let overlay = sim
        .overlay_grid
        .as_ref()
        .and_then(|grid| grid.cell(rx, ry).overlay_id);
    (wall_overlay.is_some() && overlay == wall_overlay)
        || sim
            .substrate
            .occupancy
            .first_building_on_layer(rx, ry, MovementLayer::Ground)
            .is_some()
}

/// `BaseClass @ 0x0042E820`: the house's building of the node's type whose
/// top-left cell is the node's cell. The `PowersUpBuilding=` arm is dormant
/// (module residual).
pub(crate) fn node_building(
    sim: &Simulation,
    owner: InternedId,
    node: &BasePlanNode,
) -> Option<u64> {
    if node.packed_cell == 0 || node.type_or_control < 0 {
        return None;
    }
    let (rx, ry) = map_cell(node.packed_cell)?;
    let id = sim
        .substrate
        .occupancy
        .first_building_on_layer(rx, ry, MovementLayer::Ground)?;
    let building = sim.substrate.entities.get(id)?;
    ((building.position.rx(), building.position.ry()) == (rx, ry)
        && building.owner() == owner
        && building.type_facts.base_plan_type_index() == node.type_or_control)
        .then_some(id)
}

/// `HouseClass @ 0x0050CAD0`: whether a filled node whose building is gone
/// may be planned again. A control node may; a refinery that gathers (a
/// deployed Slave Miner) only while the house has fewer gatherers on the map
/// (`+0x158`) than `AISlaveMinerNumber=` allows. In a skirmish an armed type,
/// a wall next to one of the house's buildings or a power plant may; any other
/// type only once `AIRestrictReplaceTime=` frames have passed since a
/// building of the house was attacked (`+0x54D8`).
fn node_may_replan(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    node: &BasePlanNode,
) -> bool {
    if node.type_or_control < 0 {
        return true;
    }
    let Some(ty) = rules.building_type_at(node.type_or_control) else {
        return false;
    };
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    if ty.resource_destination && ty.resource_gatherer {
        // The Recalc preflight validated this row before the plan existed.
        return rules
            .ai_slave_miner_number
            .get(house.difficulty.table_index())
            .is_some_and(|&allowed| house.tracking.resource_gatherers() < allowed);
    }
    if !sim.session.game_mode_nonzero {
        return true;
    }
    // `TechnoTypeClass::GetWeapon(0) @ 0x007177C0`: the Primary slot.
    if ty.primary().is_some() {
        return true;
    }
    if ty.wall {
        let Some((rx, ry)) = map_cell(node.packed_cell) else {
            return false;
        };
        // `0x00481810` over the eight directions, `0x0047C520` on each.
        return crate::util::direction_tables::cell::CELL_DELTAS
            .iter()
            .any(|&(dx, dy)| {
                let neighbor = map_cell_offset(rx, ry, dx, dy);
                neighbor
                    .and_then(|(nx, ny)| {
                        sim.substrate.occupancy.first_building_on_layer(
                            nx,
                            ny,
                            MovementLayer::Ground,
                        )
                    })
                    .and_then(|id| sim.substrate.entities.get(id))
                    .is_some_and(|building| building.owner() == owner)
            });
    }
    if ty.power > 0 {
        return true;
    }
    house
        .strategy_emergency
        .last_building_attack_frame
        .wrapping_add(rules.general.ai_restrict_replace_time)
        <= sim.session.binary_frame as i32
}

fn plan_node(sim: &Simulation, owner: InternedId, index: usize) -> BasePlanNode {
    sim.houses[&owner].base_plan.nodes[index]
}

/// `HouseClass::AI_BuildWalls @ 0x0050C340` for house `owner`'s node at
/// `index`: walls the nearest `ProtectWithWall=` building before it
/// ([`wall_nodes`]) with the house's wall type. Whether it wrote any; it
/// makes no draw.
fn build_walls(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, index: usize) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let wall = wall_type(rules, house.side_index);
    let nodes = &house.base_plan.nodes;
    let walls = wall_nodes(nodes, index, wall, |at| {
        let building = sim
            .substrate
            .entities
            .get(node_building(sim, owner, &nodes[at])?)?;
        let ty = sim.object_type(building.type_ref(), rules)?;
        ty.protect_with_wall.then(|| {
            (
                (building.position.rx() as i16, building.position.ry() as i16),
                crate::sim::ai_base_site::foundation_size(ty),
            )
        })
    });
    let Some((at, walls)) = walls else {
        return false;
    };
    if let Some(house) = sim.houses.get_mut(&owner) {
        for node in walls {
            house.base_plan.insert_after(at, node);
        }
    }
    true
}

/// `0x0050C346..0x0050C39D`: the first `[AI] ConcreteWalls=` type planned
/// for the side, as its array index; -1 without one, which then inserts `-1`
/// nodes as native does.
fn wall_type(rules: &RuleSet, side_index: u8) -> i32 {
    rules
        .concrete_wall_types
        .iter()
        .filter_map(|id| rules.object_in_category(ObjectCategory::Building, id))
        .find(|ty| ty.planned_for_side(side_index))
        .map_or(-1, |ty| ty.base_plan_type_index)
}

/// The nodes `0x0050C340` writes, and after which node. From the node before
/// `index` down, the first whose building is `ProtectWithWall=` (`protected`
/// of its index, `0x0042E820`: the building's top-left cell, `+0x9C / 256`,
/// and its foundation) and whose next node is not already of type `wall`
/// (`0x0050C3A1..0x0050C3EB`) gets `wall` nodes on every cell around its
/// foundation, in the order native inserts each right after that node
/// (`0x0050EB70` and its inlined copies), so the plan holds them reversed:
/// per column the top then the bottom cell (`0x0050C4AA..0x0050C604`), per
/// row the left then the right cell (`0x0050C63B..0x0050C793`), then the
/// corners top-left, top-right, bottom-left, bottom-right
/// (`0x0050C79D..0x0050C89A`). Cells are 16-bit words.
fn wall_nodes(
    nodes: &[BasePlanNode],
    index: usize,
    wall: i32,
    mut protected: impl FnMut(usize) -> Option<((i16, i16), (i32, i32))>,
) -> Option<(usize, Vec<BasePlanNode>)> {
    let (at, (x, y), (width, height)) = (0..index.min(nodes.len().saturating_sub(1)))
        .rev()
        .find_map(|at| {
            let (cell, size) = protected(at)?;
            (nodes[at + 1].type_or_control != wall).then_some((at, cell, size))
        })?;
    let (x, y) = (i32::from(x), i32::from(y));
    let (left, top, right, bottom) = (x - 1, y - 1, x + width, y + height);
    let node = |cx: i32, cy: i32| BasePlanNode {
        type_or_control: wall,
        packed_cell: pack_base_plan_cell(cx, cy),
        filled: false,
        retry_count: 0,
    };
    let mut walls = Vec::new();
    for column in 1..=width {
        walls.push(node(left + column, top));
        walls.push(node(left + column, bottom));
    }
    for row in 1..=height {
        walls.push(node(left, top + row));
        walls.push(node(right, top + row));
    }
    for (cx, cy) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        walls.push(node(cx, cy));
    }
    Some((at, walls))
}

/// Removes house `owner`'s node `index`
/// ([`crate::sim::base_plan::BasePlanState::remove`]).
fn remove_node(sim: &mut Simulation, owner: InternedId, index: usize) {
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.base_plan.remove(index);
    }
}

/// A node cell as a map cell. `MapClass::operator[] @ 0x005657A0` answers
/// its empty dummy for a cell off the map, which holds no object and no
/// overlay; a negative word is always off the map.
fn map_cell(packed_cell: u32) -> Option<(u16, u16)> {
    let (x, y) = unpack_base_plan_cell(packed_cell);
    Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?))
}

fn map_cell_offset(rx: u16, ry: u16, dx: i32, dy: i32) -> Option<(u16, u16)> {
    let x = u16::try_from(i32::from(rx) + dx).ok()?;
    let y = u16::try_from(i32::from(ry) + dy).ok()?;
    Some((x, y))
}

#[cfg(test)]
#[path = "ai_base_building_tests.rs"]
mod tests;
