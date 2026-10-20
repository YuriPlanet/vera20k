//! Techno identity and component construction, before store/Reveal admission.
//!
//! Map import keeps authored identity/health/placement; live and held runtime
//! callers share one constructor. Child allocation still belongs to the parent
//! store transaction in world_spawn, after these manager descriptors exist.

use super::{GeneratedTechnoInitError, object_uses_voxel};
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::{Animation, SequenceKind};
use crate::sim::combat::Health;
use crate::sim::game_entity::{GameEntity, TechnoConstructorInit};
use crate::sim::houses::Houses;
use crate::sim::intern::StringInterner;
use crate::sim::miner::{Miner, MinerConfig, miner_kind_for_object};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::scenario_session::ScenarioSession;
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
        // Native class constructors copy signed type Strength unchanged.
        // Original copy slices: object_health corpus / constructors.
        let health = Health::new(obj.strength);
        let category = EntityCategory::from(obj.category);
        let uses_voxel = object_uses_voxel(type_id, obj, rules);
        let sight_range = (obj.sight.max(0) as u16).min(MAX_SIGHT_RANGE);
        let stable_id = self.substrate.allocate_stable_id();
        let owner_iid = self.interner.intern(owner);
        let type_iid = self.interner.intern(type_id);
        let techno_ctor_random_word =
            super::resolve_techno_constructor_word(&mut self.scenario_rng, init, None)?;
        let native_unique_id = self.next_native_runtime_id();

        let mut ge = GameEntity::new_at_frame_from_constructor_word(
            stable_id,
            native_unique_id,
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
            self.session.binary_frame(),
            techno_ctor_random_word,
        );

        Self::install_techno_components(
            &mut self.interner,
            &self.session,
            &self.houses,
            &mut ge,
            Some(obj),
            Some(rules),
            ComponentOrigin::Runtime,
        );
        Ok(Some(ge))
    }

    pub(super) fn install_techno_components(
        interner: &mut StringInterner,
        session: &ScenarioSession,
        houses: &Houses,
        ge: &mut GameEntity,
        obj: Option<&ObjectType>,
        rules: Option<&RuleSet>,
        origin: ComponentOrigin,
    ) {
        let category = ge.category();
        let initial = ge.body_facing.destination();
        let uses_voxel = ge.is_voxel();
        // InitManagers6F3F40 classifies the owner after construction; it does
        // not manufacture either discovery-history byte. The launch-owned
        // current house is distinct from the notification/viewer binding.
        ge.discovery
            .classify_owner(session.current_house() == Some(ge.owner()));

        stamp_scoring_flags(ge, obj, rules);
        if let Some(object) = obj {
            ge.sight.stamp_type_sight(object.sight);
        }
        // The class constructor's one `+0x388` rate write (`Set_ROT`): `ROT=`
        // for a unit (`0x00735579`), an aircraft (`0x00413FE7`) and a building
        // (`BuildingClass::Init`, `0x00442CA5`, without testing `Turret=`: a
        // turretless building's `+0x388` is the body Mission_Attack aims and
        // ReceiveDamage's retaliation turns); 127 for infantry (`0x00517BC5`).
        if let Some(obj) = obj {
            ge.set_body_facing_rot(obj.turret_rot);
        }
        // `+0x3A0`: a turreted unit's turret and every aircraft's Secondary. A
        // building aims its `+0x388`.
        if let Some(obj) = obj.filter(|obj| {
            (obj.has_turret && category != EntityCategory::Structure)
                || category == EntityCategory::Aircraft
        }) {
            let mut secondary = crate::sim::movement::FacingClass::new(initial, obj.turret_rot);
            if category == EntityCategory::Aircraft {
                // Aircraft413FD2..414015 supplies ROT to BOTH controllers;
                // Unlimbo414310 -> Foot4D7170 -> Techno6F6DAA snaps Primary;
                // Aircraft414417 snaps Secondary. Both timers retain this frame.
                let frame = session.binary_frame();
                ge.body_facing.snap(initial, frame);
                secondary.snap(initial, frame);
            }
            ge.barrel_facing = Some(secondary);
        }
        if category == EntityCategory::Infantry {
            match origin {
                ComponentOrigin::Authored { sub_cell, .. } => {
                    ge.sub_cell.hold(sub_cell);
                    let (lx, ly) = crate::util::lepton::subcell_lepton_offset(ge.sub_cell());
                    ge.position.put_cell_offset(lx, ly);
                }
                ComponentOrigin::Runtime => {
                    // Infantry51DFF0 chooses the spot only at Unlimbo, from
                    // the caller's actual XY/Z and live occupation bytes.
                    // Constructor-held objects reserve no spot and retain
                    // their requested XY until the shared placement boundary.
                    ge.sub_cell.release();
                }
            }
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
        if category == EntityCategory::Structure {
            // Building43BCBD..43BCD0 calls Radio65AE60 with the signed
            // NumberOfDocks floored at one. Both authored and runtime objects
            // own their actual slots before any scan or HELLO. Original
            // constructor controls: tools/spatial_oracle/refinery_dock.json.
            ge.radio_contacts =
                crate::sim::radio::Contacts::with_capacity(obj.dock_contact_capacity() as usize);
        }
        // Unit constructor735678 and InitFromType74689B initialize the empty
        // Gunner through the shared selector, including its charge-type gate.
        if category == EntityCategory::Unit && obj.gunner {
            ge.set_gunner_weapon(0, obj);
        }
        ge.initialize_voxel_recoil(obj.recoil);
        ge.crushable_facts = CrushableFacts::stamp(obj);
        ge.mover_facts = MoverFacts::stamp(obj);
        ge.removal_facts.stamp_occupier(obj.occupier);
        if category == EntityCategory::Structure && obj.gate {
            ge.building_gate = Some(crate::sim::game_entity::BuildingGateRuntime::at_frame(
                session.binary_frame(),
            ));
        }
        if category == EntityCategory::Structure && obj.bunker {
            ge.bunker_runtime = Some(crate::sim::docking::bunker_install::BunkerRuntime::idle());
        }
        // BuildingClass::Init_Managers, from the constructor (`0x0043BB29`):
        // no Buildup SHP clears the AI sale byte (`0x00442CBC`).
        if category == EntityCategory::Structure
            && let Some(rules) = rules
        {
            ge.bind_building_construction_control(rules.buildup_control(&obj.id));
            if matches!(origin, ComponentOrigin::Authored { .. }) {
                ge.initialize_building_idle_body(session.binary_frame() as i32);
            }
        }
        ge.ai_sell_repair.construct(
            category == EntityCategory::Structure
                && rules.is_some_and(|rules| rules.has_buildup(&obj.id)),
        );
        if should_construct_locomotor(category, obj) {
            ge.locomotor = Some(LocomotorState::from_object_type(
                obj,
                session.binary_frame(),
            ));
            if matches!(origin, ComponentOrigin::Runtime)
                && ge.locomotor.as_ref().is_some_and(|locomotor| {
                    locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Ship
                })
            {
                ge.locomotor
                    .as_mut()
                    .unwrap()
                    .ensure_installed_track_state();
            }
        }
        // The concrete Foot constructors all select InitialAmmo unless it is
        // exactly -1; otherwise Ammo. Chrono reinforcement65EC30 also reads
        // and decrements this count for ground Units (ammo module evidence).
        if matches!(
            category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        ) {
            ge.initialize_ammo_from_type(obj);
        }
        // InitManagers6F40FC..6F414E: a weapon-0 Parasite warhead owns its
        // ParasiteClass (Foot+69C) from construction, not from the first shot.
        if let Some(rules) = rules
            && crate::sim::combat::parasite::type_allocates_parasite(
                rules,
                obj,
                category,
                ge.veterancy.rank(),
            )
        {
            ge.parasite.allocate_class(session.binary_frame());
        }
        install_authored_bridge(ge, origin);

        if let Some(kind) = miner_kind_for_object(obj) {
            let mcfg: MinerConfig = rules.map(MinerConfig::from_rules).unwrap_or_default();
            let storage = obj.storage.max(0) as u16;
            ge.miner = Some(Miner::new(kind, &mcfg, storage));
        }
        // Passenger cargo for transports and garrisonable buildings.
        if obj.passengers > 0 {
            ge.passenger_role = crate::sim::passenger::PassengerRole::transport(
                crate::sim::passenger::PassengerCargo::new(obj.passengers as u32, obj.size_limit),
            );
        } else if obj.can_be_occupied && obj.max_number_occupants > 0 {
            ge.passenger_role = crate::sim::passenger::PassengerRole::transport(
                crate::sim::passenger::PassengerCargo::new(
                    obj.max_number_occupants,
                    obj.size_limit,
                ),
            );
        }

        stamp_building_cell_profile(ge, Some(obj));
        // TechnoClass::Init_Managers — manager-owned children are committed
        // after this parent enters the limbo store and before its Unlimbo.
        if let Some(rules) = rules {
            ge.capture_manager = crate::sim::capture_manager::init_capture_manager(obj, rules);
            ge.temporal = crate::sim::temporal::init_temporal(obj, rules);
            ge.spawn_manager = crate::sim::spawn_manager::init_spawn_manager(
                obj,
                rules,
                interner,
                session.binary_frame(),
            );
            // 0x006F41FE..0x006F4271: a CanDisguise=/PermaDisguise= type (stock
            // SPY) starts disguised as its owner, wearing the Rules disguise
            // of the owner's side (House+0x1E8: 0, 1, else Third). Native has
            // no owner guard; a Techno without a represented house is a
            // fixture case. Rust never re-runs this InitFromType tail (Unit
            // slot +0x24 can); only retail [SPY], an Infantry, sets the keys.
            if obj.can_disguise
                && obj.perma_disguise
                && let Some(side) = houses.get(&ge.owner()).map(|h| h.side_index())
            {
                let worn = rules.general.infantry_disguises[match side {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                }]
                .as_deref()
                .map(|name| interner.intern(name));
                let owner = ge.owner();
                let frame = session.binary_frame();
                ge.disguise
                    .get_or_insert_with(|| crate::sim::cloak_disguise::DisguiseRuntime::new(frame))
                    .wear_from_construction(worn, owner);
            }
        }
        // After the managers, InitFromType makes the object veteran
        // (`SetVeteran(1)`, `0x00750090`) where its house's spies entered a
        // factory (`sim::spy_infiltration`): a `Trainable=` infantryman after
        // a barracks (`+0x2BF`, `0x00517D20..0x00517D51`); after a war
        // factory (`+0x2C0`) a `Trainable=` unit that is not `Naval=` (the
        // constructor `0x0073563D..0x00735678` and InitFromType
        // `0x00746860..0x0074689B` alike) and such a building that undeploys
        // (`BuildingClass::Init_Managers`, `0x00442CD8..0x00442D0F`).
        // Aircraft read neither. The countries' `VeteranInfantry=` and
        // `VeteranUnits=` lists before them (`0x00517CE8`, `0x0074682B`) are
        // dormant: no retail country sets one.
        let house = houses.get(&ge.owner());
        let veteran = obj.trainable
            && house.is_some_and(|house| match category {
                EntityCategory::Infantry => house.veteran_infantry(),
                EntityCategory::Unit => house.veteran_units() && !obj.naval,
                EntityCategory::Structure => {
                    house.veteran_units() && !obj.naval && obj.undeploys_into.is_some()
                }
                _ => false,
            });
        if veteran {
            ge.veterancy.set_veteran();
        }
    }
}

