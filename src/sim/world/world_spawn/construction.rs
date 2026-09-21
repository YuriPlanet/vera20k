//! Techno identity and component construction, before store/Reveal admission.
//!
//! Map import keeps authored identity/health/placement; live and held runtime
//! callers share one constructor. Child allocation still belongs to the parent
//! store transaction in world_spawn, after these manager descriptors exist.

use super::{GeneratedTechnoInitError, object_uses_voxel};
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::object_type::{FactoryType, ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::{Animation, SequenceKind};
use crate::sim::components::{BridgeOccupancy, HarvestOverlay, Health, VoxelAnimation};
use crate::sim::game_entity::{GameEntity, TechnoConstructorInit};
use crate::sim::miner::{Miner, MinerConfig, miner_kind_for_object};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::vision::MAX_SIGHT_RANGE;
use crate::sim::world::Simulation;

/// Existing initialization differences are explicit here. In particular map
/// import currently omits Ship state and Fly Idle setup.
/// This is a Rust compatibility policy, not a claim of native equivalence.
#[derive(Clone, Copy)]
pub(super) enum ComponentOrigin {
    Authored {
        sub_cell: u8,
        bridge_deck: Option<u8>,
    },
    Runtime,
}

impl Simulation {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn construct_runtime_techno(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
        init: TechnoConstructorInit,
    ) -> Result<Option<GameEntity>, GeneratedTechnoInitError> {
        let Some(obj) = rules.object(type_id) else {
            return Ok(None);
        };
        let health = Health {
            // Native class constructors copy signed type Strength unchanged.
            // Original copy slices: object_health corpus / constructors.
            current: obj.strength,
        };
        let category = match obj.category {
            ObjectCategory::Infantry => EntityCategory::Infantry,
            ObjectCategory::Vehicle => EntityCategory::Unit,
            ObjectCategory::Aircraft => EntityCategory::Aircraft,
            ObjectCategory::Building => EntityCategory::Structure,
        };
        let uses_voxel = object_uses_voxel(type_id, obj, rules);
        let sight_range = (obj.sight.max(0) as u16).min(MAX_SIGHT_RANGE);
        let stable_id = self.allocate_stable_id();
        let owner_iid = self.interner.intern(owner);
        let type_iid = self.interner.intern(type_id);
        let techno_ctor_random_word = self.resolve_techno_constructor_word(init, None)?;

        let mut ge = GameEntity::new_at_frame_from_constructor_word(
            stable_id,
            rx,
            ry,
            z,
            facing,
            owner_iid,
            health,
            type_iid,
            category,
            0, // veterancy = rookie for production spawns
            sight_range,
            uses_voxel,
            self.session.binary_frame,
            techno_ctor_random_word,
        );

        self.install_techno_components(&mut ge, Some(obj), Some(rules), ComponentOrigin::Runtime);
        Ok(Some(ge))
    }

    pub(super) fn install_techno_components(
        &mut self,
        ge: &mut GameEntity,
        obj: Option<&ObjectType>,
        rules: Option<&RuleSet>,
        origin: ComponentOrigin,
    ) {
        let category = ge.category;
        let facing = ge.facing;
        let uses_voxel = ge.is_voxel;
        // InitManagers6F3F40 classifies the owner after construction; it does
        // not manufacture either discovery-history byte. The launch-owned
        // current house is distinct from the notification/viewer binding.
        ge.discovery.owned_by_current_house = self.session.current_house == Some(ge.owner());
        if self.debug_event_logging {
            ge.debug_log = Some(crate::sim::debug_event_log::DebugEventLog::new());
        }

        stamp_scoring_flags(ge, obj);
        ge.sight_is_zero = obj.is_some_and(|object| object.sight == 0);
        if let Some(obj) = obj.filter(|obj| obj.has_turret) {
            let initial = crate::sim::movement::turret::body_facing_to_turret(facing);
            ge.barrel_facing = Some(crate::sim::movement::FacingClass::new(
                initial,
                obj.turret_rot,
            ));
        }
        if uses_voxel {
            ge.voxel_animation = Some(VoxelAnimation::new(1, 1));
        }
        if category == EntityCategory::Infantry {
            ge.animation = Some(Animation::new(SequenceKind::Stand));
            ge.sub_cell = Some(match origin {
                ComponentOrigin::Authored { sub_cell, .. } => sub_cell,
                ComponentOrigin::Runtime => {
                    self.allocate_infantry_sub_cell(ge.position.rx, ge.position.ry)
                }
            });
            let (lx, ly) = crate::util::lepton::subcell_lepton_offset(ge.sub_cell);
            ge.position.sub_x = lx;
            ge.position.sub_y = ly;
        }
        // SHP vehicles also need animation for walk/attack frame cycling.
        if !uses_voxel && (category == EntityCategory::Unit || category == EntityCategory::Aircraft)
        {
            ge.animation = Some(Animation::new(SequenceKind::Stand));
        }
        let Some(obj) = obj else {
            install_authored_bridge(ge, origin);
            return;
        };
        ge.crushable = obj.crushable;
        ge.deployed_crushable = obj.deployed_crushable;
        ge.omni_crusher = obj.omni_crusher;
        ge.regular_crusher = obj.crusher;
        ge.drive_accelerates = obj.accelerates;
        ge.omni_crush_resistant = obj.omni_crush_resistant;
        ge.immune_to_radiation = obj.immune_to_radiation;
        ge.occupier = obj.occupier;
        if category == EntityCategory::Structure && obj.gate {
            ge.building_gate = Some(crate::sim::game_entity::BuildingGateRuntime::default());
        }
        if category == EntityCategory::Structure && obj.bunker {
            ge.bunker_runtime = Some(crate::sim::docking::bunker_install::BunkerRuntime::idle());
        }
        ge.zfudge_bridge = obj.zfudge_bridge;
        ge.too_big_to_fit_under_bridge = obj.too_big_to_fit_under_bridge;
        if should_construct_locomotor(category, obj) {
            ge.locomotor = Some(LocomotorState::from_object_type(
                obj,
                self.session.binary_frame,
            ));
            if matches!(origin, ComponentOrigin::Runtime)
                && ge.locomotor.as_ref().is_some_and(|locomotor| {
                    locomotor.kind == crate::rules::locomotor_type::LocomotorKind::Ship
                })
            {
                ge.ship_locomotion = Some(Default::default());
            }
        }
        // Retain the signed native count even for Ammo=-1. Mission_Attack's
        // pending-release consumer can decrement a negative count as well.
        if category == EntityCategory::Aircraft {
            ge.aircraft_ammo = Some(crate::sim::docking::aircraft_dock::AircraftAmmo::from_type(
                obj,
            ));
        }
        // Initialize aircraft mission for Fly-locomotor aircraft.
        if matches!(origin, ComponentOrigin::Runtime)
            && ge
                .locomotor
                .as_ref()
                .is_some_and(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Fly)
        {
            ge.aircraft_mission = Some(crate::sim::aircraft::AircraftMission::Idle);
        }

        install_authored_bridge(ge, origin);

        if let Some(kind) = miner_kind_for_object(obj) {
            let mcfg: MinerConfig = rules.map(MinerConfig::from_rules).unwrap_or_default();
            let storage = obj.storage.max(0) as u16;
            ge.miner = Some(Miner::new(kind, &mcfg, storage));
            ge.harvest_overlay = Some(HarvestOverlay {
                frame: 0,
                visible: false,
                elapsed_frames: 0,
            });
        }
        // Passenger cargo for transports and garrisonable buildings.
        if obj.passengers > 0 {
            ge.passenger_role = crate::sim::passenger::PassengerRole::Transport {
                cargo: crate::sim::passenger::PassengerCargo::new(
                    obj.passengers as u32,
                    obj.size_limit,
                ),
            };
        } else if obj.can_be_occupied && obj.max_number_occupants > 0 {
            ge.passenger_role = crate::sim::passenger::PassengerRole::Transport {
                cargo: crate::sim::passenger::PassengerCargo::new(obj.max_number_occupants, 1),
            };
        }

        stamp_building_cell_profile(ge, Some(obj));
        // TechnoClass::Init_Managers — manager-owned children are committed
        // after this parent enters the limbo store and before its Unlimbo.
        if let Some(rules) = rules {
            ge.capture_manager = crate::sim::capture_manager::init_capture_manager(obj, rules);
            ge.spawn_manager = crate::sim::spawn_manager::init_spawn_manager(
                obj,
                rules,
                &mut self.interner,
                self.session.binary_frame,
            );
        }
    }
}

fn install_authored_bridge(ge: &mut GameEntity, origin: ComponentOrigin) {
    if let ComponentOrigin::Authored {
        bridge_deck: Some(deck_level),
        ..
    } = origin
    {
        if let Some(loco) = &mut ge.locomotor {
            loco.layer = MovementLayer::Bridge;
        }
        ge.bridge_occupancy = Some(BridgeOccupancy { deck_level });
        ge.on_bridge = true;
    }
}

/// Copy the rules-derived scoring flags onto a freshly built entity.
///
/// Every spawn path calls this, so a type that must not appear on the score
/// screen is honored no matter how the object came into the world. The flag is
/// copied rather than looked up later because the score bookkeeping runs in the
/// lifecycle authority, which deliberately holds no `RuleSet` borrow.
fn stamp_scoring_flags(ge: &mut GameEntity, obj: Option<&crate::rules::object_type::ObjectType>) {
    ge.dont_score = obj.is_some_and(|o| o.dont_score);
}

fn stamp_building_cell_profile(
    ge: &mut GameEntity,
    obj: Option<&crate::rules::object_type::ObjectType>,
) {
    let Some(obj) = obj else {
        return;
    };
    ge.foundation = obj.foundation.clone();
    ge.spotlight_capable = obj.has_spotlight;
    if ge.category == EntityCategory::Structure {
        ge.building_hidden_occupancy = Some(obj.hidden_occupancy);
        ge.base_reservation_spacing = obj.base_reservation_spacing;
        ge.determines_waypoint_edge = obj.factory == Some(FactoryType::BuildingType);
        ge.build_const_eligible = obj.build_const_eligible;
        ge.base_plan_type_index = obj.base_plan_type_index;
        ge.base_plan_is_defense = obj.is_base_defense;
        // Projection of native BuildingType+0x408 after
        // UnitTypeClass__FindOrAllocate @ 0x007480D0 has resolved
        // `none`/`<none>` to a null pointer.
        ge.base_plan_has_undeploy_target = obj.undeploys_into.is_some();
    }
}

/// A Foot object owns its configured locomotor independently of the parsed
/// `Speed=` scalar. `DriveLocomotionClass::Process @ 0x004B0500` and
/// `ShipLocomotionClass::Process @ 0x0069FC10` consume their class-local state
/// without a Speed gate. That state is therefore load-bearing at Speed=0,
/// while Structures remain outside Foot and other zero-speed custom
/// locomotors retain the existing inactive compatibility behavior.
fn should_construct_locomotor(category: EntityCategory, object: &ObjectType) -> bool {
    object.speed > 0
        || (matches!(
            category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        ) && matches!(object.locomotor, LocomotorKind::Drive | LocomotorKind::Ship))
}
