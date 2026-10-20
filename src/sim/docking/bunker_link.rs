//! Tank-bunker reciprocal link helpers (install / break / release trio).
//!
//! Owns both sides of the bunker link ([`BunkerLinks`]: the unit's link and the
//! building's occupant) plus the three distinct teardown helpers, the admission
//! predicate. Radio contacts own the pre-install approach.
//!
//! sim/ only — never render/ui/audio/net.
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::docking::bunker_install::BunkerRuntime;
use crate::sim::mission::authority::{EntityReadyInputProvider, LiveReadyInputProvider};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::radio::{RadioMessage, RadioPayload, transmit};
use crate::sim::world::FrameChannel;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use crate::util::fixed_math::SIM_ONE;

/// The installed unit side of the tank-bunker reciprocal link. Distinct from
/// `PassengerRole` cargo: a bunker is a single reciprocal link, never cargo.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum BunkerLink {
    /// Not installed in a bunker; an approach is held by radio contacts.
    #[default]
    None,
    /// Installed inside bunker `id` (reciprocal of the building's occupant).
    Installed(u64),
}

impl BunkerLink {
    /// The bunker this unit is installed in, if any.
    pub fn installed_in(self) -> Option<u64> {
        match self {
            BunkerLink::Installed(id) => Some(id),
            _ => None,
        }
    }
}

/// Both ends of the tank-bunker reciprocal link:
/// - `occupant`: the unit installed in a `Bunker=yes` building. It mirrors
///   the live `BuildingClass+0x2E4` role for tank bunkers: an empty bunker
///   can be skipped by the NumberImpassableRows helper, while an occupied
///   bunker remains a normal building blocker.
/// - `link`: the installed unit side ([`BunkerLink`]).
///
/// This module's install, break and release helpers write both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BunkerLinks {
    #[serde(default)]
    occupant: Option<u64>,
    #[serde(default)]
    link: BunkerLink,
}

impl BunkerLinks {
    /// The unit installed in this bunker.
    pub const fn occupant(self) -> Option<u64> {
        self.occupant
    }

    /// This unit's link to a bunker.
    pub const fn link(self) -> BunkerLink {
        self.link
    }

    /// Install a fixture's occupant.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_bunker_occupant_for_test(&mut self, occupant: Option<u64>) {
        self.occupant = occupant;
    }

    /// Install a fixture's link.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_bunker_link_for_test(&mut self, link: BunkerLink) {
        self.link = link;
    }
}

/// The tank bunker admission at `0x0070FB50`: the object's type is
/// `Bunkerable=` (`+0xD2E`) and has a `Turret=` (`+0xCA1`), its current
/// weapon (vt+0x3F4, `GetCurrentWeapon @ 0x0070E1A0`) has a type, its
/// `SpeedType=` (`+0x67C`) is not Hover (3), and a Foot is not being eaten by
/// a parasite (`+0x694`, `0x0070FBAF..0x0070FBC3`). For a Foot the weapon
/// test is [`crate::sim::combat::combat_weapon::is_armed`]'s.
///
/// Native callers: the bunker's radio receiver (`0x0043C512`, CAN_LOAD's
/// Bunker arm in [`crate::sim::radio::receive::can_load`]; `0x0043C86A`)
/// and `FootClass::EnterTankBunker` (`0x004DFF4F`,
/// [`Simulation::foot_enter_tank_bunker`]). The app's cursor and click
/// admission query the same receiver; the MEGAMISSION event runs its setters.
pub fn can_auto_deploy_here(sim: &Simulation, unit_id: u64, rules: &RuleSet) -> bool {
    let Some(unit) = sim.substrate.entities.get(unit_id) else {
        return false;
    };
    let Some(obj) = sim.object_type(unit.type_ref(), rules) else {
        return false;
    };
    obj.bunkerable
        && obj.has_turret
        && crate::sim::combat::combat_weapon::is_armed(unit, obj)
        && obj.speed_type != crate::rules::locomotor_type::SpeedType::Hover
        && unit.parasite.eater().is_none()
}