fn install_authored_bridge(ge: &mut GameEntity, origin: ComponentOrigin) {
    if let ComponentOrigin::Authored {
        bridge_deck: Some(_),
        ..
    } = origin
    {
        // A map Unit's or Infantry's OnBridge (`UnitClass::Read_INI`
        // `0x007434F3`, `InfantryClass::Read_INI` `0x0051FDB2`).
        if let Some(loco) = &mut ge.locomotor {
            loco.store_ground_layer(true);
        }
        ge.on_bridge.store(true);
    }
}

/// Copy the rules-derived scoring and house-count flags onto a freshly built
/// entity.
///
/// Every spawn path calls this, so a type that must not appear on the score
/// screen is honored no matter how the object came into the world. The flags
/// are copied rather than looked up later because the score bookkeeping and
/// the house counts run in the lifecycle authority, which deliberately holds
/// no `RuleSet` borrow.
fn stamp_scoring_flags(
    ge: &mut GameEntity,
    obj: Option<&crate::rules::object_type::ObjectType>,
    rules: Option<&RuleSet>,
) {
    ge.tracking_facts = crate::sim::house_tracking::TrackingFacts::of(ge.category(), obj, rules);
}

fn stamp_building_cell_profile(
    ge: &mut GameEntity,
    obj: Option<&crate::rules::object_type::ObjectType>,
) {
    let Some(obj) = obj else {
        return;
    };
    ge.building_light.stamp(obj.has_spotlight);
    ge.type_facts.stamp(ge.category(), obj);
}

