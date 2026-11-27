//! Client reads (RFC 0001 section 10): what this player's interface may
//! show, and above all what it may not (fog, shroud, cloak, disguise,
//! allies' details, spectators).
//!
//! The arena: Americans (the player), British (its mutual ally) and
//! Russians (the enemy). The player has explored the columns x < 16.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::{CloakRuntime, DisguiseRuntime};
use crate::sim::ext_modules::cell_ref;
use crate::sim::house_state::HouseState;
use crate::sim::runtime::{SimResources, SimRuntime};
use vera20k_wasm_ext::{Area, MAX_SCAN, NAME_COUNTRY, NAME_VARIABLE, SCOPE_HOUSES, VarDecl, VarScope};

const DRIVE: &str = "Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n";

fn rules() -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n2=British\n\
         [InfantryTypes]\n0=E1\n1=SPY\n\
         [E1]\nStrength=125\n\
         [SPY]\nStrength=100\n\
         [VehicleTypes]\n0=MTNK\n1=STLTNK\n\
         [MTNK]\nStrength=300\nSpeed=4\n{DRIVE}\
         [STLTNK]\nStrength=200\nSpeed=4\nInvisible=yes\n{DRIVE}"
    ));
    RuleSet::from_ini(&ini).expect("test rules")
}

/// The explored columns of the player.
const EXPLORED: u16 = 16;

struct Scene {
    runtime: SimRuntime,
    americans: usize,
    russians: usize,
    british: usize,
    own_tank: u64,
    own_gi: u64,
    /// A British tank in unexplored ground: allies are always shown.
    ally_tank: u64,
    /// An enemy tank in explored ground.
    seen_tank: u64,
    /// An enemy tank in unexplored ground.
    fogged_tank: u64,
    /// An enemy `Invisible=yes` tank in explored ground.
    stealth_tank: u64,
    /// An enemy spy in explored ground wearing the player's GI.
    spy: u64,
    /// An enemy tank in explored ground, fully cloaked.
    cloaked_tank: u64,
}

fn scene() -> Scene {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x10C1);
    let british = sim.intern("British");
    sim.houses
        .register(HouseState::new(british, 0, None, true, 10_000, 10));
    for (from, to) in [("AMERICANS", "BRITISH"), ("BRITISH", "AMERICANS")] {
        sim.house_alliances_mut_for_test()
            .entry(from.into())
            .or_default()
            .insert(to.into());
    }
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let mut spawn = |kind: &str, house: &str, x: u16, y: u16| {
        sim.spawn_object(kind, house, x, y, 0, &rules)
            .unwrap_or_else(|| panic!("{kind} at ({x}, {y})"))
    };
    let own_tank = spawn("MTNK", "Americans", 4, 4);
    let own_gi = spawn("E1", "Americans", 4, 8);
    let ally_tank = spawn("MTNK", "British", 26, 26);
    let seen_tank = spawn("MTNK", "Russians", 10, 10);
    let fogged_tank = spawn("MTNK", "Russians", 26, 10);
    let stealth_tank = spawn("STLTNK", "Russians", 10, 14);
    let spy = spawn("SPY", "Russians", 12, 6);
    let cloaked_tank = spawn("MTNK", "Russians", 8, 20);
    sim.resolve_type_handles(&rules);
    let index = |sim: &crate::sim::world::Simulation, name: &str| {
        let id = sim.interner.get(name).expect("house");
        sim.houses.index_of(id).expect("registered")
    };
    let (americans, russians, british) = (
        index(&sim, "Americans"),
        index(&sim, "Russians"),
        index(&sim, "British"),
    );
    let player = sim.interner.get("Americans").expect("player");
    // The arena starts without a fog map; the scene's is its 32x32 cells.
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    sim.fog_mut_for_test().reveal_cells_for_owner(
        player,
        (0..32_u16).flat_map(|y| (0..EXPLORED).map(move |x| (x, y))),
    );
    let gi = sim.interner.get("E1").expect("E1");
    sim.entity_mut_for_test(spy)
        .expect("spy")
        .seldom
        .disguise
        .get_or_insert_with(|| DisguiseRuntime::new(0))
        .acquire(0, Some(gi), Some(player));
    let mut cloak = CloakRuntime::new(0);
    cloak.set_state_for_test(2);
    sim.entity_mut_for_test(cloaked_tank).expect("tank").cloak = Some(Box::new(cloak));
    Scene {
        runtime: SimRuntime {
            simulation: sim,
            resources: std::sync::Arc::new(SimResources::new(rules)),
        },
        americans,
        russians,
        british,
        own_tank,
        own_gi,
        ally_tank,
        seen_tank,
        fogged_tank,
        stealth_tank,
        spy,
        cloaked_tank,
    }
}

