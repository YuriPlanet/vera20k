//! Getting a base started: deploy the yard maker, and buy another one when the
//! yard is gone.
//!
//! `UnitClass::Deploy` (`0x007393C0`) is the AI's own route to a base — the
//! house's MCV becomes a Construction Yard, and the Recalc that follows anchors
//! the plan on it. A brain-driven house receives no player click, so the brain
//! issues the same deploy the click would.
//!
//! A yard is also a structure like any other, so it can be destroyed, and every
//! structure in `[AI] BuildRefinery=`/`BuildWeapons=`/`BuildBarracks=` hangs off
//! it through `Prerequisite=` (`HouseClass::CanBuild` @ `0x004F7870`: a human
//! house needs the entry on the map). Losing it is not a lost base yet — the
//! factories still produce and the army still fights — but nothing new can be
//! started, so a player buys another MCV out of the war factory, which is the
//! only type that carries `DeploysInto=` back to the yard.
//!
//! Beyond that first yard, a house that is earning more than it can spend
//! builds another: it buys a maker out of the war factory, walks it away from
//! the base it has, and deploys it there. That is a second `BuildConst=`
//! structure, and the placement sweep, the build list and the walls all read
//! it as one — there is no second ledger for it.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::prerequisite::Prerequisite;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;
use crate::sim::control::placement::{PLACEMENT_ATTEMPTS, Placement};
use crate::sim::intern::InternedId;
use crate::sim::production::CanBuild;
use crate::sim::strategy::retail::RetailBrain;

use super::advance::cell_distance;
use super::census::Census;
use super::plan::Budget;

/// The free wallet a second base is bought out of: the maker's own price and
/// the first structure that follows it out of the new yard.
pub(super) const EXPAND_MIN_CREDITS: i32 = 4_000;

/// How far from the base it already has the second one is built, in cells: near
/// enough to walk to and to defend, far enough that the two bases do not become
/// one crowded block — which is the shape the placement sweep is built to
/// avoid.
const EXPAND_MIN_CELLS: i32 = 10;

/// How far out that search looks, in cells.
///
/// A second base goes to the next patch of ground that takes a yard, which
/// is a long way off when the house stands on an ore field: the first yard's
/// own ground is all ore, and the ground that is not ore is the other field.
const EXPAND_SCAN_CELLS: i32 = 48;

/// How long a probe's answer is kept, in frames.
///
/// The search asks the deploy's question cell by cell over the map, so it is
/// a probe rather than a per-frame question: four seconds is a maker's own
/// pace, and a cell that has become unusable is re-asked on the next probe.
const EXPAND_PROBE_FRAMES: u64 = 60;

/// Whether `ty` is a Construction Yard maker: its `DeploysInto=` is the
/// building the rules' `[AI] BuildConst=` list holds. `UnitType`'s
/// `DeploysInto=` is the reader's own field, so a side whose maker is called
/// something else answers here without a name of its own.
pub(super) fn makes_a_yard(rules: &RuleSet, ty: &ObjectType) -> bool {
    ty.deploys_into
        .as_deref()
        .and_then(|name| rules.object(name))
        .is_some_and(|yard| yard.build_const_eligible)
}

/// Whether a yard stands: `BuildConst=` eligible is the rules' own mark for
/// it, and the ledger already counted them.
pub(super) fn yard_stands(census: &Census) -> bool {
    census.yards > 0
}

/// Whether the house holds a mobile yard maker: an MCV waiting to deploy, or
/// one still driving out of its factory.
pub(super) fn owns_yard_maker(census: &Census) -> bool {
    census.yard_makers > 0
}

