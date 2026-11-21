use super::*;

use crate::sim::world::Simulation;

/// A snapshot arena for the steps that do not read the scenario's own
/// layering: the repository's own `ini/`, so this holds without `RA2_DIR`.
fn snapshot(seed: u64) -> Option<(Simulation, RuleSet)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    Some((sim, rules))
}

/// A transport carries the army; it does not fight in it. The night hawk
/// carries a cannon, so "has a primary weapon" counted it as army and the
/// house's fighting strength was measured in passenger seats.
#[test]
fn a_transport_is_not_counted_as_army() {
    let Some((sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let _ = sim;
    let night_hawk = rules.object("SHAD").expect("the night hawk");
    assert!(
        !is_combat_unit(&rules, night_hawk),
        "a five-seat transport is not army"
    );
    let gi = rules.object("E1").expect("a GI");
    assert!(is_combat_unit(&rules, gi), "a soldier is");
}

/// An engineer's walk is re-read as it goes. The `Capture` window is the
/// whole walk, so an engineer that set out over clear ground and met a tank
/// halfway was committed for five seconds with nothing looking again — the
/// wishlist's "工程师…最后还是送死，完全不会识别威胁单位".
#[test]
fn an_engineer_drops_the_walk_when_the_ground_turns_bad() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    // A house to own the derrick. `CAOILD` is a `Capturable=` neutral
    // building, and the world refuses to spawn onto a house the array does
    // not hold, so the fixture registers one with the `MultiplayPassive=`
    // flag the map's own neutrals carry.
    let neutral = sim.interner.intern("Neutral");
    let mut house = crate::sim::house_state::HouseState::new(neutral, 0, None, false, 0, 10);
    house.set_multiplay_passive_for_test(true);
    sim.houses.register(house);
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 20, 20, 255, &rules)
        .expect("an engineer");
    let derrick = sim
        .spawn_object("CAOILD", "Neutral", 30, 20, 255, &rules)
        .expect("a derrick to take");
    let mut brain = AssistBrain::new(0xA17);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &Observation::new(&sim, owner), &mut out);
    assert!(
        brain.tracked.is_claimed(0, engineer),
        "the walk starts on clear ground"
    );

    // A tank arrives between the engineer and the derrick.
    sim.spawn_object("HTNK", "Russians", 25, 20, 255, &rules)
        .expect("a hostile on the walk");
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &Observation::new(&sim, owner), &mut out);
    assert!(
        !brain.tracked.is_claimed(0, engineer),
        "and is dropped when one stands on it, rather than walked into: {derrick}"
    );
}

/// The boarding half of Transport, end to end: an empty carrier with an
/// idle rifleman beside it asks him aboard, and the soldier is the actor
/// of the order — the carrier drives, the passenger walks in.
#[test]
fn an_empty_carrier_asks_a_rifleman_aboard() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let carrier = sim
        .spawn_object("SAPC", "Americans", 20, 20, 255, &rules)
        .expect("an APC");
    let gi = sim
        .spawn_object("E1", "Americans", 21, 20, 255, &rules)
        .expect("a rifleman");
    sim.spawn_object("E1", "Russians", 28, 28, 255, &rules)
        .expect("an enemy to walk at");
    let mut brain = AssistBrain::new(0xA17);
    let obs = Observation::new(&sim, owner);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &obs,
    );
    let mut out = Outbox::default();
    transports(&mut brain, &rules, &obs, &mut out);
    let boarded = out.orders.iter().any(|order| {
        order.actor == gi && matches!(order.goal, Goal::Enter { target } if target == carrier)
    });
    assert!(boarded, "the rifleman is asked aboard: {:?}", out.orders);
}

/// One order per unit per frame. The outbox is applied in order and the
/// last order wins at the receiver, so a second pass that speaks silently
/// replaces the first — which is how a unit sent to clear a neutral
/// blocker ended up walking to a new spot instead.
#[test]
fn a_second_order_in_the_same_frame_is_refused() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let gi = sim
        .spawn_object("E1", "Americans", 20, 20, 255, &rules)
        .expect("a GI");
    let obs = Observation::new(&sim, owner);
    let mut out = Outbox::default();
    assert!(issue(
        &AssistBrain::new(0xA17),
        &mut out,
        &rules,
        &obs,
        gi,
        Goal::Move { cell: (22, 22) }
    ));
    assert!(
        !issue(
            &AssistBrain::new(0xA17),
            &mut out,
            &rules,
            &obs,
            gi,
            Goal::Move { cell: (25, 25) }
        ),
        "the first answer wins"
    );
    assert_eq!(out.orders.len(), 1, "one order, not two");
}

/// A plan that names a cell for a new object has to see what already
/// stands there: the object is what the placement is refused for, and a
/// plan that keeps naming an occupied cell never finishes.
#[test]
fn ground_object_cells_names_what_stands_on_the_map() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    sim.spawn_object("GAPOWR", "Americans", 20, 20, 255, &rules)
        .expect("a power plant");
    sim.spawn_object("E1", "Americans", 30, 30, 255, &rules)
        .expect("a GI");
    let obs = Observation::new(&sim, owner);
    let cells = obs.ground_object_cells(&rules);
    assert!(cells.contains(&(20, 20)), "the building");
    assert!(cells.contains(&(30, 30)), "the soldier");
    assert!(!cells.contains(&(25, 25)), "empty ground is free");
}

/// A paradrop is soldiers, and soldiers dropped into the enemy's own base
/// land inside everything standing there. They land on the army. What the
/// house cannot see is what a spy plane is for.
#[test]
fn a_paradrop_lands_on_the_army_not_in_the_enemy_base() {
    assert_eq!(aim_of(SuperWeaponKind::AmerParaDrop), Aim::Rally);
    assert_eq!(aim_of(SuperWeaponKind::ParaDrop), Aim::Rally);
    assert_eq!(aim_of(SuperWeaponKind::SpyPlane), Aim::Recon);
    assert_eq!(aim_of(SuperWeaponKind::MultiMissile), Aim::Enemy);
}

