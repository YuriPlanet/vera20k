//! Tests for the combat system — weapon firing, damage, and entity death.
//!
//! Extracted from combat.rs to keep it under the 400-line limit.

use std::collections::BTreeMap;

use super::*;

/// Fixture input for an ordinary weapon-backed constructor plus SetWeapon.
/// Production callers provide these independent fields at their native sites.
pub(crate) fn payload_from_weapon(
    damage: i32,
    warhead: crate::sim::intern::InternedId,
    weapon: crate::sim::intern::InternedId,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &mut crate::sim::intern::StringInterner,
) -> crate::sim::projectile::ProjectilePayload {
    let kind = rules.weapon(interner.resolve(weapon));
    let projectile = kind
        .and_then(|kind| kind.projectile.as_deref())
        .map(|id| interner.intern(id));
    crate::sim::projectile::ProjectilePayload::new(
        damage,
        warhead,
        projectile,
        kind.is_some_and(|kind| kind.bright),
    )
    .with_installed_weapon(kind.map(|_| weapon))
}
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, test_intern, test_interner};
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::power_system::PowerState;
use crate::sim::projectile::ProjectileDetonationReason;
use crate::sim::rng::SimRng;
use crate::sim::vision::FogState;
use crate::util::save_codec;

#[path = "building_damage_sound_tests.rs"]
mod building_damage_sound_tests;
#[path = "infantry_fire_facing_tests.rs"]
mod infantry_fire_facing_tests;
#[path = "voice_feedback_tests.rs"]
mod voice_feedback_tests;

/// Establish the supplied fixture's Cell lists through shared Mark, in
/// ascending fixture ID order. Acquisition no longer synthesizes these lists.
fn mark_fixture_entities(store: &mut EntityStore) -> OccupancyGrid {
    let mut sim = crate::sim::world::Simulation::new();
    *sim.interner = test_interner();
    *sim.substrate.entities = std::mem::take(store);
    let ids: Vec<_> = sim
        .substrate
        .entities
        .values()
        .map(GameEntity::stable_id)
        .collect();
    for id in ids {
        sim.add_entity_occupancy(id);
    }
    *store = sim.substrate.entities.into_inner();
    sim.substrate.occupancy.into_inner()
}

/// Build a minimal RuleSet for combat testing.
fn test_rules() -> RuleSet {
    let ini_str: &str = "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\n\n\
[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[SA]\nTiberium=yes\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
[AP]\nTiberium=yes\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n";
    let ini: IniFile = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("test rules should parse")
}

mod announcements_tests;
mod damage_commit_tests;
mod damage_response_tests;
mod death_effects_tests;
mod fire_path_tests;
mod overlays_tests;
mod projectiles_tests;
mod radiation_tests;
mod rank_and_multipliers_tests;
mod retaliation_tests;
mod shrapnel_tests;

fn infantry_fire_frame_rules() -> RuleSet {
    infantry_fire_frame_rules_with("")
}

/// [`infantry_fire_frame_rules`] with `patch` loaded over its rules INI.
fn infantry_fire_frame_rules_with(patch: &str) -> RuleSet {
    let mut rules_ini: IniFile = IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nImage=GI\nPrimary=M60\nSecondary=Para\nDeployFire=yes\n\n\
[E2]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nReport=GIAttack\nOccupantAnim=UCFLASH\n\n\
[Para]\nDamage=40\nROF=15\nRange=5\nWarhead=AP\nReport=GIAttackDeployed\nOccupantAnim=UCFLASH\n\n\
[SA]\nVerses=100%,100%,100%,90%,70%,0%,100%,25%,25%,0%,0%\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    );
    rules_ini.merge(&IniFile::from_str(patch));
    // Supplied reader-built records make native action admission explicit.
    // SecondaryFire/SecondaryProne are authored controls, not stock GI ART.
    let art_ini = IniFile::from_str(
        "[GI]\nSequence=GISequence\nCrawls=yes\nFireUp=2\nFireProne=3\nSecondaryFire=4\nSecondaryProne=5\n\
         [GISequence]\nReady=0,1,1\nGuard=0,1,1\nProne=86,1,6\nWalk=8,6,6\nFireUp=164,6,6\n\
         FireProne=212,6,6\nDie1=56,15,0\nDeployed=292,1,0\nDeployedFire=292,6,0\n\
         SecondaryFire=164,6,6\nSecondaryProne=212,6,6\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini)
        .expect("infantry rules should parse");
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.install_art_data(art);
    rules
}

