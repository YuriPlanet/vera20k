//! `TechnoClass::GetFireError` (vt+0x3C0, `0x006FC0B0`) and its class
//! overrides: the one answer to "may this object fire this weapon at this
//! target now, and if not, why". Callers act on the code; the function itself
//! writes nothing and draws nothing.
//!
//! Native order: the class prefix (Unit `0x00740FD0`, Infantry `0x0051C8B0`,
//! Building `0x00447F10`), then the base's tests, then the class suffix
//! (Unit, Infantry, Building, Aircraft `0x0041A9E0`) on a base 0. A nonzero
//! base code returns unchanged. vt+0x3BC (`0x006FC090`) is this call with
//! `check_range` false in every class.
//!
//! [`FireFacts`] holds the plain fields the tests read. [`FireQuery`] answers
//! the questions whose bodies belong to other owners (weapon lookup, range,
//! flying, cells, sensors, alliances, locomotors, facings). It is asked lazily,
//! where native asks it, so a query that native never reaches never runs.
//!
//! Evidence: `tools/spatial_oracle/fire_error.py` runs the original bodies
//! (1191 rows over the four classes, codes and the ordered query log);
//! `tests::original_fire_error_rows` replays every row here.
//!
//! RESIDUAL rows: each is ported and fires on its field, but VERA has no
//! producer for that field yet, so it holds the constructor value:
//! - T4/T11/T28..T31/T42: the Magnetron hold.
//! - T6: the Robot Control Center latch.
//! - T7/T57: sinking.
//! - T15: the Chronosphere warp latch.
//! - T18: balloon docking.
//! - T19: ObjectClass `+0x8D`, VERA's `object_is_falling_down`, which no
//!   paradrop or drop-in writes yet.
//! - T20: EMP.
//! - T32/T33: open-topped passengers (VERA never fires from a transport).
//! - T37/T46: particle systems. The Sonic wave and the damage-spark system
//!   are live; Fire's own systems are not produced: `+0x304` (`0x006FF1A7`,
//!   UseFireParticles), `+0x308` (`0x006FF1AD`, UseSparkParticles, the IFV's
//!   RepairBullet) and `+0x314` (railgun), so those shots rearm on ROF alone.
//! - Dormant in retail: T44 (FiringSyncFrame), U1 (DeathFrames),
//!   U3 (DirectRocker), U4 (DeployToFire), U7 (MobileFire=no), I3 (Pushy).
//! - B3: EMPulseCannon, whose cannon fires only through its superweapon.
//! - B7: the upgrade turret (`0x004527D0`'s upgrade walk; `turret_upgrade`).
//! - A1: a paradrop plane's payload (`+0x6C9`, `paradrop_payload`).
//! - U6: a Building target counts as a vehicle when it undeploys into a unit
//!   and stands on one cell (`0x00457620` -> `0x00465D40`); held false, and no
//!   retail building qualifies (the Construction Yards are 4x4).

/// The codes, as every producer returns them (`MOV EAX, imm32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub(crate) enum FireError {
    Ok = 0,
    Ammo = 1,
    Facing = 2,
    Rearm = 3,
    Rotating = 4,
    Illegal = 5,
    Cant = 6,
    Moving = 7,
    Range = 8,
    Cloaked = 9,
    Busy = 10,
    MustDeploy = 11,
}

/// The firer's class: which override runs (vt+0x3C0 per vtable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FirerClass {
    #[default]
    Unit,
    Infantry,
    Aircraft,
    Building,
}

impl FirerClass {
    /// `AbstractFlags & 4`.
    fn is_foot(self) -> bool {
        self != Self::Building
    }
}

/// The target's class as the tests see it: its WhatAmI and flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FireTargetKind {
    /// A NULL target.
    #[default]
    None,
    Unit,
    Infantry,
    Aircraft,
    Building,
    /// A CellClass target (WhatAmI `0xB`).
    Cell,
    /// A non-techno ObjectClass (a TerrainClass, WhatAmI `0x24`).
    Object,
}

impl FireTargetKind {
    /// `AbstractFlags & 1`.
    fn is_techno(self) -> bool {
        matches!(
            self,
            Self::Unit | Self::Infantry | Self::Aircraft | Self::Building
        )
    }

    /// `AbstractFlags & 4`.
    fn is_foot(self) -> bool {
        matches!(self, Self::Unit | Self::Infantry | Self::Aircraft)
    }

    /// `AbstractFlags & 2`.
    fn is_object(self) -> bool {
        self.is_techno() || self == Self::Object
    }
}

/// A pointer field compared with the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Link {
    #[default]
    None,
    Target,
    Other,
}

/// `Transporter` (`+0x11C`), the transport this object rides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Transporter {
    #[default]
    None,
    /// The target itself (T9).
    Target,
    Plain,
    /// A transport being warped out (vt+0x1D4, T32).
    WarpedOut,
    /// A transport inside another transport (T33).
    Nested,
}

/// `RadioLinks[0]` of a tethered Unit (U5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RadioLink {
    #[default]
    None,
    Building,
    Other,
}

/// A CellClass the tests read: LandType (`+0xEC`) and flags (`+0x140`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CellFacts {
    pub land_type: i32,
    pub flags: u32,
}

/// Cell flag `0x100`: the cell holds a bridge.
const CELL_BRIDGE: u32 = 0x100;
/// LandType Water and Beach.
const LAND_WATER: i32 = 2;
const LAND_BEACH: i32 = 6;

