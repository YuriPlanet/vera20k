//! `sim::build_site` on the flat test arena. The rules text is this fixture's
//! own; the arms it exercises are the ones placement, Deploy and TryToDeploy
//! reach in a skirmish.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::overlay_grid::OverlayGrid;

const RULES: &str = "[InfantryTypes]\n0=E1\n\
     [VehicleTypes]\n0=MTNK\n\
     [AircraftTypes]\n\
     [BuildingTypes]\n0=GAPOWR\n1=GAGATE\n2=GAWALL\n3=GAFENCE\n4=NAYARD\n5=GAPAVE\n6=GAPLUG\n\
     7=NAGATE\n\
     [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n3=TIB01\n\
     [General]\nGDIGateOne=GAGATE\n\
     [E1]\nStrength=125\nSpeed=4\n\
     [MTNK]\nStrength=400\nSpeed=6\n\
     [GAPOWR]\nStrength=750\n\
     [GAGATE]\nStrength=1000\nGate=yes\n\
     [NAGATE]\nStrength=1000\nGate=yes\n\
     [GAWALL]\nStrength=300\nWall=yes\n\
     [GAFENCE]\nStrength=300\nLaserFence=yes\n\
     [NAYARD]\nStrength=1500\nWaterBound=yes\nNaval=yes\n\
     [GAPAVE]\nStrength=100\nPlaceAnywhere=yes\n\
     [GAPLUG]\nStrength=100\n\
     [GASAND]\nWall=yes\n[CYCL]\n[GAWALL]\nWall=yes\n[TIB01]\nTiberium=yes\n";

const ART: &str = "[GAPOWR]\nFoundation=2x2\n[GAGATE]\nFoundation=1x1\n\
    [GAWALL]\nFoundation=1x1\nToOverlay=GAWALL\n[GAFENCE]\nFoundation=1x1\n\
    [NAYARD]\nFoundation=2x2\n[GAPAVE]\nFoundation=1x1\n[GAPLUG]\nFoundation=1x1\n";

const CLEAR_BUILDABLE: &str = "[Clear]\nBuildable=yes\nFloat=0%\n";

struct Arena {
    sim: Simulation,
    rules: RuleSet,
    registry: OverlayTypeRegistry,
}

fn arena(land: &str) -> Arena {
    arena_from(&format!("{RULES}{land}"))
}

fn arena_from(text: &str) -> Arena {
    let ini = IniFile::from_str(text);
    let ini = if ini.section("Tiberiums").is_some() {
        crate::rules::tiberium_type::with_tiberium_image_slots(&ini)
    } else {
        ini
    };
    let art = IniFile::from_str(ART);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("build-site rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    *sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    Arena {
        sim,
        rules,
        registry,
    }
}

impl Arena {
    fn ty(&self, name: &str) -> &ObjectType {
        self.rules.object(name).expect("fixture type")
    }

    fn house(&mut self, name: &str) -> InternedId {
        self.sim.interner.intern(name)
    }

    fn cell_clear(&self, cell: (i16, i16), ty: &str, house: Option<InternedId>) -> bool {
        let terrain = self.sim.resolved_terrain.as_ref().expect("arena terrain");
        let ty = self.ty(ty);
        is_clear_to_build(
            &self.sim,
            &self.rules,
            Some(&self.registry),
            terrain.native_cell_identity(cell),
            building_speed_type(ty),
            Some(ty),
            house,
            None,
            None,
        )
    }

    fn can_place(&self, ty: &str, origin: (i16, i16), house: Option<InternedId>) -> bool {
        can_place_building_at(
            &self.sim,
            &self.rules,
            Some(&self.registry),
            self.ty(ty),
            origin,
            house,
            None,
            None,
        )
    }

    fn spawn(&mut self, ty: &str, owner: &str, cell: (u16, u16)) {
        self.sim
            .spawn_object(ty, owner, cell.0, cell.1, 0, &self.rules)
            .expect("fixture object spawns");
    }

    fn overlay(&mut self, cell: (u16, u16), name: &str, data: u8, owner: Option<InternedId>) {
        let id = self.registry.id_for_name(name).expect("fixture overlay");
        let grid = self.sim.overlay_grid.as_mut().expect("overlay grid");
        grid.place_overlay(cell.0, cell.1, id, data);
        grid.cell_mut_for_test(cell.0, cell.1).wall_owner = owner;
    }
}

#[test]
fn ground_must_be_buildable_for_a_plain_building() {
    assert!(arena(CLEAR_BUILDABLE).can_place("GAPOWR", (10, 10), None));
    // A LandType section no rules layer defines leaves its row unset.
    assert!(!arena("").can_place("GAPOWR", (10, 10), None));
    assert!(!arena("[Clear]\nBuildable=no\n").can_place("GAPOWR", (10, 10), None));
}

