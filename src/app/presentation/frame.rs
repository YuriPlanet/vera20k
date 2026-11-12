//! The presentation frame (docs/sim-thread.md): the objects as drawing reads
//! them, captured once per world state instead of read from the world while
//! drawing.
//!
//! A frame holds one [`ObjectRecord`] for every stored object, keyed by its
//! stable id. The simulation host captures it from the home runtime when a
//! draw or a lent tick finds the world changed since the last capture
//! ([`crate::app::match_runtime::sim_host::SimHost::ensure_frame`]), so a
//! frame always equals the world it stands for. Drawing reads the rest of the
//! world from the copy without the objects the host keeps beside the frame
//! ([`crate::sim::runtime::SimRuntime::presentation_copy`]).
//!
//! A record carries the object's state under the names [`GameEntity`] gives
//! it, so a draw reads a record as it would read the object: its components
//! are copied, and what reading them needs more of the world for (another
//! object, the terrain, the rules, the local player's sight) is answered at
//! capture by the same functions a draw used to call. Records hold only
//! results that depend on the world, the committed binary frame and the
//! local player, never on the camera, the cursor or render caches.

use std::collections::HashMap;

use crate::app::presentation::instances::TetherDraw;
use crate::app::presentation::render_pose::RenderPose;
use crate::map::entities::EntityCategory;
use crate::render::draw_state::ObserverDrawContext;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::runtime::SimRuntime;
use crate::sim::world::Simulation;

/// What capturing a record reads beside its object.
pub(crate) struct Capture<'a> {
    pub(crate) runtime: &'a SimRuntime,
    pub(crate) sim: &'a Simulation,
    pub(crate) binary_frame: u32,
    /// The local player drawing reads the frame for, by name and id.
    pub(crate) observer: Option<&'a str>,
    pub(crate) observer_id: Option<InternedId>,
    /// The capture's isolated query of the live map: a miss stamps its own
    /// dummy, not the simulation's.
    pub(crate) cells: Option<crate::map::resolved_terrain::NativeCellQuery<'a>>,
    /// The current house's psychic detection sites
    /// (`Simulation::psychic_detection_sites`), when it has any.
    psychic: Option<(InternedId, Vec<(crate::sim::components::DriveCoord, i32)>)>,
}

impl Capture<'_> {
    pub(crate) fn rules(&self) -> &crate::rules::ruleset::RuleSet {
        &self.runtime.resources.rules
    }

    pub(crate) fn object_type(
        &self,
        entity: &GameEntity,
    ) -> Option<&crate::rules::object_type::ObjectType> {
        self.sim
            .interner()
            .object_type(entity.type_ref(), self.rules())
    }
}

/// One object's draw record: its state under [`GameEntity`]'s names.
#[derive(Clone, Debug)]
pub(crate) struct ObjectRecord {
    id: u64,
    category: EntityCategory,
    type_ref: InternedId,
    owner: InternedId,
    is_voxel: bool,
    on_bridge: bool,
    ion_blast_lift: i8,
    pub(crate) position: crate::sim::components::Position,
    pub(crate) body_facing: crate::sim::movement::FacingClass,
    pub(crate) barrel_facing: Option<crate::sim::movement::FacingClass>,
    barrel_elevation: crate::sim::movement::FacingClass,
    pub(crate) body_frame_counter: crate::sim::animation::BodyFrameCounter,
    pub(crate) unit_firing_counter: crate::sim::animation::UnitFiringCounter,
    pub(crate) turret_anim_frame: crate::sim::combat::gattling::TurretAnimCounter,
    pub(crate) health: crate::sim::combat::Health,
    pub(crate) veterancy: crate::sim::combat::veterancy::Veterancy,
    pub(crate) flashing: Flashing,
    pub(crate) deactivated: Deactivated,
    pub(crate) repairing: Repairing,
    pub(crate) lifecycle: crate::sim::world::ObjectLifecycle,
    pub(crate) locomotor: Option<crate::sim::movement::locomotor::LocomotorState>,
    pub(crate) navigation: Navigation,
    pub(crate) attack_target: crate::sim::combat::TarCom,
    pub(crate) last_fire_frame: crate::sim::combat::world_fire::LastFireFrame,
    pub(crate) building_art: crate::util::sparse::Sparse<crate::sim::building_art::BuildingArt>,
    pub(crate) animation: Option<crate::sim::animation::Animation>,
    pub(crate) miner: Option<Box<crate::sim::miner::Miner>>,
    pub(crate) cloak: Option<Box<crate::sim::cloak_disguise::CloakRuntime>>,
    pub(crate) capture_manager: Option<Box<crate::sim::capture_manager::CaptureManagerState>>,
    pub(crate) spawn_manager: Option<Box<crate::sim::spawn_manager::SpawnManagerState>>,
    pub(crate) invulnerability:
        Option<crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    pub(crate) airstrike_tint: crate::sim::superweapon::airstrike_tint::AirstrikeTintState,
    pub(crate) temporal: crate::sim::temporal::TemporalState,
    /// Whether it carries a bomb (`GameEntity::bomb`).
    pub(crate) bomb: Option<()>,
    pub(crate) berserk: Berserk,
    pub(crate) crash: Crash,
    pub(crate) sinking: Sinking,
    pub(crate) passenger_role: crate::sim::passenger::PassengerRole,
    pub(crate) rocking: Option<crate::sim::rocking::RockingState>,
    pub(crate) mission: crate::sim::mission::MissionCom,
    pub(crate) tether: crate::sim::radio::Tether,
    pub(crate) radio_contacts: crate::sim::radio::Contacts,
    pub(crate) disguise: Option<crate::sim::cloak_disguise::DisguiseRuntime>,
    pub(crate) display_type_override: DisplayTypeOverride,
    pub(crate) selection: crate::sim::selection::Selection,
    pub(crate) discovery: crate::sim::world::TechnoDiscoveryHistory,
    /// `GameEntity::foot_speed`'s applied fraction.
    pub(crate) applied_fraction: crate::util::native_x87::NativeF64Bits,
    /// The cargo a Carryall draws inside its own draw
    /// (`Simulation::carryall_drawn_cargo`).
    pub(crate) drawn_cargo: Option<u64>,
    /// The answers of `GameEntity`'s own queries.
    answers: Answers,
    /// [`RenderPose::read`] at the committed binary frame.
    pub(crate) pose: RenderPose,
    /// Its tether redraws (`instances::tether_redraw`).
    pub(crate) tether_draw: TetherDraw,
    /// What placing it reads of the world.
    pub(crate) place: Place,
    /// What the local player sees of it.
    pub(crate) seen: Seen,
    /// What its status strip reads (`ui_overlays`).
    pub(crate) hud: crate::app::presentation::ui_overlays::HudFacts,
    /// What the scene's other drawers read of it (`frame_scene`).
    pub(crate) scene: crate::app::presentation::frame_scene::SceneFacts,
}