/// The firer's own cell (vt+0x1BC): NULL, or the target itself (I8), or a
/// cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirerCell {
    None,
    Target,
    Cell(CellFacts),
}

/// One WeaponTypeClass with its warhead and projectile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WeaponFacts {
    /// `+0xA4` Damage and `+0x98` AmbientDamage (`0x006F3970` sums them).
    pub damage: i32,
    pub ambient_damage: i32,
    /// `+0x9C`.
    pub burst: i32,
    /// `+0xB4`, leptons.
    pub range: i32,
    pub use_fire_particles: bool,  // +0x129
    pub use_spark_particles: bool, // +0x12A
    pub omni_fire: bool,           // +0x12B
    pub is_railgun: bool,          // +0x12D
    pub is_sonic: bool,            // +0x130
    pub spawner: bool,             // +0x131
    pub decloak_to_fire: bool,     // +0x133
    pub fire_while_moving: bool,   // +0x141
    pub drain_weapon: bool,        // +0x142
    pub fire_in_transport: bool,   // +0x143
    pub area_fire: bool,           // +0x150
    pub is_mag_beam: bool,         // +0x15C
    /// `+0xAC`. NULL faults natively at T36; VERA reads no flag from it.
    pub warhead: Option<WarheadFacts>,
    /// `+0xA0`. Never NULL natively (read unchecked at T38, T39, T41, U11 and
    /// U12).
    pub projectile: ProjectileFacts,
}

/// A WarheadTypeClass, as the tests read it against this target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WarheadFacts {
    pub mind_control: bool, // +0x155
    pub ivan_bomb: bool,    // +0x157
    pub parasite: bool,     // +0x159
    pub temporal: bool,     // +0x15A
    pub is_locomotor: bool, // +0x15B
    pub psychedelic: bool,  // +0x16D
    pub bomb_disarm: bool,  // +0x16E
    /// `Verses[target Armor]` compares equal to 0.0 (T54: `TEST AH,0x40`
    /// after `FCOMP`, so -0.0 and NaN count too).
    pub verses_zero: bool,
}

/// A BulletTypeClass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProjectileFacts {
    pub aa: bool, // +0x2A4
    pub ag: bool, // +0x2A5
    pub rot: i32, // +0x2DC
}

/// The firer's own fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirerFacts {
    /// `+0x2DC` SlaveOwner: an enslaved slave (T2).
    pub enslaved: bool,
    /// vt+0x1D4 (`+0x270`) and vt+0x1D8 (`+0x271`).
    pub warped_out: bool,
    pub warping_in: bool,
    /// `+0x1C8`.
    pub robot_offline: bool,
    /// `+0x3CD`.
    pub sinking: bool,
    /// `+0x298`.
    pub berserk: bool,
    /// `+0x8D`.
    pub falling: bool,
    /// `+0x82`, `+0x11C`.
    pub in_open_transport: bool,
    pub transporter: Transporter,
    /// `+0x8C`, `+0xA4` (Location.Z, leptons).
    pub on_bridge: bool,
    pub z: i32,
    /// `+0x504` and, for a Unit, `+0x6D8` DeathFrameCounter (vt+0x37C).
    pub emp_remaining: i32,
    pub death_frame_counter: i32,
    /// Live continuous effects: `+0x304` fire, `+0x308` spark and `+0x314`
    /// railgun particle systems, `+0x324` the Wave.
    pub fire_particles_live: bool,
    pub spark_particles_live: bool,
    pub railgun_particles_live: bool,
    pub wave_live: bool,
    /// The ROF timer (`+0x2EC`/`+0x2F4`) has time left.
    pub rearming: bool,
    /// `+0x3B8` CurrentBurstIndex, `+0x2FC` Ammo, `+0x220` CloakState,
    /// `+0x138` CurrentWeaponNumber.
    pub burst_index: i32,
    pub ammo: i32,
    pub cloak_state: i32,
    pub current_weapon: i32,
    /// `+0x1D0` DrainingMe, `+0x1CC` DrainTarget, `+0x2AC` LocomotorTarget.
    pub draining_me: bool,
    pub drain_target: Link,
    pub locomotor_target: Link,
    /// `+0x274` TemporalImUsing and what it holds (`Temporal+0x28`).
    pub temporal: Option<Link>,
    /// Owner `IsHumanPlayer` (`House+0x1EC`).
    pub owner_is_human: bool,
    /// Paralysed (vt+0x380: Foot `0x004DE770`, time left != 0).
    pub paralyzed: bool,
    /// SpawnManager (`+0x2D0`) counts: `0x006B7D30` spawns not
    /// regenerating, `0x006B7D80` missiles launched.
    pub spawns: Option<SpawnCounts>,
    /// The current facings (`FacingClass::Current`): PrimaryFacing `+0x388`
    /// and SecondaryFacing `+0x3A0`.
    pub primary_facing: u16,
    pub secondary_facing: u16,
    /// Foot: `+0x6AD` lifted by a Magnetron, `+0x5A4` NavCom set, `+0x578`
    /// speed fraction above 0.1 (I4).
    pub magnetron_lifted: bool,
    pub navcom: bool,
    pub moving_faster_than_tenth: bool,
    /// Unit: `+0x2A8` DirectRocker link, `+0x6E1`/`+0x6E2` deploying,
    /// `+0x418` tethered with `RadioLinks[0]`, `+0x68D` firing sequence,
    /// `+0x6AF` turret-rotation latch, `+0x6C0` firing frame.
    pub rocker_link: bool,
    pub deploying: bool,
    pub tethered: bool,
    pub radio_link: RadioLink,
    pub firing_sequence: bool,
    pub turret_rotation_latch: bool,
    pub firing_frame: i32,
    /// Infantry: `+0x6C4` SequenceAnim.
    pub sequence: i32,
    /// Aircraft: `+0x6C9` paradrop payload, `+0x118` first passenger.
    pub paradrop_payload: bool,
    pub has_passenger: bool,
    /// Building: vt+0x184 effective mission, `+0x714` delayed-fire counter,
    /// an upgrade with `Turret=yes` (HasTurret `0x004527D0`).
    pub effective_mission: i32,
    pub delayed_fire_counter: i32,
    pub turret_upgrade: bool,
}

