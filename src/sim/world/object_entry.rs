//! Shared live cell-entry authority for search, movement and repair +1AC receivers.
//! Infantry and Unit retain numeric accumulators; consumers project only the
//! answers they need. Both classes use Foot4D9C60's height/list prelude.
//! Ordered terminal answers remain significant.
//! Numeric Unit evidence: tools/spatial_oracle/unit_entry.{py,json,meta.json}.
use super::Simulation;
use crate::map::{
    cell_index::NativeCellIdentity as Cell, entities::EntityCategory,
    resolved_terrain::ResolvedTerrainGrid,
};
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::rules::{
    locomotor_type::{LocomotorKind, SpeedType},
    mission_data::MissionType,
    object_type::ObjectType,
};
use crate::sim::combat::combat_weapon;
use crate::sim::combat::veterancy::{has_weapon_ability, rank_from_u16};
use crate::sim::movement::bump_crush::{self, CrushCapability, CrushTarget};
use crate::sim::{components::NavTargetRef, game_entity::GameEntity, intern::InternedId};
use crate::sim::{movement::locomotor::MovementLayer, occupancy::CellObjectMember};

#[cfg(test)]
#[path = "unit_entry_tests.rs"]
mod unit_entry_tests;

#[cfg(test)]
#[path = "infantry_entry_priority_tests.rs"]
mod infantry_entry_priority_tests;

/// The +1AC decision reads live simulation state without borrowing the bridge
/// publisher. Native Map lookups still stamp the canonical shared Dummy through
/// its existing interior-mutable owner, in the same order as movement/repair.
struct EntryReadContext<'a> {
    sim: &'a Simulation,
    rules: &'a RuleSet,
    registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
}

impl EntryReadContext<'_> {
    fn terrain(&self) -> &ResolvedTerrainGrid {
        self.sim
            .resolved_terrain
            .as_ref()
            .expect("live cell-entry terrain")
    }

    fn coord(&self, cell: Cell) -> (i16, i16) {
        self.terrain().native_cell_coord(cell)
    }

    fn flags(&self, cell: Cell) -> u32 {
        self.terrain().native_cell_flags(cell)
    }

    fn level(&self, cell: Cell) -> i8 {
        self.terrain().native_cell_ground_fields(cell).0 as i8
    }
}