/// `GameEntity`'s answers to the queries drawing asks it.
#[derive(Clone, Debug)]
struct Answers {
    is_fully_deployed: bool,
    unit_deploying: bool,
    current_turret_index: i32,
    current_weapon_number: i32,
    is_falling_down: bool,
    is_warped_out: bool,
    is_warping_in: bool,
    construction_stage_value: i32,
    /// `None` but for a Structure, whose query it is.
    building_construction_control: Option<[i32; 3]>,
    building_up: bool,
    building_down: bool,
    in_construction_bstate: bool,
    infantry_sprite_pose: Option<(i32, i32)>,
    gap_supercharged: bool,
    building_online: bool,
    has_airstrike_marker: bool,
    building_cloak_stage: u8,
    native_cloak_offset_words: i32,
    storage_bales: Vec<crate::sim::miner::CargoBale>,
    #[cfg(feature = "ares")]
    driver_killed: bool,
    #[cfg(feature = "phobos")]
    is_power_plant_enhancer_active: bool,
    emp_remaining: i32,
    voxel_recoil: ([f32; 2], bool),
    #[cfg(feature = "phobos")]
    shield: Option<crate::sim::shield::ShieldState>,
    #[cfg(feature = "phobos")]
    phobos_attach_effects: crate::sim::attach_effect::AttachEffectList,
}

impl Answers {
    fn of(entity: &GameEntity) -> Self {
        Self {
            is_fully_deployed: entity.is_fully_deployed(),
            unit_deploying: entity.unit_deploying(),
            current_turret_index: entity.current_turret_index(),
            current_weapon_number: entity.current_weapon_number(),
            is_falling_down: entity.is_falling_down(),
            is_warped_out: entity.is_warped_out(),
            is_warping_in: entity.is_warping_in(),
            construction_stage_value: entity.construction_stage_value(),
            building_construction_control: entity.built_construction_control(),
            building_up: entity.building_up(),
            building_down: entity.building_down(),
            in_construction_bstate: entity.in_construction_bstate(),
            infantry_sprite_pose: entity.infantry_sprite_pose(),
            gap_supercharged: entity.gap_supercharged(),
            building_online: entity.building_online(),
            has_airstrike_marker: entity.has_airstrike_marker(),
            building_cloak_stage: entity.building_cloak_stage(),
            native_cloak_offset_words: entity.native_cloak_offset_words(),
            storage_bales: entity.storage_bales().to_vec(),
            #[cfg(feature = "ares")]
            driver_killed: entity.driver_killed(),
            #[cfg(feature = "phobos")]
            is_power_plant_enhancer_active: entity.is_power_plant_enhancer_active(),
            emp_remaining: entity.emp_remaining(),
            voxel_recoil: entity.voxel_recoil(),
            #[cfg(feature = "phobos")]
            shield: entity.shield().cloned(),
            #[cfg(feature = "phobos")]
            phobos_attach_effects: entity.phobos_attach_effects().clone(),
        }
    }
}