impl Default for FirerFacts {
    /// The constructor values.
    fn default() -> Self {
        Self {
            enslaved: false,
            warped_out: false,
            warping_in: false,
            robot_offline: false,
            sinking: false,
            berserk: false,
            falling: false,
            in_open_transport: false,
            transporter: Transporter::None,
            on_bridge: false,
            z: 0,
            emp_remaining: 0,
            death_frame_counter: -1,
            fire_particles_live: false,
            spark_particles_live: false,
            railgun_particles_live: false,
            wave_live: false,
            rearming: false,
            burst_index: 0,
            ammo: -1,
            cloak_state: 0,
            current_weapon: 0,
            draining_me: false,
            drain_target: Link::None,
            locomotor_target: Link::None,
            temporal: None,
            owner_is_human: false,
            paralyzed: false,
            spawns: None,
            primary_facing: 0,
            secondary_facing: 0,
            magnetron_lifted: false,
            navcom: false,
            moving_faster_than_tenth: false,
            rocker_link: false,
            deploying: false,
            tethered: false,
            radio_link: RadioLink::None,
            firing_sequence: false,
            turret_rotation_latch: false,
            firing_frame: -1,
            sequence: 0,
            paradrop_payload: false,
            has_passenger: false,
            effective_mission: 0,
            delayed_fire_counter: 0,
            turret_upgrade: false,
        }
    }
}

/// SpawnManager counts (`0x006B7D30`, `0x006B7D80`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SpawnCounts {
    pub not_regenerating: i32,
    pub launched_missiles: i32,
}

/// The firer's TechnoTypeClass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirerTypeFacts {
    pub natural: bool,        // +0x693
    pub pushy: bool,          // +0x692
    pub land_targeting: i32,  // +0x604
    pub mobile_fire: bool,    // +0x6AE
    pub turret_count: i32,    // +0x808
    pub turret: bool,         // +0xCA1
    pub is_gattling: bool,    // +0xCD5
    pub hunter_seeker: bool,  // +0xD27
    pub balloon_hover: bool,  // +0xD6A
    pub jumpjet: bool,        // +0xD94
    pub organic: bool,        // +0xD97
    pub deploy_to_fire: bool, // UnitType +0xE12
    /// UnitType `+0xE18`/`+0xE19`.
    pub small_visceroid: bool,
    pub large_visceroid: bool,
    /// UnitType `+0xE3C` Facings and `+0xE40` FiringSyncFrame0/1 (T44).
    pub facings: i32,
    pub firing_sync_frame: [i32; 2],
    pub jumpjet_turn: bool, // InfantryType +0xECB
    /// BuildingType.
    pub can_be_occupied: bool, // +0x157B
    pub can_occupy_fire: bool, // +0x157C
    pub emp_pulse_cannon: bool, // +0x16C3
    pub turret_anim_is_voxel: bool, // +0x16C5
}

impl Default for FirerTypeFacts {
    /// The constructor values.
    fn default() -> Self {
        Self {
            natural: false,
            pushy: false,
            land_targeting: 0,
            mobile_fire: true,
            turret_count: 0,
            turret: false,
            is_gattling: false,
            hunter_seeker: false,
            balloon_hover: false,
            jumpjet: false,
            organic: false,
            deploy_to_fire: false,
            small_visceroid: false,
            large_visceroid: false,
            facings: 8,
            firing_sync_frame: [-1, -1],
            jumpjet_turn: false,
            can_be_occupied: false,
            can_occupy_fire: false,
            emp_pulse_cannon: false,
            turret_anim_is_voxel: false,
        }
    }
}

/// The target's fields (techno fields are read only for a techno target).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TargetFacts {
    pub kind: FireTargetKind,
    /// `+0x38` an attached bomb (any ObjectClass).
    pub bomb: bool,
    pub in_limbo: bool,          // +0x81
    pub on_bridge: bool,         // +0x8C
    pub z: i32,                  // +0xA4
    pub mission: i32,            // +0xAC
    pub iron_curtained: bool,    // vt+0x160 0x0041BF40, time left > 0
    pub draining_me: bool,       // +0x1D0
    pub warped_out: bool,        // vt+0x1D4
    pub chrono_warp_latch: bool, // +0x27C
    pub bunkered: bool,          // +0x2E4
    pub sinking: bool,           // +0x3CD
    pub docked: bool,            // +0x418
    pub health: i32,             // +0x6C
    /// Foot: `+0x698` the frame a Parasite launch lock ends.
    pub parasite_lock_until: i32,
    /// Unit: deploying or undeploying; another object than the firer holds
    /// its `+0x2A8` DirectRocker link (I3).
    pub deploying: bool,
    pub rocked_by_another: bool,
    /// A cell target's own LandType.
    pub land_type: i32,
}