/// Deploy the house's Construction Yard maker while it has no yard.
///
/// A maker that just left the war factory stands where the yard cannot: the
/// deploy shifts its foundation one cell north-west
/// (`deploy_origin_from_unit_cell`), and the factory and its neighbours are
/// in the way. `UnitClass::Deploy` refuses that silently and drops the unit
/// back onto Guard, so the brain does what the player does — drives it to a
/// cell the yard fits on first and presses D there. Native's own answer to a
/// blocked cell is the same search, in `UnitClass::TryToDeploy`
/// (`0x00738D30`), which only a computer house's Hunt mission runs.
pub(super) fn deploy_yard_maker(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    // One line every forty seconds (600 frames at 15 a second), because this
    // branch is silent by design: a report of "the maker never deploys" has to
    // be able to say whether the brain ever saw one, and whether a yard was
    // standing over it.
    if obs.tick() % 600 == 0 {
        let maker = brain
            .census
            .units
            .iter()
            .find(|unit| unit.traits.yard_maker);
        // The two numbers that answer "why is it still standing there": how
        // far the maker is from home (the sweep's own minimum), and whether
        // the ground under it would take the deploy. A maker that walks and
        // never lands is one of the two, and neither is visible from here
        // without printing it.
        let far = maker.map_or(0, |unit| {
            brain
                .census
                .base_cell
                .map_or(0, |home| cell_distance(unit.cell, home))
        });
        let admits = maker.is_some_and(|unit| {
            obs.object_type(rules, unit.type_id)
                .and_then(|ty| ty.deploys_into.as_deref())
                .and_then(|name| rules.object(name))
                .is_some_and(|yard| obs.deploy_site_admits(rules, unit.entity_id, yard))
        });
        log::info!(
            "auto play: yard={} maker={:?} far={far} admits={admits} credits={}",
            yard_stands(&brain.census),
            maker.map(|unit| (unit.entity_id, unit.cell)),
            obs.credits()
        );
    }
    let Some((entity_id, cell, yard)) = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.yard_maker)
        // A maker the player has grouped is theirs to deploy, and one a team
        // script has recruited is the script's.
        .filter(|unit| !unit.hand)
        .find_map(|unit| {
            let ty = obs.object_type(rules, unit.type_id)?;
            let yard = rules.object(ty.deploys_into.as_deref()?)?;
            makes_a_yard(rules, ty).then_some((unit.entity_id, unit.cell, yard))
        })
    else {
        return;
    };
    // A yard already stands, so this maker is not the way back from a loss: it
    // is the second base. It walks out and deploys where the house has ground
    // of its own.
    if yard_stands(&brain.census) {
        deploy_second_yard(brain, rules, obs, out, entity_id, cell, yard);
        return;
    }
    if obs.deploy_site_admits(rules, entity_id, yard) {
        super::orders::issue(brain, out, rules, obs, entity_id, Goal::Deploy);
        return;
    }
    let Some(yard_id) = obs.type_id(&yard.id) else {
        return;
    };
    // The sweep's cells are the sidebar's own build area, so the maker ends up
    // on base ground the way a player would put it. `site_attempts` is what
    // moves past a cell that cannot be used, the same memory a waiting
    // structure uses.
    let attempt = brain.site_attempts.get(&yard_id).copied().unwrap_or(0);
    match Placement::new(obs, rules).cell_for(yard_id, attempt) {
        Some(target) if target != cell => {
            super::orders::issue(
                brain,
                out,
                rules,
                obs,
                entity_id,
                Goal::Move { cell: target },
            );
        }
        _ => {
            let next = (attempt + 1) % PLACEMENT_ATTEMPTS;
            brain.site_attempts.insert(yard_id, next);
        }
    }
}

/// Whether the house should be buying a yard maker it does not need yet: one
/// for a second base.
///
/// Native's expansion is `AI_Manage_Build_Queue`'s, which is not ported; the
/// gates here are what any house has to satisfy before a maker is worth a war
/// factory cycle — a standing yard (otherwise it is the way back from a loss,
/// which [`super::composition`] already answers), the trucks to fill the bays
/// it has, and a wallet that can still build once the maker is paid for. The
/// type itself comes from the same `CanBuild` answer the sidebar reads
/// ([`Self::yard_maker`]), so a house that cannot build one simply does not.
pub(super) fn wants_a_maker(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
) -> bool {
    if !yard_stands(&brain.census) || owns_yard_maker(&brain.census) {
        return false;
    }
    // One expansion at a time. A second yard is a second set of `BaseNormal=`
    // anchors for the placement sweep and a second perimeter for the walls, and
    // two of those half-started are worse than one finished.
    if brain.census.yards > 1 {
        return false;
    }
    // Income first: a bay with no truck at it is the dearest structure in the
    // base, and a maker is dearer than a bay.
    if !brain.census.harvesters_served() {
        return false;
    }
    // And ground to put it on. A maker that cannot deploy is a maker the
    // house paid 2500 credits for and then watches stand in the factory's
    // doorway for the rest of the match, so the house only buys one when
    // there is a patch of ground a second yard fits on.
    let Some(yard) = yard_type(rules, obs) else {
        return false;
    };
    // Without a map there is no answer, and no answer is not a refusal: the
    // fixtures and the headless harness run without one.
    if obs.map_extent().0 > 0 && expansion_site(brain, rules, obs, yard, None).is_none() {
        return false;
    }
    budget.free >= EXPAND_MIN_CREDITS
}