/// What placing an object reads of the world beside the object.
#[derive(Clone, Debug)]
pub(crate) struct Place {
    /// `render::locomotor_visual::screen_position`.
    pub(crate) screen_position: (f32, f32),
    /// `sim::movement::ground_pose::object_world_z_leptons` on the live map.
    pub(crate) world_z_leptons: i32,
    /// `sim::movement::air_movement::queried_flight_height`.
    pub(crate) flight_height: i32,
    /// `sim::movement::air_movement::aircraft_landing_base`.
    pub(crate) landing_base: i32,
    /// `sim::movement::hover::hover_draw_slope` with the pose's rocking angles.
    pub(crate) hover_draw_slope: u8,
    /// The locomotor's slope transition at the frame before the committed
    /// one, the frame Drive and Ship draw (`instances::units`).
    pub(crate) slope_render_phase: Option<crate::sim::movement::slope_transition::SlopeRenderPhase>,
    /// `sim::movement::motion_query::is_moving_now` at that frame.
    pub(crate) moving_now: bool,
    /// `sim::movement::motion_query::is_moving`.
    pub(crate) moving: Option<bool>,
    /// `sim::movement::ground_pose::object_center_xy`.
    pub(crate) center_xy: [i32; 2],
    /// `object_world_z_leptons` without the map: its exact Z, else its
    /// stored level's.
    pub(crate) world_z_leptons_off_map: i32,
    /// `sim::movement::turret::facing_toward_target` of its attack target.
    pub(crate) facing_toward_target: Option<u16>,
    /// A Structure's art (`sim::building_art`), for its own type.
    pub(crate) structure: Option<StructureArt>,
    /// Its radio contact 0, the object it talks to.
    pub(crate) contact0: Option<Contact>,
    /// `presentation::lighting::body_extra_light`.
    pub(crate) extra_light: i32,
    /// Its action line's world endpoints
    /// (`target_lines::action_line_endpoints`).
    pub(crate) action_line: Option<crate::app::presentation::target_lines::SelectedActionLine>,
    /// A Structure's rally line (`target_lines::rally_line_coords`).
    pub(crate) rally_line: Option<[crate::sim::components::DriveCoord; 2]>,
    /// `Simulation::techno_intent_psychically_detected` by the current
    /// house's detectors.
    pub(crate) psychically_detected: bool,
    /// The airstrike it designates: its link to itself
    /// (`AirstrikeState::links_to`) with
    /// `Simulation::airstrike_designation_coordinates`.
    pub(crate) airstrike_designation: Option<AirstrikeDesignation>,
    /// `Simulation::airstrike_tint_active`.
    pub(crate) airstrike_tint_active: bool,
    /// `Simulation::ares_is_operated`.
    #[cfg(feature = "ares")]
    pub(crate) ares_operated: bool,
    /// `Simulation::airstrike_laser_target_color`.
    #[cfg(feature = "phobos")]
    pub(crate) airstrike_laser_target_color: i32,
}

/// An airstrike designation line (`presentation::airstrike_lines`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct AirstrikeDesignation {
    pub(crate) coords: [crate::sim::components::DriveCoord; 2],
    /// `Simulation::airstrike_line_color`.
    #[cfg(feature = "phobos")]
    pub(crate) color: [u8; 3],
}

impl AirstrikeDesignation {
    fn of(entity: &GameEntity, at: &Capture<'_>) -> Option<Self> {
        let id = entity.stable_id();
        if !entity.airstrike.links_to(id) {
            return None;
        }
        Some(Self {
            coords: at.sim.airstrike_designation_coordinates(id, at.rules())?,
            #[cfg(feature = "phobos")]
            color: at.sim.airstrike_line_color(id, at.rules()),
        })
    }
}

/// What drawing reads of an object's radio contact 0.
#[derive(Clone, Debug)]
pub(crate) struct Contact {
    pub(crate) id: u64,
    pub(crate) category: EntityCategory,
    pub(crate) type_ref: InternedId,
    pub(crate) mission: crate::sim::mission::MissionCom,
}

/// A Structure's `sim::building_art` answers for its own type.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StructureArt {
    /// `building_image` without and with a Buildup.
    pub(crate) image: [crate::sim::building_art::BuildingImage; 2],
    /// `current_body_frame` at its type's tech level.
    pub(crate) current_body_frame: i32,
    /// `gate_open_body_frame` with its art's GateStages, for a gate.
    pub(crate) gate_open_body_frame: Option<i32>,
}