/// Bunker install state 5 (459301..459337): reciprocal links, deselection,
/// state 6, then Guard5/1. Unit virtual+150 is Object deselect5F44A0, not
/// Limbo: preserve coordinates, track, radio contacts and Logic membership.
/// Native +214=-1 also removes control-group membership; that owner currently
/// lives only in app input and is an explicit missing integration here.
pub fn install_bunker_link(sim: &mut Simulation, building_id: u64, unit_id: u64, rules: &RuleSet) {
    if !sim
        .substrate
        .entities
        .get(unit_id)
        .is_some_and(|unit| unit.category() == EntityCategory::Unit)
        || sim.substrate.entities.get(building_id).is_none()
    {
        return;
    }
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        b.bunker.occupant = Some(unit_id);
    }
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.bunker.link = BunkerLink::Installed(building_id);
        u.selection.deselect();
    }
    if let Some(rt) = sim
        .substrate
        .entities
        .get_mut(building_id)
        .and_then(|b| b.bunker_runtime.as_mut())
    {
        rt.occupy();
    }
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        unit_id,
        MissionId::from_known(MissionType::Guard),
        1,
        now,
        &LiveReadyInputProvider { rules },
    );
    emit_bunker_wall_sound(&sim.substrate, &mut sim.sound_events, building_id, true);
}

/// Clear BOTH sides of the link and send the radio BREAK. Returns the unit id
/// that was installed (for callers that re-place it). Does NOT reveal/place/anim.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn break_bunker_link(
    sim: &mut Simulation,
    building_id: u64,
    effects: FrameEffects<'_>,
) -> Option<u64> {
    let unit_id = sim.substrate.entities.get(building_id)?.bunker.occupant()?;
    // BREAK over the bus (clears any bus-level radio contact both ways).
    transmit(
        sim,
        building_id,
        unit_id,
        RadioMessage::Break,
        RadioPayload::default(),
        None,
        effects,
    );
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.bunker.link = BunkerLink::None;
    }
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        b.bunker.occupant = None;
    }
    Some(unit_id)
}

/// Emit the positional wall sound event (`up` = walls rising on install,
/// `false` = walls falling on a teardown). The app layer resolves the actual
/// sound id and skips it when the rules key is empty.
/// Played by `BuildingClass::UpdateBunker` (`0x00458E50`) state 5 (`0x0045936F`), and on
/// teardown by `EmptyBunker` (`0x004595C0`) and `ClearBunker` (`0x00459470`).
pub(crate) fn emit_bunker_wall_sound(
    substrate: &ObjectSubstrate,
    sound_events: &mut FrameChannel<SimSoundEvent>,
    building_id: u64,
    up: bool,
) {
    let Some(b) = substrate.entities.get(building_id) else {
        return;
    };
    let (rx, ry) = (b.position.rx(), b.position.ry());
    sound_events.emit(if up {
        SimSoundEvent::BunkerWallsUp { rx, ry }
    } else {
        SimSoundEvent::BunkerWallsDown { rx, ry }
    });
}

/// The head of `UnitClass::Mission_Unload @ 0x0073D630`
/// (`0x0073D63B..0x0073D66D`): a Unit linked into a tank bunker (`+0x2E4`)
/// empties the building on its own cell (GetOccupiedCell vt+0x1BC,
/// `Look_up_building_in_cell @ 0x0047C520`) through
/// `BuildingClass::EmptyBunker @ 0x004595C0` ([`release_normal`]). Returns
/// whether the unit was linked: its Unload then resumes at `0x0073D672`,
/// past the deploy-fire and transport arms.
pub(crate) fn unit_unload_empties_bunker(
    sim: &mut Simulation,
    unit_id: u64,
    rules: &RuleSet,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(unit) = sim
        .substrate
        .entities
        .get(unit_id)
        .filter(|unit| unit.bunker.link().installed_in().is_some())
    else {
        return false;
    };
    if let Some(building) = sim.substrate.occupancy.first_building_on_layer(
        unit.position.rx(),
        unit.position.ry(),
        crate::sim::movement::locomotor::MovementLayer::Ground,
    ) {
        release_normal(sim, building, rules, effects);
    }
    true
}