/// A `DeployFire=` soldier takes its deployed position when a hostile
/// reaches it. The D key is a toggle, so the step only answers on the edge:
/// a soldier standing in an empty field is not sent down, and one that has
/// taken the pose is not walked back out of it.
#[test]
fn a_soldier_deploys_when_the_fight_reaches_it() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    sim.spawn_object("E1", "Americans", 20, 20, 255, &rules)
        .expect("a GI");
    // Interned before the projection borrows the world: the ledger's
    // enemies are named by id, and the interner is the world's.
    let hostile = sim.interner.intern("E2");
    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &obs,
    );
    let Some(gi) = brain.census.units.first().map(|unit| unit.entity_id) else {
        return;
    };

    // Nothing in front of it: an empty field is not a fight.
    let mut out = Outbox::default();
    deploy(&mut brain, &obs, &mut out);
    assert!(
        out.commands.is_empty(),
        "no hostile, no D key: {:?}",
        out.commands
    );

    // A hostile three cells away is.
    brain.census.enemies.push(EnemyEntity {
        entity_id: gi.wrapping_add(1),
        type_id: hostile,
        cell: (23, 20),
    });
    let mut out = Outbox::default();
    deploy(&mut brain, &obs, &mut out);
    assert!(
        out.commands.iter().any(|command| matches!(
            command,
            Command::ToggleInfantryDeploy { entity_id } if *entity_id == gi
        )),
        "the soldier takes the deployed position: {:?}",
        out.commands
    );

    // The pose is an animation and `Is_Deployed` only follows it, so the
    // soldier still reads as standing. Asking again now would toggle it
    // straight back up, every frame.
    let mut again = Outbox::default();
    deploy(&mut brain, &obs, &mut again);
    assert!(
        again.commands.is_empty(),
        "one D key per soldier: {:?}",
        again.commands
    );
}

/// The retail rival arena, needed to field real object types. `None`
/// without `RA2_DIR`, as the rest of the suite does: these orders read the
/// scenario's own layering, which the unlayered snapshot does not carry.
fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    Some((sim, rules))
}

/// A spy's click on a building is the same Capture click an engineer's is:
/// one order, and the per-cell handler decides which of the two it is. A
/// spy has no combat value to rank, so without this order it stands where
/// it was built.
#[test]
/// A building this house owns is not a target, whatever the warhead says.
///
/// `can_engage` and `can_hurt` are both true against a friendly structure —
/// neither asks whose it is — so before this gate a squad that had captured
/// the refinery it was shelling was still handed the order. The wishlist's
/// "占领后仍然自己打自己".
#[test]
fn a_building_this_house_owns_is_not_a_target() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let enemy = sim.interner.intern("Russians");
    let rifleman = sim
        .spawn_object("E1", "Americans", 12, 12, 255, &rules)
        .expect("a rifleman");
    let refinery = sim
        .spawn_object("GAREFN", "Americans", 16, 12, 255, &rules)
        .expect("a refinery");
    let attack = Goal::Attack { target: refinery };
    assert!(
        !admits(&rules, &Observation::new(&sim, owner), rifleman, &attack),
        "the house does not shell its own refinery"
    );
    // The same building in enemy hands is a target again.
    sim.change_owner_with_rules(refinery, enemy, &rules, None, Default::default());
    assert!(
        admits(&rules, &Observation::new(&sim, owner), rifleman, &attack),
        "and it is a target once the enemy holds it"
    );
}

fn a_spy_walks_into_a_building_the_way_an_engineer_does() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let spy = sim
        .spawn_object("SPY", "Americans", 8, 8, 64, &rules)
        .expect("a spy");
    sim.spawn_object("NAPOWR", "Russians", 20, 20, 64, &rules)
        .expect("a power plant a spy may enter");
    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    let mut out = Outbox::default();
    brain.rebuild_census(&rules, &obs);
    captures(&mut brain, &rules, &obs, &mut out);

    assert!(
        out.orders
            .iter()
            .any(|order| order.actor == spy && matches!(order.goal, Goal::Capture { .. })),
        "a spy is sent into the building, not left standing: {:?}",
        out.orders
    );
    assert!(
        walks_into_buildings(&rules, rules.object("SPY").expect("the spy")),
        "Infiltrate= is a Capture click"
    );
    assert!(
        walks_into_buildings(&rules, rules.object("ENGINEER").expect("the engineer")),
        "and so is Engineer="
    );
    assert!(
        !walks_into_buildings(&rules, rules.object("E1").expect("a GI")),
        "a rifleman is not"
    );
}

/// A spy takes what is worth the walk, and only what a spy may enter: the
/// power plant over the refinery beside it, and neither over a derrick the
/// click would have refused.
#[test]
fn a_spy_targets_the_building_worth_the_most() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let spy = sim
        .spawn_object("SPY", "Americans", 8, 8, 64, &rules)
        .expect("a spy");
    // Nearest of all, and `Capturable=`, but not `Spyable=`.
    let derrick = sim
        .spawn_object("CAOILD", "Russians", 10, 10, 64, &rules)
        .expect("a derrick");
    let refinery = sim
        .spawn_object("NAREFN", "Russians", 12, 12, 64, &rules)
        .expect("a refinery");
    let power = sim
        .spawn_object("NAPOWR", "Russians", 22, 22, 64, &rules)
        .expect("a power plant");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    let mut out = Outbox::default();
    brain.rebuild_census(&rules, &obs);
    captures(&mut brain, &rules, &obs, &mut out);

    let target = out
        .orders
        .iter()
        .find(|order| order.actor == spy)
        .and_then(|order| match order.goal {
            Goal::Capture { target } => Some(target),
            _ => None,
        })
        .expect("the spy is given a target");
    assert_eq!(
        target, refinery,
        "the refinery pays in the minute it lands; the plant only blacks the enemy out"
    );
    assert_ne!(
        target, derrick,
        "a derrick is `Capturable=`, not `Spyable=`"
    );
    assert_ne!(target, power);
}