impl StructureArt {
    fn of(entity: &GameEntity, at: &Capture<'_>) -> Option<Self> {
        use crate::sim::building_art;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        let rules = at.rules();
        let object = at.object_type(entity)?;
        let art = rules
            .art()
            .resolve_metadata_entry(at.sim.interner().resolve(entity.type_ref()), &object.image);
        let gate_stages = object
            .gate
            .then(|| art.map_or(9, |entry| entry.building_gate_stages));
        Some(Self {
            image: [
                building_art::building_image(entity, false),
                building_art::building_image(entity, true),
            ],
            current_body_frame: building_art::current_body_frame(
                entity,
                object,
                rules,
                at.sim.object_tech_level(object),
            ),
            gate_open_body_frame: gate_stages.and_then(|gate_stages| {
                building_art::gate_open_body_frame(
                    entity,
                    gate_stages,
                    object.strength,
                    rules.general.condition_yellow,
                    at.binary_frame,
                )
            }),
        })
    }
}

impl Place {
    fn of(entity: &GameEntity, pose: &RenderPose, at: &Capture<'_>) -> Self {
        let sim = at.sim;
        let display_frame = at.binary_frame.wrapping_sub(1);
        Self {
            screen_position: crate::render::locomotor_visual::screen_position(entity),
            world_z_leptons: crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                sim.resolved_terrain(),
            ),
            flight_height: crate::sim::movement::air_movement::queried_flight_height(
                entity,
                at.cells.as_ref(),
            ),
            landing_base: crate::sim::movement::air_movement::aircraft_landing_base(
                entity,
                sim.entities(),
                Some((at.rules(), sim.interner())),
            ),
            hover_draw_slope: crate::sim::movement::hover::hover_draw_slope(
                entity,
                at.cells.as_ref(),
                pose.angles,
            ),
            slope_render_phase: crate::sim::movement::slope_transition::state_for_entity(entity)
                .map(|state| state.render_phase(display_frame)),
            moving_now: crate::sim::movement::motion_query::is_moving_now(
                entity,
                crate::sim::movement::SpeedRules::new(at.rules(), sim.interner(), sim.houses()),
                display_frame,
            ),
            moving: crate::sim::movement::motion_query::is_moving(entity),
            center_xy: crate::sim::movement::ground_pose::object_center_xy(entity),
            world_z_leptons_off_map: crate::sim::movement::ground_pose::object_world_z_leptons(
                entity, None,
            ),
            facing_toward_target: entity.attack_target.target().and_then(|target| {
                crate::sim::movement::turret::facing_toward_target(entity, &target, sim)
            }),
            structure: StructureArt::of(entity, at),
            extra_light: crate::app::presentation::lighting::body_extra_light(
                entity,
                at.rules(),
                sim,
                at.cells.as_ref(),
            ),
            airstrike_designation: AirstrikeDesignation::of(entity, at),
            action_line: crate::app::presentation::target_lines::action_line_endpoints(
                entity,
                sim,
                Some(at.rules()),
                at.cells.as_ref(),
            ),
            rally_line: at.cells.as_ref().and_then(|cells| {
                crate::app::presentation::target_lines::rally_line_coords(entity, sim, cells)
            }),
            psychically_detected: at.psychic.as_ref().is_some_and(|(viewer, sites)| {
                sim.techno_intent_psychically_detected(
                    entity.stable_id(),
                    *viewer,
                    sites,
                    at.cells.as_ref(),
                )
            }),
            airstrike_tint_active: sim.airstrike_tint_active(entity.stable_id()),
            #[cfg(feature = "ares")]
            ares_operated: sim.ares_is_operated(entity.stable_id(), at.rules()),
            #[cfg(feature = "phobos")]
            airstrike_laser_target_color: sim
                .airstrike_laser_target_color(entity.stable_id(), at.rules()),
            contact0: entity
                .radio_contacts
                .slot(0)
                .and_then(|id| sim.entities().get(id))
                .map(|contact| Contact {
                    id: contact.stable_id(),
                    category: contact.category(),
                    type_ref: contact.type_ref(),
                    mission: contact.mission.clone(),
                }),
        }
    }
}

/// What the local player sees of an object.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seen {
    /// `instances::observer_draw_context` for the local player.
    pub(crate) observer: ObserverDrawContext,
    /// `GameEntity::visual_character`'s screen answer for that context.
    pub(crate) visual_character: u8,
    /// `GameEntity::is_disguised_to` the local player.
    pub(crate) disguised_to_observer: bool,
    /// `GameEntity::disguise_hides_house_from` the local player.
    pub(crate) disguise_hides_house: bool,
    /// `GameEntity::techno_pip_fill_level` without a controlled count, and
    /// with the mind-control count (`Simulation::capture_effective_count`).
    pub(crate) pip_fill_level: i32,
    pub(crate) controlled_count: Option<i32>,
    pub(crate) controlled_pip_fill_level: i32,
}