impl Scene {
    fn player(&self) -> ClientView<'_> {
        self.view(Viewer {
            house: house_ref(self.americans),
            spectator: false,
            sees_whole_map: false,
        })
    }

    fn spectator(&self, sees_whole_map: bool) -> ClientView<'_> {
        self.view(Viewer {
            house: 0,
            spectator: true,
            sees_whole_map,
        })
    }

    fn view(&self, viewer: Viewer) -> ClientView<'_> {
        ClientView {
            runtime: &self.runtime,
            viewer,
            local: LocalFacts::default(),
            source: Source::Mod,
        }
    }

    fn sim_mut(&mut self) -> &mut crate::sim::world::Simulation {
        &mut self.runtime.simulation
    }

    fn techno(&self, name: &str) -> i64 {
        let handle = self.runtime.resources.rules.type_handle(name).expect(name);
        type_ref(NAME_TECHNO_TYPE, handle.0)
    }
}

fn prop(name: &str) -> i32 {
    property_id(name).unwrap_or_else(|| panic!("property {name}"))
}

fn read(view: &ClientView<'_>, reference: i64, name: &str) -> (i64, Status) {
    get(view, reference, prop(name)).expect("a valid read")
}

fn status(view: &ClientView<'_>, reference: i64, name: &str) -> Status {
    read(view, reference, name).1
}

fn query(scope: i32) -> Query {
    Query {
        scope,
        area: Area::Map,
        type_ref: 0,
        owner: 0,
        viewpoint: 0,
        after: 0,
        clauses: Vec::new(),
        max_scan: MAX_SCAN,
    }
}

fn found(view: &ClientView<'_>, query: &Query) -> Vec<i64> {
    let (page, status) = find(view, query, 64).expect("a valid find");
    assert_eq!(status, Status::Ok);
    page.found
        .into_iter()
        .map(|(_, reference)| reference)
        .collect()
}

fn objects(ids: &[u64]) -> Vec<i64> {
    ids.iter().map(|&id| id as i64).collect()
}

#[test]
fn the_own_house_reads_everything_other_houses_only_public_facts() {
    let scene = scene();
    let view = scene.player();
    let (own, ally, enemy) = (
        house_ref(scene.americans),
        house_ref(scene.british),
        house_ref(scene.russians),
    );
    assert_eq!(read(&view, own, "Money"), (0, Status::Ok));
    assert_eq!(status(&view, own, "PowerOutput"), Status::Ok);
    // An ally's money, power and production are its own.
    for name in [
        "Money",
        "PowerOutput",
        "LowPower",
        "TechLevel",
        "Factory.Vehicle.Type",
    ] {
        assert_eq!(read(&view, ally, name), (0, Status::Refused), "{name}");
        assert_eq!(read(&view, enemy, name), (0, Status::Refused), "{name}");
    }
    let britain = scene
        .runtime
        .resources
        .rules
        .country_index("British")
        .expect("country");
    assert_eq!(
        read(&view, ally, "Country"),
        (type_ref(NAME_COUNTRY, u32::from(britain.0)), Status::Ok)
    );
    assert_eq!(read(&view, enemy, "IsHuman"), (0, Status::Ok));
    assert_eq!(read(&view, ally, "IsDefeated"), (0, Status::Ok));
    assert_eq!(read(&view, ally, "Relation"), (1, Status::Ok));
    assert_eq!(read(&view, enemy, "Relation"), (2, Status::Ok));
    assert_eq!(read(&view, own, "Relation"), (0, Status::Ok));
    // Simulation-only house properties, even the own house's.
    assert_eq!(read(&view, own, "ControlDomains"), (0, Status::Refused));
    // A house past the houses is gone.
    assert_eq!(status(&view, house_ref(40), "Country"), Status::Gone);
}