/// The target's TechnoTypeClass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetTypeFacts {
    pub strength: i32,            // +0xA0
    pub drainable: bool,          // +0x5EF
    pub berserk_friendly: bool,   // +0x690
    pub unnatural: bool,          // +0x694
    pub immune_to_psionics: bool, // +0xD35
    pub spawned: bool,            // +0xD54
    pub balloon_hover: bool,      // +0xD6A
    pub jumpjet: bool,            // +0xD94
    pub organic: bool,            // +0xD97
    /// UnitType `+0xE13` and `+0xE1B`.
    pub is_simple_deployer: bool,
    pub non_vehicle: bool,
    /// A Building target counts as a vehicle for U6 (vt+0x80, `0x00457620`).
    pub building_vehicle: bool,
}

impl Default for TargetTypeFacts {
    fn default() -> Self {
        Self {
            strength: 100,
            drainable: false,
            berserk_friendly: false,
            unnatural: false,
            immune_to_psionics: false,
            spawned: false,
            balloon_hover: false,
            jumpjet: false,
            organic: false,
            is_simple_deployer: false,
            non_vehicle: false,
            building_vehicle: false,
        }
    }
}

/// Everything plain one GetFireError call reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FireFacts {
    pub class: FirerClass,
    /// `[0x00A8ED84]`.
    pub frame: i32,
    pub weapon_index: i32,
    pub firer: FirerFacts,
    pub firer_type: FirerTypeFacts,
    pub target: TargetFacts,
    pub target_type: TargetTypeFacts,
}

/// The questions GetFireError asks other owners. Each is asked only where,
/// and as often as, the original asks it.
pub(crate) trait FireQuery {
    /// GetWeapon (vt+0x3F8) of this index: its WeaponType, `None` for an
    /// empty slot.
    fn weapon(&mut self, index: i32) -> Option<WeaponFacts>;
    /// vt+0x3A8 InRange (T61).
    fn in_range(&mut self) -> bool;
    /// vt+0x2E8, the naval target selector; -1 refuses (T40).
    fn naval_selector(&mut self) -> i32;
    /// The target's vt+0x68 GetVisualState(1, firer's house) (T17).
    fn visual_state(&mut self) -> i32;
    /// The target's vt+0x54 IsHighFlying and vt+0x50 (below two levels).
    fn high_flying(&mut self) -> bool;
    fn low_flying(&mut self) -> bool;
    /// The firer's vt+0x54 (T58).
    fn firer_high_flying(&mut self) -> bool;
    /// The target's vt+0x78 InWhichLayer (T39; 2 is the ground).
    fn target_layer(&mut self) -> i32;
    /// The target's cell (vt+0x1BC) and the firer's.
    fn target_cell(&mut self) -> Option<CellFacts>;
    fn firer_cell(&mut self) -> FirerCell;
    /// CellClass `0x004870D0`: the firer's house has a sensor on the cell
    /// under the target (T17).
    fn sensor(&mut self) -> bool;
    /// HouseClass `0x004F9A50`: the target's house is allied with the
    /// firer's (T17).
    fn allied(&mut self) -> bool;
    /// `0x00703B10` IsOnBridge_ForFiring (T35).
    fn bridge_for_firing(&mut self) -> bool;
    /// ParasiteClass `0x0062A8E0` CanInfect (T50) and CaptureManager
    /// `0x00471C90` CanCapture (T53).
    fn can_infect(&mut self) -> bool;
    fn can_capture(&mut self) -> bool;
    /// CellClass `0x00487C10` on the Unit's own cell (U4).
    fn deploy_cell_ok(&mut self) -> bool;
    /// ILocomotion Is_Moving_Now of the firer and of the target.
    fn locomotor_moving(&mut self) -> bool;
    fn target_locomotor_moving(&mut self) -> bool;
    /// ILocomotion Can_Fire (U13, I10).
    fn locomotor_can_fire(&mut self) -> FireError;
    /// The firer's locomotor is the Jumpjet locomotor (I6's CLSID compare).
    fn jumpjet_locomotor(&mut self) -> bool;
    /// IFlyControl `+0x1C`: the aircraft is a fighter (A2).
    fn fighter(&mut self) -> bool;
    /// `0x005F3DB0` DirectionToTarget and Building vt+0x4E8.
    fn direction_to_target(&mut self) -> u16;
    fn turret_direction(&mut self) -> u16;
    /// Building vt+0x408, the occupant count (B1).
    fn occupants(&mut self) -> i32;
    /// Building vt+0x350 Is_Operational_For_Output (B5).
    fn operational(&mut self) -> bool;
}

/// GetFireError through the firer's class override.
pub(crate) fn get_fire_error(
    facts: &FireFacts,
    query: &mut impl FireQuery,
    check_range: bool,
) -> FireError {
    match facts.class {
        FirerClass::Unit => unit(facts, query, check_range),
        FirerClass::Infantry => infantry(facts, query, check_range),
        FirerClass::Aircraft => aircraft(facts, query, check_range),
        FirerClass::Building => building(facts, query, check_range),
    }
}