impl Seen {
    fn of(entity: &GameEntity, at: &Capture<'_>) -> Self {
        let sim = at.sim;
        let rules = at.rules();
        let observer = crate::app::presentation::instances::observer_draw_context(
            sim,
            entity,
            at.observer,
            at.observer_id,
            Some(rules),
        );
        let visual_character = entity.visual_character(
            observer.cloaking_stages,
            observer.invisible,
            entity.discovery.owned_by_current_house(),
            crate::sim::cloak_disguise::VisualCharacterQuery::screen(
                true,
                observer.observer_present,
                observer.detects_cloak,
                observer.owner_is_mutually_allied,
                observer.is_campaign,
                false,
            ),
        );
        let disguised_to = |viewer| {
            entity.is_disguised_to(
                viewer,
                Some(sim.fog()),
                Some(sim.house_alliances()),
                sim.interner(),
            )
        };
        let hides_house = |viewer| {
            entity.disguise_hides_house_from(
                viewer,
                Some(sim.fog()),
                Some(sim.house_alliances()),
                sim.interner(),
            )
        };
        let object = at.object_type(entity);
        let controlled_count = sim.capture_effective_count(entity.stable_id(), rules);
        Self {
            observer,
            visual_character,
            disguised_to_observer: at.observer_id.is_some_and(disguised_to),
            disguise_hides_house: at.observer_id.is_some_and(hides_house),
            pip_fill_level: object.map_or(0, |object| {
                entity.techno_pip_fill_level(object, rules, None)
            }),
            controlled_count,
            controlled_pip_fill_level: object.map_or(0, |object| {
                entity.techno_pip_fill_level(object, rules, controlled_count)
            }),
        }
    }
}

impl ObjectRecord {
    fn capture(entity: &GameEntity, at: &Capture<'_>) -> Self {
        let pose = RenderPose::read(entity, at.binary_frame);
        let answers = Answers::of(entity);
        let place = Place::of(entity, &pose, at);
        let seen = Seen::of(entity, at);
        let hud = crate::app::presentation::ui_overlays::HudFacts::capture(entity, at);
        let scene = crate::app::presentation::frame_scene::SceneFacts::capture(entity, at);
        let tether_draw =
            crate::app::presentation::instances::tether_draw(entity, at.sim.entities());
        Self {
            id: entity.stable_id(),
            category: entity.category(),
            type_ref: entity.type_ref(),
            owner: entity.owner(),
            is_voxel: entity.is_voxel(),
            on_bridge: entity.on_bridge(),
            ion_blast_lift: entity.ion_blast_lift_pixels(),
            position: entity.position.clone(),
            body_facing: entity.body_facing.clone(),
            barrel_facing: entity.barrel_facing.clone(),
            barrel_elevation: entity.barrel_elevation().clone(),
            body_frame_counter: entity.body_frame_counter.clone(),
            unit_firing_counter: entity.unit_firing_counter.clone(),
            turret_anim_frame: entity.turret_anim_frame.clone(),
            health: entity.health.clone(),
            veterancy: entity.veterancy.clone(),
            flashing: Flashing {
                frames: entity.flashing.frames(),
            },
            deactivated: Deactivated {
                offline: entity.deactivated.is_offline(),
            },
            repairing: Repairing {
                on: entity.building.repairing.is_on(),
            },
            lifecycle: entity.lifecycle.clone(),
            locomotor: entity.locomotor.clone(),
            navigation: Navigation {
                nav_com: entity.navigation.nav_com(),
                nav_queue: entity.navigation.nav_queue().to_vec(),
            },
            attack_target: entity.attack_target.clone(),
            last_fire_frame: entity.last_fire_frame.clone(),
            building_art: entity.building_art.clone(),
            animation: entity.animation.clone(),
            miner: entity.miner.clone(),
            cloak: entity.cloak.clone(),
            capture_manager: entity.capture_manager.clone(),
            spawn_manager: entity.spawn_manager.clone(),
            invulnerability: entity.invulnerability.clone(),
            airstrike_tint: entity.airstrike_tint.clone(),
            temporal: entity.temporal.clone(),
            bomb: entity.bomb.as_ref().map(|_| ()),
            berserk: Berserk {
                active: entity.berserk.is_active(),
            },
            crash: Crash {
                crashing: entity.crash.is_crashing(),
            },
            sinking: Sinking {
                active: entity.sinking.is_active(),
            },
            passenger_role: entity.passenger_role.clone(),
            rocking: entity.rocking.clone(),
            mission: entity.mission.clone(),
            tether: entity.tether.clone(),
            radio_contacts: entity.radio_contacts.clone(),
            disguise: entity.disguise.clone(),
            display_type_override: DisplayTypeOverride {
                image: entity.display_type_override.image(),
            },
            selection: entity.selection.clone(),
            discovery: entity.discovery.clone(),
            applied_fraction: entity.foot_speed.native_applied_fraction(),
            drawn_cargo: at.sim.carryall_drawn_cargo(entity.stable_id(), at.rules()),
            answers,
            place,
            seen,
            hud,
            scene,
            pose,
            tether_draw,
        }
    }