#[test]
fn own_objects_read_own_properties_but_not_simulation_ones() {
    let scene = scene();
    let view = scene.player();
    let tank = scene.own_tank as i64;
    assert_eq!(read(&view, tank, "Health"), (300, Status::Ok));
    assert_eq!(
        read(&view, tank, "Type"),
        (scene.techno("MTNK"), Status::Ok)
    );
    for name in [
        "Mission", "Target", "Facing", "Moving", "Visible", "Relation",
    ] {
        assert_eq!(status(&view, tank, name), Status::Ok, "{name}");
    }
    assert_eq!(read(&view, tank, "Visible"), (1, Status::Ok));
    assert_eq!(read(&view, tank, "Team"), (0, Status::Refused));
    assert_eq!(read(&view, tank, "Tag"), (0, Status::Refused));
    assert!(sees_object(&view, tank));
}

#[test]
fn nearby_enemy_count_uses_visible_objects_and_their_apparent_houses() {
    let mut scene = scene();
    let tank = scene.own_tank;
    scene
        .sim_mut()
        .entity_mut_for_test(tank)
        .expect("own tank")
        .seldom
        .sight
        .set_vision_range_for_test(24);
    let view = scene.player();
    let reference = tank as i64;
    // All five Russian objects are nearby. Only the visible tank looks hostile.
    assert_eq!(
        view.reader()
            .read_in(reference, prop("NearbyEnemyCount"), view.own_view())
            .expect("simulation count"),
        (5, Status::Ok)
    );
    assert_eq!(read(&view, reference, "NearbyEnemyCount"), (1, Status::Ok));
    let player = scene.sim_mut().interner.get("Americans").expect("player");
    scene
        .sim_mut()
        .fog_mut_for_test()
        .sensors_add_at(player, (8, 20), 2);
    assert_eq!(
        read(&scene.player(), reference, "NearbyEnemyCount"),
        (2, Status::Ok)
    );
}

#[test]
fn a_seen_enemy_shows_only_type_place_owner_and_health() {
    let scene = scene();
    let view = scene.player();
    let tank = scene.seen_tank as i64;
    assert!(sees_object(&view, tank));
    assert_eq!(
        read(&view, tank, "Type"),
        (scene.techno("MTNK"), Status::Ok)
    );
    assert_eq!(
        read(&view, tank, "Owner"),
        (house_ref(scene.russians), Status::Ok)
    );
    assert_eq!(read(&view, tank, "Health"), (300, Status::Ok));
    assert_eq!(read(&view, tank, "HealthPercent"), (100, Status::Ok));
    for name in ["X", "Y", "Z", "Cell", "Category", "Strength"] {
        assert_eq!(status(&view, tank, name), Status::Ok, "{name}");
    }
    for name in [
        "Mission",
        "Target",
        "Destination",
        "Rank",
        "Ammo",
        "PassengerCount",
        "DisguiseType",
        "Relation",
        "Visible",
        "FirepowerMultiplier",
        "Team",
    ] {
        assert_eq!(read(&view, tank, name), (0, Status::Refused), "{name}");
    }
}

#[test]
fn an_enemy_in_unexplored_ground_does_not_exist() {
    let scene = scene();
    let view = scene.player();
    let tank = scene.fogged_tank as i64;
    assert!(!sees_object(&view, tank));
    for name in ["Type", "Owner", "Health", "X", "Mission"] {
        assert_eq!(read(&view, tank, name), (0, Status::Gone), "{name}");
    }
    // Selecting it locally changes nothing.
    let selected = [scene.fogged_tank];
    let view = ClientView {
        local: LocalFacts {
            selected: &selected,
            control_groups: &[],
        },
        ..scene.player()
    };
    assert_eq!(read(&view, tank, "Selected"), (0, Status::Gone));
}

