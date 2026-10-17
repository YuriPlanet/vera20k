//! Paid stock Industrial Plant/HTNK history through the bound production runtime.
//! Native: House50BF60/50BEB0, Factory4C9C70/4C9FF0, Selling449C30,
//! HowManySurvivors451330, Rules667371/670C13, gamemd SHA256
//! 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
//! Instruction/retail-derived expectations; this is a Rust regression, not
//! original execution, a native charging timestamp or whole-game parity.

use super::*;
use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::map::entities::EntityCategory;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, SkirmishLaunchOptions,
};
use std::collections::BTreeSet;

const OWNER: &str = "VERA-OBSERVER";
const CATEGORY: ProductionCategory = ProductionCategory::Vehicle;

fn stock_scene(root: &std::path::Path) -> HeadlessScenario {
    let mut launch = battle_session(
        "XMP03T4.MAP",
        &[(LaunchCountry::Russia, AiDifficulty::Easy)],
    );
    launch.player_name = OWNER.to_owned();
    launch.local.country = LaunchCountry::Russia;
    launch.local.start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options = SkirmishLaunchOptions {
        starting_credits: 10_000,
        unit_count: 0,
        tech_level: 10,
        game_speed: 1,
        default_ai_difficulty: 0,
        short_game: false,
        bases: true,
        bridges_destroyable: true,
        super_weapons: true,
        build_off_ally: true,
        crates: false,
        mcv_redeploy: true,
        fog_of_war: false,
        shroud: true,
        tiberium_grows: true,
        multi_engineer: false,
        harvester_truce: false,
        ally_change_allowed: true,
    };
    load_session(root, "XMP03T4.MAP", 0x1234_5678, launch).expect("stock Battle loads")
}

fn tank_factory(sim: &Simulation, owner: InternedId) -> &Factory {
    sim.production
        .factories
        .factory(FactoryHolder::House(owner, CATEGORY))
        .expect("the actual paid tank Factory")
}

/// These are ordinary input choices discovered with the stock map client,
/// not expected native completion times. No map, wallet, production, AI, RNG,
/// ready-building, crew or power state is supplied after the lobby load.
fn opening_order(sim: &Simulation, step: u64) -> Option<Command> {
    let (kind, cell) = match step {
        70 => ("NAPOWR", None),
        650 => ("NAPOWR", Some((31, 89))),
        720 => ("NAHAND", None),
        1200 => ("NAHAND", Some((31, 85))),
        1210 => ("NAREFN", None),
        2900 => ("NAREFN", Some((26, 83))),
        3020 => ("NARADR", None),
        3900 => ("NARADR", Some((30, 92))),
        4000 => ("NAWEAP", None),
        5800 => ("NAWEAP", Some((25, 91))),
        6000 => ("NAPOWR", None),
        6600 => ("NAPOWR", Some((34, 89))),
        6700 => ("NAPOWR", None),
        7300 => ("NAPOWR", Some((34, 85))),
        7400 => ("NATECH", None),
        9300 => ("NATECH", Some((23, 87))),
        9400 => ("NAINDP", None),
        12000 => ("NAINDP", Some((33, 92))),
        _ => return None,
    };
    let type_id = sim.interner.get(kind).unwrap();
    Some(match cell {
        Some((rx, ry)) => Command::PlaceReadyBuilding { type_id, rx, ry },
        None => Command::QueueProduction { type_id },
    })
}

