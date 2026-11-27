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

use crate::app::presentation::instances::TetherDraw;
use crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder;
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
    /// The object types the local sidebar asks
    /// ([`crate::app::sidebar_projection::SidebarTypes`]).
    pub(crate) sidebar_types: &'a crate::app::sidebar_projection::SidebarTypes,
    /// Whether VERA's Convenience Help is on, so the observer's and its
    /// allies' objects capture their status rows (`HudFacts::capture`).
    pub(crate) status_rows: bool,
    /// The client modules' local looks of objects (`Decoration` `Tint`,
    /// `HideAnchor`), by object.
    pub(crate) local_looks: &'a LocalLooks,
    /// Whether the match has status templates, which read the amounts
    /// ([`StatusFacts`]).
    pub(crate) status_amounts: bool,
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

/// How a client module's `Decoration` changes an object's look on this
/// screen alone (appendix 7.10 `Tint`, `TintIntensity`, `HideAnchor`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LocalLook {
    /// `[R, G, B]` and the intensity (1.0 full) folded into the custom tint.
    pub(crate) tint: Option<([u8; 3], f64)>,
    /// The body is not drawn; the object is still selected and clicked.
    pub(crate) hidden: bool,
}

/// Every object's [`LocalLook`], by stable id.
pub(crate) type LocalLooks = std::collections::BTreeMap<u64, LocalLook>;

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
    /// The barrel's facing and elevation are the pose's `turret` and
    /// `barrel`, read at the same frame.
    pub(crate) body_facing: crate::sim::movement::FacingClass,
    pub(crate) body_frame_counter: crate::sim::animation::BodyFrameCounter,
    pub(crate) unit_firing_counter: crate::sim::animation::UnitFiringCounter,
    pub(crate) unit_death_counter: crate::sim::animation::UnitDeathCounter,
    /// The Foot occupation enable (`+0x6B6`), which a still SHP Unit's body
    /// image reads (`0x0073C6D5`).
    pub(crate) foot_occupation_enabled: bool,
    /// IsOnCarryall (`+0x424`): `UnitClass::DrawIt` draws a carried Unit's
    /// body lower, and its SHP shadows higher, by
    /// [`crate::app::presentation::instances::CARRIED_DROP_PIXELS`].
    pub(crate) on_carryall: bool,
    pub(crate) turret_anim_frame: crate::sim::combat::gattling::TurretAnimCounter,
    pub(crate) health: crate::sim::combat::Health,
    pub(crate) veterancy: crate::sim::combat::veterancy::Veterancy,
    pub(crate) flashing: Flashing,
    pub(crate) deactivated: Deactivated,
    pub(crate) repairing: Repairing,
    pub(crate) lifecycle: crate::sim::world::ObjectLifecycle,
    pub(crate) in_tube: bool,
    pub(crate) locomotor: Option<LocomotorView>,
    pub(crate) navigation: Navigation,
    pub(crate) last_fire_frame: crate::sim::combat::world_fire::LastFireFrame,
    /// Copied as the object keeps it: only a Structure that wrote its art
    /// copies the allocation, and every other record carries one pointer
    /// instead of the art inline.
    pub(crate) building_art: crate::util::sparse::Sparse<crate::sim::building_art::BuildingArt>,
    pub(crate) animation: Option<crate::sim::animation::Animation>,
    pub(crate) miner: Option<Miner>,
    pub(crate) cloak: Option<Cloak>,
    /// Whether it has a capture manager (`GameEntity::capture_manager`).
    pub(crate) capture_manager: Option<()>,
    pub(crate) spawn_manager: Option<SpawnManager>,
    pub(crate) invulnerability:
        Option<crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    pub(crate) airstrike_tint: crate::sim::superweapon::airstrike_tint::AirstrikeTintState,
    pub(crate) temporal: Temporal,
    pub(crate) berserk: Berserk,
    pub(crate) crash: Crash,
    pub(crate) sinking: Sinking,
    pub(crate) passenger_role: PassengerRole,
    /// Whether it rocks (`GameEntity::rocking`); its angles are the pose's.
    pub(crate) rocking: Option<()>,
    pub(crate) mission: Mission,
    pub(crate) tether: Tether,
    pub(crate) disguise: Option<crate::sim::cloak_disguise::DisguiseRuntime>,
    pub(crate) display_type_override: DisplayTypeOverride,
    pub(crate) selection: crate::sim::world::Selection,
    /// `GameEntity::foot_speed`'s applied fraction.
    pub(crate) applied_fraction: crate::util::native_x87::NativeF64Bits,
    /// The cargo a Carryall draws inside its own draw
    /// (`Simulation::carryall_drawn_cargo`).
    pub(crate) drawn_cargo: Option<u64>,
    /// The answers of `GameEntity`'s own queries.
    answers: Answers,
    /// [`RenderPose::read`] at the committed binary frame.
    pub(crate) pose: RenderPose,
    /// PrimaryFacing (`+0x388`) at the committed binary frame as an SHP
    /// Unit's body draw reads it: through `FacingClass::Current @ 0x004C93D0`
    /// (called at `0x0073C636`), or under Ares through its own copy
    /// (`UnitClass_DrawSHP_FacingsA`, Ares.dll `0x10048064..0x10048102`).
    /// The pose's `body` is SecondaryFacing for a Fly locomotor.
    pub(crate) shp_body_facing: u16,
    /// Its tether redraws (`instances::tether_redraw`).
    pub(crate) tether_draw: TetherDraw,
    /// What placing it reads of the world.
    pub(crate) place: Place,
    /// What the local player sees of it.
    pub(crate) seen: Seen,
    /// What its status strip reads (`ui_overlays`).
    pub(crate) hud: crate::app::presentation::ui_overlays::HudFacts,
    /// What modules change of its look and what their status
    /// templates read of it; `None` for most objects ([`ModuleFacts`]).
    module: Option<Box<ModuleFacts>>,
    /// What the scene's other drawers read of it (`frame_scene`).
    pub(crate) scene: crate::app::presentation::frame_scene::SceneFacts,
}

