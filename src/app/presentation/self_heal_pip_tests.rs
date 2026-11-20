//! The self-heal status marker through the production decision/placement owner
//! (`ui_overlays::self_heal_marker`), for the stock arm and the Phobos
//! `Pips.SelfHeal.*` replacement.
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

const OWNER: &str = "Americans";

fn scene(tail: &str) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n0=E1\n1=E2\n2=E3\n\
         [VehicleTypes]\n0=MTNK\n1=MTNK2\n2=MTNK3\n3=MTNK4\n\
         [AircraftTypes]\n0=ORCA\n\
         [BuildingTypes]\n0=POWR\n\
         [E1]\nStrength=100\n\
         [E2]\nStrength=100\nSelfHealGainType=NoHeal\n\
         [E3]\nStrength=100\nSelfHealGainType=Units\n\
         [MTNK]\nStrength=100\n\
         [MTNK2]\nStrength=100\nSelfHealGainType=Infantry\n\
         [MTNK3]\nStrength=100\nSelfHealGainType=Units\nPixelSelectionBracketDelta=-6\n\
         [MTNK4]\nStrength=100\nOrganic=yes\n\
         [ORCA]\nStrength=100\nSelfHealGainType=Units\n\
         [POWR]\nStrength=750\nSelfHealGainType=Units\n{tail}"
    )))
    .expect("self-heal rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get(OWNER).unwrap();
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    (sim, rules)
}

fn grant(sim: &mut Simulation, infantry: i32, units: i32) {
    let owner = sim.interner().get(OWNER).unwrap();
    crate::sim::arena_fixture::install_status_self_heal(
        sim.houses_mut_for_test()
            .get_mut(&owner)
            .expect("owner house"),
        infantry,
        units,
    );
}

fn marker(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
) -> Option<super::ui_overlays::SelfHealMarker> {
    let entity = sim.entities().get(id).expect("subject");
    let object = sim
        .interner()
        .object_type(entity.type_ref(), rules)
        .expect("type");
    super::ui_overlays::self_heal_marker(sim, entity, object, rules, 100.0, 200.0)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, type_name: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object(type_name, OWNER, rx, ry, 64, rules)
        .expect("subject spawns")
}

/// The stock arm marks an Infantry (or organic Unit) with the red cross, a
/// non-organic Unit with the wrench, each at its class offset from the health
/// anchor. Phobos keeps that classification while a type's `SelfHealGainType`
/// overrides the arm, `NoHeal` suppresses the marker, the pair/offset/anchor
/// follow the drawn class with only the pair component from the arm
/// (`Body.Visuals.cpp:91-114`), the bracket delta shifts Y, and the Building
/// arm stays a residual.
#[test]
fn the_self_heal_arm_follows_gain_type() {
    let (mut sim, rules) = scene("");
    let e1 = spawn(&mut sim, &rules, "E1", 16, 16);
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    let e2 = spawn(&mut sim, &rules, "E2", 16, 18);
    let typed = spawn(&mut sim, &rules, "MTNK2", 18, 18);
    let unit_typed = spawn(&mut sim, &rules, "E3", 20, 16);
    let unit_bracket = spawn(&mut sim, &rules, "MTNK3", 20, 18);
    let organic = spawn(&mut sim, &rules, "MTNK4", 22, 18);
    let orca = spawn(&mut sim, &rules, "ORCA", 22, 16);
    let building = spawn(&mut sim, &rules, "POWR", 24, 16);
    grant(&mut sim, 1, 1);

    assert_eq!(
        marker(&sim, &rules, e1),
        Some(super::ui_overlays::SelfHealMarker {
            frame: 13,
            position: (121.0, 175.0),
            selector: 0,
        }),
        "an Infantry draws the Infantry pair's X at (25,-35) native, i.e. (-4,+10) from the anchor"
    );
    assert_eq!(
        marker(&sim, &rules, mtnk).map(|marker| (marker.frame, marker.position)),
        Some((20, (118.0, 178.0))),
        "a non-organic Unit draws the Units pair's Y at (33,-32) native"
    );
    assert_eq!(marker(&sim, &rules, e2), None, "NoHeal draws no marker");
    assert_eq!(
        marker(&sim, &rules, typed).map(|marker| (marker.frame, marker.position)),
        Some((13, (118.0, 178.0))),
        "a Unit authoring Infantry takes the hospital component of the Units pair"
    );
    assert_eq!(
        marker(&sim, &rules, unit_typed).map(|marker| (marker.frame, marker.position)),
        Some((20, (121.0, 175.0))),
        "an Infantry authoring Units keeps the Infantry pair and anchor"
    );
    assert_eq!(
        marker(&sim, &rules, unit_bracket).map(|marker| (marker.frame, marker.position)),
        Some((20, (118.0, 172.0))),
        "the type bracket delta shifts Y"
    );
    assert_eq!(
        marker(&sim, &rules, organic).map(|marker| (marker.frame, marker.position)),
        Some((13, (118.0, 178.0))),
        "an organic Unit takes the hospital component with the Unit anchor"
    );
    assert_eq!(
        marker(&sim, &rules, orca).map(|marker| (marker.frame, marker.position)),
        Some((20, (118.0, 178.0))),
        "an Aircraft takes the units arm and geometry"
    );
    assert_eq!(
        marker(&sim, &rules, building),
        None,
        "the Building arm is a recorded residual"
    );
}