    pub(crate) fn stable_id(&self) -> u64 {
        self.id
    }

    pub(crate) fn category(&self) -> EntityCategory {
        self.category
    }

    pub(crate) fn type_ref(&self) -> InternedId {
        self.type_ref
    }

    pub(crate) fn owner(&self) -> InternedId {
        self.owner
    }

    pub(crate) fn is_voxel(&self) -> bool {
        self.is_voxel
    }

    pub(crate) fn on_bridge(&self) -> bool {
        self.on_bridge
    }

    pub(crate) fn ion_blast_lift_pixels(&self) -> i8 {
        self.ion_blast_lift
    }

    pub(crate) fn body_facing_current(&self, frame: u32) -> u16 {
        self.body_facing.current(frame)
    }

    pub(crate) fn barrel_elevation(&self) -> &crate::sim::movement::FacingClass {
        &self.barrel_elevation
    }

    pub(crate) fn is_object_alive(&self) -> bool {
        self.lifecycle.object_alive()
    }

    pub(crate) fn in_limbo(&self) -> bool {
        self.lifecycle.in_limbo()
    }

    pub(crate) fn is_fully_deployed(&self) -> bool {
        self.answers.is_fully_deployed
    }

    pub(crate) fn unit_deploying(&self) -> bool {
        self.answers.unit_deploying
    }

    pub(crate) fn current_turret_index(&self) -> i32 {
        self.answers.current_turret_index
    }

    pub(crate) fn current_weapon_number(&self) -> i32 {
        self.answers.current_weapon_number
    }

    pub(crate) fn is_falling_down(&self) -> bool {
        self.answers.is_falling_down
    }

    pub(crate) fn is_warped_out(&self) -> bool {
        self.answers.is_warped_out
    }

    pub(crate) fn is_warping_in(&self) -> bool {
        self.answers.is_warping_in
    }

    pub(crate) fn construction_stage_value(&self) -> i32 {
        self.answers.construction_stage_value
    }

    /// A Structure's query (`GameEntity::building_construction_control`);
    /// zero for other objects, which never ask it.
    pub(crate) fn building_construction_control(&self) -> [i32; 3] {
        self.answers
            .building_construction_control
            .unwrap_or_default()
    }

    pub(crate) fn building_up(&self) -> bool {
        self.answers.building_up
    }

    pub(crate) fn building_down(&self) -> bool {
        self.answers.building_down
    }

    pub(crate) fn in_construction_bstate(&self) -> bool {
        self.answers.in_construction_bstate
    }

    pub(crate) fn infantry_sprite_pose(&self) -> Option<(i32, i32)> {
        self.answers.infantry_sprite_pose
    }

    pub(crate) fn gap_supercharged(&self) -> bool {
        self.answers.gap_supercharged
    }

    pub(crate) fn building_online(&self) -> bool {
        self.answers.building_online
    }

    pub(crate) fn has_airstrike_marker(&self) -> bool {
        self.answers.has_airstrike_marker
    }

    pub(crate) fn building_cloak_stage(&self) -> u8 {
        self.answers.building_cloak_stage
    }

    pub(crate) fn native_cloak_offset_words(&self) -> i32 {
        self.answers.native_cloak_offset_words
    }

    pub(crate) fn storage_bales(&self) -> &[crate::sim::miner::CargoBale] {
        &self.answers.storage_bales
    }