/// Normal release4595C0, reached by linked Unit Mission_Unload73D66D
/// ([`unit_unload_empties_bunker`]) and the bunker's own unload order.
/// Unlike sell/death, it clears the unit link before Power_On/Force, then
/// assigns a nearby destination ([`bunker_exit_cell`]) and queues Move before
/// clearing the building. The wall-animation projection remains a bounded
/// adapter; it is not a native execution comparison.
pub(crate) fn release_normal(
    sim: &mut Simulation,
    building_id: u64,
    rules: &RuleSet,
    effects: FrameEffects<'_>,
) {
    emit_bunker_wall_anim(sim, building_id, false, rules);
    emit_bunker_wall_sound(&sim.substrate, &mut sim.sound_events, building_id, false);
    let Some(unit_id) = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|building| building.bunker.occupant())
    else {
        reset_bunker_idle(&mut sim.substrate, building_id);
        queue_guard(sim, building_id);
        return;
    };
    if !is_release_unit(sim, unit_id) {
        return;
    }
    //4596E6 precedes Power_On; the building link survives until459814.
    if let Some(unit) = sim.substrate.entities.get_mut(unit_id) {
        unit.bunker.link = BunkerLink::None;
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "unit-link");
    power_and_force_release(sim, building_id, unit_id, Some(rules), effects);
    if let Some(cell) = bunker_exit_cell(sim, building_id, unit_id, rules) {
        let terrain = sim.resolved_terrain.as_ref();
        let frame = sim.session.binary_frame();
        if let Some(unit) = sim.substrate.entities.get_mut(unit_id) {
            crate::sim::movement::set_destination_internal_cell(unit, cell, terrain, frame);
        }
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "destination");
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        unit_id,
        MissionId::from_known(MissionType::Move),
        0,
        now,
        &EntityReadyInputProvider,
    );
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "move");
    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        building.bunker.occupant = None;
    }
    reset_bunker_idle(&mut sim.substrate, building_id);
    queue_guard(sim, building_id);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "building-link");
    break_first_contact(sim, building_id, effects);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "break");
}

/// Sell/death/Temporal release4593A0. The caller's reciprocal bunker link
/// gates Power_On, building-center(-128,+128,Z) Force_Track71, owner speed1,
/// unit-link clear, building-link clear, then BREAK to radio slot0, in that
/// order. It does not change coordinates/facing, queue Move, or reveal a unit.
/// Evidence: `.local/track-release-native.md` and bunker-release-native.txt.
pub(crate) fn release_sell_destroy(
    sim: &mut Simulation,
    building_id: u64,
    effects: FrameEffects<'_>,
) {
    let Some(unit_id) = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|building| building.bunker.occupant())
    else {
        return;
    };
    if !is_release_unit(sim, unit_id) {
        return;
    }
    power_and_force_release(sim, building_id, unit_id, None, effects);
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.bunker.link = BunkerLink::None;
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "unit-link");
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        b.bunker.occupant = None;
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "building-link");
    break_first_contact(sim, building_id, effects);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "break");
}

fn is_release_unit(sim: &Simulation, unit_id: u64) -> bool {
    sim.substrate
        .entities
        .get(unit_id)
        .is_some_and(|unit| unit.category() == EntityCategory::Unit)
}

/// `rules` feeds the forced head's crate pickup (`Force_Track` 0x4B0D1B);
/// the sell/destroy release has no rules at hand, so a crate on that head
/// stays until something else enters the cell (a bunker's exit cell, which
/// the crate placer never chooses while the bunker stands).
fn power_and_force_release(
    sim: &mut Simulation,
    building_id: u64,
    unit_id: u64,
    rules: Option<&RuleSet>,

    effects: FrameEffects<'_>,
) {
    let Some(unit) = sim.substrate.entities.get_mut(unit_id) else {
        return;
    };
    //4593CF/4596F4 signal E_POINTER for a Unit without its ILoco interface.
    unit.locomotor
        .as_mut()
        .expect("bunker release Unit requires a locomotor")
        .power_on();
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "power");
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return;
    };
    //4593F0..459407: the building's GetCoords, X - 128 and Y + 128, Z kept.
    let mut head = object_get_coords(building, sim.resolved_terrain.as_ref());
    head.x = head.x.wrapping_sub(128);
    head.y = head.y.wrapping_add(128);
    // No entity borrow spans Force's synchronous world receiver. Its bool
    // describes retained-track admission, not whether the caller continues.
    let _ = sim.force_track(unit_id, 0x47, head, rules, None, effects);
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "force");
    //45944A/45976F write this even after Force's null/limbo early return.
    // Re-resolve after its callback; never resurrect a removed receiver.
    if let Some(unit) = sim.substrate.entities.get_mut(unit_id) {
        unit.foot_speed.set_speed_fraction(SIM_ONE);
    }
    #[cfg(test)]
    release_tests::record(sim, building_id, unit_id, "speed");
}

/// Radio65ACB0 reads slot0, including an empty slot0 before later live slots.
/// It neither searches for a live contact nor substitutes the bunker occupant.
fn break_first_contact(sim: &mut Simulation, building_id: u64, effects: FrameEffects<'_>) {
    let contact = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|building| building.radio_contacts.slot(0));
    if let Some(contact) = contact {
        transmit(
            sim,
            building_id,
            contact,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
            effects,
        );
    }
}

/// `Queue_Mission(Guard, 0)` (vt+0x1E8) on the bunker, which leaves its
/// Repair mission at the next commence.
pub(super) fn queue_guard(sim: &mut Simulation, building_id: u64) {
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        building_id,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &EntityReadyInputProvider,
    );
}