/// What modules change of an object's look and what their status
/// templates read of it, kept out of the record of an object they leave
/// alone: the record is copied for every object each lent tick.
#[derive(Clone, Debug, Default)]
struct ModuleFacts {
    tint: Option<([u8; 3], f64)>,
    image: Option<InternedId>,
    local_look: LocalLook,
    /// Captured only while the match has status templates.
    status: Option<StatusFacts>,
}

impl ModuleFacts {
    fn capture(entity: &GameEntity, at: &Capture<'_>, seen: Seen) -> Option<Box<Self>> {
        let facts = Self {
            tint: entity.module_tint(),
            image: at.sim.ext_module_image(entity.stable_id()).and_then(|name| at.sim.interner().get(name)),
            local_look: at
                .local_looks
                .get(&entity.stable_id())
                .filter(|_| {
                    // A tracked object disguised as terrain offers no decoration anchor.
                    !seen.disguise_hides_house && (!seen.disguised_to_observer
                        || at.sim.forced_disguise_type(entity.stable_id(), at.rules())
                            .and_then(|kind| at.sim.interner().object_type(kind, at.rules())).is_some())
                })
                .copied()
                .unwrap_or_default(),
            status: at.status_amounts.then(|| StatusFacts::capture(entity, at)),
        };
        let plain = facts.tint.is_none() && facts.image.is_none()
            && facts.local_look == LocalLook::default() && facts.status.is_none();
        (!plain).then(|| Box::new(facts))
    }
}

/// The amounts a module status template reads of an object
/// (`ui_modules::status`): read with the frame, so the templates never read
/// the live world. Visibility is the templates' to apply when they draw.
#[derive(Clone, Debug, Default)]
pub(crate) struct StatusFacts {
    /// Ammo and `Ammo=`.
    pub(crate) ammo: Option<(i32, i32)>,
    /// Passengers (and garrison occupants) and the type's `Passengers=`.
    pub(crate) passengers: Option<(i32, i32)>,
    /// The object-scope module variables it holds apart from their
    /// defaults (`VarStore::object_values`), by variable ID.
    pub(crate) vars: Option<Box<[(i32, i64)]>>,
}

impl StatusFacts {
    fn capture(entity: &GameEntity, at: &Capture<'_>) -> Self {
        let ammo = entity
            .ammo()
            .filter(|ammo| ammo.max() > 0)
            .map(|ammo| (ammo.current(), ammo.max()));
        let room = at.object_type(entity).map_or(0, |object| object.passengers);
        let role = entity.passenger_role();
        let riders = i32::try_from(role.passenger_count())
            .unwrap_or(i32::MAX)
            .saturating_add(role.occupants().count().max(0));
        let passengers = (room > 0).then_some((riders, room));
        let vars: Box<[(i32, i64)]> = at
            .sim
            .ext_modules()
            .vars()
            .object_values(entity.stable_id())
            .collect();
        Self {
            ammo,
            passengers,
            vars: (!vars.is_empty()).then_some(vars),
        }
    }
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
    building_online: bool,
    has_airstrike_marker: bool,
    building_cloak_stage: u8,
    driver_killed: bool,
    #[cfg(feature = "phobos")]
    is_power_plant_enhancer_active: bool,
    emp_remaining: i32,
    is_under_emp: bool,
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
            building_online: entity.building_online(),
            has_airstrike_marker: entity.has_airstrike_marker(),
            building_cloak_stage: entity.building_cloak_stage(),
            driver_killed: entity.driver_killed(),
            #[cfg(feature = "phobos")]
            is_power_plant_enhancer_active: entity.is_power_plant_enhancer_active(),
            emp_remaining: entity.emp_remaining(),
            is_under_emp: entity.is_under_emp(),
            #[cfg(feature = "phobos")]
            shield: entity.shield().cloned(),
            #[cfg(feature = "phobos")]
            phobos_attach_effects: entity.phobos_attach_effects().clone(),
        }
    }
}

/// What drawing reads of an object's locomotor, without its runtime
/// payload, which most kinds keep on the heap.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LocomotorView {
    kind: crate::rules::locomotor_type::LocomotorKind,
    fly_phase_callbacks: Option<(bool, bool)>,
    current_fly_speed: Option<crate::util::native_x87::NativeF64Bits>,
    rocket: Option<RocketDraw>,
    /// A Tunnel locomotor's `Is_To_Have_Shadow`.
    tunnel_has_shadow: Option<bool>,
    /// The dig-timer state `Draw_Matrix` and `Z_Adjust` read. Both take their
    /// phase from the current binary frame, so the draw snapshot must retain
    /// the timer itself rather than a pre-baked angle or depth.
    tunnel_runtime: Option<crate::sim::movement::tunnel_movement::TunnelRuntime>,
    /// A Drive locomotor's track destination and head.
    drive_track: Option<[Option<crate::sim::components::DriveCoord>; 2]>,
}