#[test]
fn under_fog_of_war_explored_ground_out_of_sight_hides_enemies() {
    let mut scene = scene();
    scene
        .sim_mut()
        .session_mut_for_test()
        .game_options_mut_for_test()
        .fog_of_war = true;
    let view = scene.player();
    // Explored, but nothing of the player's sees it now.
    assert!(!sees_object(&view, scene.seen_tank as i64));
    assert_eq!(status(&view, scene.seen_tank as i64, "Type"), Status::Gone);
    // Cells out of sight keep their public facts only.
    let cell = cell_ref(10, 10);
    assert_eq!(status(&view, cell, "LandType"), Status::Ok);
    assert_eq!(read(&view, cell, "Ore"), (0, Status::Refused));
    // The player's own and allied objects stay.
    assert!(sees_object(&view, scene.own_tank as i64));
    assert!(sees_object(&view, scene.ally_tank as i64));
}

#[test]
fn an_allys_objects_show_like_any_seen_object() {
    let scene = scene();
    let view = scene.player();
    let tank = scene.ally_tank as i64;
    // Outside the explored ground, yet shown: allies always are.
    assert!(sees_object(&view, tank));
    assert_eq!(
        read(&view, tank, "Owner"),
        (house_ref(scene.british), Status::Ok)
    );
    assert_eq!(read(&view, tank, "Health"), (300, Status::Ok));
    for name in ["Mission", "Target", "Ammo", "Visible"] {
        assert_eq!(read(&view, tank, name), (0, Status::Refused), "{name}");
    }
}

#[test]
fn invisible_and_cloaked_enemies_do_not_exist_unless_detected() {
    let mut scene = scene();
    {
        let view = scene.player();
        for tank in [scene.stealth_tank, scene.cloaked_tank] {
            assert!(!sees_object(&view, tank as i64));
            assert_eq!(read(&view, tank as i64, "Type"), (0, Status::Gone));
        }
    }
    // A sensor of the player's over the cloaked tank detects it.
    let player = scene.sim_mut().interner.get("Americans").expect("player");
    scene
        .sim_mut()
        .fog_mut_for_test()
        .sensors_add_at(player, (8, 20), 2);
    let view = scene.player();
    assert!(sees_object(&view, scene.cloaked_tank as i64));
    assert_eq!(
        read(&view, scene.cloaked_tank as i64, "Type"),
        (scene.techno("MTNK"), Status::Ok)
    );
    assert!(!sees_object(&view, scene.stealth_tank as i64));
}

#[test]
fn a_disguised_enemy_reads_as_its_disguise() {
    let mut scene = scene();
    let object = scene.spy;
    scene.sim_mut().ext_modules.held.set_image(object, Some("SPY".into()));
    let view = scene.player();
    let spy = scene.spy as i64;
    assert!(sees_object(&view, spy));
    assert_eq!(read(&view, spy, "Type"), (scene.techno("E1"), Status::Ok));
    assert_eq!(
        read(&view, spy, "Owner"),
        (house_ref(scene.americans), Status::Ok)
    );
    assert_eq!(read(&view, spy, "Category"), (0, Status::Ok));
    assert_eq!(read(&view, spy, "ImageOverride"), (0, Status::Ok));
    // The GI's Strength, at the spy's full health.
    assert_eq!(read(&view, spy, "Strength"), (125, Status::Ok));
    assert_eq!(read(&view, spy, "Health"), (125, Status::Ok));
    assert_eq!(read(&view, spy, "HealthPercent"), (100, Status::Ok));
    // Its real identity stays hidden.
    for name in ["DisguiseType", "DisguiseOwner", "Mission", "Relation"] {
        assert_eq!(read(&view, spy, name), (0, Status::Refused), "{name}");
    }
    // A spectator sees through it.
    let spectator = scene.spectator(true);
    assert_eq!(read(&spectator, spy, "ImageOverride"), (scene.techno("SPY"), Status::Ok));
    assert_eq!(
        read(&spectator, spy, "Type"),
        (scene.techno("SPY"), Status::Ok)
    );
    assert_eq!(
        read(&spectator, spy, "Owner"),
        (house_ref(scene.russians), Status::Ok)
    );
}