/// `BuildingClass::ClearBunker @ 0x00459470`, for an occupant that left
/// without the release (the Chrono Warp, `0x006CC955`): anim slots 10 and 11
/// are cleared; with an occupant, `BunkerWallsDownSound=`, the wall-down slots
/// 12 and 13 (their damaged variants at or below ConditionYellow), BREAK to
/// radio slot 0 (vt+0x274(3), `0x0065ACB0`), both links cleared, the bunker
/// state reset and Guard queued. The occupant is not moved.
pub(crate) fn clear_bunker(
    sim: &mut Simulation,
    building_id: u64,
    rules: &RuleSet,
    effects: FrameEffects<'_>,
) {
    let occupant = sim
        .substrate
        .entities
        .get(building_id)
        .and_then(|b| b.bunker.occupant());
    let Some(unit_id) = occupant else {
        sim.clear_building_anim_slot(building_id, 10);
        sim.clear_building_anim_slot(building_id, 11);
        return;
    };
    emit_bunker_wall_anim(sim, building_id, false, rules);
    emit_bunker_wall_sound(&sim.substrate, &mut sim.sound_events, building_id, false);
    break_first_contact(sim, building_id, effects);
    if let Some(u) = sim.substrate.entities.get_mut(unit_id) {
        u.bunker.link = BunkerLink::None;
    }
    if let Some(b) = sim.substrate.entities.get_mut(building_id) {
        b.bunker.occupant = None;
    }
    reset_bunker_idle(&mut sim.substrate, building_id);
    queue_guard(sim, building_id);
}

/// Clear the installed reciprocal link when either object expires.
/// Techno PointerExpired707849 clears +2E4; the shared radio owner
/// separately expires the contact that owns an approach (Radio65AAC0).
pub fn break_links_on_despawn(substrate: &mut ObjectSubstrate, id: u64) {
    let Some(e) = substrate.entities.get(id) else {
        return;
    };
    let occupant = e.bunker.occupant();
    let host = e.bunker.link().installed_in();
    if let Some(unit_id) = occupant
        && let Some(u) = substrate.entities.get_mut(unit_id)
    {
        u.bunker.link = BunkerLink::None;
    }
    if let Some(building_id) = host
        && let Some(b) = substrate.entities.get_mut(building_id)
    {
        b.bunker.occupant = None;
        if let Some(rt) = b.bunker_runtime.as_mut() {
            *rt = BunkerRuntime::idle();
        }
    }
}

/// Reset the bunker runtime to empty/idle. Native zeroes the state (`+0x718`)
/// in `BuildingClass::EmptyBunker` (`0x004596C2`, `0x0045981A`), `ClearBunker`
/// (`0x0045959C`) and UpdateBunker's no-object arm (`0x00458E85`).
pub(super) fn reset_bunker_idle(substrate: &mut ObjectSubstrate, building_id: u64) {
    if let Some(b) = substrate.entities.get_mut(building_id)
        && let Some(rt) = b.bunker_runtime.as_mut()
    {
        *rt = BunkerRuntime::idle();
    }
}

/// The normal release's exit cell, `BuildingClass::EmptyBunker @ 0x004595C0`:
/// FNPC on g_Map (`0x004597E3`) seeded one cell west and one south of the
/// building's cell (`0x0045977E..0x00459794`). The pushes at
/// `0x0045979D..0x004597DD` are the plain shape: the occupant's `Type+0x67C`
/// SpeedType, zone -1, MovementZone Normal, 1x1, bridges allowed and a
/// zeroed target. A failed search is the zero cell, which the caller still
/// takes as the destination (`0x004597E8..0x004597FA`).
fn bunker_exit_cell(
    sim: &Simulation,
    building_id: u64,
    unit_id: u64,
    rules: &RuleSet,
) -> Option<(u16, u16)> {
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
    let building = sim.substrate.entities.get(building_id)?;
    let unit = sim.substrate.entities.get(unit_id)?;
    let speed_type = sim.object_type(unit.type_ref(), rules)?.speed_type;
    let seed = (
        i32::from((building.position.rx() as i16).wrapping_sub(1)),
        i32::from((building.position.ry() as i16).wrapping_add(1)),
    );
    Some(
        sim.nearby_passable_cell(
            seed,
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type,
                    required_zone_id: None,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::SINGLE,
            ),
        )
        .unwrap_or((0, 0)),
    )
}

