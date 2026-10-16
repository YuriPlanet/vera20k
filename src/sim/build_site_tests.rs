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
     [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n3=TIB01\n\
     [General]\nGDIGateOne=GAGATE\n\
     [E1]\nStrength=125\nSpeed=4\n\
     [MTNK]\nStrength=400\nSpeed=6\n\
     [GAPOWR]\nStrength=750\n\
     [GAGATE]\nStrength=1000\nGate=yes\n\
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
    let ini = IniFile::from_str(&format!("{RULES}{land}"));
    let art = IniFile::from_str(ART);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("build-site rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
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
        grid.cell_mut(cell.0, cell.1).wall_owner = owner;
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
    a.sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        off_104: 139,
        ..crate::sim::arena_fixture::OPEN_PLAYFIELD
    });
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

/// Ore is a `Tiberium=` overlay that some TiberiumType claims; without one,
/// `OverlayToTiberiumIndex` answers -1 and the overlay refuses the fence too.
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

#[test]
fn a_gate_shares_its_cell_only_with_its_own_houses_laser_fence() {
    let mut a = arena(CLEAR_BUILDABLE);
    let americans = a.house("Americans");
    a.spawn("MTNK", "Americans", (10, 10));
    assert!(!a.cell_clear((10, 10), "GAGATE", Some(americans)));
    assert!(a.cell_clear((12, 12), "GAGATE", Some(americans)));
}