fn friendly(live: &EntryReadContext<'_>, a: InternedId, b: InternedId) -> bool {
    crate::map::houses::are_houses_friendly(
        &live.sim.house_alliances,
        live.sim.interner.resolve(a),
        live.sim.interner.resolve(b),
    )
}
/// Infantry5227F0 checks raw disguise first, then resolves the actual center
/// Cell before testing owner alliance, detection and the disguised-as House.
fn infantry_disguised_to(
    live: &EntryReadContext<'_>,
    entity: &GameEntity,
    observer: InternedId,
) -> Result<bool, String> {
    let Some(disguise) = entity
        .disguise
        .as_ref()
        .filter(|state| state.is_disguised())
    else {
        return Ok(false);
    };
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    let cell = live
        .terrain()
        .native_cell_identity(((x / 256) as i16, (y / 256) as i16));
    if friendly(live, entity.owner(), observer) {
        return Ok(false);
    }
    let coord = live.coord(cell);
    let detected =
        live.sim
            .fog
            .detects_disguise_for_house(observer, coord.0 as u16, coord.1 as u16);
    Ok(crate::sim::cloak_disguise::is_disguised_to(
        true,
        false,
        detected,
        disguise
            .house()
            .is_some_and(|fake| friendly(live, observer, fake)),
        disguise.house().is_some(),
    ))
}
fn row_nonzero(live: &EntryReadContext<'_>, cell: Cell, speed: SpeedType) -> Result<bool, String> {
    let Cell::Real(index) = cell else {
        return Ok(false);
    };
    live.terrain().cells()[index]
        .speed_costs
        .cost_for_speed_type(speed)
        .map(|speed| speed != 0)
        .ok_or("repair admission has no resolved speed row".into())
}
fn raw(live: &EntryReadContext<'_>, cell: Cell, layer: MovementLayer) -> (u8, Option<InternedId>) {
    let key = crate::sim::occupancy::RawCellKey::from_native(live.terrain(), cell);
    let grid = &live.sim.substrate.raw_cell_occupation;
    (grid.bits_at(key, layer), grid.owner_at(key, layer))
}
// Unit73F5EF/73F628/73F823 compare actual object pointers, never a Cell
// sharing the blocker's coordinates. Infantry has a distinct Cell shortcut.
fn object_target_is(mover: &GameEntity, blocker: &GameEntity) -> bool {
    matches!(mover.navigation.nav_com, Some(NavTargetRef::Entity{id}|NavTargetRef::Object{id}|NavTargetRef::Building{id}) if id==blocker.stable_id())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfantryTargetAdmission {
    Ordinary,
    DirectTarget,
    SkipCurrent,
}

fn infantry_target_admission(
    live: &EntryReadContext<'_>,
    mover: &GameEntity,
    obj: &ObjectType,
    blocker: &GameEntity,
    selected_cell: Cell,
) -> Result<InfantryTargetAdmission, String> {
    //51C2D3..51C37D: gate effects first. InfantryType EC2=C4 (52453D/
    //825978), EC3=Engineer (524571/82596C). +1D4 is the warp latch.
    if blocker.is_warped_out() {
        return Ok(InfantryTargetAdmission::Ordinary);
    }
    let eligible = match mover.mission.current().known() {
        Some(MissionType::Enter | MissionType::Capture | MissionType::Eaten) => true,
        Some(MissionType::Sabotage) => obj.c4,
        Some(MissionType::AreaGuard | MissionType::Patrol | MissionType::Guard) => obj.engineer,
        _ => false,
    };
    if !eligible {
        return Ok(InfantryTargetAdmission::Ordinary);
    }
    if object_target_is(mover, blocker) {
        return Ok(InfantryTargetAdmission::DirectTarget);
    }
    //51C3B1 executes even with NULL or a nonmatching object NavCom. The
    //retained Cell NavCom resolves by identity without another lookup/stamp.
    let current = crate::sim::movement::ground_pose::position_world_coord(&blocker.position);
    let blocker_cell = live
        .terrain()
        .native_cell_identity(((current.x / 256) as i16, (current.y / 256) as i16));
    let cell_target_is = |cell| {
        if let Some(NavTargetRef::Cell { rx, ry }) = mover.navigation.nav_com {
            live.terrain()
                .native_fixed_cell_index(rx as i16, ry as i16)
                .map(Cell::Real)
                .unwrap_or(Cell::Dummy)
                == cell
        } else {
            false
        }
    };
    let attack_target_is = |id| {
        mover.attack_target.as_ref().is_some_and(
            |a| matches!(a.target, crate::sim::combat::TargetKind::Entity(target) if target == id),
        )
    };
    if cell_target_is(blocker_cell) || attack_target_is(blocker.stable_id()) {
        return Ok(InfantryTargetAdmission::DirectTarget);
    }

    //51C3CE queries the original selected Cell's GROUND first Building only
    //after all direct-target checks fail. These matches jump to51C70F and
    //continue the list, unlike the direct target's terminal51C71B path.
    let Cell::Real(index) = selected_cell else {
        return Ok(InfantryTargetAdmission::Ordinary);
    };
    let selected = &live.terrain().cells()[index];
    let Some(first_id) = live.sim.substrate.occupancy.first_building_on_layer(
        selected.rx,
        selected.ry,
        MovementLayer::Ground,
    ) else {
        return Ok(InfantryTargetAdmission::Ordinary);
    };
    if first_id == blocker.stable_id() {
        return Ok(InfantryTargetAdmission::Ordinary);
    }
    let first = live
        .sim
        .substrate
        .entities
        .get(first_id)
        .ok_or("repair first-Building target has retired entity")?;
    if object_target_is(mover, first) {
        return Ok(InfantryTargetAdmission::SkipCurrent);
    }
    //51C3F6 uses Building+48 (foundation center), not the approach+4C.
    //565730 runs even for NULL/nonmatching object NavCom, before +2B4.
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(first);
    let first_cell = live
        .terrain()
        .native_cell_identity(((x / 256) as i16, (y / 256) as i16));
    if cell_target_is(first_cell) || attack_target_is(first_id) {
        return Ok(InfantryTargetAdmission::SkipCurrent);
    }
    Ok(InfantryTargetAdmission::Ordinary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impassable(
        live: &EntryReadContext<'_>,
        object: CellObjectMember,
        cell: Cell,
    ) -> Result<bool, String> {
        classify_entry(
            live,
            object,
            cell,
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
        )
        .map(|code| code == 7)
    }

    #[test]
    fn aircraft_entry_answers_as_the_owner_after_the_team_arm() {
        for kind in [
            LocomotorKind::Fly,
            LocomotorKind::Jumpjet,
            LocomotorKind::Rocket,
            LocomotorKind::Teleport,
        ] {
            let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture();
            let id = sim
                .spawn_object("HORNET", "Americans", 17, 15, 0, &rules)
                .unwrap();
            let mut entity = sim.substrate.entities.get(id).unwrap().clone();
            let mut object = rules.object("HORNET").unwrap().clone();
            object.locomotor = kind;
            entity.locomotor =
                Some(crate::sim::movement::locomotor::LocomotorState::from_object_type(&object, 0));
            // The answer is the one Aircraft owner's; canonical lookups
            // reproduce the shared-dummy stamps, so a dummy cell is readable.
            let probe = |sim: &mut Simulation| {
                aircraft_entry(
                    &EntryReadContext {
                        sim,
                        rules: &rules,
                        registry: None,
                    },
                    &entity,
                    Cell::Dummy,
                )
            };
            sim.session.game_mode_nonzero = false;
            assert_eq!(
                probe(&mut sim),
                Ok(sim.aircraft_can_enter_cell(id, Cell::Dummy))
            );
            sim.session.game_mode_nonzero = true;
            assert_eq!(probe(&mut sim), Ok(0));
            let script_id = sim.intern("REPAIR_WAYPOINT_EFFECT");
            sim.team_script_vm
                .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
                    id: script_id,
                    actions: vec![crate::sim::team_script_vm::TeamScriptAction {
                        action_id: 3,
                        argument: 0,
                    }],
                    source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                });
            sim.team_script_vm.create_team(
                entity.owner(),
                script_id,
                vec![id],
                sim.session.binary_frame as i32,
            );
            assert!(
                probe(&mut sim).unwrap_err().contains("action3"),
                "mode cannot bypass the earlier Team receiver"
            );
        }
    }

    #[test]
    fn aircraft_quotient_requires_one_of_the_four_proved_interfaces() {
        let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture();
        sim.session.game_mode_nonzero = true;
        for kind in [
            None,
            Some(LocomotorKind::Walk),
            Some(LocomotorKind::Drive),
            Some(LocomotorKind::Ship),
            Some(LocomotorKind::Hover),
        ] {
            let mut entity = GameEntity::test_default(90, "HORNET", "Americans", 17, 15);
            entity.locomotor = kind.map(|kind| {
                let mut object = rules.object("HORNET").unwrap().clone();
                object.locomotor = kind;
                crate::sim::movement::locomotor::LocomotorState::from_object_type(&object, 0)
            });
            let result = aircraft_entry(
                &EntryReadContext {
                    sim: &sim,
                    rules: &rules,
                    registry: None,
                },
                &entity,
                Cell::Dummy,
            );
            assert!(
                result.unwrap_err().contains("constant-false AtCoord"),
                "{kind:?}"
            );
        }
    }

    fn unit_fixture(flags: &str, blocker_category: EntityCategory) -> (Simulation, RuleSet, Cell) {
        let (mut sim, _, _) = crate::sim::world::entry_test_fixture::fixture();
        let rules=RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MOVER\n1=BLOCKER\n[MOVER]\nSpeedType=Track\n[BuildingTypes]\n0=BUILDING\n1=HIDDEN\n[HIDDEN]\nInvisibleInGame=yes\n[BLOCKER]\nSpeedType=Track\n[BUILDING]\nFoundation=1x1\n{flags}\n"
        ))).unwrap();
        for (id, name, category, cell) in [
            (90, "MOVER", EntityCategory::Unit, (15, 15)),
            (
                91,
                if blocker_category == EntityCategory::Structure {
                    "BUILDING"
                } else {
                    "BLOCKER"
                },
                blocker_category,
                (16, 15),
            ),
        ] {
            let mut e = GameEntity::test_default(id, name, "Americans", cell.0, cell.1);
            e.owner = sim.intern("Americans");
            e.type_ref = sim.intern(name);
            e.category = category;
            sim.substrate.entities.insert(e);
        }
        sim.substrate.occupancy.add(
            16,
            15,
            91,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::from_category(blocker_category),
        );
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 15));
        (sim, rules, cell)
    }

    #[test]
    fn infantry_usable_area_admission_is_independent_of_game_mode() {
        // Original51BF90 has no A8B238 read. Executed counter0/mode0/1
        // controls both return7 for this otherwise-clear unusable rim.
        let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture();
        let mut mover = GameEntity::test_default(90, "ENGINEER", "Americans", 15, 15);
        mover.owner = sim.intern("Americans");
        mover.type_ref = sim.intern("ENGINEER");
        mover.category = EntityCategory::Infantry;
        mover.in_playfield = true;
        sim.substrate.entities.insert(mover);
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 1, 1),
        );
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 15));
        for mode in [false, true] {
            sim.session.game_mode_nonzero = mode;
            assert_eq!(
                sim.foot_entry_receiver(90, &rules, None)
                    .unwrap()
                    .can_enter(
                        cell,
                        crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
                    ),
                Ok(7),
                "mode {mode} cannot bypass Infantry51C13A..51C161",
            );
        }
    }

    #[test]
    fn nested_escape_priority_restores_the_callers_cell_admission() {
        let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture();
        let mut mover = GameEntity::test_default(90, "ENGINEER", "Americans", 15, 15);
        mover.owner = sim.intern("Americans");
        mover.type_ref = sim.intern("ENGINEER");
        mover.category = EntityCategory::Infantry;
        mover.in_playfield = true;
        sim.substrate.entities.insert(mover);
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 1, 1),
        );
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 15));
        let probe = |sim: &Simulation| {
            sim.foot_entry_receiver(90, &rules, None)
                .unwrap()
                .can_enter(
                    cell,
                    crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
                )
        };
        let rng_before = [
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        ];
        assert_eq!(probe(&sim), Ok(7));
        sim.with_object_placement_scope(|sim| {
            assert_eq!(probe(sim), Ok(0));
            sim.with_object_placement_scope(|sim| assert_eq!(probe(sim), Ok(0)));
            assert_eq!(probe(sim), Ok(0), "nested return retains the outer bracket");
        });
        assert_eq!(
            probe(&sim),
            Ok(7),
            "outer return restores ordinary admission"
        );
        assert_eq!(
            [
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            ],
            rng_before,
        );
    }

    fn unit_probe(
        sim: &mut Simulation,
        rules: &RuleSet,
        cell: Cell,
        mission: MissionType,
        nav: Option<NavTargetRef>,
    ) -> bool {
        sim.mission_assign_exact(
            90,
            crate::sim::mission::MissionId::from_known(mission),
            sim.session.binary_frame,
        )
        .unwrap();
        sim.substrate
            .entities
            .get_mut(90)
            .unwrap()
            .navigation
            .nav_com = nav;
        impassable(
            &EntryReadContext {
                sim,
                rules,
                registry: None,
            },
            CellObjectMember::Entity(90),
            cell,
        )
        .unwrap()
    }

    #[test]
    fn unit_absorb_and_grinding_keep_distinct_missions_and_object_targets() {
        let target = Some(NavTargetRef::Building { id: 91 });
        let (mut sim, rules, cell) = unit_fixture("UnitAbsorb=yes", EntityCategory::Structure);
        assert!(!unit_probe(
            &mut sim,
            &rules,
            cell,
            MissionType::Enter,
            target
        ));
        assert!(unit_probe(
            &mut sim,
            &rules,
            cell,
            MissionType::Eaten,
            target
        ));
        assert!(
            unit_probe(
                &mut sim,
                &rules,
                cell,
                MissionType::Enter,
                Some(NavTargetRef::Cell { rx: 16, ry: 15 })
            ),
            "a Cell is not the Unit's object NavCom"
        );
        let (mut sim, rules, cell) = unit_fixture("Grinding=yes", EntityCategory::Structure);
        assert!(!unit_probe(
            &mut sim,
            &rules,
            cell,
            MissionType::Eaten,
            target
        ));
        assert!(unit_probe(
            &mut sim,
            &rules,
            cell,
            MissionType::Enter,
            target
        ));
        // Supplied list topology isolates the later current-ground check.
        // It does not claim a stock grinder/repair footprint scene.
        sim.substrate.occupancy.add(
            15,
            15,
            91,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        assert!(
            !unit_probe(&mut sim, &rules, cell, MissionType::Move, None),
            "current-ground Grinding arm has no mission/NavCom gate"
        );
    }

    #[test]
    fn exact_enter_unit_target_returns_before_a_later_hard_building() {
        let (mut sim, rules, cell) = unit_fixture("", EntityCategory::Unit);
        let mut b = GameEntity::test_default_of_category(
            92,
            "BUILDING",
            "Americans",
            16,
            15,
            EntityCategory::Structure,
        );
        b.owner = sim.intern("Americans");
        b.type_ref = sim.intern("BUILDING");
        sim.substrate.entities.insert(b);
        sim.substrate.occupancy.add(
            16,
            15,
            92,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        assert!(!unit_probe(
            &mut sim,
            &rules,
            cell,
            MissionType::Enter,
            Some(NavTargetRef::Entity { id: 91 })
        ));
        assert!(unit_probe(
            &mut sim,
            &rules,
            cell,
            MissionType::Enter,
            Some(NavTargetRef::Cell { rx: 16, ry: 15 })
        ));
    }

    #[test]
    fn contacted_building_skips_when_a_different_building_is_first() {
        let (mut sim, rules, cell) =
            unit_fixture("NumberImpassableRows=-1", EntityCategory::Structure);
        let mut b = GameEntity::test_default_of_category(
            92,
            "HIDDEN",
            "Americans",
            16,
            15,
            EntityCategory::Structure,
        );
        assert!(rules.object("HIDDEN").unwrap().invisible_in_game);
        b.owner = sim.intern("Americans");
        b.type_ref = sim.intern("HIDDEN");
        sim.substrate.entities.insert(b);
        sim.substrate.occupancy.remove(16, 15, 91);
        for id in [92, 91] {
            sim.substrate.occupancy.add(
                16,
                15,
                id,
                MovementLayer::Ground,
                None,
                crate::sim::occupancy::CellListInsertion::AppendBuilding,
            );
        }
        assert!(unit_probe(&mut sim, &rules, cell, MissionType::Move, None));
        sim.substrate
            .entities
            .get_mut(90)
            .unwrap()
            .mark_live_contact_with(91);
        assert!(
            !unit_probe(&mut sim, &rules, cell, MissionType::Move, None),
            "radio false row receiver skips only its checked Building"
        );
    }

    fn infantry_fallback_fixture() -> (Simulation, RuleSet, Cell) {
        let (mut sim, _, _) = crate::sim::world::entry_test_fixture::fixture();
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=MOVER\n1=BLOCKER\n[BuildingTypes]\n0=TARGET\n[MOVER]\nEngineer=yes\nSpeedType=Foot\n[BLOCKER]\nSpeedType=Foot\n[TARGET]\nFoundation=2x2\n",
        )).unwrap();
        // Supplied overlap isolates the literal ordered +1AC receiver; this
        // is not a retail footprint/construction or complete capture scene.
        for (id, name, owner, category, p) in [
            (70, "MOVER", "Americans", EntityCategory::Infantry, (15, 15)),
            (
                71,
                "TARGET",
                "Americans",
                EntityCategory::Structure,
                (16, 15),
            ),
            (
                72,
                "BLOCKER",
                "Russians",
                EntityCategory::Infantry,
                (16, 15),
            ),
        ] {
            let mut entity = GameEntity::test_default(id, name, owner, p.0, p.1);
            entity.owner = sim.intern(owner);
            entity.type_ref = sim.intern(name);
            entity.category = category;
            if category == EntityCategory::Structure {
                // Construction stamps the type's `Foundation=2x2`.
                entity.foundation = "2x2".to_string();
            }
            entity.position.sub_x = crate::util::fixed_math::SimFixed::from_num(128);
            entity.position.sub_y = crate::util::fixed_math::SimFixed::from_num(128);
            sim.substrate.entities.insert(entity);
        }
        for (id, insertion) in [
            (71, crate::sim::occupancy::CellListInsertion::AppendBuilding),
            (
                72,
                crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
            ),
        ] {
            sim.substrate
                .occupancy
                .add(16, 15, id, MovementLayer::Ground, None, insertion);
        }
        sim.mission_assign_exact(
            70,
            crate::sim::mission::MissionId::from_known(MissionType::Capture),
            sim.session.binary_frame,
        )
        .unwrap();
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 15));
        (sim, rules, cell)
    }

    #[test]
    fn infantry_first_building_skip_preserves_a_later_target_refusal() {
        let (mut sim, rules, cell) = infantry_fallback_fixture();
        let probe = |sim: &mut Simulation| {
            impassable(
                &EntryReadContext {
                    sim,
                    rules: &rules,
                    registry: None,
                },
                CellObjectMember::Entity(70),
                cell,
            )
            .unwrap()
        };
        assert!(
            probe(&mut sim),
            "unarmed mover cannot pass the ordinary enemy blocker"
        );
        sim.substrate
            .entities
            .get_mut(70)
            .unwrap()
            .navigation
            .nav_com = Some(NavTargetRef::Building { id: 71 });
        assert!(
            !probe(&mut sim),
            "first-Building target skips the earlier enemy blocker"
        );
        let frame = sim.session.binary_frame;
        crate::sim::superweapon::invulnerability::apply_invulnerability(
            sim.substrate.entities.get_mut(71).unwrap(),
            frame,
            30,
            crate::sim::superweapon::invulnerability::InvulnKind::ForceShield,
        );
        assert!(
            probe(&mut sim),
            "skip-current must continue into the protected target's refusal"
        );
    }

    #[test]
    fn infantry_first_building_lookup_uses_center_then_attack_identity() {
        let (mut sim, rules, cell) = infantry_fallback_fixture();
        let probe = |sim: &mut Simulation, blocker| {
            let live = EntryReadContext {
                sim,
                rules: &rules,
                registry: None,
            };
            infantry_target_admission(
                &live,
                live.sim.substrate.entities.get(70).unwrap(),
                rules.object("MOVER").unwrap(),
                live.sim.substrate.entities.get(blocker).unwrap(),
                cell,
            )
            .unwrap()
        };
        sim.substrate
            .entities
            .get_mut(70)
            .unwrap()
            .navigation
            .nav_com = Some(NavTargetRef::Cell { rx: 17, ry: 16 });
        assert_eq!(
            probe(&mut sim, 72),
            InfantryTargetAdmission::SkipCurrent,
            "first Building+48 center differs from its raw anchor"
        );
        assert_eq!(
            probe(&mut sim, 71),
            InfantryTargetAdmission::Ordinary,
            "the first Building cannot skip itself via its own center"
        );
        sim.substrate
            .entities
            .get_mut(70)
            .unwrap()
            .navigation
            .nav_com = None;
        sim.substrate.entities.get_mut(70).unwrap().attack_target =
            Some(crate::sim::combat::AttackTarget {
                target: crate::sim::combat::TargetKind::Entity(71),
            });
        // Raw blocker lookup stamps(60,60), then first Building+48 stamps
        //(62,62). Attack-target identity is deliberately evaluated last.
        for (id, p) in [(72, (60, 60)), (71, (61, 61))] {
            let position = &mut sim.substrate.entities.get_mut(id).unwrap().position;
            position.rx = p.0;
            position.ry = p.1;
        }
        assert_eq!(probe(&mut sim, 72), InfantryTargetAdmission::SkipCurrent);
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (62, 62)
        );
    }

    #[test]
    fn infantry_target_lookup_obeys_gates_and_retains_cell_identity() {
        let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
        let hut = sim
            .spawn_object("CABHUT", "Americans", 16, 15, 0, &rules)
            .unwrap();
        let infantry = sim
            .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules)
            .unwrap();
        // Supplied query coordinates isolate the effectful51C3B1 boundary.
        sim.substrate.entities.get_mut(hut).unwrap().position.rx = 60;
        sim.substrate.entities.get_mut(hut).unwrap().position.ry = 60;
        let probe = |sim: &mut Simulation| {
            let live = EntryReadContext {
                sim,
                rules: &rules,
                registry: Some(&registry),
            };
            infantry_target_admission(
                &live,
                live.sim.substrate.entities.get(infantry).unwrap(),
                rules.object("ENGINEER").unwrap(),
                live.sim.substrate.entities.get(hut).unwrap(),
                live.terrain().native_cell_identity((16, 15)),
            )
            .unwrap()
                == InfantryTargetAdmission::DirectTarget
        };
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .stamp_dummy_cell_requested_coord(7, 8);
        sim.mission_assign_exact(
            infantry,
            crate::sim::mission::MissionId::from_known(MissionType::Move),
            sim.session.binary_frame,
        )
        .unwrap();
        assert!(!probe(&mut sim));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (7, 8)
        );
        sim.mission_assign_exact(
            infantry,
            crate::sim::mission::MissionId::from_known(MissionType::Capture),
            sim.session.binary_frame,
        )
        .unwrap();
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .navigation
            .nav_com = Some(NavTargetRef::Building { id: hut });
        assert!(probe(&mut sim));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (7, 8),
            "exact object match precedes map lookup"
        );
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .navigation
            .nav_com = Some(NavTargetRef::Cell { rx: 61, ry: 61 });
        assert!(
            probe(&mut sim),
            "two absent coordinates denote the same retained native Dummy Cell"
        );
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (60, 60)
        );
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .navigation
            .nav_com = None;
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .attack_target = Some(crate::sim::combat::AttackTarget {
            target: crate::sim::combat::TargetKind::Entity(hut),
        });
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .stamp_dummy_cell_requested_coord(7, 8);
        assert!(probe(&mut sim));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (60, 60),
            "attack target is checked after the map lookup"
        );
    }

    #[test]
    fn slave_deposit_skip_keeps_later_building_refusal_and_raw_history() {
        let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
        let hut = sim
            .spawn_object("CABHUT", "Americans", 16, 15, 0, &rules)
            .unwrap();
        let slave = sim
            .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules)
            .unwrap();
        sim.substrate.entities.get_mut(slave).unwrap().slave =
            crate::sim::slave_manager::SlaveLink::for_test(Some(hut), Vec::new());
        let slav = sim.intern("SLAV");
        let manager = |slaves: &[u64]| {
            Some(crate::sim::slave_manager::SlaveManager::new(
                slav,
                slaves.iter().copied().map(Some),
                0,
                0,
                0,
            ))
        };
        sim.substrate.entities.get_mut(hut).unwrap().slave_manager = manager(&[slave]);
        sim.substrate.raw_cell_occupation.mark_ground(16, 15, 0x20);
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 15));
        let probe = |sim: &mut Simulation| {
            let live = EntryReadContext {
                sim,
                rules: &rules,
                registry: Some(&registry),
            };
            impassable(&live, CellObjectMember::Entity(slave), cell).unwrap()
        };
        assert!(
            !probe(&mut sim),
            "master membership admits this deposit cell"
        );
        assert_ne!(
            sim.substrate.raw_cell_occupation.ground_bits(16, 15) & 0x20,
            0,
            "receiver clears only a local latch"
        );
        sim.substrate.entities.get_mut(hut).unwrap().slave_manager = manager(&[]);
        assert!(probe(&mut sim), "empty manager cannot skip the blocker");
        sim.substrate.entities.get_mut(hut).unwrap().slave_manager = manager(&[slave]);
        // Supplied overlapping Building list tests native continuation, not
        // ordinary construction legality or a retail repair footprint scene.
        let mut later = GameEntity::test_default_of_category(
            100,
            "CABHUT",
            "Americans",
            16,
            15,
            EntityCategory::Structure,
        );
        later.owner = sim.intern("Americans");
        later.type_ref = sim.intern("CABHUT");
        sim.substrate.entities.insert(later);
        sim.substrate.occupancy.add(
            16,
            15,
            100,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        assert!(
            probe(&mut sim),
            "true6B0880 continues into the later hard blocker"
        );
    }
}