/// The garrison order is the Capture click, and the click's own admission
/// has to answer for a rifleman: an `Occupier=` infantryman over a
/// `CanBeOccupied=` building is action 9 (`FootClass::ClickedAction`
/// `0x004D7716..0x004D773A`), the same arm an engineer and a spy use. Sent
/// as Enter — the *dock* mission — the rifleman walks at the wall and
/// stands there, because a `CanBeOccupied=` building answers no `DOCK_NOW`
/// (`radio::receive`'s `building_dock_now`).
#[test]
fn a_rifleman_garrisons_with_the_capture_click() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=GI\n[BuildingTypes]\n0=SHACK\n\
         [GI]\nStrength=100\nSpeed=4\nOccupier=yes\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [SHACK]\nStrength=500\nFoundation=2x2\nCanBeOccupied=yes\n\
         MaxNumberOccupants=5\n",
    ))
    .expect("garrison rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    let gi = sim
        .spawn_object("GI", "Americans", 10, 12, 64, &rules)
        .expect("a rifleman");
    let shack = sim
        .spawn_object("SHACK", "Americans", 12, 12, 64, &rules)
        .expect("a shack");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    garrison(&mut brain, &rules, &obs, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == gi)
            .map(|order| order.goal),
        Some(Goal::Capture { target: shack }),
        "the rifleman takes the shack with the click that garrisons it"
    );
}

/// A hurt vehicle is sent *into* the depot, not at its cell. Enter is the
/// click a player makes on its own `UnitRepair=` building
/// (`FootClass::ClickedAction`'s depot arm): the mission asks the bay for
/// `CAN_DOCK`, walks the unit onto the pad, and the `DOCK_NOW` that follows
/// hands the building its `Repair` mission — which is what heals the unit
/// and takes the money. A Move to the depot's cell has no arrival at all:
/// the pad is inside the foundation.
#[test]
fn a_hurt_vehicle_is_sent_into_the_depot() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=DEPOT\n\
         [TANK]\nStrength=100\nSpeed=5\nPrimary=Gun\nArmor=none\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [Gun]\nDamage=10\nROF=20\nRange=4\nWarhead=SA\nProjectile=Bullet\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [DEPOT]\nStrength=1000\nFoundation=3x3\nUnitRepair=yes\n\
         NumberImpassableRows=1\n",
    ))
    .expect("depot rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    let tank = sim
        .spawn_object("TANK", "Americans", 6, 20, 64, &rules)
        .expect("a tank");
    // A fifth of its strength: under the line the retreat is called at.
    sim.substrate
        .entities
        .get_mut(tank)
        .expect("the tank")
        .health
        .set_for_test(20);
    let depot = sim
        .spawn_object("DEPOT", "Americans", 20, 20, 255, &rules)
        .expect("a repair depot");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    managed(&mut brain, &rules, &obs, 0, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == tank)
            .map(|order| order.goal),
        Some(Goal::Enter { target: depot }),
        "the hurt tank clicks the bay, it does not walk at its cell"
    );
}
/// And the other half of the retreat rule: a hurt unit in a fight the
/// house is winning stays in it.
///
/// Every hurt unit walking home is a third of the attackers peeling off
/// mid-exchange, which is what makes a push a string of small fights
/// instead of one. The house's army outweighs what stands around the
/// unit, so the unit keeps fighting.
#[test]
fn a_hurt_unit_in_a_won_fight_stays_in_it() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=TANK\n\n\
         [TANK]\nStrength=100\nSpeed=5\nPrimary=Gun\nArmor=none\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [Gun]\nDamage=10\nROF=20\nRange=4\nWarhead=SA\nProjectile=Bullet\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("tank rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    let tank = sim
        .spawn_object("TANK", "Americans", 6, 20, 64, &rules)
        .expect("a tank");
    sim.substrate
        .entities
        .get_mut(tank)
        .expect("the tank")
        .health
        .set_for_test(20);
    // Two more of ours, whole, and one of theirs across the field.
    sim.spawn_object("TANK", "Americans", 9, 20, 64, &rules)
        .expect("a tank");
    sim.spawn_object("TANK", "Americans", 12, 20, 64, &rules)
        .expect("a tank");
    sim.spawn_object("TANK", "Russians", 7, 20, 64, &rules)
        .expect("an enemy tank");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    managed(&mut brain, &rules, &obs, 0, &mut out);

    assert!(
        !out.orders
            .iter()
            .any(|order| order.actor == tank && matches!(order.goal, Goal::Enter { .. })),
        "the hurt tank stays in a fight the house is winning"
    );
}

/// The whole way in: the Capture click the brain now sends has to end with
/// the rifleman *inside*, not standing at the wall. This is the order the
/// player's right button sends (`Command::CaptureBuilding`), driven through
/// the real receiver, mission and `PerCellProcess`.
#[test]
fn a_capture_order_walks_the_rifleman_into_the_building() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=GI\n[BuildingTypes]\n0=SHACK\n\
         [GI]\nStrength=100\nSpeed=4\nOccupier=yes\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [SHACK]\nStrength=500\nFoundation=2x2\nCanBeOccupied=yes\n\
         MaxNumberOccupants=5\n",
    ))
    .expect("garrison rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let gi = sim
        .spawn_object_at_height("GI", "Americans", 10, 12, 64, 0, &rules)
        .expect("a rifleman");
    let shack = sim
        .spawn_object_at_height("SHACK", "Americans", 12, 12, 64, 0, &rules)
        .expect("a shack");
    assert!(
        sim.apply_command(
            "Americans",
            &Command::CaptureBuilding {
                engineer_id: gi,
                target_building_id: shack,
            },
            Some(&rules),
        ),
        "the receiver takes the capture"
    );

    let mut boarded = 0;
    for frame in 0..900 {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), Some(&grid), None, 33);
        boarded = sim
            .substrate
            .entities
            .get(shack)
            .map_or(0, |b| b.passenger_role().occupants().count());
        if boarded == 1 {
            break;
        }
        if frame % 150 == 0 {
            let unit = sim.substrate.entities.get(gi).expect("the rifleman");
            log::debug!(
                "frame {frame}: mission {:?} nav {:?}",
                unit.mission.effective(),
                unit.navigation.nav_com()
            );
        }
    }
    assert_eq!(
        boarded, 1,
        "the rifleman is inside the building, not standing at its wall"
    );
}