/// `Crushable=` and `DeployedCrushable=`, copied when the object is built for
/// the crush passes (natively read through the type pointer). Only the
/// construction stamp writes them.
///
/// Nothing reads the `DeployedCrushable=` copy: the deploy completion
/// re-reads the type's key (`0x00520B3B`, `0x00520B9A`) and stamps the
/// object's crush immunity byte instead. The copy stays because saves carry
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CrushableFacts {
    /// `Crushable=`: a crusher may crush the object.
    crushable: bool,
    /// `DeployedCrushable=`: a deployed infantryman stays crushable by a
    /// regular crusher; `no` (low-silhouette infantry) blocks it. Unread
    /// (see the type doc).
    #[serde(default = "default_true")]
    deployed_crushable: bool,
}

impl Default for CrushableFacts {
    /// The object before the stamp: not crushable, crushable when deployed.
    fn default() -> Self {
        Self {
            crushable: false,
            deployed_crushable: true,
        }
    }
}

impl CrushableFacts {
    /// Copy the type's facts.
    fn stamp(obj: &ObjectType) -> Self {
        Self {
            crushable: obj.crushable,
            deployed_crushable: obj.deployed_crushable,
        }
    }

    /// `Crushable=`.
    pub(crate) const fn crushable(self) -> bool {
        self.crushable
    }