fn moving(e: &GameEntity) -> bool {
    crate::sim::movement::motion_query::is_moving(e).unwrap_or(e.movement_target.is_some())
}

fn head_on(mover: &GameEntity, blocker: &GameEntity, frame: u32) -> bool {
    use crate::util::direction_tables::{dir_from_facing16, facing16_from_delta};
    let facing = |e: &GameEntity| e.body_facing_current(frame);
    let direction = dir_from_facing16(facing(mover));
    if direction != dir_from_facing16(facing(blocker).wrapping_add(0x7fff)) {
        return false;
    }
    let a = crate::sim::movement::ground_pose::position_world_coord(&mover.position);
    let b = crate::sim::movement::ground_pose::position_world_coord(&blocker.position);
    //73F976 uses the same native atan polynomial/constants as FacingFromDelta.
    direction
        == dir_from_facing16(facing16_from_delta(
            b.x.wrapping_sub(a.x),
            b.y.wrapping_sub(a.y),
        ))
        && crate::util::native_x87::distance_3d_leptons([a.x, a.y, a.z], [b.x, b.y, b.z]) <= 511
}

impl Simulation {
    /// Actual Infantry+1AC. The returned quotient preserves the distinct
    /// consumers in Foot4D3920, Infantry51DAF0 and the repair receiver487A10.
    pub(crate) fn infantry_can_enter(
        &self,
        id: u64,
        cell: Cell,
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<crate::sim::movement::infantry_entry::InfantryEntryClass, String> {
        if self
            .substrate
            .entities
            .get(id)
            .is_none_or(|e| e.category != EntityCategory::Infantry)
        {
            return Err("Infantry entry requires a live Infantry receiver".into());
        }
        self.foot_can_enter(id, cell, args, rules, registry)
            .map(crate::sim::movement::infantry_entry::InfantryEntryClass::from_raw)
    }

    /// The class's own `Can_Enter_Cell` (+0x1AC) for a live Foot receiver:
    /// Infantry 0x0051BF90 or Unit 0x0073F0A0, as the raw native code 0..7.
    /// Foot4D3920's target query and the Drive/Ship Process continuations
    /// (0x4B2B17, 0x4B2FF9 and the Ship twins) consume the code directly.
    /// Native comparisons: tools/spatial_oracle/unit_entry* corpora.
    /// Search and movement share this fallible read context. Canonical Map
    /// queries can still stamp the shared off-map Dummy; they are not cloned.
    pub(crate) fn foot_can_enter(
        &self,
        id: u64,
        cell: Cell,
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<u8, String> {
        if self
            .substrate
            .entities
            .get(id)
            .is_none_or(|e| !matches!(e.category, EntityCategory::Infantry | EntityCategory::Unit))
        {
            return Err("Foot entry requires a live Infantry or Unit receiver".into());
        }
        // An installed oracle seam answers before the receiver's type is
        // resolved, as it did before the search shared this receiver.
        #[cfg(test)]
        if let Some(terrain) = self.resolved_terrain.as_ref()
            && let Some(code) = crate::sim::movement::fresh_oracle_seam::supplied_can_enter(
                terrain.native_cell_coord(cell),
                args.direction,
                args.height,
            )
        {
            return Ok(code);
        }
        self.foot_entry_receiver(id, rules, registry)?
            .can_enter(cell, args)
    }

    /// `AircraftClass::Can_Enter_Cell` (`0x004196B0`) for a live aircraft,
    /// as its native code: 0 admits, 7 refuses. Fly landing (`0x004CE840`)
    /// consumes it, and so does [`Self::mover_can_enter`]'s Aircraft arm.
    /// The body reads only its cell argument (`0x004196B5`; it returns with
    /// `RET 0x14`).
    /// - The occupant arm tests Winged passability (SpeedType 4 at
    ///   `0x0041974D`), which always succeeds, so it never refuses.
    /// - In game mode 0, an aircraft owned by the current house that is not
    ///   mission-only (`+0x41A`, `+0x3D4`) refuses a cell whose ground is
    ///   shrouded: `MapClass::IsShrouded` (`0x00586360`, the one port
    ///   `vision::coordinate_is_shrouded`) at the cell's centre
    ///   (`0x00419764..0x004197A7`). A Cell is open when the owner has mapped
    ///   its ground (not current sight); the shared dummy never is.
    ///
    /// Native executions: tools/spatial_oracle/fly_can_enter.{py,json,meta.json}.
    /// RESIDUAL: the Team arm (`0x004196BC..0x004196DE`) is not ported. A team
    /// member whose script step answers `0x006EC300` admits a cell outside the
    /// playfield before the shroud test. Only a player-owned team aircraft
    /// reaches that test, so the gap is a campaign's scripted aircraft sent or
    /// landing outside the playfield.
    pub(crate) fn aircraft_can_enter(&self, id: u64, cell: (i16, i16)) -> u8 {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return 0;
        };
        let cell = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(cell);
        self.aircraft_can_enter_cell(id, cell)
    }

    /// [`Self::aircraft_can_enter`] on a cell its caller has already looked
    /// up (the native argument is the `CellClass`).
    pub(crate) fn aircraft_can_enter_cell(&self, id: u64, cell: Cell) -> u8 {
        let e = self.substrate.entities.get(id).unwrap();
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return 0;
        };
        if self.session.game_mode_nonzero
            || !e.discovery.owned_by_current_house
            || e.is_mission_only()
        {
            return 0;
        }
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let coord = terrain.native_cell_coord(cell);
        let shrouded = crate::sim::vision::cell_is_shrouded(
            &self.fog,
            &cells,
            e.owner(),
            (coord.0 as u16, coord.1 as u16),
        );
        if shrouded { 7 } else { 0 }
    }