#[test]
fn every_foundation_cell_must_be_clear() {
    let mut a = arena(CLEAR_BUILDABLE);
    a.spawn("MTNK", "Americans", (11, 11));
    assert!(a.cell_clear((10, 10), "GAPOWR", None));
    assert!(!a.cell_clear((11, 11), "GAPOWR", None));
    assert!(!a.can_place("GAPOWR", (10, 10), None));
    assert!(a.can_place("GAPOWR", (12, 12), None));
    // Infantry blocks through the list as well as its sub-cell bit.
    a.spawn("E1", "Americans", (14, 14));
    assert!(!a.can_place("GAPOWR", (13, 13), None));
}

#[test]
fn the_empty_cell_and_cells_outside_the_playfield_refuse_but_place_anywhere_does_not() {
    let mut a = arena(CLEAR_BUILDABLE);
    assert!(!a.can_place("GAPLUG", (0, 0), None));
    assert!(a.can_place("GAPAVE", (0, 0), None));
    // Narrow the playfield's right edge to `x - y < 2`.
    a.sim
        .set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
            off_104: 139,
            ..crate::sim::arena_fixture::OPEN_PLAYFIELD
        }));
    assert!(a.can_place("GAPLUG", (11, 10), None));
    assert!(!a.can_place("GAPLUG", (12, 10), None));
    assert!(a.can_place("GAPOWR", (10, 10), None));
    assert!(!a.can_place("GAPOWR", (11, 10), None));
    assert!(a.can_place("GAPAVE", (12, 10), None));
}

#[test]
fn a_water_bound_type_reads_its_float_speed_not_buildable() {
    let a = arena(CLEAR_BUILDABLE);
    assert_eq!(building_speed_type(a.ty("NAYARD")), Some(SpeedType::Float));
    assert_eq!(building_speed_type(a.ty("GAPOWR")), None);
    assert!(!a.can_place("NAYARD", (10, 10), None));
    let floats = arena("[Clear]\nBuildable=no\nFloat=100%\n");
    assert!(floats.can_place("NAYARD", (10, 10), None));
    assert!(!floats.can_place("GAPOWR", (10, 10), None));
}

#[test]
fn a_gate_stands_on_its_own_house_wall() {
    let mut a = arena(CLEAR_BUILDABLE);
    let americans = a.house("Americans");
    let soviets = a.house("Soviets");
    a.overlay((10, 10), "GAWALL", 0, Some(americans));
    assert!(a.cell_clear((10, 10), "GAGATE", Some(americans)));
    assert!(!a.cell_clear((10, 10), "GAGATE", Some(soviets)));
    assert!(!a.cell_clear((10, 10), "GAPLUG", Some(americans)));
    // Any other overlay refuses every type.
    a.overlay((11, 10), "CYCL", 0, Some(americans));
    assert!(!a.cell_clear((11, 10), "GAGATE", Some(americans)));
}

/// Ares 10015780 (hook 0x47C8AB): any `Gate=` type stands on its own
/// house's `Wall=yes` overlay; the stock arms admit only the `[General]`
/// gates (NAGATE is none of them).
#[test]
fn a_gate_outside_the_general_list_stands_on_a_wall_only_under_ares() {
    let mut a = arena(CLEAR_BUILDABLE);
    let americans = a.house("Americans");
    let soviets = a.house("Soviets");
    a.overlay((10, 10), "GASAND", 0, Some(americans));
    assert_eq!(
        a.cell_clear((10, 10), "NAGATE", Some(americans)),
        a.rules.ares_form()
    );
    assert!(!a.cell_clear((10, 10), "NAGATE", Some(soviets)));
    // CYCL is no wall.
    a.overlay((11, 10), "CYCL", 0, Some(americans));
    assert!(!a.cell_clear((11, 10), "NAGATE", Some(americans)));
}

#[test]
fn a_wall_rebuilds_only_over_its_own_damaged_wall() {
    let mut a = arena(CLEAR_BUILDABLE);
    let americans = a.house("Americans");
    a.overlay((10, 10), "GAWALL", 0x10, Some(americans));
    a.overlay((11, 10), "GAWALL", 0x0F, Some(americans));
    assert!(a.cell_clear((10, 10), "GAWALL", Some(americans)));
    assert!(!a.cell_clear((11, 10), "GAWALL", Some(americans)));
    assert!(!a.cell_clear((10, 10), "GAWALL", None));
}