    /// Install a fixture's `Crushable=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_crushable_for_test(&mut self, value: bool) {
        self.crushable = value;
    }
}

/// `OmniCrusher=`, `Crusher=`, `Accelerates=`, `OmniCrushResistant=` and
/// `ImmuneToRadiation=`, copied when the object is built for the crush, drive
/// and radiation passes (natively read through the type pointer). Only the
/// construction stamp writes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MoverFacts {
    /// `OmniCrusher=`: crushes targets that are not `Crushable=` (only the
    /// Battle Fortress in YR).
    omni_crusher: bool,
    /// `Crusher=`: the ordinary crush, kept apart from MovementZone and
    /// `OmniCrusher=`; it acts on the Drive PerCellProcess path.
    #[serde(default)]
    regular_crusher: bool,
    /// `Accelerates=`: Drive/Ship locomotion ramps toward its target speed
    /// fraction.
    #[serde(default = "default_true")]
    drive_accelerates: bool,
    /// `OmniCrushResistant=`: immune to every crush.
    omni_crush_resistant: bool,
    /// `ImmuneToRadiation=`: per-cell radiation does no damage.
    #[serde(default)]
    immune_to_radiation: bool,
}

impl Default for MoverFacts {
    /// The object before the stamp: accelerates, nothing else.
    fn default() -> Self {
        Self {
            omni_crusher: false,
            regular_crusher: false,
            drive_accelerates: true,
            omni_crush_resistant: false,
            immune_to_radiation: false,
        }
    }
}

impl MoverFacts {
    /// Copy the type's facts.
    fn stamp(obj: &ObjectType) -> Self {
        Self {
            omni_crusher: obj.omni_crusher,
            regular_crusher: obj.crusher,
            drive_accelerates: obj.accelerates,
            omni_crush_resistant: obj.omni_crush_resistant,
            immune_to_radiation: obj.immune_to_radiation,
        }
    }

    /// `OmniCrusher=`.
    pub(crate) const fn omni_crusher(self) -> bool {
        self.omni_crusher
    }