    /// A mover's own `Can_Enter_Cell` (vtable `+0x1AC`) at a cell, as its
    /// native code: [`Self::aircraft_can_enter`] for an Aircraft, which reads
    /// no argument but the cell, otherwise [`Self::foot_can_enter`] with
    /// `args` on the cell's canonical lookup.
    ///
    /// `mode` retains the fifth argument: GetDock44EFB0 uses1 for its
    /// preferred/scan cells and0 for offered/table cells; Object5F4F3C's
    /// ordinary Unlimbo gate uses0. Foot4D9C10 forwards1 to the installed
    /// locomotor's +1C, whose eight active retail receivers are all the
    /// side-effect-free constant-zero55ABF0 leaf. The class owner already
    /// seeds its result0, so both modes have identical numeric behavior.
    /// Native48-call proof: tools/spatial_oracle/foot_locomotor_entry.*;
    /// bounded factory/depot modes: basic-factory-exit-geometry-research.
    ///
    /// Callers: the group spread (`0x0064D52F`), the team centre
    /// (`0x006EAEE0`) and Jumpjet State 4 (`0x0054C66D`). Each decides what
    /// an answer VERA cannot read (`Err`) means for it; native has no such
    /// state.
    pub(crate) fn mover_can_enter(
        &self,
        id: u64,
        cell: (i16, i16),
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
        _mode: crate::sim::movement::infantry_entry::EntryQueryMode,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<u8, String> {
        let mover = self
            .substrate
            .entities
            .get(id)
            .ok_or("Can_Enter_Cell of a retired mover")?;
        if mover.category == EntityCategory::Aircraft {
            return Ok(self.aircraft_can_enter(id, cell));
        }
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Foot entry requires map cells")?;
        let cell = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(cell);
        self.foot_can_enter(id, cell, args, rules, registry)
    }
}

/// Repair's object-list controller uses the same numeric decision owner as
/// Foot search/movement, including non-Foot residents' class-specific receivers.
pub(super) fn classify_object(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    object: CellObjectMember,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Result<u8, String> {
    classify_entry(
        &EntryReadContext {
            sim,
            rules,
            registry,
        },
        object,
        cell,
        args,
    )
}

fn classify_entry(
    live: &EntryReadContext<'_>,
    object: CellObjectMember,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Result<u8, String> {
    let CellObjectMember::Entity(id) = object else {
        return terrain_impassable(live, object, cell).map(|hard| if hard { 7 } else { 0 });
    };
    let e = live
        .sim
        .substrate
        .entities
        .get(id)
        .ok_or("repair admission retired entity")?;
    let obj = live
        .rules
        .object(live.sim.interner.resolve(e.type_ref()))
        .ok_or("repair admission missing ObjectType")?;
    if e.category == EntityCategory::Structure {
        return building_impassable(live, e, obj, cell).map(|hard| if hard { 7 } else { 0 });
    }
    if e.category == EntityCategory::Aircraft {
        return aircraft_entry(live, e, cell);
    }
    classify_foot_entry(live, &EntryMover::new(live, id, e, obj), cell, args)
}

/// The receiver's own facts one Foot +1AC call reads: its type, weapon slot 0
/// and crusher state. Nothing in them changes while a Find_Path search runs
/// (Mark0 has already lifted the mover and the search writes no object), so
/// the search resolves them once instead of per neighbour.
struct EntryMover<'a> {
    id: u64,
    e: &'a GameEntity,
    obj: &'a ObjectType,
    weapon0: Option<&'a crate::rules::weapon_type::WeaponType>,
    crusher: bool,
    /// Answers of `friendly(mover owner, house)` already asked, and blocker
    /// types already resolved by name; the alliance graph and rules are fixed
    /// for the receiver's lifetime.
    allies: std::cell::RefCell<Vec<(InternedId, bool)>>,
    types: std::cell::RefCell<Vec<(InternedId, &'a ObjectType)>>,
}

impl<'a> EntryMover<'a> {
    fn new(live: &EntryReadContext<'a>, id: u64, e: &'a GameEntity, obj: &'a ObjectType) -> Self {
        let weapon0 = combat_weapon::weapon_for_index(obj, e.veterancy(), 0)
            .and_then(|(name, _)| live.rules.weapon(name));
        let crusher =
            obj.crusher || has_weapon_ability(rank_from_u16(e.veterancy()), obj, Ability::Crusher);
        Self {
            id,
            e,
            obj,
            weapon0,
            crusher,
            allies: std::cell::RefCell::default(),
            types: std::cell::RefCell::default(),
        }
    }

    fn object_type(&self, live: &EntryReadContext<'a>, id: InternedId) -> Option<&'a ObjectType> {
        if let Some(&(_, kind)) = self.types.borrow().iter().find(|(t, _)| *t == id) {
            return Some(kind);
        }
        let kind = live.rules.object(live.sim.interner.resolve(id))?;
        self.types.borrow_mut().push((id, kind));
        Some(kind)
    }

    fn friendly(&self, live: &EntryReadContext<'_>, house: InternedId) -> bool {
        if let Some(&(_, answer)) = self.allies.borrow().iter().find(|(h, _)| *h == house) {
            return answer;
        }
        let answer = friendly(live, self.e.owner(), house);
        self.allies.borrow_mut().push((house, answer));
        answer
    }
}

/// One live Foot receiver's +1AC with its own facts resolved once. Find_Path's
/// search (`0x00429F54`) asks it for every neighbour; a single query goes
/// through `Simulation::foot_can_enter`, which builds one per call.
pub(crate) struct FootEntryReceiver<'a> {
    live: EntryReadContext<'a>,
    mover: EntryMover<'a>,
}

impl FootEntryReceiver<'_> {
    /// Shared56BCD0 read for this Foot's House. The map lookup and retained
    /// grid remain with their owners; path consumers carry no second census.
    pub(crate) fn house_threat(&self, coord: (i16, i16)) -> Result<i32, String> {
        self.live
            .sim
            .house_threat_at_cell(self.mover.e.owner(), coord)
    }
    pub(crate) fn hierarchy_house_threat(&self, index: i32) -> Result<i32, String> {
        self.live
            .sim
            .houses
            .get(&self.mover.e.owner())
            .ok_or("hierarchy threat requires a live House")?
            .spatial_threat_at_padded_index(index)
    }

    pub(crate) fn can_enter(
        &self,
        cell: Cell,
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
    ) -> Result<u8, String> {
        #[cfg(test)]
        if let Some(code) = crate::sim::movement::fresh_oracle_seam::supplied_can_enter(
            self.live.coord(cell),
            args.direction,
            args.height,
        ) {
            return Ok(code);
        }
        classify_foot_entry(&self.live, &self.mover, cell, args)
    }
}

impl Simulation {
    /// The live Infantry or Unit receiver `id`, for repeated +1AC queries
    /// against one unchanged world (a Find_Path search).
    pub(crate) fn foot_entry_receiver<'a>(
        &'a self,
        id: u64,
        rules: &'a RuleSet,
        registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<FootEntryReceiver<'a>, String> {
        if self.resolved_terrain.is_none() {
            return Err("Foot entry requires map cells".into());
        }
        let live = EntryReadContext {
            sim: self,
            rules,
            registry,
        };
        let e = self
            .substrate
            .entities
            .get(id)
            .filter(|e| matches!(e.category, EntityCategory::Infantry | EntityCategory::Unit))
            .ok_or("Foot entry requires a live Infantry or Unit receiver")?;
        let obj = rules
            .object(self.interner.resolve(e.type_ref()))
            .ok_or("repair admission missing ObjectType")?;
        let mover = EntryMover::new(&live, id, e, obj);
        Ok(FootEntryReceiver { live, mover })
    }
}

/// Infantry 0x0051BF90 / Unit 0x0073F0A0 for a resolved Foot receiver.
fn classify_foot_entry<'a>(
    live: &EntryReadContext<'a>,
    mover: &EntryMover<'a>,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Result<u8, String> {
    let &EntryMover {
        id,
        e,
        obj,
        weapon0,
        crusher,
        ..
    } = mover;
    let infantry = e.category == EntityCategory::Infantry;
    let (mut bits, mut owner) = raw(live, cell, MovementLayer::Ground);
    let initial_level = live.level(cell);
    let mut vehicle_occupied = bits & 0x20 != 0;
    if !infantry && let Some(required) = obj.movement_restricted_to {
        let Cell::Real(index) = cell else {
            return Ok(7);
        };
        let c = &live.terrain().cells()[index];
        if c.yr_cell_land_type == 10 {
            //73F12E..73F1D9: passing the Tube shape gate bypasses only
            //required-land equality; later list/speed/raw predicates still run.
            let required_subtile =
                match live.terrain().current_tile_dimensions(c.final_tile_index)? {
                    (5 | 4, 3) => Some(2),
                    (3, 4 | 5) => Some(6),
                    _ => None,
                };
            if required_subtile.is_some_and(|sub| c.final_sub_tile != sub) {
                return Ok(7);
            }
        } else if c.yr_cell_land_type != required.as_index()
            && !(matches!(c.bridge_facts.overlay_id, Some(237 | 238))
                && args.height != i32::from(initial_level))
        {
            return Ok(7);
        }
    }
    let layer = {
        use crate::sim::movement::infantry_entry::{adjust_height_and_list, backstep_cell};
        let mut height = args.height;
        let mut list_bridge = live.flags(cell) & 0x100 != 0
            && (height == -1 || height.wrapping_sub(i32::from(initial_level)).wrapping_abs() > 1);
        let terrain = live.terrain();
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let tube = terrain.tube_for_native_cell(cell);
        if args.direction == 8 {
            //51BFFD tests distinct endpoints; Unit73F218 tests a nonzero
            //exit, including an automatic Tube whose endpoints are equal.
            //Both return before neighbor/height/list work. Original calls:
            //tools/spatial_oracle/unit_entry_traversal.{py,json,meta.json}.
            return Ok(
                if tube.is_some_and(|tube| {
                    if infantry {
                        tube.entry != tube.exit
                    } else {
                        tube.exit != (0, 0)
                    }
                }) {
                    0
                } else {
                    7
                },
            );
        }
        let cross_tube = |tube: Option<&crate::map::tube_facts::TubeFact>, direction: i32| {
            tube.is_some_and(|tube| {
                let distance = direction.wrapping_sub(tube.direction).wrapping_abs();
                distance > 2 && distance < 6 && args.direction != -1
            })
        };
        if cross_tube(tube, args.direction) {
            return Ok(7);
        }
        let backstep = backstep_cell(&cells, cell, args.direction);
        if cross_tube(
            terrain.tube_for_native_cell(backstep),
            (args.direction - 4) & 7,
        ) {
            return Ok(7);
        }
        //51C0AE reloads the target level after the first neighbor lookup.
        if infantry && height.wrapping_sub(i32::from(live.level(cell))) > 4 {
            return Ok(0);
        }
        if !adjust_height_and_list(
            terrain,
            cell,
            args.direction,
            args.previous_cell,
            &mut height,
            &mut list_bridge,
        ) {
            return Ok(7);
        }
        //51C0FB..136 /73F303..348: raw occupation selection is independent
        //of the object list plane modified by4D9C60. Keep both authorities.
        if height != -1
            && live.flags(cell) & 0x100 != 0
            && height == i32::from(live.level(cell)) + 4
        {
            (bits, owner) = raw(live, cell, MovementLayer::Bridge);
            vehicle_occupied = bits & 0x20 != 0;
        }
        if list_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        }
    };
    //Unit73F34C / Infantry51C13A read A8E7AC, independent of GameMode.
    //Unit still performs the read/+320 when3D5 is false; Infantry skips it.
    //578540 consumes the pointer directly, without578460's extra lookup.
    let boundary_refused = !live.sim.object_placement_scope_active()
        && (!infantry || e.in_playfield)
        && !crate::sim::cell_rect::retained_cell_is_in_playfield(
            cell,
            live.sim.playfield_bounds,
            live.terrain(),
        )
        && !live.sim.foot_allows_outside_playfield(id)?;
    if boundary_refused && e.in_playfield {
        return Ok(7);
    }
    let p = live.coord(cell);
    let capability = CrushCapability::new(crusher, obj.omni_crusher);
    let mut entry_result: u8 = 0;
    let mut stationary_infantry = 0u32;
    let mut crush_latch = false;
    let overlay = match cell {
        Cell::Real(index) => live.terrain().cells()[index].bridge_facts.overlay_id,
        Cell::Dummy => u8::try_from(
            live.terrain()
                .shared_cell_dummy()
                .overlay_identity_state()
                .0,
        )
        .ok(),
    };
    if let Some(overlay) = overlay {
        let flags = live
            .registry
            .and_then(|r| r.flags(overlay))
            .ok_or("repair overlay receiver lacks registered type")?;
        if flags.crate_type
            && (infantry || !live.sim.session.game_mode_nonzero)
            && !live.sim.owner_is_human(e.owner())
        {
            return Ok(7);
        }
        if flags.wall {
            let allied = live
                .sim
                .overlay_grid
                .as_ref()
                .and_then(|g| g.cell(p.0 as u16, p.1 as u16).wall_owner)
                .is_some_and(|o| mover.friendly(live, o));
            if !infantry
                && ((flags.crushable && crusher)
                    || obj.movement_zone == crate::rules::locomotor_type::MovementZone::CrusherAll)
            {
                if allied {
                    entry_result = entry_result.max(4);
                }
            } else {
                let warhead = weapon0
                    .and_then(|w| w.warhead.as_deref())
                    .and_then(|name| live.rules.warhead(name));
                if !combat_weapon::is_armed(e, obj)
                    || !warhead.is_some_and(|w| w.wall || (w.wood && flags.armor_is_wood))
                {
                    return Ok(7);
                }
                entry_result = entry_result.max(if allied { 4 } else { 5 });
            }
        }
    }
    //47C520 is asked only inside the Building arms; defer the list walk.
    let first_building = || {
        live.sim.substrate.occupancy.first_building_on_layer(
            p.0 as u16,
            p.1 as u16,
            MovementLayer::Ground,
        )
    };
    let members = live.sim.substrate.occupancy.cell_objects(
        p.0 as u16,
        p.1 as u16,
        layer,
        live.sim
            .production
            .terrain_object_cells
            .get(&(p.0 as u16, p.1 as u16))
            .copied(),
    );
    for member in members {
        let CellObjectMember::Entity(blocker_id) = member else {
            if !infantry {
                // Unit73FBAC calls the target-sensitive +2E4 selector. Terrain
                // has no Techno bit and is not a Cell (746CD0 ->6F3330).
                let selected = combat_weapon::what_weapon_should_i_use(
                    live.rules,
                    obj,
                    &combat_weapon::attacker_facts(e, obj),
                    Some(&combat_weapon::TargetFacts::Terrain),
                );
                let weapon = combat_weapon::weapon_for_index(obj, e.veterancy(), selected)
                    .and_then(|(name, _)| live.rules.weapon(name));
                if !weapon
                    .and_then(|w| w.warhead.as_deref())
                    .and_then(|name| live.rules.warhead(name))
                    .is_some_and(|w| w.wood)
                {
                    return Ok(7);
                }
                entry_result = entry_result.max(5);
            }
            continue;
        };
        if blocker_id == id {
            if !infantry {
                bits &= !0x20;
                vehicle_occupied = false;
            }
            continue;
        }
        let b = live
            .sim
            .substrate
            .entities
            .get(blocker_id)
            .ok_or("repair list has retired blocker")?;
        let bt = mover
            .object_type(live, b.type_ref())
            .ok_or("repair list missing blocker type")?;
        if infantry && e.slave.owner() == Some(blocker_id) {
            let query = crate::sim::slave_deposit::SlaveDepositQuery {
                entities: &live.sim.substrate.entities,
                occupancy: &live.sim.substrate.occupancy,
                terrain: live.terrain(),
                rules: live.rules,
                interner: &live.sim.interner,
            };
            if query.admits(id, blocker_id, cell) {
                //51C2CA clears the local vehicle latch and CONTINUES; earlier
                //soft results and later blockers retain their significance.
                vehicle_occupied = false;
                continue;
            }
        }
        let allied = mover.friendly(live, b.owner());
        let mission = if infantry {
            e.mission.current().known()
        } else {
            e.mission.effective().known()
        };
        if infantry {
            match infantry_target_admission(live, e, obj, b, cell)? {
                InfantryTargetAdmission::Ordinary => {}
                InfantryTargetAdmission::SkipCurrent => continue,
                InfantryTargetAdmission::DirectTarget => {
                    if crate::sim::superweapon::invulnerability::is_invulnerable(
                        b.invulnerability.as_ref(),
                        live.sim.session.binary_frame,
                    ) {
                        return Ok(7);
                    }
                    return Ok(
                        if layer == MovementLayer::Ground
                            && e.dock_entered_with.is_none()
                            && !row_nonzero(live, cell, obj.speed_type)?
                        {
                            7
                        } else {
                            0
                        },
                    );
                }
            }
        }
        if b.category == EntityCategory::Structure {
            if !infantry {
                //73F57C..5A2: a contacted Building whose458A00 result is
                //false is skipped before the ordinary Building branches.
                if matches!(crate::sim::pathfinding::cell_entry::decide_live_vehicle_building_entry(
                    crate::sim::pathfinding::cell_entry::LiveVehicleBuildingEntry {
                        mover_category:e.category,
                        branch:crate::sim::pathfinding::cell_entry::VehicleBuildingEntryBranch::RadioContact { mover_has_contact:e.has_live_contact_with(blocker_id) },
                        checked_building_id:blocker_id,candidate_building_id:first_building(),
                        candidate_x:p.0 as u16,building_origin_x:b.position.rx,
                        number_impassable_rows:bt.number_impassable_rows,is_unit_repair:bt.unit_repair,is_bunker:bt.bunker,
                        bunker_occupied:b.bunker_occupant.is_some(),
                    }),crate::sim::pathfinding::cell_entry::BuildingOccupantEntryDecision::SkipBlocker) {
                    continue;
                }
                //73F5EF/73F628 have distinct type/mission pairs. +1D4 is
                //the live warp latch.
                if object_target_is(e, b)
                    && ((mission == Some(MissionType::Enter) && bt.unit_absorb)
                        || (mission == Some(MissionType::Eaten) && bt.grinding))
                    && !b.is_warped_out()
                {
                    return Ok(0);
                }
                //73F661..6CF: Grinding additionally walks CURRENT ground E4,
                //without mission/NavCom/radio/warp gates. Do the actual lookup
                //at this list position, preserving shared-Dummy stamping.
                if bt.grinding {
                    let current =
                        crate::sim::movement::ground_pose::position_world_coord(&e.position);
                    let current_cell = live
                        .terrain()
                        .native_cell_identity(((current.x / 256) as i16, (current.y / 256) as i16));
                    if let Cell::Real(index) = current_cell {
                        let c = &live.terrain().cells()[index];
                        if live
                            .sim
                            .substrate
                            .occupancy
                            .get(c.rx, c.ry)
                            .is_some_and(|list| {
                                list.iter_layer(MovementLayer::Ground)
                                    .any(|entry| entry.entity_id == blocker_id)
                            })
                        {
                            return Ok(0);
                        }
                    }
                }
            }
            if infantry && bt.invisible_in_game {
                continue;
            }
            if bt.gate {
                if b.is_open_gate() {
                    continue;
                }
                if !allied && !combat_weapon::is_armed(e, obj) {
                    return Ok(7);
                }
                entry_result = entry_result.max(if allied { 3 } else { 5 });
                continue;
            }
            if !infantry {
                if (bt.unit_repair||bt.bunker)&&first_building()==Some(blocker_id)
                    && matches!(crate::sim::pathfinding::cell_entry::decide_live_vehicle_building_entry(
                        crate::sim::pathfinding::cell_entry::LiveVehicleBuildingEntry {
                            mover_category:e.category,branch:crate::sim::pathfinding::cell_entry::VehicleBuildingEntryBranch::UnitRepairOrBunker,
                            checked_building_id:blocker_id,candidate_building_id:first_building(),
                            candidate_x:p.0 as u16,building_origin_x:b.position.rx,
                            number_impassable_rows:bt.number_impassable_rows,is_unit_repair:bt.unit_repair,is_bunker:bt.bunker,
                            bunker_occupied:b.bunker_occupant.is_some(),
                        }),crate::sim::pathfinding::cell_entry::BuildingOccupantEntryDecision::SkipBlocker) {continue;}
                if bt.invisible_in_game {
                    continue;
                }
                if bt.bib
                    && live.sim.substrate.occupancy.first_building_on_layer(
                        (p.0 as u16).wrapping_add(1),
                        p.1 as u16,
                        MovementLayer::Ground,
                    ) != Some(blocker_id)
                {
                    continue;
                }
            }
            if allied {
                return Ok(7);
            }
        }
        //73F823..844: this exact Unit target is admitted immediately. Other
        //transport capacity/radio requirements belong to the order/PerCell
        //producers; there is no extra gate in this admission arm.
        if !infantry
            && mission == Some(MissionType::Enter)
            && b.category == EntityCategory::Unit
            && object_target_is(e, b)
        {
            return Ok(0);
        }
        //Infantry51C579..58D's occupant-list alliance override is local to
        //this gate. Earlier Building/wall checks and the later raw-owner
        //query use actual House alliances, including during crew escape.
        let allied = allied || (infantry && live.sim.object_placement_scope_active());
        if !allied {
            if b.cloak.as_ref().is_some_and(|s| s.is_fully_cloaked()) {
                entry_result = entry_result.max(1);
                continue;
            }
            if !infantry
                && crusher
                && bump_crush::can_crush(
                    capability,
                    CrushTarget::from_entity(b, live.sim.session.binary_frame),
                )
            {
                crush_latch = true;
                continue;
            }
            if if infantry {
                crate::sim::combat::combat_weapon::weapon_damage_value(e, obj, live.rules) <= 0
            } else {
                weapon0.is_none()
            } {
                return Ok(7);
            }
            if b.category == EntityCategory::Structure && bt.bridge_repair_hut {
                return Ok(7);
            }
            if infantry {
                if b.category == EntityCategory::Infantry {
                    if infantry_disguised_to(live, b, e.owner())? {
                        //51C61F is an assignment, not a boolean soft latch.
                        entry_result = 6;
                    }
                } else {
                    entry_result = entry_result.max(5);
                }
            } else {
                entry_result = entry_result.max(5);
            }
        } else if !infantry {
            //73F865..8C0: Foot NavCom, body turn, then active IsMoving.
            if b.navigation.nav_com.is_some()
                || b.body_facing.is_rotating(live.sim.session.binary_frame)
                || moving(b)
            {
                if head_on(e, b, live.sim.session.binary_frame) {
                    return Ok(7);
                }
                if (b.foot_occupation_enabled && b.category != EntityCategory::Infantry)
                    || crate::sim::movement::drive_track::occupant_slot_a4_answers_true(b)
                {
                    entry_result = entry_result.max(2);
                }
            } else {
                entry_result = entry_result.max(6);
            }
        } else {
            match b.category {
                EntityCategory::Aircraft | EntityCategory::Structure => return Ok(7),
                EntityCategory::Unit => {
                    if !moving(b) && b.navigation.nav_com.is_none() {
                        entry_result = entry_result.max(6);
                    } else if b.foot_occupation_enabled
                        || crate::sim::movement::drive_track::occupant_slot_a4_answers_true(b)
                    {
                        entry_result = entry_result.max(2);
                    }
                }
                EntityCategory::Infantry => {
                    if !moving(b) {
                        stationary_infantry = stationary_infantry.wrapping_add(1);
                    }
                }
            }
        }
    }
    if layer == MovementLayer::Ground
        && (!infantry || e.dock_entered_with.is_none())
        && !row_nonzero(live, cell, obj.speed_type)?
    {
        return Ok(7);
    }
    if infantry {
        if entry_result == 0 && vehicle_occupied {
            //51C7DF..7F7 returns2 before owner/full-subcell tests.
            return Ok(2);
        }
        if let Some(owner) = owner {
            if mover.friendly(live, owner) {
                if bits & 0x1c == 0x1c && entry_result < 2 {
                    entry_result = if stationary_infantry == 3 { 6 } else { 2 };
                }
            } else {
                if crate::sim::combat::combat_weapon::weapon_damage_value(e, obj, live.rules) <= 0 {
                    return Ok(7);
                }
                entry_result = entry_result.max(5);
            }
        }
        Ok(if entry_result == 0 && bits & 0x1c == 0x1c {
            7
        } else {
            entry_result
        })
    } else {
        //73FC24: a nonzero running code wins before every raw-byte branch.
        if entry_result != 0 {
            return Ok(entry_result);
        }
        if crush_latch {
            //73FCF6 asks for the first GROUND Unit even when the selected
            //object list/raw byte came from the bridge deck. Keep the native
            //Object IsCrushableBy subset shared with the track entry owner.
            let first_unit_is_crushable = || {
                live.sim
                    .substrate
                    .occupancy
                    .get(p.0 as u16, p.1 as u16)
                    .into_iter()
                    .flat_map(|list| list.iter_layer(MovementLayer::Ground))
                    .filter_map(|member| live.sim.substrate.entities.get(member.entity_id))
                    .find(|unit| unit.category == EntityCategory::Unit)
                    .is_some_and(|unit| {
                        crate::sim::pathfinding::cell_entry::unit_tail_is_crushable_by(
                            unit,
                            capability,
                            mover.friendly(live, unit.owner()),
                            live.sim.session.binary_frame,
                        )
                    })
            };
            return Ok(if !vehicle_occupied || first_unit_is_crushable() {
                0
            } else {
                2
            });
        }
        if bits & 0x3f == 0 {
            return Ok(0);
        }
        if vehicle_occupied || owner.is_some_and(|owner| mover.friendly(live, owner)) {
            return Ok(2);
        }
        if !crusher {
            return Ok(
                if !weapon0
                    .and_then(|w| w.projectile.as_deref())
                    .and_then(|name| live.rules.projectile(name))
                    .is_some_and(|p| p.ag)
                {
                    7
                } else {
                    5
                },
            );
        }
        Ok(0)
    }
}