/// What a Rocket's draw reads of its `RocketLocomotionClass`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RocketDraw {
    /// `RocketRuntime::draw_key_bits` with its type's block
    /// (`RuleSet::rocket_block`).
    pub(crate) key_bits: Option<u32>,
}

impl LocomotorView {
    /// `rocket_block` gives a Rocket's block (`RuleSet::rocket_block`); a
    /// view without one keys no draw.
    pub(crate) fn of<'r>(
        locomotor: &crate::sim::movement::locomotor::LocomotorState,
        rocket_block: impl FnOnce() -> Option<&'r crate::rules::missile_spawn::MissileSpawnParams>,
    ) -> Self {
        use crate::sim::movement::track_process::TrackFamily;
        let kind = locomotor.active_kind();
        Self {
            kind,
            fly_phase_callbacks: locomotor.fly_phase_callbacks(),
            current_fly_speed: locomotor.current_fly_speed(),
            rocket: locomotor.rocket_runtime().map(|rocket| RocketDraw {
                key_bits: rocket_block().and_then(|block| rocket.draw_key_bits(block)),
            }),
            tunnel_has_shadow: locomotor
                .tunnel_runtime()
                .map(crate::sim::movement::tunnel_movement::TunnelRuntime::has_shadow),
            tunnel_runtime: locomotor.tunnel_runtime().copied(),
            drive_track: (kind == crate::rules::locomotor_type::LocomotorKind::Drive).then(|| {
                [
                    locomotor.track_destination(TrackFamily::Drive),
                    locomotor.track_head(TrackFamily::Drive),
                ]
            }),
        }
    }

    pub(crate) fn active_kind(&self) -> crate::rules::locomotor_type::LocomotorKind {
        self.kind
    }

    pub(crate) fn fly_phase_callbacks(&self) -> Option<(bool, bool)> {
        self.fly_phase_callbacks
    }

    pub(crate) fn current_fly_speed(&self) -> Option<crate::util::native_x87::NativeF64Bits> {
        self.current_fly_speed
    }

    pub(crate) fn rocket(&self) -> Option<RocketDraw> {
        self.rocket
    }

    /// A Tunnel locomotor's `Is_To_Have_Shadow`; `None` for other kinds.
    pub(crate) fn tunnel_has_shadow(&self) -> Option<bool> {
        self.tunnel_has_shadow
    }

    /// The Tunnel dig state `Draw_Matrix` and `Z_Adjust` read; `None` for
    /// every other locomotor.
    pub(crate) fn tunnel_runtime(
        &self,
    ) -> Option<&crate::sim::movement::tunnel_movement::TunnelRuntime> {
        self.tunnel_runtime.as_ref()
    }

    /// A Drive locomotor's track destination and head.
    pub(crate) fn drive_track(&self) -> Option<[Option<crate::sim::components::DriveCoord>; 2]> {
        self.drive_track
    }
}

/// What placing an object reads of the world beside the object.
#[derive(Clone, Debug)]
pub(crate) struct Place {
    /// `sim::movement::ground_pose::object_world_z_leptons` on the live map.
    pub(crate) world_z_leptons: i32,
    /// `sim::movement::air_movement::queried_flight_height`.
    pub(crate) flight_height: i32,
    /// `sim::movement::air_movement::aircraft_landing_base`.
    pub(crate) landing_base: i32,
    /// `sim::movement::hover::hover_draw_slope` with the pose's rocking angles.
    pub(crate) hover_draw_slope: u8,
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
    /// Infantry5192C9 reads the navigation cell for top lighting.
    pub(crate) infantry_light_cell: Option<(u16, u16)>,
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
        if !entity.seldom.airstrike.links_to(id) {
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
    pub(crate) category: EntityCategory,
    pub(crate) type_ref: InternedId,
    pub(crate) mission: Mission,
}

/// A Structure's `sim::building_art` answers for its own type.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StructureArt {
    pub(crate) invisible_in_game: bool,
    /// `building_image` without and with a Buildup.
    pub(crate) image: [crate::sim::building_art::BuildingImage; 2],
    /// `current_body_frame` at its type's tech level.
    pub(crate) current_body_frame: i32,
    /// `gate_open_body_frame` with its art's GateStages, for a gate.
    pub(crate) gate_open_body_frame: Option<i32>,
    /// How a Wasm module's `DoorOpen` write holds its door drawn
    /// (`Simulation::ext_module_door`).
    pub(crate) module_door: Option<bool>,
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
            invisible_in_game: object.invisible_in_game,
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
                match at.sim.ext_module_door(entity.stable_id()) {
                    Some(open) => building_art::gate_held_body_frame(
                        entity,
                        gate_stages,
                        object.strength,
                        rules.general.condition_yellow,
                        open,
                    ),
                    None => building_art::gate_open_body_frame(
                        entity,
                        gate_stages,
                        object.strength,
                        rules.general.condition_yellow,
                        at.binary_frame,
                    ),
                }
            }),
            module_door: at.sim.ext_module_door(entity.stable_id()),
        })
    }
}