    /// `Crusher=`.
    pub(crate) const fn regular_crusher(self) -> bool {
        self.regular_crusher
    }

    /// `Accelerates=`.
    pub(crate) const fn drive_accelerates(self) -> bool {
        self.drive_accelerates
    }

    /// `OmniCrushResistant=`.
    pub(crate) const fn omni_crush_resistant(self) -> bool {
        self.omni_crush_resistant
    }

    /// `ImmuneToRadiation=`.
    pub(crate) const fn immune_to_radiation(self) -> bool {
        self.immune_to_radiation
    }

    /// Install a fixture's `OmniCrusher=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_omni_crusher_for_test(&mut self, value: bool) {
        self.omni_crusher = value;
    }

    /// Install a fixture's `Crusher=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_regular_crusher_for_test(&mut self, value: bool) {
        self.regular_crusher = value;
    }

    /// Install a fixture's `Accelerates=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_drive_accelerates_for_test(&mut self, value: bool) {
        self.drive_accelerates = value;
    }

    /// Install a fixture's `OmniCrushResistant=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_omni_crush_resistant_for_test(&mut self, value: bool) {
        self.omni_crush_resistant = value;
    }
}

fn default_true() -> bool {
    true
}

/// Facts the object's type fixes, copied when the object is built so paths
/// without a RuleSet can read them: the cell-list exits of UnInit and Limbo,
/// the House base lists, BasePlan and owner transfer. Natively a building
/// reads each through its type pointer (`BuildingClass+0x520`); the rules
/// reader owns the native reads (the matching `ObjectType` fields). Only the
/// construction stamp writes them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeFacts {
    /// Aircraft `Spawned=yes` and `MissileSpawn=no`, read by Techno6FCE05
    /// before its mid-attack retarget rejection. Independent of SpawnOwner.
    #[serde(default)]
    spawned_non_missile_aircraft: bool,
    /// `Foundation=`, as its rules string: the structure footprint the cell
    /// lists cover; `1x1` for a type without one.
    #[serde(default = "default_foundation")]
    foundation: String,
    /// A building's hidden-object counter inputs
    /// (`ObjectType::hidden_occupancy`), so cell-list exit can reverse its
    /// contribution.
    #[serde(default)]
    building_hidden_occupancy: Option<crate::rules::object_type::BuildingHiddenOccupancyProfile>,
    /// `[AI] AIBaseSpacing=` for a building that runs the House base
    /// reservation writer; `None` is the native BuildingType gate.
    #[serde(default)]
    base_reservation_spacing: Option<i32>,
    /// `Factory=BuildingType`: the House edge refresh callback's input.
    #[serde(default)]
    determines_waypoint_edge: bool,
    /// Membership in the resolved `[AI] BuildConst=` vector
    /// (`RulesClass__ReadAI @ 0x00672AE0`, `0x00672B14..0x00672C01`).
    #[serde(default)]
    build_const_eligible: bool,
    /// The BuildingType registry index ordered House BasePlan nodes use;
    /// `-1` for any other type.
    #[serde(default = "default_base_plan_type_index")]
    base_plan_type_index: i32,
    /// `IsBaseDefense=` (`BuildingType+0x1706`).
    #[serde(default)]
    base_plan_is_defense: bool,
    /// `UndeploysInto=` resolved to a type (`BuildingType+0x408`, null for
    /// `none`/`<none>` after `UnitTypeClass__FindOrAllocate @ 0x007480D0`).
    #[serde(default)]
    base_plan_has_undeploy_target: bool,
}

fn default_foundation() -> String {
    "1x1".to_string()
}

fn default_base_plan_type_index() -> i32 {
    -1
}

impl TypeFacts {
    /// A new object's facts before the construction stamp: a building's
    /// empty hidden-occupancy profile, no other type's.
    pub(crate) fn new(category: EntityCategory) -> Self {
        Self {
            spawned_non_missile_aircraft: false,
            foundation: default_foundation(),
            building_hidden_occupancy: (category == EntityCategory::Structure)
                .then(crate::rules::object_type::BuildingHiddenOccupancyProfile::default),
            base_reservation_spacing: None,
            determines_waypoint_edge: false,
            build_const_eligible: false,
            base_plan_type_index: default_base_plan_type_index(),
            base_plan_is_defense: false,
            base_plan_has_undeploy_target: false,
        }
    }