/// The yard type the house's maker deploys into, when the house has one.
fn yard_type<'r>(rules: &'r RuleSet, obs: &Observation<'_>) -> Option<&'r ObjectType> {
    super::build::all_class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| makes_a_yard(rules, ty))
        .filter_map(|(maker, _)| {
            rules
                .object(maker.deploys_into.as_deref()?)
                .filter(|yard| yard.build_const_eligible)
        })
        .next()
}

/// Drive a maker the house bought for a second base out, and deploy it there.
///
/// A second yard beside the first is not an expansion, it is the same base with
/// more of it — the sweep's own job is to keep one base from becoming a block,
/// so the second one is started far enough away to be its own.
fn deploy_second_yard(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
    entity_id: u64,
    cell: (u16, u16),
    yard: &ObjectType,
) {
    let Some(home) = brain.census.base_cell else {
        return;
    };
    // Standing on ground that takes a yard, far enough from home to be a
    // second base: that is the player's own D press.
    //
    // The maker does not have to reach the cell the house picked: any ground it
    // passes that takes a yard is ground it can take, and a maker that walks
    // past a dozen of them and keeps looking for the one it was told about
    // never deploys at all.
    if cell_distance(cell, home) >= EXPAND_MIN_CELLS
        && obs.deploy_site_admits(rules, entity_id, yard)
    {
        super::orders::issue(brain, out, rules, obs, entity_id, Goal::Deploy);
        return;
    }
    // The cell the house decided on: a patch of ground a second yard fits on,
    // away from the first base. The same probe the purchase gate asked, so the
    // maker walks to ground the house already knows about.
    let Some(target) = expansion_site(brain, rules, obs, yard, Some(entity_id)) else {
        return;
    };
    // A maker already walking is a maker whose cell has been chosen: the
    // walk is over the map's ground and costs a foundation test per cell, so
    // it is asked when the unit stands still and not once a frame while it
    // drives.
    if obs.destination(entity_id).is_some() {
        return;
    }
    if cell != target
        && !super::orders::issue(
            brain,
            out,
            rules,
            obs,
            entity_id,
            Goal::Move { cell: target },
        )
    {
        // The walk was refused: that ground is not reachable, so the house
        // picks another patch instead of asking for the same one every frame.
        brain.expansion_probe = None;
    }
}

/// The nearest cell a second yard may be deployed on: away from the house's own
/// base, on ground a unit can stand on, and a cell a yard of this type fits on.
///
/// The rings are walked outwards from the base and the first admissible cell in
/// the fixed ring order wins, so the walk is the shortest one that gets the
/// maker clear of what the house has already built. Ore is not a factor yet:
/// the rules' overlay is not reachable from [`Observation`], and guessing at a
/// field from the truck's path is not evidence.
pub(super) fn expansion_site(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    yard: &ObjectType,
    maker: Option<u64>,
) -> Option<(u16, u16)> {
    let tick = obs.tick();
    // The deploy's own answer for this cell. With a maker on the field the
    // question is asked with the maker's body lifted -- the deploy lifts it
    // -- and without one the ground has to be clear of everything.
    let admits = |cell: (u16, u16)| match maker {
        Some(id) => obs.deploy_admits_at(rules, id, yard, cell),
        None => obs.clear_for_deploy(rules, yard, cell),
    };
    // A cached answer is used while it still holds: the probe walks the
    // map's ground, and a maker that re-aims on every probe spends the match
    // walking between two patches instead of reaching one.
    if let Some((_, cell)) = brain.expansion_probe
        && let Some(cell) = cell
        && admits(cell)
    {
        return Some(cell);
    }
    let Some(home) = brain.census.base_cell else {
        brain.expansion_probe = Some((tick, None));
        return None;
    };
    let found = search_expansion_site(
        rules,
        obs,
        home,
        &brain.census.home_cells,
        yard,
        &admits,
        &super::terrain::Space::read(obs, rules, &brain.census.home_cells, brain.census.base_cell),
    );
    if found.is_some() {
        log::info!("auto play: second base site at {found:?}",);
    }
    brain.expansion_probe = Some((tick, found));
    found
}