#[test]
fn a_disguise_worn_without_a_house_is_no_object() {
    let mut scene = scene();
    let gi = scene.sim_mut().interner.get("E1").expect("E1");
    // A unit worn as something with no house behind it: the screen shows no
    // object there.
    let tank = scene.seen_tank;
    scene
        .sim_mut()
        .entity_mut_for_test(tank)
        .expect("tank")
        .seldom
        .disguise
        .get_or_insert_with(|| DisguiseRuntime::new(0))
        .acquire(0, Some(gi), None);
    let view = scene.player();
    assert!(!sees_object(&view, tank as i64));
    assert_eq!(read(&view, tank as i64, "Type"), (0, Status::Gone));
}

#[test]
fn a_spectator_sees_the_whole_map_only_when_the_room_allows() {
    let scene = scene();
    let all = scene.spectator(true);
    for tank in [
        scene.fogged_tank,
        scene.seen_tank,
        scene.ally_tank,
        scene.own_tank,
    ] {
        assert!(sees_object(&all, tank as i64));
        assert_eq!(read(&all, tank as i64, "Health"), (300, Status::Ok));
        assert_eq!(read(&all, tank as i64, "Mission"), (0, Status::Refused));
    }
    // A spectator has no house: no house's own facts.
    for house in [scene.americans, scene.russians, scene.british] {
        assert_eq!(read(&all, house_ref(house), "Money"), (0, Status::Refused));
        assert_eq!(status(&all, house_ref(house), "Country"), Status::Ok);
    }
    assert_eq!(status(&all, cell_ref(26, 10), "Obstructed"), Status::Ok);
    assert_eq!(
        read(&all, cell_ref(26, 10), "Revealed"),
        (0, Status::Refused)
    );
    let none = scene.spectator(false);
    for tank in [scene.fogged_tank, scene.seen_tank, scene.own_tank] {
        assert!(!sees_object(&none, tank as i64));
        assert_eq!(status(&none, tank as i64, "Type"), Status::Gone);
    }
    assert_eq!(read(&none, cell_ref(4, 4), "Ore"), (0, Status::Refused));
    assert_eq!(status(&none, cell_ref(4, 4), "Height"), Status::Ok);
    assert!(found(&none, &query(SCOPE_OBJECTS)).is_empty());
}

#[test]
fn cells_show_terrain_everywhere_and_the_rest_in_sight() {
    let scene = scene();
    let view = scene.player();
    let (near, far) = (cell_ref(4, 4), cell_ref(26, 10));
    for cell in [near, far] {
        for name in ["LandType", "Height", "Waypoint"] {
            assert_eq!(status(&view, cell, name), Status::Ok, "{name}");
        }
        // Answered (or not kept), never hidden.
        assert_ne!(status(&view, cell, "Zone.Normal"), Status::Refused);
        assert_eq!(read(&view, cell, "CellTag"), (0, Status::Refused));
    }
    for name in ["Overlay", "Obstructed", "Passable.Track"] {
        assert_eq!(status(&view, near, name), Status::Ok, "{name}");
    }
    assert_ne!(status(&view, near, "Ore"), Status::Refused);
    for name in ["Ore", "Overlay", "Obstructed", "Passable.Track"] {
        assert_eq!(read(&view, far, name), (0, Status::Refused), "{name}");
    }
    assert_eq!(read(&view, near, "Revealed"), (1, Status::Ok));
    assert_eq!(read(&view, far, "Revealed"), (0, Status::Ok));
    assert_eq!(status(&view, cell_ref(200, 200), "Ore"), Status::NotFound);
    // The global rules reference and types are everyone's.
    let global = 4_i64 << 48;
    assert_eq!(status(&view, global, "Frame"), Status::Ok);
    assert_eq!(
        read(&view, scene.techno("MTNK"), "Strength"),
        (300, Status::Ok)
    );
}