#[test]
#[ignore = "slow; paid retail Industrial Plant uses two 14500-frame histories"]
fn stock_plant_gain_and_sale_preserve_tank_debt_and_settle_current_price() {
    let (root, _) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("run with RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1");
    for sell in [false, true] {
        let mut scene = stock_scene(&root);
        // Hash collection is observational; the final frame still gets one.
        scene
            .runtime
            .simulation
            .hash_state_every(std::num::NonZeroU64::new(14500).unwrap());
        let sim = scene.sim();
        let owner = sim.interner.get(OWNER).unwrap();
        let tank = sim.interner.get("HTNK").unwrap();
        let plant_type = sim.interner.get("NAINDP").unwrap();
        assert_eq!(sim.houses[&owner].side_index, 1);
        let local: Vec<_> = sim
            .entities()
            .iter_sorted()
            .filter(|(_, entity)| entity.owner() == owner)
            .collect();
        assert_eq!(local.len(), 1);
        let mcv = local[0].0;
        assert_eq!(sim.interner.resolve(local[0].1.type_ref()), "SMCV");
        let rules = &scene.runtime.resources.rules;
        assert_eq!(
            [
                rules.general.allied_survivor_divisor,
                rules.general.soviet_survivor_divisor,
                rules.general.third_survivor_divisor
            ],
            [500, 250, 750]
        );
        let plant_rules = rules.object("NAINDP").unwrap();
        assert!(plant_rules.crewed && plant_rules.factory_type().is_none());
        assert_eq!((plant_rules.cost, plant_rules.soylent), (2500, 1000));

        let mut plant = None;
        let mut miner = None;
        let mut earned_income = false;
        let mut first = None;
        let mut second = None;
        let mut delivered = None;
        let mut sale_finished = false;
        let mut crew = BTreeSet::new();
        for step in 0..14500 {
            let sim = scene.sim();
            let rules = &scene.runtime.resources.rules;
            if miner.is_none() {
                miner = sim.entities().iter_sorted().find_map(|(id, entity)| {
                    (entity.owner() == owner
                        && !entity.lifecycle.in_limbo()
                        && sim.interner.resolve(entity.type_ref()) == "HARV")
                        .then_some(id)
                });
            }
            let carrying_before = miner
                .and_then(|id| sim.entities().get(id))
                .and_then(|entity| entity.miner.as_ref())
                .is_some_and(|miner| !miner.cargo().is_empty());
            let wallet = &sim.houses[&owner].economy;
            let money_before = (wallet.credits(), wallet.spent_credits(), wallet.score());
            let gross_before = i64::from(money_before.0) + i64::from(money_before.1);
            if step == 11800 {
                assert!(earned_income, "the base exceeds the genuine opening wallet");
                assert_eq!(wallet.spent_credits(), 11800);
                assert_eq!(power_balance_for_owner(sim, rules, OWNER), (450, 235));
                plant = ready_building_entity_for_owner(sim, rules, owner, plant_type);
                let id = plant.expect("the plant was paid and retained ready");
                assert!(sim.entities().get(id).unwrap().lifecycle.in_limbo());
                assert!(
                    sim.houses[&owner]
                        .base_projection
                        .factory_plants()
                        .is_empty()
                );
                assert_eq!(
                    sim.cost_of(owner, rules.object("HTNK").unwrap(), rules),
                    900
                );
            }
            if step == 12000 {
                let preview = placement_preview_for_owner_with_overlays(
                    sim,
                    rules,
                    OWNER,
                    "NAINDP",
                    33,
                    92,
                    Some(&scene.runtime.resources.overlay_registry),
                )
                .unwrap();
                assert!(preview.valid);
                assert_eq!(preview.cells.len(), 9);
                for cell in preview.cells {
                    assert_eq!((cell.level, cell.slope), (4, 0));
                    assert!(
                        sim.resolved_terrain
                            .as_ref()
                            .unwrap()
                            .cell(cell.coord.0, cell.coord.1)
                            .unwrap()
                            .bridge_deck_level_if_any()
                            .is_none()
                    );
                }
            }
            let cancel = matches!(step, 12100 | 12600).then(|| {
                let factory = tank_factory(sim, owner);
                assert!(factory.suspended && factory.progress > 0);
                (
                    factory.object.as_ref().unwrap().entity_id.unwrap(),
                    factory.balance,
                    sim.cost_of(owner, rules.object("HTNK").unwrap(), rules),
                )
            });
            let plant_was_present = plant.is_some_and(|id| {
                sim.entities()
                    .get(id)
                    .is_some_and(|entity| !entity.lifecycle.in_limbo())
            });
            let command = opening_order(sim, step).or_else(|| match step {
                0 => Some(Command::DeployMcv { entity_id: mcv }),
                11800 | 12200 | 12700 => Some(Command::QueueProduction { type_id: tank }),
                11890 | 12300 => Some(Command::SuspendProduction { category: CATEGORY }),
                12100 | 12600 => Some(Command::CancelProductionByType {
                    type_id: tank,
                    all: false,
                }),
                12400 if sell => Some(Command::SellBuilding {
                    entity_id: plant.unwrap(),
                }),
                _ => None,
            });
            let commands: Vec<_> = command
                .into_iter()
                .map(|command| CommandEnvelope::new(owner, sim.session.tick, command))
                .collect();
            scene
                .runtime
                .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
                .expect("ordinary bound-resource frame");
            let sim = scene.sim();
            let rules = &scene.runtime.resources.rules;
            let wallet = &sim.houses[&owner].economy;
            if step < 11800 && carrying_before {
                earned_income |= miner
                    .and_then(|id| sim.entities().get(id))
                    .and_then(|entity| entity.miner.as_ref())
                    .is_some_and(|miner| miner.cargo().is_empty())
                    && i64::from(wallet.credits()) + i64::from(wallet.spent_credits())
                        > gross_before;
            }
            if matches!(step, 11800 | 12200 | 12700) {
                let price = if step == 11800 || (step == 12700 && sell) {
                    900
                } else {
                    675
                };
                let factory = tank_factory(sim, owner);
                assert_eq!((factory.progress, factory.balance), (0, price));
                let id = factory.object.as_ref().unwrap().entity_id.unwrap();
                assert!(sim.entities().get(id).unwrap().lifecycle.in_limbo());
                if step == 12700 {
                    delivered = Some(id);
                }
            }
            if matches!(step, 11890 | 12300) {
                let factory = tank_factory(sim, owner);
                assert!(factory.suspended && factory.progress > 0);
                let id = factory.object.as_ref().unwrap().entity_id.unwrap();
                if step == 11890 {
                    assert!((675..900).contains(&factory.balance));
                    first = Some((id, factory.balance));
                } else {
                    assert!((0..675).contains(&factory.balance));
                    second = Some((id, factory.balance));
                }
            }
            if step == 12000 {
                let id = plant.unwrap();
                let entity = sim.entities().get(id).unwrap();
                assert!(!entity.lifecycle.in_limbo());
                assert_eq!(entity.building_ready_latch(), 0);
                assert_eq!(sim.houses[&owner].base_projection.factory_plants(), [id]);
                assert_eq!(
                    sim.cost_of(owner, rules.object("HTNK").unwrap(), rules),
                    675
                );
            }
            if (11890..12100).contains(&step) {
                let factory = tank_factory(sim, owner);
                assert_eq!(
                    factory.object.as_ref().unwrap().entity_id,
                    Some(first.unwrap().0)
                );
                assert_eq!(factory.balance, first.unwrap().1);
            }
            if (12300..12600).contains(&step) {
                let factory = tank_factory(sim, owner);
                assert_eq!(
                    factory.object.as_ref().unwrap().entity_id,
                    Some(second.unwrap().0)
                );
                assert_eq!(factory.balance, second.unwrap().1);
            }
            if let Some((id, balance, price)) = cancel {
                assert_eq!(wallet.credits(), money_before.0 + price - balance);
                assert_eq!(
                    (wallet.spent_credits(), wallet.score()),
                    (money_before.1, money_before.2)
                );
                assert!(sim.entities().get(id).is_none());
                assert_eq!(
                    sim.production.factories.count_total(owner, CATEGORY, tank),
                    0
                );
                if step == 12100 {
                    assert!(
                        wallet.credits() < money_before.0,
                        "gain makes cancellation negative"
                    );
                } else if sell {
                    assert!(
                        price - balance > 675 - balance,
                        "loss refunds at the restored price"
                    );
                }
            }
            if step >= 12400 {
                if let Some(entity) = sim.entities().get(plant.unwrap()) {
                    assert_eq!(
                        sim.cost_of(owner, rules.object("HTNK").unwrap(), rules),
                        675
                    );
                    if sell {
                        assert_eq!(
                            entity.mission.effective().known(),
                            Some(MissionType::Selling)
                        );
                    }
                } else if plant_was_present {
                    assert!(sell && !sale_finished);
                    assert_eq!(wallet.credits(), money_before.0 + 1000);
                    assert_eq!(wallet.spent_credits(), money_before.1);
                    assert!(
                        sim.houses[&owner]
                            .base_projection
                            .factory_plants()
                            .is_empty()
                    );
                    assert_eq!(
                        sim.cost_of(owner, rules.object("HTNK").unwrap(), rules),
                        900
                    );
                    sale_finished = true;
                }
                for (id, entity) in sim.entities().iter_sorted().filter(|(_, entity)| {
                    entity.owner() == owner && entity.category() == EntityCategory::Infantry
                }) {
                    if crew.insert(id) {
                        assert!(sell);
                        assert_eq!(sim.interner.resolve(entity.type_ref()), "E2");
                        assert!(
                            entity.is_alive()
                                && !entity.lifecycle.in_limbo()
                                && !entity.on_bridge()
                        );
                        assert_eq!(
                            entity.health.current(),
                            rules.object("E2").unwrap().strength
                        );
                        assert_eq!(entity.position.exact_z_leptons(), Some(416));
                    }
                }
            }
        }
        let sim = scene.sim();
        assert_eq!(sale_finished, sell);
        assert_eq!(crew.len(), if sell { 4 } else { 0 });
        for id in [first.unwrap().0, second.unwrap().0] {
            assert!(sim.entities().get(id).is_none());
        }
        let tank = sim.entities().get(delivered.unwrap()).unwrap();
        assert!(tank.is_active() && tank.is_alive() && !tank.lifecycle.in_limbo());
        assert_eq!(sim.interner.resolve(tank.type_ref()), "HTNK");
        assert_eq!(tank.health.current(), 400);
        assert_eq!(sim.entities().get(plant.unwrap()).is_none(), sell);
        for id in crew {
            assert!(
                sim.entities()
                    .get(id)
                    .is_some_and(|entity| entity.is_active() && entity.is_alive())
            );
        }
    }
}