/// A cell past the edge is not an order: the receiver takes a `Move` to it
/// and the unit walks at the boundary for the whole term.
#[test]
fn an_off_map_cell_is_not_an_order() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=GI\n[GI]\nStrength=100\nSpeed=4\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    let owner = sim.interner.intern("Americans");
    let gi = sim
        .spawn_object("GI", "Americans", 4, 4, 64, &rules)
        .expect("a rifleman");
    let obs = Observation::new(&sim, owner);
    let (width, height) = obs.map_extent();
    assert!(width > 0 && height > 0, "the fixture has a map");
    let mut out = Outbox::default();

    assert!(
        !issue(
            &AssistBrain::new(0xA17),
            &mut out,
            &rules,
            &obs,
            gi,
            Goal::Move {
                cell: (width + 4, height + 4)
            }
        ),
        "the edge is not a destination"
    );
    assert!(out.orders.is_empty(), "nothing was ordered");
    assert!(
        issue(
            &AssistBrain::new(0xA17),
            &mut out,
            &rules,
            &obs,
            gi,
            Goal::Move {
                cell: (width - 1, height - 1)
            }
        ),
        "the last cell is"
    );
    assert_eq!(out.orders.len(), 1);
}

/// `Occupier=` infantry garrison the house's own bunker, through the same
/// Capture click the player's right button sends.
#[test]
fn riflemen_garrison_a_bunker_of_their_own() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let gi = sim
        .spawn_object("E1", "Americans", 10, 12, 64, &rules)
        .expect("a GI");
    // Above `ConditionRed`: a bunker at red health admits nobody, the same
    // rule the sim's own garrison ask enforces.
    let bunker = sim
        .spawn_object("NABNKR", "Americans", 12, 12, 64, &rules)
        .expect("a battle bunker");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    let mut out = Outbox::default();
    brain.rebuild_census(&rules, &obs);
    garrison(&mut brain, &rules, &obs, &mut out);

    assert!(
        out.orders.iter().any(|order| order.actor == gi
            && matches!(order.goal, Goal::Capture { target } if target == bunker)),
        "the rifleman takes the bunker: {:?}",
        out.orders
    );
}

/// A charged power is fired at the enemy's Construction Yard. Native has
/// this decision (`AI_TryFireSW`), but it is gated on the house not being
/// human — and a brain-driven house is a human's, so without this step the
/// button is never pressed.
#[test]
fn a_charged_super_is_fired_at_the_enemy_yard() {
    use crate::sim::superweapon::SuperWeaponInstance;

    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    sim.spawn_object("NACNST", "Russians", 24, 24, 255, &rules)
        .expect("an enemy Construction Yard");
    let Some(name) = rules
        .super_weapon_order
        .iter()
        .find(|name| {
            rules
                .super_weapon(name)
                .is_some_and(|sw| spent_on_the_enemy(sw.kind))
        })
        .cloned()
    else {
        return;
    };
    let type_id = sim.interner.intern(&name);
    let mut instance = SuperWeaponInstance::new(type_id, owner, 0);
    instance.set_is_active_for_test(true);
    instance.set_is_ready_for_test(true);
    instance.set_charge_start_tick_for_test(0);
    instance.set_charge_duration_for_test(9_000);
    sim.super_weapons_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(type_id, instance);

    let obs = Observation::new(&sim, owner);
    assert!(obs.super_weapons(), "the arena plays with powers on");
    assert_eq!(
        obs.ready_super_weapons(&rules),
        vec![type_id],
        "the charged power is the one live button"
    );

    let mut brain = AssistBrain::new(0xA17);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &obs,
    );
    let mut out = Outbox::default();
    supers(&mut brain, &rules, &obs, &mut out);

    assert_eq!(out.intents.len(), 1, "{:?}", out.intents);
    match out.intents[0] {
        Intent::LaunchSuperWeapon { sw_type_id, cell } => {
            assert_eq!(sw_type_id, type_id);
            assert_eq!(cell, (24, 24), "fired at the enemy yard");
        }
        ref other => panic!("a launch, not {other:?}"),
    }
    // The same frame again: the cooldown keeps a refused or recharging
    // button from being clicked every frame.
    let mut again = Outbox::default();
    supers(&mut brain, &rules, &obs, &mut again);
    assert!(again.intents.is_empty(), "{:?}", again.intents);
}

/// The Iron Curtain and the Force Shield are spent on the house's own
/// force. Firing them at the enemy burns the charge for nothing.
#[test]
fn a_power_held_for_our_own_force_is_not_fired_at_the_enemy() {
    assert!(!spent_on_the_enemy(SuperWeaponKind::IronCurtain));
    assert!(!spent_on_the_enemy(SuperWeaponKind::ForceShield));
    assert!(!spent_on_the_enemy(SuperWeaponKind::ChronoSphere));
    assert!(spent_on_the_enemy(SuperWeaponKind::MultiMissile));
    assert!(spent_on_the_enemy(SuperWeaponKind::LightningStorm));
}

/// The wrench is a toggle, so the house clicks it once per wound: a second
/// click turns the repair off, and a house that asks every frame never heals
/// anything. A building still going up is not a wound either.
#[test]
fn the_wrench_is_clicked_once_per_wound() {
    let Some((mut sim, rules)) = arena(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    // The wound is set after the spawn: the fixture's health argument also
    // sets the entity's `max_health`, so a plant spawned "at 200" reads as a
    // plant at full health and the step correctly does nothing. Damage is a
    // current-health change against the type's own strength, as the sim's.
    let hurt = sim
        .spawn_object("GAPOWR", "Americans", 16, 16, 255, &rules)
        .expect("a power plant");
    sim.substrate
        .entities
        .get_mut(hurt)
        .expect("the plant")
        .health
        .set(300);

    let mut brain = AssistBrain::new(0xA17);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );
    let obs = Observation::new(&sim, owner);
    // Nothing but a healthy building: nothing to click.
    let mut out = Outbox::default();
    repairs(&mut brain, &rules, &obs, &mut out);
    assert!(
        out.commands.is_empty(),
        "a building that has never stood whole is not a wound: {:?}",
        out.commands
    );

    brain.repaired.insert(hurt);
    let mut out = Outbox::default();
    repairs(&mut brain, &rules, &obs, &mut out);
    assert_eq!(
        out.commands
            .iter()
            .filter(|command| matches!(command, Command::ToggleRepair { .. }))
            .count(),
        1,
        "one click for one wound, not one per frame: {:?}",
        out.commands
    );
    let mut again = Outbox::default();
    repairs(&mut brain, &rules, &obs, &mut again);
    assert!(
        again.commands.is_empty(),
        "the wrench is already on: {:?}",
        again.commands
    );
}