    #[cfg(feature = "ares")]
    pub(crate) fn driver_killed(&self) -> bool {
        self.answers.driver_killed
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn is_power_plant_enhancer_active(&self) -> bool {
        self.answers.is_power_plant_enhancer_active
    }

    pub(crate) fn emp_remaining(&self) -> i32 {
        self.answers.emp_remaining
    }

    pub(crate) fn voxel_recoil(&self) -> ([f32; 2], bool) {
        self.answers.voxel_recoil
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn shield(&self) -> Option<&crate::sim::shield::ShieldState> {
        self.answers.shield.as_ref()
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn phobos_attach_effects(&self) -> &crate::sim::attach_effect::AttachEffectList {
        &self.answers.phobos_attach_effects
    }
}

impl crate::render::locomotor_visual::Placed for ObjectRecord {
    fn placed_position(&self) -> &crate::sim::components::Position {
        &self.position
    }

    fn placed_category(&self) -> EntityCategory {
        self.category
    }
}

impl crate::render::snapshot::SnapshotObject for ObjectRecord {
    fn stable_id(&self) -> u64 {
        self.id
    }
    fn category(&self) -> EntityCategory {
        self.category
    }
    fn owner(&self) -> InternedId {
        self.owner
    }
    fn type_ref(&self) -> InternedId {
        self.type_ref
    }
    fn is_voxel(&self) -> bool {
        self.is_voxel
    }
    fn on_bridge(&self) -> bool {
        self.on_bridge
    }
    fn position(&self) -> &crate::sim::components::Position {
        &self.position
    }
    fn lifecycle(&self) -> &crate::sim::world::ObjectLifecycle {
        &self.lifecycle
    }
    fn disguise(&self) -> Option<&crate::sim::cloak_disguise::DisguiseRuntime> {
        self.disguise.as_ref()
    }
    fn cloak_state(&self) -> i32 {
        self.cloak.as_ref().map_or(0, |cloak| cloak.state())
    }
    fn cloak_depth(&self) -> i32 {
        self.cloak.as_ref().map_or(0, |cloak| cloak.depth() as i32)
    }
    fn veterancy(&self) -> &crate::sim::combat::veterancy::Veterancy {
        &self.veterancy
    }
    fn sinking(&self) -> bool {
        self.sinking.is_active()
    }
    fn building_cloak_stage(&self) -> u8 {
        self.answers.building_cloak_stage
    }
    fn native_cloak_offset_words(&self) -> i32 {
        self.answers.native_cloak_offset_words
    }
    fn is_warped_out(&self) -> bool {
        self.answers.is_warped_out
    }
    fn is_warping_in(&self) -> bool {
        self.answers.is_warping_in
    }
    fn visual_character(&self) -> u8 {
        self.seen.visual_character
    }
    fn center_xy(&self) -> [i32; 2] {
        self.place.center_xy
    }
    fn screen_position(&self) -> (f32, f32) {
        self.place.screen_position
    }
    fn world_z_leptons_off_map(&self) -> i32 {
        self.place.world_z_leptons_off_map
    }
    fn radar(&self) -> &crate::render::radar::RadarFacts {
        &self.scene.radar
    }
}

impl crate::render::snapshot::SnapshotObjects for PresentationFrame {
    type Object = ObjectRecord;
    fn object(&self, id: u64) -> Option<&ObjectRecord> {
        PresentationFrame::object(self, id)
    }
    fn objects(&self) -> impl Iterator<Item = &ObjectRecord> {
        PresentationFrame::objects(self)
    }
}

/// `GameEntity::navigation` as drawing reads it.
#[derive(Clone, Debug)]
pub(crate) struct Navigation {
    nav_com: Option<crate::sim::components::NavTargetRef>,
    nav_queue: Vec<crate::sim::components::NavTargetRef>,
}

impl Navigation {
    pub(crate) fn nav_com(&self) -> Option<crate::sim::components::NavTargetRef> {
        self.nav_com
    }

    pub(crate) fn nav_queue(&self) -> &[crate::sim::components::NavTargetRef] {
        &self.nav_queue
    }
}

/// `GameEntity::deactivated` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deactivated {
    offline: bool,
}

impl Deactivated {
    pub(crate) fn is_offline(self) -> bool {
        self.offline
    }
}

/// `GameEntity::repairing` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Repairing {
    on: bool,
}

impl Repairing {
    pub(crate) fn is_on(self) -> bool {
        self.on
    }
}

/// `GameEntity::berserk` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Berserk {
    active: bool,
}

impl Berserk {
    pub(crate) fn is_active(self) -> bool {
        self.active
    }
}

/// `GameEntity::display_type_override` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DisplayTypeOverride {
    image: Option<InternedId>,
}

impl DisplayTypeOverride {
    pub(crate) fn image(self) -> Option<InternedId> {
        self.image
    }
}

/// `GameEntity::crash` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Crash {
    crashing: bool,
}

impl Crash {
    pub(crate) fn is_crashing(&self) -> bool {
        self.crashing
    }
}

/// `GameEntity::sinking` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sinking {
    active: bool,
}

impl Sinking {
    pub(crate) fn is_active(self) -> bool {
        self.active
    }
}

/// `GameEntity::flashing` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flashing {
    frames: i32,
}

impl Flashing {
    pub(crate) fn frames(&self) -> i32 {
        self.frames
    }

    /// Bit 1 of the countdown (`sim::flashing::Flashing::flashed`).
    pub(crate) fn flashed(&self) -> bool {
        self.frames & 2 != 0
    }
}

/// Every stored object's record, for one world state and local player.
#[derive(Clone, Debug, Default)]
pub(crate) struct PresentationFrame {
    binary_frame: u32,
    /// In ascending stable-id order, which [`Self::object`] searches.
    objects: Vec<ObjectRecord>,
    /// Every anim's absolute coordinate (`Simulation::anim_absolute_coord`),
    /// which an attached anim takes from its owner object.
    anim_coords: HashMap<u64, crate::sim::anim_class::AnimWorldCoord>,
    /// What the scene's other drawers read of the world's objects
    /// (`frame_scene`).
    pub(crate) scene: crate::app::presentation::frame_scene::SceneFrame,
    /// Whether the current house has psychic detection sites.
    pub(crate) psychic_detection: bool,
}