impl Place {
    /// `world_z_leptons` is `object_world_z_leptons` on the live map.
    fn of(entity: &GameEntity, pose: &RenderPose, world_z_leptons: i32, at: &Capture<'_>) -> Self {
        let sim = at.sim;
        let display_frame = at.binary_frame.wrapping_sub(1);
        let psychically_detected = at.psychic.as_ref().is_some_and(|(viewer, sites)| {
            sim.techno_intent_psychically_detected(
                entity.stable_id(),
                *viewer,
                sites,
                at.cells.as_ref(),
            )
        });
        // Only a selected object's line, a psychically detected one's and the
        // attack line of a `TargetLaser=` Unit, selected or not, are drawn
        // (`target_lines`, `target_laser_draws`).
        let draws_action_line = psychically_detected
            || (entity.seldom.selection.is_selected()
                && entity.category() != EntityCategory::Structure)
            || (entity.category() == EntityCategory::Unit
                && entity.attack_target.target().is_some()
                && at
                    .rules()
                    .object(sim.interner().resolve(entity.type_ref()))
                    .is_some_and(|object| object.target_laser));
        Self {
            world_z_leptons,
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
            moving_now: crate::sim::movement::motion_query::is_moving_now(
                entity,
                crate::sim::movement::SpeedRules::new(at.rules(), sim.interner(), sim.houses()),
                display_frame,
            ),
            moving: crate::sim::movement::motion_query::is_moving(entity),
            center_xy: crate::sim::movement::ground_pose::object_center_xy(entity),
            infantry_light_cell: (entity.category() == EntityCategory::Infantry)
                .then(|| sim.foot_navigation_coordinate(entity.stable_id()).ok())
                .flatten()
                .map(|coord| {
                    (
                        crate::util::lepton::lepton_to_cell_packed(coord.x) as u16,
                        crate::util::lepton::lepton_to_cell_packed(coord.y) as u16,
                    )
                }),
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
            action_line: draws_action_line
                .then(|| {
                    crate::app::presentation::target_lines::action_line_endpoints(
                        entity,
                        sim,
                        Some(at.rules()),
                        at.cells.as_ref(),
                    )
                })
                .flatten(),
            rally_line: at.cells.as_ref().and_then(|cells| {
                crate::app::presentation::target_lines::rally_line_coords(entity, sim, cells)
            }),
            psychically_detected,
            // Only a Structure's is ever active.
            airstrike_tint_active: entity.category() == EntityCategory::Structure
                && sim.airstrike_tint_active(entity.stable_id()),
            // Read only by the DeactivateDim lighting, which runs when Ares plays.
            ares_operated: at.rules().ares()
                && sim.ares_is_operated(entity.stable_id(), at.rules()),
            #[cfg(feature = "phobos")]
            airstrike_laser_target_color: sim
                .airstrike_laser_target_color(entity.stable_id(), at.rules()),
            contact0: entity
                .seldom
                .radio_contacts
                .slot(0)
                .and_then(|id| sim.entities().get(id))
                .map(|contact| Contact {
                    category: contact.category(),
                    type_ref: contact.type_ref(),
                    mission: Mission::of(&contact.mission),
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
            entity.seldom.discovery.owned_by_current_house(),
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
        Self {
            observer,
            visual_character,
            disguised_to_observer: at.observer_id.is_some_and(disguised_to),
            disguise_hides_house: at.observer_id.is_some_and(hides_house),
        }
    }
}

impl ObjectRecord {
    fn capture(entity: &GameEntity, at: &Capture<'_>) -> Self {
        let pose = RenderPose::read(entity, at.binary_frame);
        let answers = Answers::of(entity);
        // `object_location` on the live map, which placing, the HUD and the
        // radar all read.
        let location =
            crate::sim::movement::ground_pose::object_location(entity, at.sim.resolved_terrain());
        let place = Place::of(entity, &pose, location.z, at);
        let seen = Seen::of(entity, at);
        let hud = crate::app::presentation::ui_overlays::HudFacts::capture(entity, at, location);

        let scene =
            crate::app::presentation::frame_scene::SceneFacts::capture(entity, at, location);
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
            position: entity.position,
            body_facing: entity.body_facing,
            body_frame_counter: entity.body_frame_counter,
            unit_firing_counter: entity.unit_firing_counter,
            unit_death_counter: entity.unit_death_counter,
            foot_occupation_enabled: entity.foot_occupation_enabled(),
            on_carryall: entity.is_on_carryall(),
            turret_anim_frame: entity.turret_anim_frame,
            health: entity.health,
            veterancy: entity.veterancy,
            flashing: Flashing {
                frames: entity.flashing.frames(),
            },
            deactivated: Deactivated {
                offline: entity.deactivated.is_offline(),
            },
            repairing: Repairing {
                on: entity.building.repairing.is_on(),
            },
            lifecycle: entity.lifecycle,
            in_tube: entity.low_bridge_tube_state.is_some(),
            locomotor: entity.locomotor.as_ref().map(|locomotor| {
                LocomotorView::of(locomotor, || {
                    let type_name = at.sim.interner().resolve(entity.type_ref());
                    Some(at.rules().rocket_block(type_name).0)
                })
            }),
            navigation: Navigation {
                nav_com: entity.navigation.nav_com(),
            },
            last_fire_frame: entity.last_fire_frame,
            // The frame owns what it draws: the simulation moves on before
            // the frame reaches the screen. Only a Structure holds art.
            building_art: entity.building_art.clone(),
            animation: entity.animation,
            miner: entity.miner.as_deref().map(|miner| Miner {
                harvesting: miner.harvesting(),
            }),
            cloak: entity.cloak.as_deref().map(|cloak| Cloak {
                state: cloak.state(),
                depth: cloak.depth(),
            }),
            capture_manager: entity.capture_manager.as_ref().map(|_| ()),
            spawn_manager: entity.spawn_manager.as_deref().map(|manager| SpawnManager {
                docked: manager.count_docked_spawns(),
            }),
            invulnerability: entity.invulnerability,
            airstrike_tint: entity.airstrike_tint,
            temporal: Temporal {
                warped: entity.temporal.is_warped(),
            },
            berserk: Berserk {
                active: entity.berserk.is_active(),
            },
            crash: Crash {
                crashing: entity.crash.is_crashing(),
            },
            sinking: Sinking {
                active: entity.sinking.is_active(),
            },
            passenger_role: PassengerRole::of(entity.passenger_role()),
            rocking: entity.rocking.as_ref().map(|_| ()),
            mission: Mission::of(&entity.mission),
            tether: Tether {
                partner: entity.seldom.tether.partner().is_some(),
            },
            disguise: entity.seldom.disguise,
            display_type_override: DisplayTypeOverride {
                image: entity.seldom.display_type_override.image(),
            },
            selection: entity.seldom.selection,
            applied_fraction: entity.foot_speed.native_applied_fraction(),
            // Only an Aircraft can be a Carryall.
            drawn_cargo: (entity.category() == EntityCategory::Aircraft)
                .then(|| at.sim.carryall_drawn_cargo(entity.stable_id(), at.rules()))
                .flatten(),
            answers,
            place,
            seen,
            hud,
            module: ModuleFacts::capture(entity, at, seen),
            scene,
            pose,
            shp_body_facing: if at.rules().ares() {
                entity.body_facing_current_ares(at.binary_frame)
            } else {
                entity.body_facing_current(at.binary_frame)
            },
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

    pub(crate) fn building_online(&self) -> bool {
        self.answers.building_online
    }

    pub(crate) fn has_airstrike_marker(&self) -> bool {
        self.answers.has_airstrike_marker
    }

    pub(crate) fn building_cloak_stage(&self) -> u8 {
        self.answers.building_cloak_stage
    }

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

    /// vt+0x37C IsUnderEMP ([`GameEntity::is_under_emp`]).
    pub(crate) fn is_under_emp(&self) -> bool {
        self.answers.is_under_emp
    }

    /// `GameEntity::voxel_recoil`, which the pose holds.
    pub(crate) fn voxel_recoil(&self) -> ([f32; 2], bool) {
        self.pose.recoil
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn shield(&self) -> Option<&crate::sim::shield::ShieldState> {
        self.answers.shield.as_ref()
    }

    /// The simulation module's tint and intensity.
    pub(crate) fn module_tint(&self) -> Option<([u8; 3], f64)> {
        self.module.as_ref().and_then(|module| module.tint)
    }

    /// The type a simulation module draws in place of this object's own.
    pub(crate) fn module_image(&self) -> Option<InternedId> {
        self.module.as_ref().and_then(|module| module.image)
    }

    /// A client module's local look of it.
    pub(crate) fn local_look(&self) -> LocalLook {
        self.module.as_ref().map_or_else(LocalLook::default, |module| module.local_look)
    }

    /// What status templates read of it; `None` in a match without
    /// templates.
    pub(crate) fn status(&self) -> Option<&StatusFacts> {
        self.module.as_ref().and_then(|module| module.status.as_ref())
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
        let [x, y] = self.pose.screen;
        (x, y)
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
}

impl Navigation {
    pub(crate) fn nav_com(&self) -> Option<crate::sim::components::NavTargetRef> {
        self.nav_com
    }
}

/// `GameEntity::mission` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mission {
    current: crate::sim::mission::MissionId,
    queued: crate::sim::mission::MissionId,
    effective: crate::sim::mission::MissionId,
}

impl Mission {
    fn of(mission: &crate::sim::mission::MissionCom) -> Self {
        Self {
            current: mission.current(),
            queued: mission.queued(),
            effective: mission.effective(),
        }
    }

    pub(crate) fn current(&self) -> crate::sim::mission::MissionId {
        self.current
    }

    pub(crate) fn queued(&self) -> crate::sim::mission::MissionId {
        self.queued
    }

    pub(crate) fn effective(&self) -> crate::sim::mission::MissionId {
        self.effective
    }
}

/// `GameEntity::miner` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Miner {
    harvesting: bool,
}

impl Miner {
    pub(crate) fn harvesting(&self) -> bool {
        self.harvesting
    }
}

/// `GameEntity::cloak` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cloak {
    state: i32,
    depth: u32,
}

impl Cloak {
    pub(crate) fn state(&self) -> i32 {
        self.state
    }

    pub(crate) fn depth(&self) -> u32 {
        self.depth
    }
}

/// `GameEntity::spawn_manager` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpawnManager {
    docked: usize,
}

impl SpawnManager {
    /// `SpawnManagerClass::CountDockedSpawns` (`0x006B7D50`).
    pub(crate) fn count_docked_spawns(&self) -> usize {
        self.docked
    }
}

/// `GameEntity::temporal` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Temporal {
    warped: bool,
}

impl Temporal {
    /// `TemporalState::is_warped`: a chain warps it.
    pub(crate) fn is_warped(&self) -> bool {
        self.warped
    }
}

/// `GameEntity::tether` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tether {
    partner: bool,
}