/// A hero pulls out early, and an ordinary soldier does not.
///
/// `BuildLimit=1` is the rules' own statement of "there is one of these and
/// it cannot be bought again". In the retail rules that cap lands on exactly
/// two *army* types — `[BORIS]` and `[YURIPR]` — and on nine buildings, none
/// of which is ever army; Tanya carries the same cap but is a C4
/// infiltrator, which `is_army` already excludes. So the cap is the
/// wishlist's own definition of a hero and no name list is needed.
///
/// Both units are set to the same fraction of their strength here: above the
/// ordinary line, below the hero's. Only Boris is walked home.
#[test]
fn a_hero_pulls_out_well_before_the_ordinary_line() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Russians");
    // `[BORIS] Strength=200`, `[E1] Strength=125`. The fixture's health
    // argument also sets `max_health`, so the wound is written after the
    // spawn against the type's own strength.
    let boris = sim
        .spawn_object("BORIS", "Russians", 14, 14, 255, &rules)
        .expect("Boris");
    sim.substrate
        .entities
        .get_mut(boris)
        .expect("Boris")
        .health
        .set(150);
    let gi = sim
        .spawn_object("E1", "Russians", 12, 14, 255, &rules)
        .expect("a conscript");
    sim.substrate
        .entities
        .get_mut(gi)
        .expect("the conscript")
        .health
        .set(93);

    let mut brain = AssistBrain::new(0xA17);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );
    let obs = Observation::new(&sim, owner);
    let boris_traits = brain
        .census
        .units
        .iter()
        .find(|unit| unit.entity_id == boris)
        .map(|unit| unit.traits)
        .expect("Boris is in the census");
    assert!(
        boris_traits.combat && boris_traits.irreplaceable,
        "Boris is army and irreplaceable"
    );
    let gi_traits = brain
        .census
        .units
        .iter()
        .find(|unit| unit.entity_id == gi)
        .map(|unit| unit.traits)
        .expect("the conscript is in the census");
    assert!(
        gi_traits.combat && !gi_traits.irreplaceable,
        "a conscript is army and replaceable"
    );

    let mut out = Outbox::default();
    managed(&mut brain, &rules, &obs, 0, &mut out);

    assert!(
        out.attachments.iter().any(|a| a.actor == boris),
        "the hero walks out at three quarters: {:?}",
        out.attachments.iter().map(|a| a.actor).collect::<Vec<_>>()
    );
    assert!(
        !out.attachments.iter().any(|a| a.actor == gi),
        "the conscript stays: three quarters is not its line"
    );
}

/// A sale that does not take is written off, not retried for the match.
///
/// Measured: a captured `CAFRMB` at (110, 63) qualified for sale every
/// `SALE_GAP_FRAMES` for a whole twenty-thousand-tick match and was still
/// standing at the end. The step had no feedback at all — it read "still
/// there" as "not tried yet" and spent a sale slot every few seconds for the
/// rest of the game.
#[test]
fn a_building_that_does_not_sell_is_written_off() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
        .expect("the house's own yard");
    let captured = sim
        .spawn_object("CAFRMB", "Americans", 110, 63, 255, &rules)
        .expect("a captured French structure");
    let mut brain = AssistBrain::new(0xA17);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );
    let obs = Observation::new(&sim, owner);

    let attempts = |brain: &mut AssistBrain, out: &mut Outbox| {
        brain.sale_at = 0;
        sell_hard_to_hold(brain, &rules, &obs, out);
    };

    let mut out = Outbox::default();
    attempts(&mut brain, &mut out);
    assert!(
        out.intents.iter().any(|intent| matches!(
            intent,
            Intent::SellBuilding { entity_id } if *entity_id == captured
        )),
        "the first attempt sells it: {:?}",
        out.intents
    );

    // The sale does not take — the object is still standing — so the step
    // tries again, and again, and then stops.
    let mut spent = 0;
    for _ in 0..SALE_ATTEMPTS {
        let mut out = Outbox::default();
        attempts(&mut brain, &mut out);
        spent += out
            .intents
            .iter()
            .filter(|intent| {
                matches!(intent, Intent::SellBuilding { entity_id } if *entity_id == captured)
            })
            .count();
    }
    assert!(
        spent > 0,
        "the retries happen before the write-off: {spent}"
    );

    let mut after = Outbox::default();
    attempts(&mut brain, &mut after);
    assert!(
        !after.intents.iter().any(|intent| matches!(
            intent,
            Intent::SellBuilding { entity_id } if *entity_id == captured
        )),
        "and then it is written off, not retried for the match: {:?}",
        after.intents
    );
}

/// An engineer is spent on the enemy's tech, not on the map's own money.
///
/// A captured building is a building the house holds, and
/// `HouseClass::Can_Build @ 0x004F7870` reads what the house holds: take the
/// Battle Lab and its tree opens to you, take the yard and the side does.
/// The derrick standing in the same base is 500 credits and the engineer.
#[test]
fn an_engineer_takes_the_enemy_tech_before_the_maps_own_money() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 16, 10, 255, &rules)
        .expect("an engineer");
    let lab = sim
        .spawn_object("GATECH", "Russians", 14, 12, 255, &rules)
        .expect("their battle lab");
    let derrick = sim
        .spawn_object("CAOILD", "Russians", 20, 10, 255, &rules)
        .expect("an oil derrick");
    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    // Two bays at home: the ledger is where the house keeps them, and the
    // derrick is the prize only while it is short of them.
    brain.census.refineries = 2;
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &obs, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == engineer)
            .map(|order| order.goal),
        Some(Goal::Capture { target: lab }),
        "the lab is the prize, not the derrick: {derrick}"
    );
}