/// `UnitClass::GetFireError @ 0x00740FD0`.
fn unit(facts: &FireFacts, query: &mut impl FireQuery, check_range: bool) -> FireError {
    let firer = &facts.firer;
    let firer_type = &facts.firer_type;
    // U1 `0x00740FD9`: a dying Unit's death-frame counter is running.
    if firer.death_frame_counter >= 0 {
        return FireError::Cant;
    }
    // U2 `0x00740FF2`: a spawned missile is in flight.
    if firer
        .spawns
        .is_some_and(|spawns| spawns.launched_missiles != 0)
    {
        return FireError::Busy;
    }
    // U3 `0x00741014`.
    if firer.rocker_link {
        return FireError::Illegal;
    }
    let base = techno(facts, query, check_range);
    // `0x0074104B`: the base has no producer of Facing, so only 0 goes on.
    if base != FireError::Ok && base != FireError::Facing {
        return base;
    }
    // U4 `0x00741050..0x007410A8`: the cell is asked first, on every call.
    // vt+0x4E4 (`0x00736D50`) answers true at once for DeployToFire.
    let cell_ok = query.deploy_cell_ok();
    if firer_type.deploy_to_fire && !cell_ok {
        return FireError::MustDeploy;
    }
    if base != FireError::Ok {
        return base;
    }
    // U5 `0x007410C3`: docked at a building.
    if firer.tethered && firer.radio_link == RadioLink::Building {
        return FireError::Cant;
    }
    // The base's T21 passed, so the weapon exists.
    let Some(weapon) = query.weapon(facts.weapon_index) else {
        return FireError::Cant;
    };
    // U6 `0x007410F9..0x00741138`: a healing weapon fires only at a damaged
    // vehicle.
    if weapon_value(facts, query) < 0 {
        let target = &facts.target;
        let vehicle = match target.kind {
            FireTargetKind::Unit => !facts.target_type.non_vehicle,
            // Aircraft vt+0x80 `0x0041B910` jumps to vt+0x50.
            FireTargetKind::Aircraft => query.low_flying(),
            FireTargetKind::Building => facts.target_type.building_vehicle,
            _ => false,
        };
        if !vehicle || health_ratio_full(target.health, facts.target_type.strength) {
            return FireError::Illegal;
        }
    }
    // U7 `0x00741149`.
    if !firer_type.mobile_fire && firer.navcom {
        return FireError::Moving;
    }
    // U8 `0x00741172`: a balloon asks its locomotor, anything else its NavCom.
    if !weapon.fire_while_moving {
        if firer_type.balloon_hover {
            if query.locomotor_moving() {
                return FireError::Moving;
            }
        } else if firer.navcom {
            return FireError::Moving;
        }
    }
    // U9 `0x007411D9`.
    if (weapon.use_spark_particles || weapon.use_fire_particles) && firer.navcom {
        return FireError::Moving;
    }
    // U10 `0x00741206`.
    if firer.temporal.is_some() && firer.navcom {
        return FireError::Moving;
    }
    // U11 `0x00741229`: the turret is still finishing its arc.
    if !firer.firing_sequence && firer.turret_rotation_latch && weapon.projectile.rot == 0 {
        return FireError::Rotating;
    }
    // U12 `0x0074125C..0x007412EF`: `Turret=` picks the turret or the hull;
    // a homing projectile widens the tolerance.
    if !weapon.omni_fire && !firer_type.large_visceroid && !firer_type.small_visceroid {
        let current = if firer_type.turret {
            firer.secondary_facing
        } else {
            firer.primary_facing
        };
        let tolerance = if weapon.projectile.rot != 0 {
            0x1000
        } else {
            0x0800
        };
        if facing_off(current, query.direction_to_target(), tolerance) {
            return FireError::Facing;
        }
    }
    // U13 `0x00741300`.
    query.locomotor_can_fire()
}

/// `InfantryClass::GetFireError @ 0x0051C8B0`.
fn infantry(facts: &FireFacts, query: &mut impl FireQuery, check_range: bool) -> FireError {
    let firer = &facts.firer;
    let target = &facts.target;
    // I1 `0x0051C8B8`: Die1..5, WetDie1/2, AirDeathStart/Falling/Finish.
    if matches!(firer.sequence, 0xB..=0xF | 0x14 | 0x15 | 0x22..=0x24) {
        return FireError::Cant;
    }
    // I2 `0x0051C8FE`: a medic heals only damaged infantry.
    if weapon_value(facts, query) < 0
        && (target.kind != FireTargetKind::Infantry
            || health_ratio_full(target.health, facts.target_type.strength))
    {
        return FireError::Illegal;
    }
    // I3 `0x0051C947`: another object rocks the Unit.
    if target.kind == FireTargetKind::Unit && facts.firer_type.pushy && target.rocked_by_another {
        return FireError::Illegal;
    }
    let base = techno(facts, query, check_range);
    if base != FireError::Ok {
        return base;
    }
    // I4 `0x0051C9B8`.
    if firer.moving_faster_than_tenth {
        return FireError::Moving;
    }
    // I5 `0x0051C9CF`: moving in an action fire cannot interrupt.
    if firer.navcom
        && firer.sequence != -1
        && crate::rules::infantry_sequence::action_record(firer.sequence)
            .is_some_and(|record| !record.interruptible)
    {
        return FireError::Moving;
    }
    // I6 `0x0051C9F3`: a JumpJetTurn jumpjet turning in flight.
    if facts.firer_type.jumpjet
        && query.jumpjet_locomotor()
        && facts.firer_type.jumpjet_turn
        && query.locomotor_moving()
    {
        return FireError::Moving;
    }
    // `0x0051CAE4`: an empty slot skips to the locomotor.
    if let Some(weapon) = query.weapon(facts.weapon_index) {
        // I7 `0x0051CAD1`.
        if weapon.use_fire_particles && firer.navcom {
            return FireError::Moving;
        }
        // I8 `0x0051CB08`: AreaFire fires only at the infantry's own cell.
        if weapon.area_fire && query.firer_cell() != FirerCell::Target {
            return FireError::Illegal;
        }
        // I9 `0x0051CB2E`: any bombed object, not only a techno (T56).
        if weapon.warhead.is_some_and(|warhead| warhead.ivan_bomb)
            && target.kind.is_object()
            && target.bomb
        {
            return FireError::Illegal;
        }
    }
    // I10 `0x0051CB61`.
    query.locomotor_can_fire()
}