#[test]
fn find_lists_only_what_the_player_sees() {
    let scene = scene();
    let view = scene.player();
    let listed = found(&view, &query(SCOPE_OBJECTS));
    for id in [
        scene.own_tank,
        scene.own_gi,
        scene.ally_tank,
        scene.seen_tank,
        scene.spy,
    ] {
        assert!(listed.contains(&(id as i64)), "{id} listed");
    }
    for id in [scene.fogged_tank, scene.stealth_tank, scene.cloaked_tank] {
        assert!(!listed.contains(&(id as i64)), "{id} not listed");
    }
}

#[test]
fn find_filters_owner_and_type_as_the_player_sees_them() {
    let scene = scene();
    let view = scene.player();
    let mut mine = query(SCOPE_OBJECTS);
    mine.owner = house_ref(scene.americans);
    assert_eq!(
        found(&view, &mine),
        objects(&[scene.own_tank, scene.own_gi, scene.spy])
    );
    let mut theirs = query(SCOPE_OBJECTS);
    theirs.owner = house_ref(scene.russians);
    assert_eq!(found(&view, &theirs), objects(&[scene.seen_tank]));
    let mut spies = query(SCOPE_OBJECTS);
    spies.type_ref = scene.techno("SPY");
    assert!(found(&view, &spies).is_empty());
    let mut gis = query(SCOPE_OBJECTS);
    gis.type_ref = scene.techno("E1");
    assert_eq!(found(&view, &gis), objects(&[scene.own_gi, scene.spy]));
    // A gone owner, and arguments the simulation calls invalid.
    let mut gone = query(SCOPE_OBJECTS);
    gone.owner = house_ref(40);
    assert_eq!(find(&view, &gone, 8).expect("valid").1, Status::Gone);
    let mut weapon = query(SCOPE_OBJECTS);
    weapon.type_ref = type_ref(vera20k_wasm_ext::NAME_WEAPON, 0);
    assert!(find(&view, &weapon, 8).is_err());
}

#[test]
fn find_clauses_read_through_the_gate() {
    let scene = scene();
    let view = scene.player();
    // `Mission` is the owner's: no other object passes a clause on it.
    let mut missions = query(SCOPE_OBJECTS);
    missions.clauses.push(Clause {
        subject: ClauseSubject::Property(prop("Mission")),
        op: Op::Ge,
        value: 0,
    });
    assert_eq!(
        found(&view, &missions),
        objects(&[scene.own_tank, scene.own_gi])
    );
    // A viewpoint other than the player's own house is not honoured.
    let mut relation = query(SCOPE_HOUSES);
    relation.viewpoint = house_ref(scene.russians);
    relation.clauses.push(Clause {
        subject: ClauseSubject::Property(prop("Relation")),
        op: Op::Eq,
        value: 0,
    });
    assert_eq!(found(&view, &relation), vec![house_ref(scene.americans)]);
    // Houses are listed, money only for the own one.
    let mut money = query(SCOPE_HOUSES);
    money.clauses.push(Clause {
        subject: ClauseSubject::Property(prop("Money")),
        op: Op::Ge,
        value: 0,
    });
    assert_eq!(found(&view, &money), vec![house_ref(scene.americans)]);
    assert_eq!(found(&view, &query(SCOPE_HOUSES)).len(), 3);
}

#[test]
fn find_lists_only_explored_cells_and_no_simulation_scopes() {
    let scene = scene();
    let view = scene.player();
    let mut row = query(vera20k_wasm_ext::SCOPE_CELLS);
    row.area = Area::Rect {
        cell: cell_ref(EXPLORED - 2, 3),
        width: 4,
        height: 1,
    };
    assert_eq!(
        found(&view, &row),
        vec![cell_ref(EXPLORED - 2, 3), cell_ref(EXPLORED - 1, 3)]
    );
    for scope in [4, 5, 6, SCOPE_MODIFIERS] {
        let (page, status) = find(&view, &query(scope), 8).expect("valid");
        assert_eq!(
            (page.found.len(), status),
            (0, Status::Refused),
            "scope {scope}"
        );
    }
    // Superweapons are the own house's: nobody's are granted here.
    assert!(found(&view, &query(vera20k_wasm_ext::SCOPE_SUPER_WEAPONS)).is_empty());
}