/// Point every fixture attacker at its own target.
///
/// `GameEntity::test_default` spawns facing north (0) with no turret, and the
/// fixture rules author no `Turret=`, so every fixture attacker is a TURRETLESS
/// vehicle. Under the native body gate — `UnitClass::GetFireError @ 0x00740FD0`
/// step 17, which compares the HULL `+0x388` when `Turret=` is unset — such a
/// unit spends its first ticks turning through `Fire_At_Target @ 0x00736DF0`
/// case 2 instead of shooting. Tests whose subject is damage, RNG consumption or
/// resolution ordering pre-align here so they still observe the shot on the
/// single tick they run; the turn-to-fire behaviour itself is covered by
/// `combat_turret_facing_tests`.
fn align_attackers_to_targets(store: &mut EntityStore) {
    for id in store.keys_sorted() {
        let desired = {
            let Some(entity) = store.get(id) else {
                continue;
            };
            let Some(attack) = entity.attack_target.target() else {
                continue;
            };
            // Author this phase fixture's guaranteed Techno/Cell aim through
            //the existing pure coordinate and facing kernels. This helper
            //does not stand in for the production Object target resolver.
            let coords = match attack {
                TargetKind::Entity(target) => store
                    .get(target)
                    .map(crate::sim::combat::attack_target::target_coords),
                TargetKind::Cell(rx, ry) => Some(cell_center_coords(rx, ry)),
            };
            let Some((rx, ry, sub_x, sub_y)) = coords else {
                continue;
            };
            crate::sim::movement::turret::facing_toward_lepton(
                entity.position.rx(),
                entity.position.ry(),
                entity.position.sub_x(),
                entity.position.sub_y(),
                rx,
                ry,
                sub_x,
                sub_y,
            )
        };
        if let Some(entity) = store.get_mut(id) {
            entity.body_facing.snap(desired & 0xFF00, 0);
            if let Some(ref mut barrel) = entity.barrel_facing {
                barrel.snap(desired, 0);
            }
        }
    }
}

fn make_entity(id: u64, type_ref: &str, rx: u16, ry: u16, hp: i32) -> GameEntity {
    let mut e = GameEntity::test_default(id, type_ref, "Test", rx, ry);
    e.health = Health::new(hp);
    e.lifecycle.set_in_limbo_for_test(false);
    e
}

fn make_entity_owned(
    id: u64,
    type_ref: &str,
    rx: u16,
    ry: u16,
    hp: i32,
    owner: &str,
) -> GameEntity {
    let mut e = GameEntity::test_default(id, type_ref, owner, rx, ry);
    e.health = Health::new(hp);
    e.lifecycle.set_in_limbo_for_test(false);
    e
}

/// [`make_entity`] carrying `owner` and `type_ref` from the test's own
/// interner, marked into the cell lists.
fn marked_entity(
    id: u64,
    type_name: &str,
    rx: u16,
    ry: u16,
    hp: i32,
    owner: InternedId,
    type_ref: InternedId,
) -> GameEntity {
    let mut entity = make_entity(id, type_name, rx, ry, hp);
    entity.set_owner_for_test(owner);
    entity.set_type_ref_for_test(type_ref);
    entity.lifecycle.set_cell_marked_for_test(true);
    entity
}