/// `BuildingClass::GetFireError @ 0x00447F10`.
fn building(facts: &FireFacts, query: &mut impl FireQuery, check_range: bool) -> FireError {
    let firer = &facts.firer;
    let firer_type = &facts.firer_type;
    // B1 `0x00447F15`.
    if firer_type.can_be_occupied && (!firer_type.can_occupy_fire || query.occupants() == 0) {
        return FireError::Illegal;
    }
    // B2 `0x00447F45`: a Floating Disc drains it.
    if firer.draining_me {
        return FireError::Illegal;
    }
    // B3 `0x00447F54`.
    if firer_type.emp_pulse_cannon {
        return FireError::Cant;
    }
    // B4 `0x00447F6F`: Selling, then Construction.
    if firer.effective_mission == 0x13 || firer.effective_mission == 0x12 {
        return FireError::Illegal;
    }
    // B5 `0x00447F95`.
    if !query.operational() {
        return FireError::Cant;
    }
    // B6 `0x00447FAE`: a delayed shot is counting down.
    if firer.delayed_fire_counter != 0 {
        return FireError::Rearm;
    }
    let base = techno(facts, query, check_range);
    if base != FireError::Ok {
        return base;
    }
    // B7 `0x00447FDF..0x00448045`: HasTurret (`0x004527D0`); a voxel turret
    // must point exactly.
    if firer_type.turret || firer.turret_upgrade {
        let direction = query.turret_direction();
        let tolerance = if firer_type.turret_anim_is_voxel {
            0
        } else {
            0x0800
        };
        if facing_off(firer.primary_facing, direction, tolerance) {
            return FireError::Facing;
        }
    }
    FireError::Ok
}

/// `AircraftClass::GetFireError @ 0x0041A9E0`.
fn aircraft(facts: &FireFacts, query: &mut impl FireQuery, check_range: bool) -> FireError {
    let base = techno(facts, query, check_range);
    if base != FireError::Ok {
        return base;
    }
    let firer = &facts.firer;
    // A1 `0x0041A9FF`: a payload plane with nothing left aboard.
    if firer.paradrop_payload && !firer.has_passenger {
        return FireError::Ammo;
    }
    // A2 `0x0041AA1E..0x0041AA6A`: a fighter fires at any heading.
    if !query.fighter() && facing_off(firer.secondary_facing, query.direction_to_target(), 0x0800) {
        return FireError::Facing;
    }
    FireError::Ok
}