#[test]
fn selection_and_control_groups_come_from_the_client() {
    let scene = scene();
    let selected = [scene.own_tank, scene.seen_tank];
    let groups = [(scene.own_gi, 3)];
    let view = ClientView {
        local: LocalFacts {
            selected: &selected,
            control_groups: &groups,
        },
        ..scene.player()
    };
    assert_eq!(
        read(&view, scene.own_tank as i64, "Selected"),
        (1, Status::Ok)
    );
    assert_eq!(
        read(&view, scene.seen_tank as i64, "Selected"),
        (1, Status::Ok)
    );
    assert_eq!(
        read(&view, scene.own_gi as i64, "Selected"),
        (0, Status::Ok)
    );
    assert_eq!(
        read(&view, scene.own_gi as i64, "ControlGroup"),
        (3, Status::Ok)
    );
    assert_eq!(
        read(&view, scene.own_tank as i64, "ControlGroup"),
        (-1, Status::Ok)
    );
}

/// Declared variables 1000.. in order: house private, house allies, object
/// in sight, global public, global private, object private.
fn declare_vars(scene: &mut Scene) {
    let decl = |name: &str, scope, visibility, default| {
        (
            ModuleSource::Mod,
            VarDecl {
                name: name.to_owned(),
                scope,
                visibility,
                default,
            },
        )
    };
    scene.sim_mut().ext_modules_mut().declare_vars_for_test(&[
        decl("HousePrivate", VarScope::House, Visibility::Private, 7),
        decl("HouseAllies", VarScope::House, Visibility::Allies, 8),
        decl("ObjectInSight", VarScope::Object, Visibility::InSight, 9),
        decl("GlobalPublic", VarScope::Global, Visibility::Public, 10),
        decl("GlobalPrivate", VarScope::Global, Visibility::Private, 11),
        decl("ObjectPrivate", VarScope::Object, Visibility::Private, 12),
    ]);
}

#[test]
fn variables_follow_their_visibility() {
    let mut scene = scene();
    declare_vars(&mut scene);
    let view = scene.player();
    let var = |scope: i64, var: i32| var_get(&view, scope, var).expect("a valid read");
    let (own, ally, enemy) = (
        house_ref(scene.americans),
        house_ref(scene.british),
        house_ref(scene.russians),
    );
    assert_eq!(var(own, 1000), (7, Status::Ok));
    assert_eq!(var(ally, 1000), (0, Status::Refused));
    assert_eq!(var(ally, 1001), (8, Status::Ok));
    assert_eq!(var(enemy, 1001), (0, Status::Refused));
    assert_eq!(var(scene.seen_tank as i64, 1002), (9, Status::Ok));
    assert_eq!(var(scene.fogged_tank as i64, 1002), (0, Status::Gone));
    assert_eq!(var(scene.seen_tank as i64, 1005), (0, Status::Refused));
    assert_eq!(var(scene.own_tank as i64, 1005), (12, Status::Ok));
    assert_eq!(var(scene.ally_tank as i64, 1005), (0, Status::Refused));
    assert_eq!(var(0, 1003), (10, Status::Ok));
    assert_eq!(var(0, 1004), (0, Status::Refused));
    // The Red Alert 2 variables are private globals.
    assert_eq!(var(0, 5), (0, Status::Refused));
    assert_eq!(var(0, 120), (0, Status::Refused));
    // A wrong scope stays invalid.
    assert!(var_get(&view, own, 1003).is_err());
    let spectator = scene.spectator(true);
    let seen = |scope: i64, var: i32| var_get(&spectator, scope, var).expect("valid");
    assert_eq!(seen(0, 1003), (10, Status::Ok));
    assert_eq!(seen(own, 1000), (0, Status::Refused));
    assert_eq!(seen(own, 1001), (0, Status::Refused));
    assert_eq!(seen(scene.fogged_tank as i64, 1002), (9, Status::Ok));
    assert_eq!(seen(scene.fogged_tank as i64, 1005), (0, Status::Refused));
}