/// Ore is a `Tiberium=` overlay that a TiberiumType resolves. A registry with
/// no Tiberium class is native-invalid; the fence safely refuses that input.
#[test]
fn a_laser_fence_may_stand_on_ore() {
    let mut a = arena(&format!(
        "{CLEAR_BUILDABLE}[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\n"
    ));
    a.overlay((10, 10), "TIB01", 3, None);
    assert!(a.cell_clear((10, 10), "GAFENCE", None));
    assert!(!a.cell_clear((10, 10), "GAPLUG", None));

    let mut without = arena(CLEAR_BUILDABLE);
    without.overlay((10, 10), "TIB01", 3, None);
    assert!(!without.cell_clear((10, 10), "GAFENCE", None));
}

mod can_be_built_on {
    use super::*;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellObjectMember;
    use crate::sim::terrain_object::TerrainObjectState;

    const TERRAIN_TYPES: &str = "[TerrainTypes]\n0=TREE\n1=ROCK\n\
        [TREE]\nSpawnsTiberium=yes\nTemperateOccupationBits=7\n{tree}\
        [ROCK]\nTemperateOccupationBits=7\n{rock}";

    fn terrain_arena(general: &str, tree: &str, rock: &str) -> Arena {
        arena_from(&format!(
            "{RULES}{}{CLEAR_BUILDABLE}",
            TERRAIN_TYPES
                .replace("{tree}", tree)
                .replace("{rock}", rock)
                .replace(
                    "[TerrainTypes]",
                    &format!("[General]\n{general}[TerrainTypes]")
                )
        ))
    }

    fn put_terrain(a: &mut Arena, name: &str, cell: (u16, u16)) -> u64 {
        let ty = a
            .rules
            .terrain_object_type_case_insensitive(name)
            .expect("terrain type")
            .clone();
        let type_ref = a.sim.interner.intern(name);
        let state = TerrainObjectState::new(
            900,
            type_ref,
            cell,
            &ty,
            false,
            a.sim.resolved_terrain.as_ref(),
        );
        a.sim
            .terrain_objects
            .insert_constructed(state, ty.spawns_tiberium);
        a.sim.terrain_objects.add_to_cell(900, cell);
        assert_eq!(a.sim.terrain_objects.id_at(cell), Some(900));
        900
    }

    /// Phobos `CanBeBuiltOn=` (`Ext/TerrainType/Body.h:34`) resolves the type
    /// key else `Tibtree.CanBeBuiltOn=`/`Terrain.CanBeBuiltOn=` by
    /// `SpawnsTiberium=` (`Ext/Rules/Body.cpp:481-482`, default no).
    #[test]
    fn the_type_key_and_globals_resolve_by_spawns_tiberium() {
        let yes = |a: &Arena, name: &str| {
            a.rules
                .terrain_object_type_case_insensitive(name)
                .expect("terrain type")
                .can_be_built_on()
        };
        let keyed = terrain_arena("", "CanBeBuiltOn=yes\n", "");
        assert!(yes(&keyed, "TREE"));
        assert!(!yes(&keyed, "ROCK"));
        let terrain = terrain_arena("Terrain.CanBeBuiltOn=yes\n", "", "");
        assert!(yes(&terrain, "ROCK"), "ordinary terrain global");
        assert!(!yes(&terrain, "TREE"), "tiberium tree untouched");
        let tibtree = terrain_arena("Tibtree.CanBeBuiltOn=yes\n", "", "");
        assert!(yes(&tibtree, "TREE"), "tiberium tree global");
        assert!(!yes(&tibtree, "ROCK"));
        let no = terrain_arena(
            "Terrain.CanBeBuiltOn=yes\nTibtree.CanBeBuiltOn=yes\n",
            "CanBeBuiltOn=no\n",
            "",
        );
        assert!(!yes(&no, "TREE"), "explicit no beats globals");
    }