/// One [`tick_combat`] pass at `current_tick` on empty cell lists: a 100 ms
/// tick in binary frame 0.
fn tick(
    store: &mut EntityStore,
    rules: &RuleSet,
    interner: &mut StringInterner,
    current_tick: u64,
    scenario_rng: &mut SimRng,
) -> CombatTickResult {
    tick_combat(
        store,
        &mut OccupancyGrid::new(),
        rules,
        interner,
        current_tick,
        100,
        0,
        scenario_rng,
    )
}

/// [`tick`] at tick 0 over the given map layers, in the given live object
/// order.
fn tick_with_map(
    store: &mut EntityStore,
    rules: &RuleSet,
    interner: &mut StringInterner,
    overlays: Option<&mut OverlayGrid>,
    registry: Option<&OverlayTypeRegistry>,
    terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    live_order: &[u64],
    scenario_rng: &mut SimRng,
) -> CombatTickResult {
    tick_combat_with_fog(
        store,
        &mut OccupancyGrid::new(),
        rules,
        interner,
        None,
        &BTreeMap::new(),
        None,
        overlays,
        registry,
        terrain,
        0,
        100,
        0,
        live_order,
        None,
        scenario_rng,
    )
}

/// One [`tick_combat_with_fog_and_main_rng`] pass in binary `frame` (also its
/// tick) that lands `detonations`, with rule handles resolved as sim init
/// does and no fog, power, sound, map layers, House order or hooks.
fn tick_detonations(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    houses: &mut crate::sim::houses::Houses,
    alliances: &HouseAllianceMap,
    frame: u32,
    live_order: &[u64],
    detonations: &[ProjectileDetonation],
    scenario_rng: &mut SimRng,
    main_rng: &mut SimRng,
) -> CombatTickResult {
    let handles = crate::sim::type_handle_table::ResolvedRuleHandles::resolve(rules, interner);
    tick_combat_with_fog_and_main_rng(
        entities,
        occupancy,
        rules,
        interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        houses,
        alliances,
        None,
        None,
        None,
        None,
        u64::from(frame),
        100,
        frame,
        live_order,
        detonations,
        None,
        scenario_rng,
        main_rng,
        None,
    )
}

/// [`emit_projectile_detonations`] of `detonations` on empty cell lists, with
/// rule handles resolved as sim init does and no map layers or alliances:
/// what they emitted.
fn emit_detonations(
    detonations: &[ProjectileDetonation],
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &mut StringInterner,
    scenario_no_damage: bool,
    scenario_rng: &mut SimRng,
    mut hooks: Option<&mut FixtureTrace>,
) -> CombatEmit {
    let handles = crate::sim::type_handle_table::ResolvedRuleHandles::resolve(rules, interner);
    let mut emitted = CombatEmit::default();
    emit_projectile_detonations(
        detonations,
        entities,
        &OccupancyGrid::new(),
        rules,
        interner,
        Some(handles),
        None,
        None,
        None,
        None,
        None,
        None,
        scenario_no_damage,
        &HouseAllianceMap::new(),
        scenario_rng,
        &mut hooks,
        &mut emitted,
    );
    emitted
}

/// [`commit_damage_events`] on empty cell lists with no map layers, sound or
/// alliances, from Main seed 5 and Scenario seed 7.
fn commit_hits(
    events: &[EntityDamageEvent],
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &mut StringInterner,
    houses: &mut crate::sim::houses::Houses,
    current_tick: u64,
    mut hooks: Option<&mut FixtureTrace>,
) {
    let _ = commit_damage_events(
        events,
        entities,
        &mut OccupancyGrid::new(),
        rules,
        interner,
        houses,
        &HouseAllianceMap::new(),
        &mut SimRng::new(5),
        &mut SimRng::new(7),
        &mut Vec::new(),
        None,
        None,
        None,
        current_tick,
        &mut hooks,
        &mut None,
    );
}

/// Put `entity` on `current` with `suspended` behind it; its queue, handler
/// state and dispatch timer at rest.
fn set_mission(entity: &mut GameEntity, current: MissionType, suspended: MissionId) {
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(current),
        suspended,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
}