/// `TechnoClass::GetFireError @ 0x006FC0B0`.
fn techno(facts: &FireFacts, query: &mut impl FireQuery, check_range: bool) -> FireError {
    use FireError::{Ammo, Cant, Cloaked, Illegal, Range, Rearm};
    let class = facts.class;
    let firer = &facts.firer;
    let firer_type = &facts.firer_type;
    let target = &facts.target;
    let target_type = &facts.target_type;
    let kind = target.kind;

    // T1..T11 `0x006FC0BA..0x006FC1B0`.
    if kind == FireTargetKind::None || firer.enslaved {
        return Illegal;
    }
    if firer.warping_in || firer.locomotor_target == Link::Target {
        return Rearm;
    }
    if firer.warped_out
        || firer.robot_offline
        || firer.sinking
        || firer.drain_target == Link::Target
        || firer.transporter == Transporter::Target
    {
        return Illegal;
    }
    // T10: my TemporalClass holds this target.
    if firer.temporal == Some(Link::Target) {
        return Rearm;
    }
    if class.is_foot() && firer.magnetron_lifted {
        return Illegal;
    }

    // T12..T18 `0x006FC1B6`: a techno target only.
    if kind.is_techno() {
        if target.in_limbo
            || (firer.berserk && target_type.berserk_friendly)
            || (firer_type.natural && target_type.unnatural)
            || target.chrono_warp_latch
            // T16: a computer house never fires at the Iron Curtain.
            || (!firer.owner_is_human && target.iron_curtained)
        {
            return Illegal;
        }
        // T17 `0x006FC24D..0x006FC29D`: a cloaked target this house cannot
        // sense takes only a harmless shot from an ally.
        if query.visual_state() == 5
            && !query.sensor()
            && (weapon_value(facts, query) > 0 || !query.allied())
        {
            return Cant;
        }
        // T18 `0x006FC2A3`: a docked balloon.
        if target.docked && target_type.balloon_hover && !query.high_flying() {
            return Illegal;
        }
    }
    // T19 `0x006FC2D2`.
    if firer.falling {
        return Illegal;
    }
    // T20 `0x006FC2E0`: EMP (vt+0x37C; a Unit also while its death frames
    // run), except a visceroid.
    let emp =
        firer.emp_remaining > 0 || (class == FirerClass::Unit && firer.death_frame_counter != -1);
    if emp
        && (class != FirerClass::Unit
            || !(firer_type.large_visceroid || firer_type.small_visceroid))
    {
        return Cant;
    }
    // T21 `0x006FC31C`.
    let Some(weapon) = query.weapon(facts.weapon_index) else {
        return Cant;
    };
    // T22 (IonSensitive) asks `0x0053A130`, which is `XOR AL,AL; RET`.
    let warhead = weapon.warhead.unwrap_or_default();

    // T23 `0x006FC356`: one drainer per victim, and only a drainable one.
    if weapon.drain_weapon && kind.is_techno() && (target.draining_me || !target_type.drainable) {
        return Illegal;
    }
    // T24 `0x006FC393`: a bunkered tank needs a weapon of at least 1.5 cells.
    if kind == FireTargetKind::Unit && target.bunkered && weapon.range < 384 {
        return Illegal;
    }
    // T25 `0x006FC3C5`.
    if class == FirerClass::Unit && firer.deploying {
        return Illegal;
    }
    // T26/T27 `0x006FC3F4`: gas.
    if warhead.psychedelic
        && kind.is_techno()
        && (target_type.immune_to_psionics || target.bunkered)
    {
        return Illegal;
    }
    // T28..T31 `0x006FC44B..0x006FC577`: the Magnetron.
    if warhead.is_locomotor {
        if kind == FireTargetKind::Unit && target.deploying {
            return Illegal;
        }
        if kind.is_foot() && target_type.jumpjet && query.target_locomotor_moving() {
            return Illegal;
        }
        if kind == FireTargetKind::Unit && target_type.is_simple_deployer && target.mission == 0x10
        {
            return Illegal;
        }
        if kind.is_techno() && target_type.organic {
            return Illegal;
        }
    }
    // T32/T33 `0x006FC57D..0x006FC5CF`: firing out of an open-topped
    // transport.
    if firer.in_open_transport
        && (!weapon.fire_in_transport
            || matches!(
                firer.transporter,
                Transporter::WarpedOut | Transporter::Nested
            ))
    {
        return Illegal;
    }
    // T34 `0x006FC5D5`: only Chrono shots join an erase.
    if kind.is_techno() && weapon.warhead.is_some() && target.warped_out && !warhead.temporal {
        return Illegal;
    }
    // T35 `0x006FC606`: a spawner launches from a stable cell, unparalysed,
    // with a spawn ready. A missing SpawnManager faults natively.
    if weapon.spawner {
        if query.bridge_for_firing() || (class.is_foot() && firer.paralyzed) {
            return Cant;
        }
        if firer
            .spawns
            .is_none_or(|spawns| spawns.not_regenerating == 0)
        {
            return Rearm;
        }
    }
    // T36 `0x006FC64F`: a Chrono shot cannot erase spawned aircraft.
    if warhead.temporal && kind == FireTargetKind::Aircraft && target_type.spawned {
        return Illegal;
    }
    // T37 `0x006FC689`: the OTHER slot still has a live continuous effect.
    let other_index = if facts.weapon_index == 0 { 1 } else { 0 };
    if query
        .weapon(other_index)
        .is_some_and(|other| effect_live(&other, firer))
    {
        return Cant;
    }
    // T38 `0x006FC705`. Its REARM arm needs `+0x2AC == target`, which T4
    // already answered, so it returns ILLEGAL.
    if query.high_flying() && !weapon.projectile.aa {
        return Illegal;
    }
    // T39 `0x006FC73C`.
    if kind.is_foot() && query.target_layer() != 2 && !weapon.projectile.aa {
        return Illegal;
    }
    if kind.is_techno() {
        // T40 `0x006FC76A..0x006FC7E9`: water or beach under a ground
        // target asks the naval selector; a low target on land asks
        // LandTargeting. A NULL cell faults natively.
        let first = query.target_cell().unwrap_or_default();
        let mut water = first.land_type == LAND_WATER
            || query.target_cell().unwrap_or_default().land_type == LAND_BEACH;
        if query.high_flying() {
            water = false;
        }
        if target.on_bridge {
            water = false;
        } else if water && query.naval_selector() == -1 {
            return Illegal;
        }
        if query.low_flying() && !water && firer_type.land_targeting == 1 {
            return Illegal;
        }
    } else {
        // T41 `0x006FC7EB..0x006FC868`.
        if !query.high_flying() && !weapon.projectile.ag {
            return Illegal;
        }
        if kind == FireTargetKind::Cell
            && target.land_type != LAND_WATER
            && target.land_type != LAND_BEACH
            && firer_type.land_targeting == 1
        {
            return Illegal;
        }
    }
    // T42/T43 `0x006FC879..0x006FC8E6`: the Magnetron's beam is busy. T43
    // (`+0x2AC == target`) is dominated by T4.
    if weapon.is_mag_beam
        && kind != FireTargetKind::Building
        && (firer.locomotor_target == Link::Target
            || (firer.locomotor_target == Link::Other && firer.wave_live))
    {
        return Rearm;
    }
    // T44 `0x006FC8F5..0x006FC940`: a primary shot waits for its
    // FiringSyncFrame and then skips the ROF test.
    let mut synced = false;
    if facts.weapon_index == 0 && class == FirerClass::Unit {
        // IDIV: Burst 0 faults natively; VERA reads no sync frame.
        if let Some(stage) = firer.burst_index.checked_rem(weapon.burst)
            && stage < 2
        {
            let sync = match stage {
                0 | 1 => firer_type.firing_sync_frame[stage as usize],
                // `+0xE40 - 4` is Facings.
                -1 => firer_type.facings,
                // Earlier fields; no VERA burst index is negative.
                _ => -1,
            };
            if sync != -1 && firer.firing_frame != -1 {
                if firer.firing_frame != sync {
                    return Rearm;
                }
                synced = true;
            }
        }
    }
    // T45 `0x006FC94F`: the ROF timer.
    if !synced && firer.rearming {
        return Rearm;
    }
    // T46 `0x006FC981`: this slot's own continuous effect.
    if effect_live(&weapon, firer) {
        return Rearm;
    }
    // T47 `0x006FCA0D`: -1 is unlimited.
    if firer.ammo == 0 {
        return Ammo;
    }
    // T48 `0x006FCA26`: an aircraft surfaces only from a full cloak.
    if weapon.decloak_to_fire
        && firer.cloak_state != 0
        && (class != FirerClass::Aircraft || firer.cloak_state == 2)
    {
        return Cloaked;
    }
    // T49 `0x006FCA5E`.
    if firer_type.hunter_seeker {
        return Range;
    }
    // T50/T51 `0x006FCA81..0x006FCAEB`: the Parasite.
    if warhead.parasite {
        if class.is_foot() && !query.can_infect() {
            return Illegal;
        }
        if kind.is_foot() && facts.frame < target.parasite_lock_until {
            return Illegal;
        }
    }
    // T52..T59 `0x006FCAFA`: a techno target only.
    if kind.is_techno() {
        if (warhead.parasite && target.iron_curtained)
            || (warhead.mind_control && !query.can_capture())
            || warhead.verses_zero
            || (warhead.bomb_disarm && !target.bomb)
            || (warhead.ivan_bomb && target.bomb)
            || target.sinking
        {
            return Illegal;
        }
        // T58/T59 `0x006FCBE6..0x006FCCAE`: a bridge deck between them.
        if firer.on_bridge != target.on_bridge {
            if bridge_cells_between(query) {
                return Illegal;
            }
            if warhead.parasite
                && firer.z.wrapping_sub(target.z).wrapping_abs()
                    > 2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
            {
                return Illegal;
            }
        }
    }
    // T60 `0x006FCCBD`.
    if firer_type.organic && class.is_foot() && firer.paralyzed {
        return Illegal;
    }
    // T61 `0x006FCCEE`: the only range test.
    if check_range && !query.in_range() {
        return Range;
    }
    FireError::Ok
}