/// The opening, with no bay at home: the same engineer takes the derrick.
///
/// A derrick is a refinery the house did not build, and the credits that
/// come out of it are what pays for the army that walks into the lab a few
/// minutes later. So while the house is short of bays the derrick outranks
/// the enemy's own base, and the tech it will take afterwards waits.
#[test]
fn an_engineer_takes_the_derrick_while_the_house_has_no_income() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 16, 10, 255, &rules)
        .expect("an engineer");
    let lab = sim
        .spawn_object("GATECH", "Russians", 14, 12, 255, &rules)
        .expect("their battle lab");
    let derrick = sim
        .spawn_object("CAOILD", "Russians", 20, 10, 255, &rules)
        .expect("an oil derrick");
    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    assert_eq!(brain.census.refineries, 0, "the opening: no bay stands");
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &obs, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == engineer)
            .map(|order| order.goal),
        Some(Goal::Capture { target: derrick }),
        "the derrick pays for the army that takes the {lab} later"
    );
}

/// The building standing in the road answers for every cell of itself, and
/// it answers whose it is.
///
/// This is the question the wall pass asks about the one cell in front of a
/// unit: a structure closes several cells of ground and it is not an
/// overlay, so "is there a wall here" does not reach it, and the walk has
/// no way to tell a building it may shell from the house's own power plant.
#[test]
fn a_structure_answers_for_every_cell_of_its_foundation() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let lab = sim
        .spawn_object("GATECH", "Russians", 14, 12, 255, &rules)
        .expect("their battle lab");
    let own = sim
        .spawn_object("GAPOWR", "Americans", 8, 20, 255, &rules)
        .expect("the house's own plant");

    let obs = Observation::new(&sim, owner);
    assert_eq!(obs.structure_at(&rules, (14, 12)), Some((lab, false)));
    assert_eq!(
        obs.structure_at(&rules, (15, 12)),
        Some((lab, false)),
        "the second cell of the lab is the lab"
    );
    assert_eq!(
        obs.structure_at(&rules, (8, 20)),
        Some((own, true)),
        "and the house is told which one is its own"
    );
    assert_eq!(obs.structure_at(&rules, (2, 2)), None);
}

/// The map's own derrick belongs to nobody's side, and an engineer takes
/// it all the same: `Capturable=` is the click's own admission, and the
/// house it stands in is neither the enemy's nor this one's.
#[test]
fn an_engineer_captures_the_neutral_building_nobody_owns() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 16, 10, 255, &rules)
        .expect("an engineer");
    // A third house, the one the scenario puts the map's tech buildings
    // in: not this house's, not its ally's, and nobody it is at war with.
    // What "neutral" means from the capture's side is exactly that it is
    // in none of the three.
    let civilian = sim.interner.intern("Civilian");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            civilian, 0, None, false, 0, 10,
        ));
    let derrick = sim
        .spawn_object("CAOILD", "Civilian", 20, 10, 255, &rules)
        .expect("the map's own oil derrick");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &obs, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == engineer)
            .map(|order| order.goal),
        Some(Goal::Capture { target: derrick }),
        "a neutral derrick is the capture the opening wants"
    );
}

/// The same engineer, with a tank standing next to the lab: it takes the
/// derrick instead.
///
/// An engineer that walks into a guard is a 500-credit gift, and the map's
/// own buildings are the income nobody is defending — which is what makes
/// them the right capture while the enemy's base still has its tanks in
/// it. The worth only decides between two targets equally safe.
#[test]
fn an_engineer_takes_the_unguarded_derrick_over_the_guarded_lab() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 16, 10, 255, &rules)
        .expect("an engineer");
    let lab = sim
        .spawn_object("GATECH", "Russians", 14, 12, 255, &rules)
        .expect("their battle lab");
    let derrick = sim
        .spawn_object("CAOILD", "Russians", 20, 10, 255, &rules)
        .expect("an oil derrick");
    let _guard = sim
        .spawn_object("HTNK", "Russians", 13, 12, 255, &rules)
        .expect("the tank standing next to it");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &obs, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == engineer)
            .map(|order| order.goal),
        Some(Goal::Capture { target: derrick }),
        "the derrick, not the {lab} with a tank beside it"
    );
}

/// A neutral derrick behind the enemy's yard is not a walk: the engineer
/// went straight *through* a construction yard to reach one, because the
/// only thing the walk was checked against was where the derrick stood.
/// Enemy buildings are cover the route has to get past.
#[test]
fn an_engineer_does_not_walk_through_the_enemy_base() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 16, 14, 255, &rules)
        .expect("an engineer");
    // The yard sits squarely on the straight line to the near derrick.
    let _yard = sim
        .spawn_object("GACNST", "Russians", 12, 12, 255, &rules)
        .expect("their yard");
    let behind = sim
        .spawn_object("CAOILD", "Russians", 8, 10, 255, &rules)
        .expect("a derrick behind the base");
    let _open = sim
        .spawn_object("CAOILD", "Russians", 26, 14, 255, &rules)
        .expect("a derrick in the open");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &obs, &mut out);

    let chosen = out
        .orders
        .iter()
        .find(|order| order.actor == engineer)
        .map(|order| order.goal);
    assert!(chosen.is_some(), "the engineer still has work to do");
    assert_ne!(
        chosen,
        Some(Goal::Capture { target: behind }),
        "the walk to {behind} goes through the base"
    );
}

/// The walk is what the guard kills, not the destination: a tank posted on
/// the line to the lab — nowhere near the lab itself — still makes the lab
/// the wrong capture. Only the target's own clearance used to be asked, so
/// an engineer walked through the tank and died half-way.
#[test]
fn an_engineer_does_not_walk_through_a_guard_to_reach_its_target() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 16, 14, 255, &rules)
        .expect("an engineer");
    // Far to the west and worth the most: the tree it unlocks is what an
    // engineer is for, and its own clearance is empty.
    let lab = sim
        .spawn_object("GATECH", "Russians", 2, 10, 255, &rules)
        .expect("their battle lab");
    let derrick = sim
        .spawn_object("CAOILD", "Russians", 28, 14, 255, &rules)
        .expect("an oil derrick");
    // Half-way to the lab, seven cells from it: outside the lab's own
    // clearance, dead on the walk to it, and nowhere near the derrick.
    let _guard = sim
        .spawn_object("HTNK", "Russians", 9, 12, 255, &rules)
        .expect("a tank on the way");

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    // Two bays at home, so the derrick is worth 3 and the lab 5: only the
    // fire on the walk can put the derrick first.
    brain.census.refineries = 2;
    let mut out = Outbox::default();
    captures(&mut brain, &rules, &obs, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == engineer)
            .map(|order| order.goal),
        Some(Goal::Capture { target: derrick }),
        "the derrick, not the {lab} the tank's gun covers the walk to"
    );
}