impl Tether {
    /// Whether it has a tether partner (`Tether::partner`).
    pub(crate) fn has_partner(&self) -> bool {
        self.partner
    }
}

/// `GameEntity::passenger_role` as drawing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PassengerRole {
    inside_transport: bool,
    #[cfg(feature = "phobos")]
    cargo: Option<Cargo>,
}

impl PassengerRole {
    fn of(role: &crate::sim::passenger::PassengerRole) -> Self {
        Self {
            inside_transport: role.is_inside_transport(),
            #[cfg(feature = "phobos")]
            cargo: role.cargo().map(|cargo| Cargo {
                count: cargo.count(),
                total_size: cargo.total_size(),
            }),
        }
    }

    pub(crate) fn is_inside_transport(&self) -> bool {
        self.inside_transport
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn cargo(&self) -> Option<&Cargo> {
        self.cargo.as_ref()
    }
}

/// A transport's `PassengerCargo` as drawing reads it.
#[cfg(feature = "phobos")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cargo {
    count: u32,
    total_size: i32,
}

#[cfg(feature = "phobos")]
impl Cargo {
    pub(crate) fn count(&self) -> u32 {
        self.count
    }

    pub(crate) fn total_size(&self) -> i32 {
        self.total_size
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
    /// Bit 1 of the countdown (`sim::flashing::Flashing::flashed`).
    pub(crate) fn flashed(&self) -> bool {
        self.frames & 2 != 0
    }
}

/// Where a frame's ascending record ids hold each id, found without searching
/// all of them: the id range is cut into buckets of `1 << shift` ids, no more
/// buckets than records, and `starts` keeps each bucket's first slot, so an
/// id's search covers only its own bucket. It answers as a search of every id
/// would.
#[derive(Debug, Default)]
struct SlotIndex {
    /// The first record's id.
    base: u64,
    shift: u32,
    /// Each bucket's first slot, then the record count.
    starts: Vec<u32>,
}

impl SlotIndex {
    fn of(ids: &[u64]) -> Self {
        let (Some(&base), Some(&last)) = (ids.first(), ids.last()) else {
            return Self::default();
        };
        debug_assert!(ids.is_sorted_by(|a, b| a < b), "record ids ascend");
        // Buckets wider than the ids' mean gap.
        let gap = (last - base) / ids.len() as u64;
        let shift = (u64::BITS - gap.leading_zeros()).min(u64::BITS - 1);
        let buckets = (last - base) >> shift;
        let mut starts = Vec::with_capacity(buckets as usize + 2);
        let mut slot = 0;
        for bucket in 0..=buckets {
            while ids
                .get(slot)
                .is_some_and(|&id| (id - base) >> shift < bucket)
            {
                slot += 1;
            }
            starts.push(slot as u32);
        }
        starts.push(ids.len() as u32);
        Self {
            base,
            shift,
            starts,
        }
    }