    /// Copy the type's facts: the foundation for every object, the rest for
    /// a building.
    fn stamp(&mut self, category: EntityCategory, obj: &ObjectType) {
        self.spawned_non_missile_aircraft =
            category == EntityCategory::Aircraft && obj.spawned && !obj.missile_spawn;
        self.foundation = obj.foundation.clone();
        if category == EntityCategory::Structure {
            self.building_hidden_occupancy = Some(obj.hidden_occupancy);
            self.base_reservation_spacing = obj.base_reservation_spacing;
            self.determines_waypoint_edge = obj.factory_type() == Some(FactoryType::BuildingType);
            self.build_const_eligible = obj.build_const_eligible;
            self.base_plan_type_index = obj.base_plan_type_index;
            self.base_plan_is_defense = obj.is_base_defense;
            self.base_plan_has_undeploy_target = obj.undeploys_into.is_some();
        }
    }

    /// `Foundation=`, as its rules string.
    pub fn foundation(&self) -> &str {
        &self.foundation
    }

    pub(crate) fn spawned_non_missile_aircraft(&self) -> bool {
        self.spawned_non_missile_aircraft
    }

    /// A building's hidden-object counter inputs.
    pub(crate) fn building_hidden_occupancy(
        &self,
    ) -> Option<crate::rules::object_type::BuildingHiddenOccupancyProfile> {
        self.building_hidden_occupancy
    }

    /// `[AI] AIBaseSpacing=` for a base-reserving building.
    pub(crate) fn base_reservation_spacing(&self) -> Option<i32> {
        self.base_reservation_spacing
    }

    /// `Factory=BuildingType`.
    pub(crate) fn determines_waypoint_edge(&self) -> bool {
        self.determines_waypoint_edge
    }

    /// In `[AI] BuildConst=`.
    pub(crate) fn build_const_eligible(&self) -> bool {
        self.build_const_eligible
    }

    /// The BuildingType registry index, `-1` for any other type.
    pub(crate) fn base_plan_type_index(&self) -> i32 {
        self.base_plan_type_index
    }

    /// `IsBaseDefense=`.
    pub(crate) fn base_plan_is_defense(&self) -> bool {
        self.base_plan_is_defense
    }

    /// `UndeploysInto=` names a type.
    pub(crate) fn base_plan_has_undeploy_target(&self) -> bool {
        self.base_plan_has_undeploy_target
    }

    /// Install a fixture's `foundation`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_foundation_for_test(&mut self, value: String) {
        self.foundation = value;
    }

    /// Install a fixture's `building_hidden_occupancy`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_building_hidden_occupancy_for_test(
        &mut self,
        value: Option<crate::rules::object_type::BuildingHiddenOccupancyProfile>,
    ) {
        self.building_hidden_occupancy = value;
    }

    /// Install a fixture's `base_reservation_spacing`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_base_reservation_spacing_for_test(&mut self, value: Option<i32>) {
        self.base_reservation_spacing = value;
    }

    /// Install a fixture's `determines_waypoint_edge`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_determines_waypoint_edge_for_test(&mut self, value: bool) {
        self.determines_waypoint_edge = value;
    }

    /// Install a fixture's `build_const_eligible`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_build_const_eligible_for_test(&mut self, value: bool) {
        self.build_const_eligible = value;
    }

    /// Install a fixture's `base_plan_type_index`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_base_plan_type_index_for_test(&mut self, value: i32) {
        self.base_plan_type_index = value;
    }

    /// Install a fixture's `base_plan_is_defense`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_base_plan_is_defense_for_test(&mut self, value: bool) {
        self.base_plan_is_defense = value;
    }

    /// Install a fixture's `base_plan_has_undeploy_target`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_base_plan_has_undeploy_target_for_test(&mut self, value: bool) {
        self.base_plan_has_undeploy_target = value;
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

#[cfg(test)]
#[path = "perma_disguise_tests.rs"]
mod perma_disguise_tests;