/// A captured yard packs and comes home; the house's own last one does not.
///
/// The wishlist's "占领敌方建造场以后…打包撤离到己方基地". A captured yard
/// stands where the enemy built it, and its type is `BuildConst=` eligible so
/// the sale step refuses it by name: without this it is a second base the
/// enemy shells at leisure and the house can never move.
#[test]
fn a_captured_yard_packs_and_the_last_one_does_not() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    // Two yards, so packing one still leaves the house a base.
    let home = sim
        .spawn_object("GACNST", "Americans", 20, 20, 255, &rules)
        .expect("the home yard");
    let far = sim
        .spawn_object("GACNST", "Americans", 40, 40, 255, &rules)
        .expect("a captured yard");
    // A base is its buildings, not one yard: the pack step tells the two
    // yards apart by what stands around them, which is what a real base
    // looks like and what two lone yards cannot express.
    // A 3x3 yard at (20,20) covers 20..22, so the neighbours go clear of it.
    for (name, cell) in [("GAPOWR", (25u16, 20u16)), ("GAREFN", (20, 26))] {
        sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
            .unwrap_or_else(|| panic!("{name} beside the home yard"));
    }
    let mut brain = AssistBrain::new(0xA17);
    brain.census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );
    let obs = Observation::new(&sim, owner);
    let mut out = Outbox::default();
    pack_captured_yard(&mut brain, &rules, &obs, &mut out);
    let packed = |out: &Outbox| {
        out.commands
            .iter()
            .any(|command| matches!(command, Command::UndeployBuilding { entity_id } if *entity_id == far))
    };
    // The far one packs.
    if obs.can_repack_yard(&rules, far) {
        assert!(
            packed(&out),
            "the captured yard is packed: {:?}",
            out.commands
        );
    }
    // The home one never does, whatever the wallet.
    assert!(
        !out
            .commands
            .iter()
            .any(|command| matches!(command, Command::UndeployBuilding { entity_id } if *entity_id == home)),
        "the house's own base yard is not packed: {:?}",
        out.commands
    );
    let _ = sim.spawn_object("GACNST", "Russians", 10, 10, 255, &rules);
}

/// A dog's jaw is a `Parasite=yes` warhead, and the parasite arm takes only
/// infantry: `GetFireError` answers Illegal over a tank's armour and over a
/// structure. The order used to go out anyway, and the dog stood at the
/// target its whole term for nothing — the "stuck unit" that was a refused
/// order. The infantry one cell further off is what it can take.
#[test]
fn a_dog_is_not_sent_at_what_its_jaw_cannot_take() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let dog = sim
        .spawn_object("ADOG", "Americans", 10, 10, 255, &rules)
        .expect("a dog");
    let tank = sim
        .spawn_object("MTNK", "Russians", 11, 10, 255, &rules)
        .expect("an enemy tank");
    let infantry = sim
        .spawn_object("E1", "Russians", 12, 10, 255, &rules)
        .expect("an enemy rifleman");
    let building = sim
        .spawn_object("GAPOWR", "Russians", 14, 10, 255, &rules)
        .expect("an enemy power plant");
    let obs = Observation::new(&sim, owner);

    assert!(
        !admits(&rules, &obs, dog, &Goal::Attack { target: tank }),
        "a jaw does not take a tank"
    );
    assert!(
        !admits(&rules, &obs, dog, &Goal::Attack { target: building }),
        "nor a building"
    );
    assert!(
        admits(&rules, &obs, dog, &Goal::Attack { target: infantry }),
        "infantry is what the jaw is for"
    );
}

/// An engineer takes an enemy's `Capturable=` structure and nothing else.
/// An allied building answers Repair, a defence structure (`GAPILL`
/// `Capturable=false`) answers nothing, and both used to be walked into
/// and stood at.
#[test]
fn an_engineer_is_not_sent_at_a_building_it_cannot_take() {
    let Some((mut sim, rules)) = snapshot(0xA17) else {
        return;
    };
    let owner = sim.interner.intern("Americans");
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 10, 10, 255, &rules)
        .expect("an engineer");
    let enemy_plant = sim
        .spawn_object("GAPOWR", "Russians", 12, 10, 255, &rules)
        .expect("an enemy power plant");
    let our_plant = sim
        .spawn_object("GAPOWR", "Americans", 10, 12, 255, &rules)
        .expect("our own power plant");
    let enemy_pillbox = sim
        .spawn_object("GAPILL", "Russians", 8, 10, 255, &rules)
        .expect("an enemy pillbox");
    let obs = Observation::new(&sim, owner);

    assert!(
        admits(
            &rules,
            &obs,
            engineer,
            &Goal::Capture {
                target: enemy_plant
            }
        ),
        "a `Capturable=` enemy structure is the click"
    );
    assert!(
        !admits(&rules, &obs, engineer, &Goal::Capture { target: our_plant }),
        "our own building is a Repair click, never a capture"
    );
    assert!(
        !admits(
            &rules,
            &obs,
            engineer,
            &Goal::Capture {
                target: enemy_pillbox
            }
        ),
        "`Capturable=false` answers no action at all"
    );
}

/// A hostile pacing across the old single radius had the soldier kneeling
/// and rising forever, and a soldier mid-animation cannot shoot — which is
/// what "sometimes it fires, sometimes it does not" looked like on the
/// screen. The pose now has a dead band, so nothing between the two radii
/// changes it.
#[test]
fn a_dug_in_soldier_keeps_its_pose_until_the_fight_is_gone() {
    let squared = |cells: i32| cells * cells;
    assert_eq!(pose_to_take(false, squared(5)), Some(true), "down it goes");
    assert_eq!(
        pose_to_take(true, squared(8)),
        None,
        "still kneeling: the fight has not left"
    );
    assert_eq!(
        pose_to_take(true, squared(12)),
        Some(false),
        "the fight is gone, stand up"
    );
    assert_eq!(
        pose_to_take(true, i32::MAX),
        Some(false),
        "nothing at all: stand up"
    );
    assert_eq!(
        pose_to_take(false, squared(8)),
        None,
        "a standing soldier does not kneel for a distant one"
    );
}