/// `AircraftClass::Can_Enter_Cell` (`0x004196B0`) as an object's own +1AC
/// inside the repair/damage occupant pass. The answer is the one owner's,
/// [`Simulation::aircraft_can_enter_cell`]; the pass itself ignores it (an
/// Aircraft's kind 2 always takes the first pass's damage at `0x00487A3B`,
/// and the neighbour pass admits only Foot objects), so only the read's
/// effects matter here. Canonical lookups reproduce the shared-dummy stamps.
/// Two preconditions VERA cannot yet reproduce refuse with an error:
/// - Fly, Jumpjet, Rocket and Teleport have the constant-false `+A0`
///   (`0x004B6630`) the neighbour pass relies on; other families are
///   unproved. See LIVE_REPAIR_AIRCRAFT_RECEIVER.md and
///   locomotor_at_coord.json.
/// - The Team arm (`0x004196BC..0x004196DE`) reaches `0x006EC300`'s
///   waypoint read for a formed team on action 3, which the simulation
///   cannot perform (no waypoint table).
fn aircraft_entry(
    live: &EntryReadContext<'_>,
    entity: &GameEntity,
    cell: Cell,
) -> Result<u8, String> {
    if entity.locomotor.as_ref().is_none_or(|l| {
        !matches!(
            l.kind,
            LocomotorKind::Fly
                | LocomotorKind::Jumpjet
                | LocomotorKind::Rocket
                | LocomotorKind::Teleport
        )
    }) {
        return Err("repair Aircraft requires a proved constant-false AtCoord family".into());
    }
    if live
        .sim
        .team_script_vm
        .member_step_reads_waypoint(entity.stable_id())
    {
        return Err("repair Aircraft Team action3 has unresolved waypoint lookup effects".into());
    }
    Ok(live.sim.aircraft_can_enter_cell(entity.stable_id(), cell))
}