#[test]
fn variable_clauses_in_find_follow_visibility_too() {
    let mut scene = scene();
    declare_vars(&mut scene);
    let view = scene.player();
    let mut private = query(SCOPE_OBJECTS);
    private.clauses.push(Clause {
        subject: ClauseSubject::Var {
            var: 1005,
            subject: true,
        },
        op: Op::Eq,
        value: 12,
    });
    assert_eq!(
        found(&view, &private),
        objects(&[scene.own_tank, scene.own_gi])
    );
}

#[test]
fn lookup_and_name_of_answer_the_simulations_names() {
    let mut scene = scene();
    declare_vars(&mut scene);
    let view = scene.player();
    assert_eq!(
        lookup(&view, NAME_VARIABLE, "HouseAllies").expect("valid"),
        (1001, Status::Ok)
    );
    assert_eq!(
        lookup(&view, NAME_VARIABLE, "map.HouseAllies").expect("valid"),
        (-1, Status::NotFound)
    );
    assert_eq!(
        lookup(&view, NAME_PROPERTY, "Money").expect("valid"),
        (prop("Money"), Status::Ok)
    );
    let mtnk = lookup(&view, NAME_TECHNO_TYPE, "MTNK").expect("valid");
    assert_eq!(mtnk.1, Status::Ok);
    assert_eq!(
        name_of(&view, NAME_TECHNO_TYPE, mtnk.0).expect("valid"),
        ("MTNK".to_owned(), Status::Ok)
    );
    assert_eq!(
        name_of(&view, NAME_VARIABLE, 1003).expect("valid"),
        ("GlobalPublic".to_owned(), Status::Ok)
    );
    // The glue's name kinds, and kinds past the table.
    assert_eq!(
        lookup(&view, NAME_NOTIFY, "x").expect("valid").1,
        Status::Unsupported
    );
    assert_eq!(
        lookup(&view, NAME_HOTKEY, "x").expect("valid").1,
        Status::Unsupported
    );
    assert!(lookup(&view, NAME_KINDS, "x").is_err());
    assert!(name_of(&view, -1, 0).is_err());
}

#[test]
fn has_kind_adds_the_client_tables() {
    let scene = scene();
    let view = scene.player();
    assert!(has_kind(&view, TABLE_PROPERTY, prop("Selected")));
    assert!(has_kind(&view, TABLE_PROPERTY, prop("Money")));
    assert!(has_kind(&view, TABLE_NAME_KIND, NAME_NOTIFY));
    assert!(has_kind(&view, TABLE_NAME_KIND, NAME_HOTKEY));
    assert!(has_kind(&view, TABLE_NODE_KIND, node_kind::BAR));
    assert!(!has_kind(&view, TABLE_NODE_KIND, node_kind::BAR + 1));
    assert!(has_kind(&view, TABLE_CLIENT_EVENT, client_event::SIM_EVENT));
    assert!(has_kind(
        &view,
        TABLE_CLIENT_ACTION,
        client_action::REGISTER_HOTKEY
    ));
    assert!(!has_kind(
        &view,
        TABLE_CLIENT_ACTION,
        client_action::HOME_CAMERA
    ));
}

#[test]
fn references_to_nothing_are_gone() {
    let scene = scene();
    let view = scene.player();
    assert_eq!(read(&view, 0, "Type"), (0, Status::Gone));
    assert_eq!(read(&view, 999_999, "Type"), (0, Status::Gone));
    assert!(!sees_object(&view, 999_999));
    assert!(!sees_object(&view, house_ref(scene.americans)));
    assert!(get(&view, -5, prop("Type")).is_err());
}