/// `Pips.SelfHeal.{Infantry,Units}` and their `.Offset` pairs
/// (`Ext/Rules/Body.cpp:154-159`): the infantry arm reads the Infantry pair's
/// X, the units arm the Units pair's Y, and the offsets replace the native
/// draw offset before the anchor and bracket-delta shift.
#[test]
fn authored_self_heal_frames_and_offsets_apply() {
    let (mut sim, rules) = scene(
        "[AudioVisual]\nPips.SelfHeal.Units=7,8\nPips.SelfHeal.Units.Offset=40,-30\n\
         Pips.SelfHeal.Infantry=6,9\nPips.SelfHeal.Infantry.Offset=30,-40\n",
    );
    let e1 = spawn(&mut sim, &rules, "E1", 16, 16);
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    let typed = spawn(&mut sim, &rules, "MTNK2", 18, 18);
    let unit_typed = spawn(&mut sim, &rules, "E3", 20, 16);
    grant(&mut sim, 1, 1);

    assert_eq!(
        marker(&sim, &rules, e1).map(|marker| (marker.frame, marker.position)),
        Some((6, (126.0, 170.0))),
        "the Infantry class pair's X with the infantry offset"
    );
    assert_eq!(
        marker(&sim, &rules, mtnk).map(|marker| (marker.frame, marker.position)),
        Some((8, (125.0, 180.0))),
        "the Units class pair's Y with the units offset"
    );
    assert_eq!(
        marker(&sim, &rules, typed).map(|marker| (marker.frame, marker.position)),
        Some((7, (125.0, 180.0))),
        "a Unit authoring Infantry reads the Units pair's X, not the Infantry pair"
    );
    assert_eq!(
        marker(&sim, &rules, unit_typed).map(|marker| (marker.frame, marker.position)),
        Some((9, (126.0, 170.0))),
        "an Infantry authoring Units reads the Infantry pair's Y"
    );
}

/// `GainSelfHealFromAllies` and `GainSelfHealAllowMultiplayPassive`
/// (`Ext/Rules/Body.cpp:111-115`) widen or close the marker's house scan; the
/// damaged pulse divides the session frame by the arm period and selects the
/// darkening material while the remainder is at most 5 and Health is low.
#[test]
fn house_sources_and_the_damage_pulse_gate_the_marker() {
    let (mut sim, rules) = scene("[General]\nGainSelfHealFromAllies=yes\n");
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    assert_eq!(marker(&sim, &rules, mtnk), None, "no house has a source");

    let russians = sim.interner().get("Russians").unwrap();
    grant_at(&mut sim, russians, 0, 1);
    sim.house_alliances_mut_for_test()
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    assert!(
        marker(&sim, &rules, mtnk).is_some(),
        "an allied house's counter feeds the marker"
    );

    // `GainSelfHealFromPlayerControl`: in a campaign, another human or
    // player-controlled house's counter feeds the scan.
    let (mut sim, rules) = scene("[General]\nGainSelfHealFromPlayerControl=yes\n");
    let british = sim.intern("British");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            british, 0, None, true, 0, 10,
        ));
    grant_at(&mut sim, british, 0, 1);
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    assert!(
        marker(&sim, &rules, mtnk).is_some(),
        "a campaign human house's counter feeds the marker"
    );

    let (mut sim, rules) = scene("[General]\nGainSelfHealFromAllies=no\n");
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    let russians = sim.interner().get("Russians").unwrap();
    grant_at(&mut sim, russians, 0, 1);
    sim.house_alliances_mut_for_test()
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    assert_eq!(
        marker(&sim, &rules, mtnk),
        None,
        "GainSelfHealFromAllies=no closes the allied scan"
    );

    let (mut sim, rules) = scene("[General]\nGainSelfHealAllowMultiplayPassive=no\n");
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    grant(&mut sim, 0, 1);
    let owner = sim.interner().get(OWNER).unwrap();
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .set_multiplay_passive_for_test(true);
    assert_eq!(
        marker(&sim, &rules, mtnk),
        None,
        "a passive house draws no marker unless allowed"
    );

    let (mut sim, rules) = scene("");
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    grant(&mut sim, 0, 1);
    sim.session_mut_for_test().set_binary_frame_for_test(0);
    assert_eq!(
        marker(&sim, &rules, mtnk).map(|marker| marker.selector),
        Some(0),
        "a healthy object keeps the ordinary material"
    );
    sim.entities_mut_for_test()
        .get_mut_for_test(mtnk)
        .unwrap()
        .health
        .set_for_test(50);
    assert_eq!(
        marker(&sim, &rules, mtnk).map(|marker| marker.selector),
        Some(1),
        "a damaged object at remainder<=5 takes the darkening material"
    );
}

fn grant_at(
    sim: &mut Simulation,
    owner: crate::sim::intern::InternedId,
    infantry: i32,
    units: i32,
) {
    crate::sim::arena_fixture::install_status_self_heal(
        sim.houses_mut_for_test().get_mut(&owner).expect("house"),
        infantry,
        units,
    );
}

/// The stock build keeps the native frames (13/20) and offsets (23,-22 for a
/// Unit, 15,-25 for an Infantry) through the same marker owner.
#[cfg(not(feature = "phobos"))]
#[test]
fn the_stock_marker_keeps_its_frames_and_offsets() {
    let (mut sim, rules) = scene("");
    let e1 = spawn(&mut sim, &rules, "E1", 16, 16);
    let mtnk = spawn(&mut sim, &rules, "MTNK", 18, 16);
    grant(&mut sim, 1, 1);
    assert_eq!(
        marker(&sim, &rules, e1).map(|marker| (marker.frame, marker.position)),
        Some((13, (115.0, 175.0)))
    );
    assert_eq!(
        marker(&sim, &rules, mtnk).map(|marker| (marker.frame, marker.position)),
        Some((20, (123.0, 178.0)))
    );
}