/// T58's cell reads: each cell is fetched twice (a NULL test, then the
/// flags), and a high-flying firer is exempt.
fn bridge_cells_between(query: &mut impl FireQuery) -> bool {
    if query.firer_cell() == FirerCell::None {
        return false;
    }
    let FirerCell::Cell(own) = query.firer_cell() else {
        return false;
    };
    if own.flags & CELL_BRIDGE == 0 || query.firer_high_flying() {
        return false;
    }
    query.target_cell().is_some()
        && query
            .target_cell()
            .is_some_and(|cell| cell.flags & CELL_BRIDGE != 0)
}

/// A weapon's continuous effect is still alive on the firer (T37, T46).
fn effect_live(weapon: &WeaponFacts, firer: &FirerFacts) -> bool {
    (weapon.use_fire_particles && firer.fire_particles_live)
        || (weapon.is_railgun && firer.railgun_particles_live)
        || (weapon.use_spark_particles && firer.spark_particles_live)
        || (weapon.is_sonic && firer.wave_live)
}

/// `0x006F3970` with -1, through GetFireError's weapon query (a garrison
/// answers its occupant's weapon).
pub(crate) fn weapon_value(facts: &FireFacts, query: &mut impl FireQuery) -> i32 {
    weapon_damage_value(
        facts.firer_type.turret_count,
        facts.firer_type.is_gattling,
        facts.firer.current_weapon,
        |slot| {
            query
                .weapon(slot)
                .map(|weapon| weapon.damage.wrapping_add(weapon.ambient_damage))
        },
    )
}

/// `TechnoClass::GetWeaponDamageValue(-1) @ 0x006F3970`: Damage + AmbientDamage
/// of the current weapon for a `TurretCount` type that is not Gattling, else
/// the truncated average over the non-empty slots 0 and 1. `value` answers
/// GetWeapon (vt+0x3F8) of a slot as that weapon's Damage + AmbientDamage.
pub(crate) fn weapon_damage_value(
    turret_count: i32,
    is_gattling: bool,
    current_weapon: i32,
    mut value: impl FnMut(i32) -> Option<i32>,
) -> i32 {
    if turret_count > 0 && !is_gattling {
        return value(current_weapon).unwrap_or(0);
    }
    let mut total = 0i32;
    let mut count = 0i32;
    for slot in 0..2 {
        if let Some(slot_value) = value(slot) {
            total = total.wrapping_add(slot_value);
            count += 1;
        }
    }
    if count == 0 { 0 } else { total / count }
}

/// `ObjectClass::GetHealthRatio` (`0x005F5C60`, Health / Strength in
/// binary64) compared `>=` `Rules+0x16F8` (1.0, not an INI key) with an
/// ordered FCOMP: 0/0 (NaN) is not full and x/0 for x > 0 (+inf) is. The
/// quotient of two dwords cannot round across 1.0, so the integer form is
/// exact.
pub(crate) fn health_ratio_full(health: i32, strength: i32) -> bool {
    match strength.cmp(&0) {
        std::cmp::Ordering::Greater => health >= strength,
        std::cmp::Ordering::Less => health <= strength,
        std::cmp::Ordering::Equal => health > 0,
    }
}

/// U12, B7 and A2: the signed 16-bit difference against the desired
/// direction, compared by magnitude (`JGE` passes `|delta| == tolerance`).
fn facing_off(current: u16, desired: u16, tolerance: i32) -> bool {
    i32::from(current.wrapping_sub(desired) as i16).abs() > tolerance
}

#[cfg(test)]
#[path = "fire_error_tests.rs"]
mod tests;
