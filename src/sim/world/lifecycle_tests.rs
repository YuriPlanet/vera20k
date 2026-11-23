//! Focused regression tests for ordered lifecycle authority.

use std::collections::BTreeMap;

use crate::map::bridge_facts::{
    BRIDGE_FLAG_ANCHOR_SELF, BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts, BridgeStampFamily,
};
use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::rule_policy::RulePolicy;
use crate::rules::terrain_rules::SpeedCostProfile;
use crate::sim::anim_class::{AnimObject, AnimRuntime, AnimWorldCoord};
use crate::sim::combat::{Health, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::locomotor::{AirMovePhase, LocomotorState, MovementLayer};
use crate::sim::occupancy::CellListInsertion;
use crate::sim::particles::ParticleSystem;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectileGuidance, ProjectilePayload,
    ProjectileSpawn, ProjectileTarget, ProjectileTrajectory, ProjectileVelocity,
    ProjectileVisualState, TargetExpiryPolicy, cell_target_coord,
};
use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};
use crate::sim::wave::{Wave, WaveRecordedCell};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF64Bits;
use crate::util::save_codec;
use glam::IVec3;

use super::techno_ai::ObjectAiCtx;
use super::{ConcealOutcome, DeferredDeleteQueue, DeferredDeleteTicket, FrameEffects, TickLane};
use super::{
    LifecycleOutput, LifecycleTestEvent, PlacementEvidence, RevealFailure, RevealOutcome,
    RevealPosition, RevealRequest, Simulation,
};
use super::{UninitContext, bridge_orchestrator, display_layers, lifecycle, techno_ai};

mod authority_order_tests;
mod base_reservation_tests;
mod detach_and_ownership_tests;
mod display_and_layers_tests;
mod pointer_expiry_tests;
mod projectile_logic_tests;
mod raw_occupation_tests;
mod reveal_placement_tests;
mod score_stats_tests;
mod teardown_tests;
mod wave_tests;

pub(super) fn insert_entity(sim: &mut Simulation, stable_id: u64, category: EntityCategory) {
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("TEST");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        2,
        3,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        category,
        0,
        5,
        category != EntityCategory::Infantry,
    );
    entity.set_category_for_test(category);
    sim.substrate.entities.insert(entity);
}

/// An Infantry fixture on the Walk locomotor, whose Limbo hook clears his
/// sub-cell.
pub(super) fn insert_walker(sim: &mut Simulation, stable_id: u64) {
    insert_entity(sim, stable_id, EntityCategory::Infantry);
    sim.substrate.entities.get_mut(stable_id).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
}

fn insert_reservation_building(
    sim: &mut Simulation,
    stable_id: u64,
    owner_name: &str,
    rx: u16,
    ry: u16,
    foundation: &crate::rules::foundation::Foundation,
    spacing: i32,
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(owner_name);
    sim.houses.get_or_register_for_test(owner, || {
        HouseState::new(
            owner,
            0,
            None,
            owner_name.eq_ignore_ascii_case("Americans"),
            0,
            10,
        )
    });
    let type_ref = sim.interner.intern(&format!("BUILDING{stable_id}"));
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        rx,
        ry,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    entity
        .seldom
        .type_facts
        .set_foundation_for_test(foundation.clone());
    entity
        .seldom
        .type_facts
        .set_base_reservation_spacing_for_test(Some(spacing));
    sim.substrate.entities.insert(entity);
    owner
}

fn request(rx: u16, ry: u16, placement: PlacementEvidence) -> RevealRequest {
    RevealRequest {
        position: RevealPosition::new(
            rx,
            ry,
            2,
            None,
            SimFixed::from_num(128),
            SimFixed::from_num(64),
        ),
        placement,
        logic_eligible: true,
    }
}

fn receiver_authority_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[InfantryTypes]\n0=TEST\n[BuildingTypes]\n0=BUILDING41\n\
         [TEST]\nStrength=100\nArmor=light\n\
         [BUILDING41]\nStrength=100\nArmor=concrete\n\
         [Warheads]\n0=KILLWH\n[KILLWH]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("receiver world-authority rules")
}

fn commit_receiver_authority_lethal_hit(sim: &mut Simulation, target_id: u64) {
    let rules = receiver_authority_rules();
    let warhead = sim.interner.intern("KILLWH");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        target_id,
        100,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[hit],
        &mut crate::sim::world::FrameEffects::default(),
    );
}

fn common_raw_request(rx: u16, ry: u16, z: u8, sub_x: i32, sub_y: i32) -> RevealRequest {
    RevealRequest {
        position: RevealPosition::new(
            rx,
            ry,
            z,
            None,
            SimFixed::from_num(sub_x),
            SimFixed::from_num(sub_y),
        ),
        placement: PlacementEvidence::MarkSucceeded,
        logic_eligible: true,
    }
}