fn terrain_impassable(
    live: &EntryReadContext<'_>,
    object: CellObjectMember,
    cell: Cell,
) -> Result<bool, String> {
    let CellObjectMember::Terrain(id) = object else {
        unreachable!()
    };
    let terrain = live
        .sim
        .production
        .terrain_objects
        .get(&id)
        .ok_or("repair retired Terrain")?;
    let ty = live
        .rules
        .terrain_object_type_case_insensitive(live.sim.interner.resolve(terrain.type_ref))
        .ok_or("repair missing TerrainType")?;
    // TerrainClass::Can_Enter_Cell @ 0x0071C4D0: the first foundation cell
    // that is not clear for a typeless, houseless build refuses (7).
    let p = live.coord(cell);
    let speed = if ty.water_bound {
        SpeedType::Float
    } else {
        SpeedType::Track
    };
    for offset in crate::rules::foundation::foundation_cell_offsets(&ty.foundation) {
        let selected = live
            .terrain()
            .native_cell_identity((p.0.wrapping_add(offset.0), p.1.wrapping_add(offset.1)));
        if !crate::sim::build_site::is_clear_to_build(
            live.sim,
            live.rules,
            live.registry,
            selected,
            Some(speed),
            None,
            None,
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}
/// BuildingClass::Can_Enter_Cell @ 0x00449440: a marked building that can
/// undeploy tests the one cell, otherwise its type's CanPlaceAt from the
/// cell; either for the building's own house.
fn building_impassable(
    live: &EntryReadContext<'_>,
    e: &GameEntity,
    obj: &ObjectType,
    cell: Cell,
) -> Result<bool, String> {
    let admitted = if obj.undeploys_into.is_some() && e.lifecycle.cell_marked {
        crate::sim::build_site::is_clear_to_build(
            live.sim,
            live.rules,
            live.registry,
            cell,
            crate::sim::build_site::building_speed_type(obj),
            Some(obj),
            Some(e.owner()),
        )
    } else {
        crate::sim::build_site::can_place_building_at(
            live.sim,
            live.rules,
            live.registry,
            obj,
            live.coord(cell),
            Some(e.owner()),
        )
    };
    Ok(!admitted)
}