/// Execute native wall-slot changes and record an observation. `up` raises the walls
/// (install), `false` drops them (teardown). The `damaged` flag picks the
/// `…Damaged` SpecialAnim variant when the building is at/below ConditionYellow
/// (`459254/4592A2/4594CF/459520/459614/459665` compare Rules+1700).
/// TEST AH,0x41 also selects the damaged variant for an unordered ratio.
pub(crate) fn emit_bunker_wall_anim(
    sim: &mut Simulation,
    building_id: u64,
    up: bool,
    rules: &RuleSet,
) {
    let Some(b) = sim.substrate.entities.get(building_id) else {
        return;
    };
    let Some(object) = sim.object_type(b.type_ref(), rules) else {
        return;
    };
    let damaged = at_or_below_condition_yellow(
        b.health.current(),
        object.strength,
        rules.general.condition_yellow,
    );
    crate::sim::world::building_anim::set_bunker_wall_slots(sim, rules, building_id, up, damaged);
    #[cfg(any(test, feature = "test-support"))]
    sim.bunker_wall_events
        .push(crate::sim::components::BunkerWallAnimEvent {
            building_id,
            up,
            damaged,
        });
}

/// Native masked PC53/chop ratio comparison, including unordered.
fn at_or_below_condition_yellow(current: i32, strength: i32, condition_yellow: f64) -> bool {
    use crate::util::native_x87::MaskedX87Ordering::{Equal, Less, Unordered};
    matches!(
        crate::sim::combat::Health::new(current).compare_ratio(strength, condition_yellow),
        Less | Equal | Unordered
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
    use crate::sim::docking::bunker_install::BunkerRuntime;
    use crate::sim::docking::bunker_install::BunkerState;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::MissionId;
    use crate::sim::world::RevealOutcome;

    pub(super) fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=NOGUN\n2=NOTURRET\n3=HOVER\n4=NOBUNKER\n\n\
             [InfantryTypes]\n0=GI\n\n[AircraftTypes]\n\n\
             [BuildingTypes]\n0=NATBNK\n\n\
             [TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\nPrimary=120mm\n\n\
             [NOGUN]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\n\n\
             [NOTURRET]\nStrength=400\nArmor=heavy\nSpeed=6\nPrimary=120mm\n\n\
             [HOVER]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\nPrimary=120mm\n\
             SpeedType=Hover\n\n\
             [NOBUNKER]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\nPrimary=120mm\n\
             Bunkerable=no\n\n\
             [GI]\nStrength=125\nSpeed=4\nTurret=yes\nPrimary=120mm\n\n\
             [NATBNK]\nStrength=1000\nArmor=heavy\nBunker=yes\n",
        ))
        .expect("rules parse")
    }

    fn spawn_bunker(sim: &mut Simulation, sid: u64, owner: &str) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern("NATBNK");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            10,
            10,
            0,
            0,
            owner_id,
            Health::new(1000),
            type_id,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.bunker_runtime = Some(BunkerRuntime::idle());
        sim.substrate.entities.insert(ge);
    }

    #[test]
    fn original_health_ratio_corpus_drives_bunker_wall_events() {
        for row in crate::sim::health_ratio_fixture::rows() {
            let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[BuildingTypes]\n0=NATBNK\n[NATBNK]\nStrength={}\nBunker=yes\n",
                row.input.strength
            )))
            .unwrap();
            rules.general.condition_yellow = row.input.yellow();
            let mut sim = Simulation::new();
            spawn_bunker(&mut sim, 1, "A");
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .health
                .set_for_test(row.input.current);
            for up in [false, true] {
                emit_bunker_wall_anim(&mut sim, 1, up, &rules);
                let event = sim.bunker_wall_events.last().unwrap();
                assert_eq!(event.building_id, 1);
                assert_eq!(event.up, up);
                assert_eq!(event.damaged, row.output.bunker_wall_damaged, "{row:?}");
            }
            assert_eq!(sim.bunker_wall_events.len(), 2);
        }
    }

    fn spawn_tank(sim: &mut Simulation, sid: u64, owner: &str, type_name: &str) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern(type_name);
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            12,
            12,
            0,
            0,
            owner_id,
            Health::new(400),
            type_id,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        ge.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        ge.locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        sim.substrate.entities.insert(ge);
    }

    /// Native `0x0070FB50` (`tools/team_recruit_oracle.py`'s `tank_bunker`
    /// rows, under Unicorn): `Bunkerable=` (a vehicle's default), `Turret=`,
    /// a current weapon, a SpeedType other than Hover and no parasite, each
    /// refusing alone. An infantryman is not `Bunkerable=` by default.
    #[test]
    fn can_auto_deploy_here_admits_as_native() {
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/team_recruit_oracle.json"))
                .unwrap();
        let mut cases = Vec::new();
        for row in oracle["tank_bunker"].as_array().unwrap() {
            let case = (row["admission"].clone(), row["admitted"] == 1);
            if !cases.contains(&case) {
                cases.push(case);
            }
        }
        assert_eq!(cases.len(), 6);
        let mut sim = Simulation::new();
        let rules = rules();
        for (id, (admission, admitted)) in (1..).zip(&cases) {
            // The fixture type with the row's one refusing input.
            let type_name = if admission["bunkerable"] == false {
                "NOBUNKER"
            } else if admission["turret"] == false {
                "NOTURRET"
            } else if admission["weapon"] == false {
                "NOGUN"
            } else if admission["speed_type"] == 3 {
                "HOVER"
            } else {
                "TANK"
            };
            spawn_tank(&mut sim, id, "Americans", type_name);
            if admission["parasite"] == true {
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .parasite
                    .set_eater_for_test(Some(99));
            }
            assert_eq!(
                can_auto_deploy_here(&sim, id, &rules),
                *admitted,
                "{admission}"
            );
        }
        spawn_tank(&mut sim, 7, "Americans", "GI");
        assert!(!can_auto_deploy_here(&sim, 7, &rules));
    }

    #[test]
    fn install_writes_both_sides_deselects_without_limbo_and_emits_up_sound() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_tank(&mut sim, 1, "Americans", "TANK");
        // Native deselection preserves this live membership.
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));

        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        install_bunker_link(&mut sim, 2, 1, &rules);

        let bunker = sim.substrate.entities.get(2).unwrap();
        assert_eq!(bunker.bunker.occupant(), Some(1));
        assert_eq!(
            bunker.bunker_runtime.unwrap().state(),
            BunkerState::Occupied
        );

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker.link(), BunkerLink::Installed(2));
        assert!(
            unit.in_logic_vector.is_member(),
            "install leaves the unit active"
        );
        assert!(unit.lifecycle.object_alive());
        assert!(!unit.lifecycle.in_limbo());
        assert!(unit.lifecycle.cell_marked());
        assert!(!unit.selection.is_selected());
        assert_eq!(
            unit.mission.current(),
            MissionId::from_known(MissionType::Guard)
        );

        let up = sim
            .sound_events
            .iter()
            .filter(|e| matches!(e, SimSoundEvent::BunkerWallsUp { .. }))
            .count();
        assert_eq!(up, 1, "exactly one walls-up sound on install");
    }

    #[test]
    fn break_clears_both_sides_and_returns_unit() {
        let mut sim = Simulation::new();
        let rules = rules();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_tank(&mut sim, 1, "Americans", "TANK");
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        install_bunker_link(&mut sim, 2, 1, &rules);

        let released = break_bunker_link(&mut sim, 2, crate::sim::world::FrameEffects::default());
        assert_eq!(released, Some(1));
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            None
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker.link(),
            BunkerLink::None
        );
    }

    pub(super) fn installed_sim() -> Simulation {
        let mut sim = Simulation::new();
        // Clear ground with its Size, playfield and zones: the release's exit
        // search runs on the map.
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        let rules = rules();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_tank(&mut sim, 1, "Americans", "TANK");
        assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
        install_bunker_link(&mut sim, 2, 1, &rules);
        sim.sound_events.clear_for_test(); // drop the install up-sound; assert on down events
        sim
    }

    /// Techno Assign_Target's bunker redirect (`0x006FCE4B..0x006FCEB5`): a
    /// bunkered tank becomes its bunker for an airstrike owner or an
    /// IsLocomotor weapon 0, not for an ordinary gun; the receiver itself
    /// becomes the Cell under it (`0x006FCEB8`).
    #[test]
    fn assign_target_redirects_a_bunkered_tank_to_its_bunker() {
        use crate::sim::combat::TargetKind;
        use crate::sim::mission::concrete_effects::assign_target_stored;
        let mut sim = installed_sim();
        let rules = rules();
        spawn_tank(&mut sim, 3, "Russians", "TANK");
        spawn_tank(&mut sim, 4, "Russians", "MAGNET");
        let magnet_rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MAGNET\n\n[InfantryTypes]\n\n[AircraftTypes]\n\n\
             [BuildingTypes]\n\n[MAGNET]\nStrength=400\nPrimary=MagBeam\n\n\
             [WeaponTypes]\n0=MagBeam\n\n[MagBeam]\nDamage=1\nRange=5\nWarhead=Mag\n\n\
             [Warheads]\n0=Mag\n\n[Mag]\nIsLocomotor=yes\n",
        ))
        .unwrap();
        let tank = Some(TargetKind::Entity(1));
        let stored = |sim: &Simulation, receiver, rules| {
            assign_target_stored(
                &sim.substrate.entities,
                sim.terrain_objects.objects(),
                receiver,
                tank,
                Some((rules, &sim.interner)),
            )
        };
        assert_eq!(stored(&sim, 3, &rules), tank);
        assert_eq!(stored(&sim, 4, &magnet_rules), Some(TargetKind::Entity(2)));
        sim.substrate
            .entities
            .get_mut(3)
            .unwrap()
            .airstrike
            .set_test_link(Some(3));
        assert_eq!(stored(&sim, 3, &rules), Some(TargetKind::Entity(2)));
        let own = assign_target_stored(
            &sim.substrate.entities,
            sim.terrain_objects.objects(),
            3,
            Some(TargetKind::Entity(3)),
            None,
        );
        assert_eq!(own, Some(TargetKind::Cell(12, 12)));
        // The same-target test compares the request, so asking for the tank
        // again while the bunker is held stores the bunker again.
        for _ in 0..2 {
            let held = stored(&sim, 3, &rules);
            let receiver = sim.substrate.entities.get_mut(3).unwrap();
            crate::sim::mission::concrete_effects::represented_assign_target_admitted(
                receiver, tank, held,
            );
            assert_eq!(receiver.attack_target.target(), Some(TargetKind::Entity(2)));
        }
        // An object only approaching its bunker (no +0x2E4) is not redirected.
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::None);
        assert_eq!(stored(&sim, 3, &rules), tank);
    }

    fn down_sounds(sim: &Simulation) -> usize {
        sim.sound_events
            .iter()
            .filter(|e| matches!(e, SimSoundEvent::BunkerWallsDown { .. }))
            .count()
    }

    #[test]
    fn release_normal_forces_without_teleporting_queues_move_and_plays_down_sound() {
        let mut sim = installed_sim();
        release_normal(
            &mut sim,
            2,
            &rules(),
            crate::sim::world::FrameEffects::default(),
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker.link(), BunkerLink::None);
        assert!(unit.in_logic_vector.is_member(), "unit revealed");
        assert!(unit.lifecycle.object_alive());
        assert!(!unit.lifecycle.in_limbo());
        assert!(unit.lifecycle.cell_marked());
        assert_eq!((unit.position.rx(), unit.position.ry()), (12, 12));
        // FNPC's seed, one cell west and south of the bunker, is clear and
        // supplies NavCom; Force moves later.
        assert_eq!(
            unit.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::cell(9, 11))
        );
        assert_eq!(
            unit.locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .unwrap()
                .track()
                .turn_index(),
            0x47
        );
        // Release queues Move (the host's Ready→Commence promotes it next pass).
        assert_eq!(
            unit.mission.queued(),
            MissionId::from_known(MissionType::Move)
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            None
        );
        assert_eq!(down_sounds(&sim), 1, "one walls-down on normal eject");
    }

    /// `EmptyBunker`'s exit search is FNPC (`0x004597E3`): CellRect passability
    /// reads the live raw occupation byte with both ignore flags clear
    /// (`0x0056E854`, `0x0056E856`), so a raw bit no object list shows still
    /// blocks a cell. With the seed and seven of its eight first-ring cells
    /// marked, the one clear ring cell is the exit.
    #[test]
    fn release_normal_exit_skips_raw_occupied_cells_around_the_seed() {
        let mut sim = installed_sim();
        let seed = (9u16, 11u16);
        let exit = (10u16, 12u16);
        for dx in -1i32..=1 {
            for dy in -1i32..=1 {
                let cell = (
                    (i32::from(seed.0) + dx) as u16,
                    (i32::from(seed.1) + dy) as u16,
                );
                if cell != exit {
                    sim.substrate
                        .raw_cell_occupation
                        .mark_ground(cell.0, cell.1, 0x01);
                }
            }
        }
        release_normal(
            &mut sim,
            2,
            &rules(),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::cell(exit.0, exit.1))
        );
    }

    #[test]
    fn release_sell_destroy_forces_without_reposition_no_move_no_sound() {
        let mut sim = installed_sim();
        release_sell_destroy(&mut sim, 2, crate::sim::world::FrameEffects::default());
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker.link(), BunkerLink::None);
        assert!(unit.in_logic_vector.is_member(), "unit remains active");
        assert!(unit.lifecycle.object_alive());
        assert!(!unit.lifecycle.in_limbo());
        assert!(unit.lifecycle.cell_marked());
        assert_eq!((unit.position.rx(), unit.position.ry()), (12, 12));
        assert_eq!(unit.body_facing.destination(), 0);
        assert_eq!(
            unit.locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .unwrap()
                .track()
                .turn_index(),
            0x47
        );
        assert_eq!(
            unit.mission.current(),
            MissionId::from_known(MissionType::Guard),
            "no Move order"
        );
        assert_eq!(down_sounds(&sim), 0, "sell teardown is silent");
    }

    #[test]
    fn clear_bunker_plays_down_sound_but_does_not_reposition() {
        let mut sim = installed_sim();
        clear_bunker(
            &mut sim,
            2,
            &rules(),
            crate::sim::world::FrameEffects::default(),
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker.link(), BunkerLink::None);
        assert!(
            unit.in_logic_vector.is_member(),
            "clear preserves live membership"
        );
        assert!(unit.lifecycle.object_alive());
        assert!(!unit.lifecycle.in_limbo());
        assert!(unit.lifecycle.cell_marked());
        assert_eq!(
            (unit.position.rx(), unit.position.ry()),
            (12, 12),
            "not repositioned"
        );
        assert_eq!(down_sounds(&sim), 1, "one walls-down on clear");
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            None
        );
    }

    #[test]
    fn despawn_safety_net_clears_surviving_side() {
        // Building despawns → the occupant unit's link is cleared.
        let mut sim = installed_sim();
        break_links_on_despawn(&mut sim.substrate, 2);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker.link(),
            BunkerLink::None
        );

        // Unit despawns → the surviving building's back-pointer is cleared.
        let mut sim = installed_sim();
        break_links_on_despawn(&mut sim.substrate, 1);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().bunker.occupant(),
            None
        );
    }

    #[test]
    fn uninit_bunker_clears_occupant_back_link() {
        // The despawn safety net runs inside uninit.
        let mut sim = installed_sim();
        sim.uninit(2);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker.link(),
            BunkerLink::None
        );
    }

    fn down_anim_events(sim: &Simulation) -> usize {
        sim.bunker_wall_events.iter().filter(|e| !e.up).count()
    }

    #[test]
    fn release_normal_emits_one_walls_down_anim_event() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        release_normal(
            &mut sim,
            2,
            &rules(),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            down_anim_events(&sim),
            1,
            "one walls-down anim on normal eject"
        );
        let ev = sim.bunker_wall_events.iter().find(|e| !e.up).unwrap();
        assert_eq!(ev.building_id, 2);
        assert!(
            !ev.damaged,
            "full-health bunker uses the non-damaged variant"
        );
    }

    #[test]
    fn clear_bunker_emits_one_walls_down_anim_event() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        clear_bunker(
            &mut sim,
            2,
            &rules(),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(down_anim_events(&sim), 1, "one walls-down anim on clear");
    }

    #[test]
    fn release_sell_destroy_emits_no_anim_event() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        release_sell_destroy(&mut sim, 2, crate::sim::world::FrameEffects::default());
        assert!(
            sim.bunker_wall_events.is_empty(),
            "sell/destroy teardown emits no wall anim (UndockUnit)"
        );
    }

    #[test]
    fn walls_down_damaged_flag_set_below_condition_yellow() {
        let mut sim = installed_sim();
        sim.bunker_wall_events.clear();
        // Between native ConditionRed25% and ConditionYellow50%: walls use yellow.
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .health
            .set_for_test(400);
        release_normal(
            &mut sim,
            2,
            &rules(),
            crate::sim::world::FrameEffects::default(),
        );
        let ev = sim.bunker_wall_events.iter().find(|e| !e.up).unwrap();
        assert!(
            ev.damaged,
            "below-ConditionYellow bunker selects the damaged variant"
        );
    }

    #[test]
    fn sell_occupied_bunker_starts_release_track_without_teleport() {
        let mut rules = rules();
        rules.set_buildup_control_for_test("NATBNK", [0, 25, 2]);
        let mut sim = installed_sim();
        assert!(crate::sim::production::sell_building_now_for_test(
            &mut sim, &rules, 2
        ));
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.bunker.link(), BunkerLink::None);
        assert!(
            unit.in_logic_vector.is_member(),
            "occupant remains active after sell"
        );
        assert_eq!((unit.position.rx(), unit.position.ry()), (12, 12));
        assert_eq!(
            unit.locomotor
                .as_ref()
                .and_then(|loco| loco.selected_drive_runtime())
                .and_then(|runtime| runtime.retained())
                .unwrap()
                .track()
                .turn_index(),
            0x47
        );
        assert_eq!(down_sounds(&sim), 0, "sell is silent (UndockUnit)");
    }
}

#[cfg(test)]
#[path = "bunker_release_tests.rs"]
mod release_tests;