    /// `id`'s slot in `ids`, the ids this index was built of.
    fn slot(&self, ids: &[u64], id: u64) -> Option<usize> {
        let bucket = usize::try_from(id.checked_sub(self.base)? >> self.shift).ok()?;
        let start = *self.starts.get(bucket)? as usize;
        let end = *self.starts.get(bucket + 1)? as usize;
        let at = ids.get(start..end)?.binary_search(&id).ok()?;
        Some(start + at)
    }
}

/// What a frame lists of its records, in their order: each one's id, with
/// its index, and screen span, and the slots of the Structures and of the
/// records that may draw a line.
struct Listed {
    ids: Vec<u64>,
    index: SlotIndex,
    spans: Vec<crate::app::presentation::display_members::ScreenSpan>,
    structures: Vec<usize>,
    line_sources: Vec<usize>,
}

impl Listed {
    fn of(objects: &[ObjectRecord]) -> Self {
        let mut listed = Self {
            ids: Vec::with_capacity(objects.len()),
            index: SlotIndex::default(),
            spans: Vec::with_capacity(objects.len()),
            structures: Vec::new(),
            line_sources: Vec::new(),
        };
        for (slot, object) in objects.iter().enumerate() {
            listed.ids.push(object.id);
            listed
                .spans
                .push(crate::app::presentation::display_members::ScreenSpan::of(
                    object,
                ));
            if object.category() == EntityCategory::Structure {
                listed.structures.push(slot);
            }
            if object.may_draw_a_line() {
                listed.line_sources.push(slot);
            }
        }
        listed.index = SlotIndex::of(&listed.ids);
        listed
    }
}

/// Every stored object's record, for one world state and local player.
#[derive(Debug, Default)]
pub(crate) struct PresentationFrame {
    /// In ascending stable-id order.
    objects: Vec<ObjectRecord>,
    /// Each record's stable id, in the same order: what [`Self::object`]
    /// searches. A draw asks for every Display member, and a search over the
    /// records themselves lands on a different large record at each step.
    ids: Vec<u64>,
    /// Where in `ids` each id's search starts ([`SlotIndex`]).
    index: SlotIndex,
    /// Each record's screen span, in the same order: what the object loop
    /// culls from without reading the record.
    spans: Vec<crate::app::presentation::display_members::ScreenSpan>,
    /// The slots of the building records, ascending: what the building
    /// passes walk without reading every other record ([`Self::structures`]).
    structures: Vec<usize>,
    /// The slots of the records that can draw a tactical line, ascending
    /// ([`ObjectRecord::may_draw_a_line`], [`Self::line_sources`]).
    line_sources: Vec<usize>,
    /// The Display members of the world this frame was captured from, filed
    /// by the first read ([`Self::display_members`]): before a tick is lent
    /// (`SimHost::lend`), or by the first draw.
    display_members:
        crate::util::sync::OnceLock<crate::app::presentation::display_members::DisplayMembers>,
    /// The Display order of the world this frame was captured from, sorted
    /// beside its records: Ground sorts by the objects, which the world copy
    /// a lent tick's draws read leaves out.
    display_order: NativeDisplayOrder,
    /// Every anim's absolute coordinate (`Simulation::anim_absolute_coord`),
    /// which an attached anim takes from its owner object.
    anim_coords: crate::util::lookup_hash::LookupMap<u64, crate::sim::anim_class::AnimWorldCoord>,
    /// Object+88's held canopy identities, copied from GameEntity at capture.
    /// The frame rebuilds this sparse lookup each time; ordinary objects add no entry.
    parachute_animations: crate::util::lookup_hash::LookupMap<u64, u64>,
    /// Each control group's members, with the queued assignments
    /// (`Simulation::projected_control_groups`), for the group digits
    /// (`ui_overlays`).
    control_groups: [Vec<u64>; 10],
    /// What the scene's other drawers read of the world's objects
    /// (`frame_scene`).
    pub(crate) scene: crate::app::presentation::frame_scene::SceneFrame,
    /// Whether the current house has psychic detection sites.
    pub(crate) psychic_detection: bool,
}

impl PresentationFrame {
    /// Free the frame's 20,000 object records on the pool. The frame holds
    /// none of the match's resources, so a Rules write need not wait for it.
    pub(crate) fn retire(self: Box<Self>) {
        crate::sim::parallel::drop_on_pool(self);
    }