impl PresentationFrame {
    /// The records of `runtime`'s objects, as its local player (the
    /// scenario's current house) sees them.
    pub(crate) fn capture(runtime: &SimRuntime) -> Self {
        let sim = &runtime.simulation;
        let observer_id = sim.session().current_house();
        let observer = observer_id.map(|id| sim.interner().resolve(id));
        let at = Capture {
            runtime,
            sim,
            binary_frame: sim.session().binary_frame(),
            observer,
            observer_id,
            cells: sim
                .resolved_terrain()
                .map(crate::map::resolved_terrain::NativeCellQuery::isolated),
            psychic: sim.session().current_house().and_then(|viewer| {
                let sites: Vec<_> = sim
                    .psychic_detection_sites(viewer, &runtime.resources.rules)
                    .collect();
                (!sites.is_empty()).then_some((viewer, sites))
            }),
        };
        let objects: Vec<ObjectRecord> = sim
            .entities()
            .par_filter_map(|_, entity| Some(ObjectRecord::capture(entity, &at)));
        let anim_coords = sim
            .anims()
            .filter_map(|(&id, _)| Some((id, sim.anim_absolute_coord(id)?)))
            .collect();
        let scene = crate::app::presentation::frame_scene::SceneFrame::capture(&at);
        let psychic_detection = at.psychic.is_some();
        Self {
            binary_frame: at.binary_frame,
            objects,
            anim_coords,
            scene,
            psychic_detection,
        }
    }

    /// The committed binary frame the records were captured at.
    pub(crate) fn binary_frame(&self) -> u32 {
        self.binary_frame
    }

    pub(crate) fn object(&self, id: u64) -> Option<&ObjectRecord> {
        self.objects
            .binary_search_by_key(&id, |object| object.id)
            .ok()
            .map(|slot| &self.objects[slot])
    }

    /// `Simulation::anim_absolute_coord` of anim `id`.
    pub(crate) fn anim_coord(&self, id: u64) -> Option<crate::sim::anim_class::AnimWorldCoord> {
        self.anim_coords.get(&id).copied()
    }

    pub(crate) fn contains(&self, id: u64) -> bool {
        self.object(id).is_some()
    }

    /// Every record, in stable-id order.
    pub(crate) fn objects(&self) -> impl ExactSizeIterator<Item = &ObjectRecord> {
        self.objects.iter()
    }
}

#[cfg(test)]
impl PresentationFrame {
    /// The frame of `sim` under `rules`, for a test that draws a world it
    /// built without a `SimHost`.
    pub(crate) fn of_test(sim: &Simulation, rules: &crate::rules::ruleset::RuleSet) -> Self {
        let mut simulation = sim.clone();
        // A fixture may have interned its objects' names elsewhere: give the
        // world's interner as many names, so capture can resolve their ids.
        let needed = sim
            .entities()
            .values()
            .map(|entity| entity.type_ref().index().max(entity.owner().index()) as usize + 1)
            .max()
            .unwrap_or(0);
        let interner = simulation.interner_mut_for_test();
        while interner.len() < needed {
            let filler = interner.len();
            interner.intern(&format!("TEST-FILLER-{filler}"));
        }
        let runtime = SimRuntime {
            simulation,
            resources: std::sync::Arc::new(crate::sim::runtime::SimResources::new(rules.clone())),
        };
        Self::capture(&runtime)
    }
}

#[cfg(test)]
impl ObjectRecord {
    /// The record of a test-built `entity` alone in an empty world under
    /// empty rules, with no local player, for a test that draws one object
    /// without a world. Ids resolve through the test interner.
    pub(crate) fn of_test(entity: &GameEntity) -> Self {
        Self::of_test_in(
            entity,
            None,
            crate::rules::ruleset::RuleSet::empty_for_test(),
        )
    }

    /// The record of `sim`'s object `id` under `rules`.
    pub(crate) fn of_test_sim(
        sim: &Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        id: u64,
    ) -> Self {
        PresentationFrame::of_test(sim, rules)
            .object(id)
            .cloned()
            .expect("the object has a record")
    }

    /// [`Self::of_test`] on `terrain`'s map under `rules`.
    pub(crate) fn of_test_in(
        entity: &GameEntity,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Self {
        let mut sim = Simulation::new();
        *sim.interner_mut_for_test() = crate::sim::intern::test_interner();
        // A fixture may have interned the entity's names elsewhere: give the
        // world's interner as many names, so capture can resolve its ids.
        let needed = entity.type_ref().index().max(entity.owner().index()) as usize + 1;
        let interner = sim.interner_mut_for_test();
        while interner.len() < needed {
            let filler = interner.len();
            interner.intern(&format!("TEST-FILLER-{filler}"));
        }
        *sim.resolved_terrain_mut_for_test() = terrain.cloned();
        sim.entities_mut_for_test().insert(entity.clone());
        PresentationFrame::of_test(&sim, rules)
            .object(entity.stable_id())
            .cloned()
            .expect("the inserted entity has a record")
    }
}