    /// Phobos `MapClass_IsAreaFree` (`0x586780`, wrapper `0x5069DB`): during
    /// the AI's site search a buildable Terrain stops blocking and the
    /// passability byte also admits `HasFreeSpots` — native passability 5
    /// (`RecalcZoneType` writes `[edi+0x4C],5` at `0x483E1E`), VERA's
    /// `zone_class::BUILDING` name for that value.
    #[test]
    fn the_ai_site_search_admits_a_buildable_terrain_with_zone_five() {
        let mut a = terrain_arena(
            "",
            "CanBeBuiltOn=yes
",
            "",
        );
        put_terrain(&mut a, "TREE", (16, 16));
        let cell = a
            .sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 16));
        let crate::map::cell_index::NativeCellIdentity::Real(index) = cell else {
            panic!("fixture cell is real");
        };
        let admits = |a: &Arena, ai_build_location: bool| {
            crate::sim::cell_rect::check_occupancy_rect(
                crate::sim::cell_rect::CellRectOccupancyContext {
                    native_cells: None,
                    rect: crate::sim::cell_rect::CellRect::single(16, 16),
                    reservation_arg: -1,
                    reservations: None,
                    occupancy: Some(&a.sim.substrate.occupancy),
                    terrain_object_cells: Some(a.sim.terrain_objects.cell_index()),
                    terrain_objects: Some(&a.sim.terrain_objects),
                    ai_build_location,
                    resolved_terrain: a.sim.resolved_terrain.as_ref(),
                    overlay_grid: None,
                    playfield_bounds: a.sim.playfield_bounds(),
                },
            )
        };
        a.sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cells_mut_for_test()[index]
            .zone_type = crate::map::resolved_terrain::zone_class::BUILDING;
        assert!(
            admits(&a, true),
            "HasFreeSpots is admitted while evaluating"
        );
        assert!(
            !admits(&a, false),
            "outside the AI search the terrain blocks"
        );
        a.sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cells_mut_for_test()[index]
            .zone_type = crate::map::resolved_terrain::zone_class::CRUSHABLE;
        assert!(!admits(&a, true), "crushable is not HasFreeSpots");
        a.sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cells_mut_for_test()[index]
            .zone_type = crate::map::resolved_terrain::zone_class::GROUND;
        assert!(admits(&a, true));
    }

    /// Phobos `CanBeBuiltOn` hooks: the admission (`0x47C640` ordinary arm),
    /// the placement flush (`0x45EF3A`) and the PlaceDown removal
    /// (`0x5684B1`) all read the resolved key.
    #[test]
    fn a_buildable_terrain_is_admitted_flushed_and_removed_with_the_building() {
        use crate::sim::house_state::HouseState;
        let mut a = terrain_arena("", "CanBeBuiltOn=yes\n", "");
        let owner = a.house("Americans");
        a.sim
            .houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        put_terrain(&mut a, "TREE", (16, 16));
        assert!(a.cell_clear((16, 16), "GAPOWR", None), "admitted");
        assert!(a.can_place("GAPOWR", (16, 16), None));
        let power = a.ty("GAPOWR").clone();
        assert_eq!(power.foundation.dimensions(), (2, 2), "fixture foundation");
        assert!(
            a.sim.map_cell_in_bounds((16, 16)),
            "fixture cell on the map"
        );
        assert_eq!(
            flush_for_placement(
                &mut a.sim,
                &a.rules,
                Some(&a.registry),
                &power,
                (16, 16),
                owner,
                &mut FrameEffects::default(),
            ),
            Flush::Clear,
            "the buildable terrain does not block the flush"
        );
        let building = a
            .sim
            .construct_object_limbo_at_height(
                "GAPOWR",
                "Americans",
                0,
                0,
                0,
                0,
                &a.rules,
                &mut FrameEffects::default(),
            )
            .expect("limbo building");
        let placed = a.sim.place_building_with_cleared_foundation(
            building,
            crate::sim::components::DriveCoord::cell(16, 16, 0),
            0,
            &a.rules,
            Some(&a.registry),
            &mut FrameEffects::default(),
        );
        assert_eq!(placed, Some(building));
        assert_eq!(
            a.sim.terrain_objects.id_at((16, 16)),
            None,
            "PlaceDown removed the buildable terrain"
        );

        // The same cell with an ordinary terrain refuses both the admission
        // and the flush, and keeps the object.
        let mut a = terrain_arena("", "", "");
        let owner = a.house("Americans");
        a.sim
            .houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        put_terrain(&mut a, "ROCK", (16, 16));
        assert!(
            !a.sim
                .terrain_objects
                .get(900)
                .expect("rock")
                .can_be_built_on()
        );
        assert!(
            a.sim
                .cell_objects((16, 16), MovementLayer::Ground)
                .any(|member| matches!(member, CellObjectMember::Terrain(900))),
            "the rock heads the cell content"
        );
        assert!(!a.cell_clear((16, 16), "GAPOWR", None));
        assert!(!a.can_place("GAPOWR", (16, 16), None));
        let power = a.ty("GAPOWR").clone();
        assert_eq!(
            flush_for_placement(
                &mut a.sim,
                &a.rules,
                Some(&a.registry),
                &power,
                (16, 16),
                owner,
                &mut FrameEffects::default(),
            ),
            Flush::Blocked
        );
    }
}