/// Whether `cell` is far enough from the house's own buildings to be a
/// second base's yard.
///
/// A maker that deploys beside the first yard is the same base with more of
/// it, and the point of the walk is to leave.
fn is_a_second_base_cell(home_cells: &[(u16, u16)], cell: (u16, u16)) -> bool {
    home_cells
        .iter()
        .all(|home| cell_distance(*home, cell) >= EXPAND_MIN_CELLS)
}

/// The nearest cell a second yard may be deployed on: away from the house's
/// own base, on ground the yard fits, and clear of everything that stands
/// there.
///
/// The search asks the deploy's own question -- the foundation's origin and
/// the clear-to-build test at that origin -- and not the sidebar's build-area
/// adjacency: a second base is outside the first base's build area by
/// definition, so asking the placement question answered No for every cell
/// and the maker stood where the factory put it.
fn search_expansion_site(
    rules: &RuleSet,
    obs: &Observation<'_>,
    home: (u16, u16),
    home_cells: &[(u16, u16)],
    yard: &ObjectType,
    admits: &dyn Fn((u16, u16)) -> bool,
    space: &super::terrain::Space,
) -> Option<(u16, u16)> {
    let (width, height) = obs.map_extent();
    // No map, no expansion: the headless harness and the fixtures run without
    // one, and a walk into cell (0, 0) is worse than staying put.
    if width == 0 || height == 0 {
        return None;
    }
    for ring in EXPAND_MIN_CELLS..=EXPAND_SCAN_CELLS {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let (Some(rx), Some(ry)) = (
                    u16::try_from(i32::from(home.0) + dx).ok(),
                    u16::try_from(i32::from(home.1) + dy).ok(),
                ) else {
                    continue;
                };
                if rx >= width || ry >= height || !obs.is_ground_walkable(rx, ry) {
                    continue;
                }
                // Not inside a base the house already has: that is where the
                // first yard is, and the point of the walk is to leave it.
                if !is_a_second_base_cell(home_cells, (rx, ry)) {
                    continue;
                }
                // And the ground has to take a yard.
                if !admits((rx, ry)) {
                    continue;
                }
                // And it has to be ground the house can keep. A second yard
                // inside the enemy's reach is a yard they destroy for free,
                // and one inside their base is a yard they never let stand —
                // the wishlist's "new buildings appear in the enemy's face".
                // Read through the shared map reading rather than a second
                // distance rule of this file's own.
                if !space.ground_at((rx, ry)).buildable() {
                    continue;
                }
                return Some((rx, ry));
            }
        }
    }
    None
}

/// The maker the house may buy now, or `None` when it may buy none.
///
/// The type comes from the same `CanBuild` answer the sidebar reads, so a
/// side without the war factory that carries it — or one at its build limit —
/// simply has no way back, exactly as a player would.
pub(super) fn yard_maker<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    super::build::class_types(rules, obs, ObjectCategory::Vehicle)
        .find(|(ty, _)| makes_a_yard(rules, ty) && yard_is_ours(rules, obs, ty))
}

/// Whether the yard a maker deploys into belongs to the house's own country.
///
/// The maker's own type is the vehicle; the country it belongs to is the yard
/// it becomes (`DeploysInto=`), and that is the type whose country list is
/// the rules' own gate. A house that bought the other side's maker would be
/// holding a vehicle it cannot deploy.
fn yard_is_ours(rules: &RuleSet, obs: &Observation<'_>, maker: &ObjectType) -> bool {
    maker
        .deploys_into
        .as_deref()
        .and_then(|name| rules.object(name))
        .is_some_and(|yard| super::build::house_owns(rules, obs, yard))
}

/// A building the yard maker's own `Prerequisite=` list asks for and the
/// house does not hold yet, when the house may build it.
///
/// Retail's `[AMCV]` wants `GAWEAP,GADEPT`: the war factory is the army's
/// anyway, but the service depot is a structure nothing else asks for, and a
/// house whose depot was never built has no way back from a lost yard — the
/// maker it needs is `CanBuild` No for good. One is bought while the yard
/// still stands, so the recovery has something to stand on.
///
/// The list is the type's own: a side whose depot is called something else is
/// answered by `CanBuild`, which is where `RequiredHouses=` and the depot's
/// own prerequisites are read.
pub(super) fn yard_maker_support<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
) -> Option<(&'r ObjectType, InternedId)> {
    super::build::all_class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| makes_a_yard(rules, ty) && yard_is_ours(rules, obs, ty))
        .find_map(|(maker, _)| missing_prerequisite(rules, obs, census, maker))
}