/// A retail Clear cell at `level`, optionally under a structural bridge deck.
pub fn common_raw_terrain_cell(
    rx: u16,
    ry: u16,
    level: u8,
    has_bridge_deck: bool,
) -> ResolvedTerrainCell {
    let bridge_facts = if has_bridge_deck {
        BridgeCellFacts {
            raw_flags: BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_STRUCTURAL,
            family: BridgeStampFamily::Nesw,
            direction: Some(0),
            ..BridgeCellFacts::default()
        }
    } else {
        BridgeCellFacts::default()
    };
    ResolvedTerrainCell {
        level,
        template_height: level,
        has_bridge_deck,
        bridge_walkable: has_bridge_deck,
        bridge_deck_level: if has_bridge_deck {
            level.saturating_add(4)
        } else {
            level
        },
        bridge_facts,
        ..crate::map::resolved_terrain::test_retail_clear_cell(rx, ry)
    }
}

pub(super) fn install_common_raw_terrain(
    sim: &mut Simulation,
    width: u16,
    height: u16,
    level: u8,
    bridge_cell: Option<(u16, u16)>,
) {
    let cells = (0..height)
        .flat_map(|ry| {
            (0..width).map(move |rx| {
                common_raw_terrain_cell(rx, ry, level, bridge_cell == Some((rx, ry)))
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    terrain.set_land_speed_rules_for_test(&crate::map::resolved_terrain::test_retail_clear_rules());
    terrain.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1500),
    );
    sim.resolved_terrain = Some(terrain);
}

/// A Fly aircraft in limbo holding `target_height`. A Reveal without rules
/// commits no exact Z, so a caller lifts it afterwards through SetHeight.
fn install_fly_aircraft(sim: &mut Simulation, stable_id: u64, target_height: SimFixed) {
    insert_entity(sim, stable_id, EntityCategory::Aircraft);
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Fly);
    locomotor.set_fly_target_height(target_height.to_num::<i32>());
    // Process steps the height of a moving Fly only (`0x004CDA0B`); a null
    // destination keeps Horizontal_Step and the landing drift out of it.
    locomotor.fly_runtime_mut().unwrap().retain_destination(
        crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 },
        None,
        || 0,
    );
    sim.substrate
        .entities
        .get_mut(stable_id)
        .expect("aircraft")
        .locomotor = Some(locomotor);
}

fn insert_anim(sim: &mut Simulation, stable_id: u64, inactive: bool) {
    let type_id = sim.interner.intern("TESTANIM");
    let anim = AnimObject::new_unattached_for_test(
        stable_id,
        stable_id as i32,
        type_id,
        AnimWorldCoord { x: 0, y: 0, z: 0 },
        AnimRuntime {
            current_frame: 0,
            frame_step: 1,
            delay_remaining: 0,
            rate_reload: 1,
            frame_timer: crate::sim::timer::CdTimer::started(0, 1),
            loop_remaining: 0,
            first_ai_guard: false,
            constructor_reverse: false,
            inactive,
            paused: false,
            temporal_paused: false,
        },
    );
    assert!(sim.substrate.anims.insert(anim).is_none());
}

fn insert_particle_system(sim: &mut Simulation, stable_id: u64) {
    sim.substrate.particle_systems.insert({
        let mut system = ParticleSystem::test_fixture(
            stable_id,
            crate::rules::particle_system_type::ParticleSystemTypeId(0),
            IVec3::ZERO,
        );
        system.set_facing_for_test(0);
        system
    });
}

pub(super) fn gsi_05_02_projectile(source_id: u64, fuse_frames: Option<u16>) -> ProjectileSpawn {
    ProjectileSpawn {
        native_unique_id: 0,
        line_trail: None,
        flat: false,
        source_id,
        origin: ProjectileCoord::new(0, 0, 0),
        target: ProjectileTarget::Cell { rx: 16, ry: 0 },
        initial_target_position: ProjectileCoord::new(4096, 0, 0),
        payload: ProjectilePayload::new(
            1,
            crate::sim::intern::InternedId::from_index(0),
            None,
            false,
        ),
        speed_leptons_per_frame: 64,
        velocity: ProjectileVelocity::new(64, 0, 0),
        trajectory: ProjectileTrajectory::Straight,
        guidance: None,
        visual: ProjectileVisualState::new(0, 0, 0),
        arm_frames: 0,
        fuse_frames,
        ranged_fuse: false,
        tracks_target: false,
        target_expiry: TargetExpiryPolicy::Expire,
        collision: ProjectileCollisionPolicy::NONE,
    }
}