    /// The records of `runtime`'s objects, as its local player (the
    /// scenario's current house) sees them, with the sidebar's answers for
    /// `sidebar_types`.
    pub(crate) fn capture(
        runtime: &SimRuntime,
        sidebar_types: &crate::app::sidebar_projection::SidebarTypes,
        status_rows: bool,
    ) -> Self {
        Self::capture_with_looks(runtime, sidebar_types, status_rows, &LocalLooks::new(), false)
    }

    /// [`Self::capture`] with the client modules' local looks of objects.
    pub(crate) fn capture_with_looks(
        runtime: &SimRuntime,
        sidebar_types: &crate::app::sidebar_projection::SidebarTypes,
        status_rows: bool,
        local_looks: &LocalLooks,
        status_amounts: bool,
    ) -> Self {
        let sim = &runtime.simulation;
        let observer_id = sim.session().current_house();
        let observer = observer_id.map(|id| sim.interner().resolve(id));
        let at = Capture {
            runtime,
            sim,
            binary_frame: sim.session().binary_frame(),
            observer,
            observer_id,
            sidebar_types,
            status_rows,
            local_looks,
            status_amounts,
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
        // What reads no record is captured beside the records, each part a
        // task of the pool: the Display order, and the anims' coordinates,
        // the parachutes and the control groups together. The scene frame is
        // captured here.
        let mut display_order = None;
        let mut anim_coords = Default::default();
        let mut parachute_animations = Default::default();
        let mut control_groups = Default::default();
        let mut records = None;
        let mut scene = None;
        let sort = || display_order = Some(NativeDisplayOrder::from_world(sim, Some(at.rules())));
        let gather = || {
            anim_coords = sim.anim_absolute_coords().collect();
            parachute_animations = sim
                .entities()
                .values()
                .filter_map(|entity| {
                    entity
                        .parachute_animation()
                        .map(|animation| (entity.stable_id(), animation))
                })
                .collect();
            control_groups = sim.projected_control_groups();
        };
        let record = || {
            let objects = sim
                .entities()
                .par_map(|_, entity| ObjectRecord::capture(entity, &at));
            records = Some((Listed::of(&objects), objects));
        };
        crate::sim::parallel::alongside(
            || {
                crate::sim::parallel::alongside(sort, || {
                    crate::sim::parallel::alongside(gather, record)
                })
            },
            || {
                scene = Some(crate::app::presentation::frame_scene::SceneFrame::capture(
                    &at,
                ))
            },
        );
        let scene = scene.expect("captured beside the records");
        let display_order = display_order.expect("sorted beside the records");
        let (listed, objects) = records.expect("captured on the pool");
        let Listed {
            ids,
            index,
            spans,
            structures,
            line_sources,
        } = listed;
        let psychic_detection = at.psychic.is_some();
        Self {
            objects,
            index,
            ids,
            spans,
            structures,
            line_sources,
            display_members: Default::default(),
            display_order,
            anim_coords,
            parachute_animations,
            control_groups,
            scene,
            psychic_detection,
        }
    }

    pub(crate) fn object(&self, id: u64) -> Option<&ObjectRecord> {
        self.index
            .slot(&self.ids, id)
            .and_then(|slot| self.record_at(slot))
    }

    /// Record `id`'s slot and screen span, without reading the record.
    pub(crate) fn slot_and_span(
        &self,
        id: u64,
    ) -> Option<(usize, crate::app::presentation::display_members::ScreenSpan)> {
        let slot = self.index.slot(&self.ids, id)?;
        Some((slot, *self.spans.get(slot)?))
    }

    /// The record in `slot` ([`Self::slot_and_span`]).
    pub(crate) fn record_at(&self, slot: usize) -> Option<&ObjectRecord> {
        self.objects.get(slot)
    }

    /// The Display members of `sim`, filed once for this frame's draws.
    /// `sim` is the world the frame was captured from.
    pub(crate) fn display_members(
        &self,
        sim: &Simulation,
    ) -> &crate::app::presentation::display_members::DisplayMembers {
        self.display_members.get_or_init(|| {
            crate::app::presentation::display_members::DisplayMembers::of(self, sim)
        })
    }

    /// The draw order of the world this frame was captured from, for every
    /// draw of it.
    pub(crate) fn display_order(&self) -> &NativeDisplayOrder {
        &self.display_order
    }

    /// `Simulation::anim_absolute_coord` of anim `id`.
    pub(crate) fn anim_coord(&self, id: u64) -> Option<crate::sim::anim_class::AnimWorldCoord> {
        self.anim_coords.get(&id).copied()
    }

    /// `GameEntity::parachute_animation` of object `id` at capture.
    pub(crate) fn parachute_animation(&self, id: u64) -> Option<u64> {
        self.parachute_animations.get(&id).copied()
    }

    /// Each control group's members at capture, with the queued assignments
    /// (`Simulation::projected_control_groups`).
    pub(crate) fn control_groups(&self) -> &[Vec<u64>; 10] {
        &self.control_groups
    }

    /// Whether `object` stands for its own sound handles
    /// (`Simulation::object_sound_owner_alive`): an object while it is out of
    /// limbo, else a live terrain object.
    pub(crate) fn sound_object_stands(&self, object: u64) -> bool {
        self.object(object).map_or_else(
            || self.scene.ui.terrain_object_stands(object),
            |record| !record.in_limbo(),
        )
    }

    pub(crate) fn contains(&self, id: u64) -> bool {
        self.object(id).is_some()
    }

    /// Every record, in stable-id order.
    pub(crate) fn objects(&self) -> impl ExactSizeIterator<Item = &ObjectRecord> {
        self.objects.iter()
    }

    /// Every building's record, in stable-id order: the [`Self::objects`]
    /// whose category is Structure, found without reading the others.
    pub(crate) fn structures(&self) -> impl ExactSizeIterator<Item = &ObjectRecord> {
        self.structures.iter().map(|&slot| &self.objects[slot])
    }

    /// The records that can draw a tactical line, in stable-id order: the
    /// [`Self::objects`] the line passes visit, without reading the others.
    pub(crate) fn line_sources(&self) -> impl ExactSizeIterator<Item = &ObjectRecord> {
        self.line_sources.iter().map(|&slot| &self.objects[slot])
    }
}

impl ObjectRecord {
    /// Whether Tactical's Techno walk (`TacticalClass_Draw`, `0x006D4750..`)
    /// can draw a line for it: a selected object's action line (vt+0x438),
    /// the mind-control links of its own CaptureManager or its controller's
    /// (`CaptureManagerClass::DrawLinks`), its airstrike designation, or its
    /// psychic detection line (`FootClass::DrawPsychicDetectionLine`). An
    /// object with none of these draws nothing there; each line's own tests
    /// still decide for the others.
    fn may_draw_a_line(&self) -> bool {
        self.selection.is_selected()
            || self.capture_manager.is_some()
            || self.hud.controller().is_some()
            || self.place.airstrike_designation.is_some()
            || self.place.psychically_detected
    }
}

#[cfg(test)]
impl PresentationFrame {
    /// The frame of `sim` under `rules`, for a test that draws a world it
    /// built without a `SimHost`.
    pub(crate) fn of_test(sim: &Simulation, rules: &crate::rules::ruleset::RuleSet) -> Self {
        Self::of_test_with_status_rows(sim, rules, false)
    }

    /// [`Self::of_test`], capturing VERA's Convenience Help rows when
    /// `status_rows`.
    pub(crate) fn of_test_with_status_rows(
        sim: &Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        status_rows: bool,
    ) -> Self {
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
        let types =
            crate::app::sidebar_projection::SidebarTypes::every_rules_type(&simulation, rules);
        let runtime = SimRuntime {
            simulation,
            resources: std::sync::Arc::new(crate::sim::runtime::SimResources::new(rules.clone())),
        };
        Self::capture(&runtime, &types, status_rows)
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

#[cfg(all(test, tests_app_presentation))]
#[path = "frame_tests.rs"]
mod tests;