/// A crate is a bonus, not a job. The pickup is the walk — the map takes
/// the crate when a Foot commits to the cell — so the order is a plain
/// Move, and what is worth testing is who it is given to: a scout with
/// nothing else to do, for a crate it can walk to.
#[test]
fn an_idle_scout_walks_to_a_crate_near_it() {
    use crate::sim::crates::CrateSlot;

    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=DOG\n\
         [DOG]\nStrength=100\nSpeed=6\nNotHuman=yes\nNatural=yes\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n",
    ))
    .expect("scout rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    let dog = sim
        .spawn_object("DOG", "Americans", 10, 10, 64, &rules)
        .expect("a scout");
    *sim.crate_authority_mut_for_test().slot_mut_for_test(0) = CrateSlot {
        cell_x: 13,
        cell_y: 10,
        ..CrateSlot::default()
    };

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    crates(&mut brain, &rules, &obs, &mut out);

    assert_eq!(
        out.orders
            .iter()
            .find(|order| order.actor == dog)
            .map(|order| order.goal),
        Some(Goal::Move { cell: (13, 10) }),
        "the scout detours for the crate: {:?}",
        out.orders
    );

    // The walk is the order, so asking again while it is under way would
    // restart a walk that is already happening.
    let mut again = Outbox::default();
    crates(&mut brain, &rules, &obs, &mut again);
    assert!(again.orders.is_empty(), "{:?}", again.orders);
}

/// A crate across the map is not worth the scan or the walk: a scout that
/// spends the match fetching crates is a scout that is not scouting.
#[test]
fn a_crate_out_of_reach_is_not_a_job_for_a_scout() {
    use crate::sim::crates::CrateSlot;

    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=DOG\n\
         [DOG]\nStrength=100\nSpeed=6\nNotHuman=yes\nNatural=yes\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n",
    ))
    .expect("scout rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.spawn_object("DOG", "Americans", 10, 10, 64, &rules)
        .expect("a scout");
    // Ten cells east: past `CRATE_SEEK_CELLS`, so not a detour.
    *sim.crate_authority_mut_for_test().slot_mut_for_test(0) = CrateSlot {
        cell_x: 20,
        cell_y: 10,
        ..CrateSlot::default()
    };

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    crates(&mut brain, &rules, &obs, &mut out);

    assert!(out.orders.is_empty(), "{:?}", out.orders);
}

/// A crate that is not next to the unit is a job, and the house does not
/// give one: a fighter that walks thirty cells for a box is a fighter
/// that is not fighting, and the army that sent it is the army that lost
/// the match. Measured on the pinned map: sending one across turned a win
/// at 22915 into a timeout at 36000.
#[test]
fn a_crate_across_the_map_is_not_a_job_for_a_fighter_either() {
    use crate::sim::crates::CrateSlot;

    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=DOG\n\
         [DOG]\nStrength=100\nSpeed=6\nNotHuman=yes\nNatural=yes\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [VehicleTypes]\n0=TANK\n\
         [TANK]\nStrength=100\nSpeed=5\nPrimary=Gun\nArmor=none\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [Gun]\nDamage=10\nROF=20\nRange=4\nWarhead=SA\nProjectile=Bullet\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("crate rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.spawn_object("DOG", "Americans", 10, 10, 64, &rules)
        .expect("a scout");
    sim.spawn_object("TANK", "Americans", 11, 10, 64, &rules)
        .expect("a tank");
    // Twenty cells east: a walk, not a detour.
    *sim.crate_authority_mut_for_test().slot_mut_for_test(0) = CrateSlot {
        cell_x: 30,
        cell_y: 10,
        ..CrateSlot::default()
    };

    let obs = Observation::new(&sim, owner);
    let mut brain = AssistBrain::new(0xA17);
    brain.rebuild_census(&rules, &obs);
    let mut out = Outbox::default();
    crates(&mut brain, &rules, &obs, &mut out);

    assert!(out.orders.is_empty(), "{:?}", out.orders);
}

/// Every unit a scenario spawns for the player starts on area guard. The
/// brain used to read that as "this unit already has a job" and left the
/// whole opening force standing in the yard for the match; a guard is the
/// posture to replace, not a reason to leave a unit alone.
#[test]
fn an_area_guard_unit_is_not_a_unit_with_a_job() {
    assert!(
        !holds_or_fights(MissionId::from_known(MissionType::AreaGuard)),
        "the posture it was born with is the posture to replace"
    );
    assert!(
        holds_or_fights(MissionId::from_known(MissionType::Attack)),
        "a unit in a fight is not to be re-tasked"
    );
    assert!(
        holds_or_fights(MissionId::from_known(MissionType::Move)),
        "a unit on its way somewhere is not idle"
    );
}

/// A destination the walker cannot get to is not an order.
///
/// The receiver takes it, the nav target is never reached, and the unit
/// stands against the obstacle for the whole term while its intent holds
/// every other layer off — the "it was told to go and it never arrives"
/// report. The question is asked in the unit's own movement zone, so the
/// same cell is a walk for a hovercraft and an island for a rifleman.
#[test]
fn a_cell_across_the_water_is_not_an_order() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
    let owner = sim.interner.intern("Americans");
    let size = 32u16;
    crate::sim::arena_fixture::flood(
        &mut sim,
        &rules,
        size,
        (18u16..20).flat_map(|rx| (0u16..size).map(move |ry| (rx, ry))),
    );
    sim.rebuild_dynamic_navigation(&rules);
    let rifleman = sim
        .spawn_object("E1", "Americans", 4, 4, 255, &rules)
        .expect("a rifleman");
    let obs = Observation::new(&sim, owner);
    let mut out = Outbox::default();
    assert!(
        !issue(
            &AssistBrain::new(0xA17),
            &mut out,
            &rules,
            &obs,
            rifleman,
            Goal::Move { cell: (28, 4) }
        ),
        "the far bank is not somewhere a rifleman can walk"
    );
    assert!(
        issue(
            &AssistBrain::new(0xA17),
            &mut out,
            &rules,
            &obs,
            rifleman,
            Goal::Move { cell: (10, 4) }
        ),
        "and the near bank is"
    );
}