/// Computer houses with no country or credits at tech level 10, each on the
/// side its position in `owners` gives.
fn computer_houses(owners: &[InternedId]) -> crate::sim::houses::Houses {
    let mut houses = crate::sim::houses::Houses::default();
    register_computer_houses(&mut houses, owners);
    houses
}

/// Register [`computer_houses`] in `owners` order.
fn register_computer_houses(houses: &mut crate::sim::houses::Houses, owners: &[InternedId]) {
    for (side, &owner) in owners.iter().enumerate() {
        houses.register(HouseState::new(owner, side as u8, None, false, 0, 10));
    }
}

fn make_infantry_entity(id: u64, type_ref: &str, rx: u16, ry: u16, hp: i32) -> GameEntity {
    let mut e = make_entity(id, type_ref, rx, ry, hp);
    e.set_category_for_test(EntityCategory::Infantry);
    e.mission_leaf =
        crate::sim::mission::leaf::MissionLeafState::for_entity_category(EntityCategory::Infantry);
    e.set_is_voxel_for_test(false);
    e.animation = None;
    e.infantry = Some(crate::sim::infantry::InfantryRuntime::new());
    e
}

fn make_structure_entity(
    id: u64,
    type_ref: &str,
    rx: u16,
    ry: u16,
    current: i32,
    max: i32,
) -> GameEntity {
    let mut entity = make_entity(id, type_ref, rx, ry, max);
    entity.set_category_for_test(EntityCategory::Structure);
    entity.set_is_voxel_for_test(false);
    entity.health = Health::new(current);
    entity
}

/// `damage_events` holds each killing call's record; an infantryman's death
/// arm reads its warhead's InfDeath.
fn run_combat_death_handoff(
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &mut crate::sim::intern::StringInterner,
    dead_entities: &[u64],
    damage_events: &[EntityDamageEvent],
) -> DeathEffects {
    let mut occupancy = OccupancyGrid::new();
    let mut houses = crate::sim::houses::Houses::default();
    let mut main_rng = SimRng::new(0);
    let mut scenario_rng = SimRng::new(0);
    let mut handled_deaths = Vec::new();
    let handles = crate::sim::type_handle_table::ResolvedRuleHandles::resolve(rules, interner);
    handle_entity_deaths(
        entities,
        &mut occupancy,
        rules,
        interner,
        Some(handles),
        &mut houses,
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        dead_entities,
        damage_events,
        None,
        None,
        None,
        &mut None,
        false,
        0,
        &mut None,
        &mut None,
    )
}

/// A receiver-only visit does not advance the AI's absolute Stage clock.
fn set_infantry_stage(store: &mut EntityStore, id: u64, stage: i32) {
    store.get_mut(id).unwrap().set_native_stage_value(stage);
}

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::overlay_grid::{OverlayGrid, WallPointerTarget};
use crate::sim::world::Simulation;

/// The app loader publishes the canonical AnimType read receipt through this
/// owner before combat. These phase fixtures supply no ART/SHP, so registered
/// AnimList types retain real constructor-only End0 state; they still create
/// AnimClass objects. Do not fabricate spawn observations or loaded frames.
fn initialize_fixture_anim_types(rules: &mut RuleSet) {
    rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
}

#[cfg(not(feature = "phobos"))]
fn constructed_anim_coord(
    anim: &super::receiver_fixture::ConstructedAnimObservation,
) -> (u16, u16, SimFixed, SimFixed) {
    let p = anim.world_coord;
    (
        p.x.div_euclid(256) as u16,
        p.y.div_euclid(256) as u16,
        SimFixed::from_num(p.x.rem_euclid(256)),
        SimFixed::from_num(p.y.rem_euclid(256)),
    )
}

fn flat_level_zero_terrain(
    width: u16,
    height: u16,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(width, height, |rx, ry| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            filled_clear: true,
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_clear_cell(rx, ry)
        }
    })
}