/// The first `Prerequisite=` building of `maker` the house lacks and may
/// build now.
fn missing_prerequisite<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    maker: &ObjectType,
) -> Option<(&'r ObjectType, InternedId)> {
    maker.prerequisite.iter().find_map(|entry| {
        let Prerequisite::Building(index) = *entry else {
            return None;
        };
        let want = rules.type_array_at(ObjectCategory::Building, index)?;
        let type_id = obs.type_id(&want.id)?;
        if census.type_count(type_id) > 0 {
            return None;
        }
        (obs.can_build(rules, want, true) != CanBuild::No).then_some((want, type_id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sim::arena_fixture;
    use crate::sim::strategy::retail::RetailBrain;
    use crate::sim::world::Simulation;

    /// The maker belongs to the yard it becomes: an American house's maker
    /// deploys into the Allied yard and a Russian house's into the Soviet
    /// one, and the yard's own country list is the rules' answer.
    #[test]
    fn the_yard_maker_is_the_house_s_own_country_s() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::world::Simulation::new();
        let american = sim.interner.intern("Americans");
        let russian = sim.interner.intern("Russians");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                american,
                0,
                Some(american),
                true,
                0,
                10,
            ));
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                russian,
                0,
                Some(russian),
                false,
                0,
                10,
            ));
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let allied = rules.object("AMCV").expect("the Allied maker");
        let soviet = rules.object("SMCV").expect("the Soviet maker");

        let obs = Observation::new(&sim, american);
        assert!(
            yard_is_ours(&rules, &obs, allied),
            "the Allied maker deploys into the Allied yard"
        );
        assert!(
            !yard_is_ours(&rules, &obs, soviet),
            "and the Soviet maker does not"
        );

        let obs = Observation::new(&sim, russian);
        assert!(
            yard_is_ours(&rules, &obs, soviet),
            "the Soviet maker deploys into the Soviet yard"
        );
        assert!(
            !yard_is_ours(&rules, &obs, allied),
            "and the Allied maker does not"
        );
    }
    /// The flat arena with the computer `Russians` and the human `Americans`.
    fn arena(seed: u64) -> Option<(Simulation, RuleSet, InternedId)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        Some((sim, rules, owner))
    }

    /// One maker, bought while the yard stands, and only then.
    ///
    /// The gate used to be "no yard and no maker", which is the way back from a
    /// loss and nothing else: a house that had never lost its base never bought
    /// a second one, and the whole map's ore past its own field was somebody
    /// else's.
    #[test]
    fn a_yard_maker_is_bought_for_a_second_base_once_the_income_holds() {
        let Some((mut sim, rules, owner)) = arena(0xA17) else {
            return;
        };
        let _ = sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules);
        let _ = sim.spawn_object("GAREFN", "Americans", 18, 16, 255, &rules);
        let _ = sim.spawn_object("CMIN", "Americans", 20, 16, 255, &rules);
        let _ = sim.spawn_object("CMIN", "Americans", 20, 18, 255, &rules);
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(10_000);

        let mut brain = RetailBrain::new(0xA17);
        {
            let obs = Observation::new(&sim, owner);
            brain.rebuild_census(&rules, &obs);
            let budget = super::super::plan::budget(&rules, &obs, &brain.census, None);
            assert!(
                wants_a_maker(&mut brain, &rules, &obs, &budget),
                "a house with a yard, its trucks and a full wallet wants a second one"
            );
        }

        // A second yard is enough: the sweep and the walls have one more base
        // to keep tidy than they can while the first is still filling in.
        let _ = sim.spawn_object("GACNST", "Americans", 24, 16, 255, &rules);
        {
            let obs = Observation::new(&sim, owner);
            brain.census = Default::default();
            brain.rebuild_census(&rules, &obs);
            let budget = super::super::plan::budget(&rules, &obs, &brain.census, None);
            assert!(
                !wants_a_maker(&mut brain, &rules, &obs, &budget),
                "one expansion at a time"
            );
        }
    }

    /// A second base is not the first base with more of it: the cell has to be
    /// clear of every building the house already holds.
    #[test]
    fn a_second_base_is_not_inside_the_first_one() {
        let homes = [(10, 10), (12, 10)];
        assert!(
            is_a_second_base_cell(&homes, (40, 40)),
            "a cell far away is a second base's"
        );
        assert!(
            !is_a_second_base_cell(&homes, (14, 10)),
            "and a cell beside the first yard is not"
        );
    }
}
