//! A house's PsychicDetection buildings (`HouseClass+0x128`: placed buildings
//! whose type's `PsychicDetectionRadius=` (`+0x170C`) is positive, added by
//! `BuildingClass::Unlimbo` at `0x004414A0` and moved by
//! `BuildingClass::ChangeOwner`) and what they reveal to the local player:
//! a `PsiWarning=` anim ([`Simulation::psi_warning_detected_by`]) and an
//! enemy Techno's intent ([`Simulation::techno_intent_psychically_detected`]).
//!
//! Each test reads the list in place; presentation asks it every frame.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

impl Simulation {
    /// `viewer`'s PsychicDetection buildings that are operational (vt+0x350),
    /// with their GetCoords and `PsychicDetectionRadius=` in leptons
    /// (`<< 8`).
    pub fn psychic_detection_sites<'a>(
        &'a self,
        viewer: InternedId,
        rules: &'a RuleSet,
    ) -> impl Iterator<Item = (DriveCoord, i32)> + 'a {
        let terrain = self.resolved_terrain.as_ref();
        self.substrate.entities.values().filter_map(move |entity| {
            if entity.owner() != viewer
                || entity.category() != crate::map::entities::EntityCategory::Structure
                || !entity.lifecycle.object_alive()
                || entity.lifecycle.in_limbo()
            {
                return None;
            }
            let radius = self
                .interner
                .object_type(entity.type_ref(), rules)
                .map_or(0, |object| i32::from(object.psychic_detection_radius));
            if radius <= 0
                || self.building_operational_state(entity.stable_id(), rules) != Some(true)
            {
                return None;
            }
            Some((
                crate::sim::movement::ground_pose::object_get_coords(entity, terrain),
                radius << 8,
            ))
        })
    }

    /// `0x0043B150`, Tactical's gate for an enemy Foot's psychic detection
    /// line (`0x006D4766`): does `viewer`, the local player (`0x00A83D4C`), sense what
    /// Techno `id` means to do? Not when the player is allied with its owner
    /// (`HouseClass::IsAlliedWith @ 0x004F9A50`). Otherwise, for each
    /// operational detection building, the Techno's Target (`+0x2B4`) — a
    /// Techno (AbstractFlags bit 0) at its GetCoords or a cell at its centre
    /// (`0x00480A30`) — lies within the radius (`jle`, `0x0043B24F`,
    /// `0x0043B2ED`); failing that, a Foot's last queued waypoint (`+0x58C`
    /// `[+0x598 - 1]`) or else its NavCom (`+0x5A4`), a cell or a Techno, does
    /// (`0x0043B3F4`, `0x0043B487`). Any other Target or destination, such
    /// as Terrain, is skipped. Distances are `CoordStruct::Distance3D`
    /// (`0x0041C380`; the cell destination inlines it at `0x0043B395..
    /// 0x0043B3DE`). `sites` are `viewer`'s
    /// [`Simulation::psychic_detection_sites`], collected once for a frame's
    /// Techno walk; `cells` an isolated query. Evidence: instruction reading
    /// only; no native run compares the distance at the radius edge.
    pub fn techno_intent_psychically_detected(
        &self,
        id: u64,
        viewer: InternedId,
        sites: &[(DriveCoord, i32)],
        cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if sites.is_empty()
            || crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(self.house_alliances.as_map()),
                &self.interner,
                viewer,
                entity.owner(),
            )
        {
            return false;
        }
        // A Techno or a cell; Terrain and other objects are skipped.
        let coords = |target: TargetKind| match target {
            TargetKind::Entity(id) if !self.substrate.entities.contains(id) => None,
            target => crate::sim::movement::ground_pose::target_get_coords(target, self, cells),
        };
        let target = entity.attack_target.target().and_then(coords);
        let destination = entity
            .category()
            .is_foot()
            .then(|| {
                entity
                    .navigation
                    .nav_queue()
                    .last()
                    .copied()
                    .or(entity.navigation.nav_com())
            })
            .flatten()
            .map(TargetKind::from)
            .and_then(coords);
        sites.iter().any(|&(site, radius)| {
            [target, destination].into_iter().flatten().any(|at| {
                crate::util::native_x87::distance_3d_leptons(
                    [site.x, site.y, site.z],
                    [at.x, at.y, at.z],
                ) <= radius
            })
        })
    }
}

/// A 32-cell arena on `rules` (retail) where the human Americans, the local
/// player, hold a powered Psychic Sensor (`NAPSIS`) at cell (16, 16) and the
/// computer Russians a heavy tank at (2, 2) aiming at cell (18, 18), inside
/// the sensor's 15 cells. Answers the world, the tank and the Americans.
#[cfg(any(test, feature = "test-support"))]
pub fn psychic_sensor_watch_fixture(rules: &RuleSet) -> (Simulation, u64, InternedId) {
    let mut sim = Simulation::with_seed(3);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let americans = sim.interner.intern("Americans");
    let russians = sim.interner.intern("Russians");
    for (id, side, human) in [(americans, 0, true), (russians, 1, false)] {
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id, side, None, human, 0, 10,
            ));
    }
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    sim.spawn_object_at_height("NAPOWR", "Americans", 10, 10, 0, 0, rules)
        .expect("the power plant stands");
    sim.spawn_object_at_height("NAPSIS", "Americans", 16, 16, 0, 0, rules)
        .expect("the sensor stands");
    let tank = sim
        .spawn_object_at_height("HTNK", "Russians", 2, 2, 0, 0, rules)
        .expect("the tank stands");
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), None, None, 33);
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Cell(18, 18)));
    sim.session.set_current_house_for_test(Some(americans));
    (sim, tank, americans)
}

#[cfg(test)]
#[path = "psychic_detection_tests.rs"]
mod tests;
