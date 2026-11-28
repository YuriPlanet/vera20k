//! Phobos `Trajectory` base and the `Straight`, `Bombard` and `Parabola`
//! readers (`Ext/Bullet/Trajectories/PhobosTrajectory.cpp:24-68`,
//! `StraightTrajectory.cpp:66-102`, `BombardTrajectory.cpp:53-80`,
//! `ParabolaTrajectory.cpp:50-88`; constructor defaults
//! `StraightTrajectory.h:5-39`, `BombardTrajectory.h:8-30`,
//! `ParabolaTrajectory.h:18-37`, base speed default `PhobosTrajectory.h:38`).
//!
//! `TrajectoryTypePointer::LoadFromINI` reads the `Trajectory` flag as a
//! `Nullable<TrajectoryFlag>`: an absent or unknown name keeps the previous
//! state, and the flag names are `Straight`, `Bombard` and `Parabola`
//! (case-insensitive `_strcmpi`). A constructed type reads its
//! `Trajectory.Speed` (clamped to `max(0.001, x)`) first and then its own
//! keys. VERA reads the projected (merged) section, so a same-section
//! re-declaration collapses to its final value; the per-layer
//! "same flag keeps the type, a changed flag reconstructs it" order is
//! recorded as a residual in `docs/phobos-trajectory-straight.md`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// `enum class TrajectoryFlag` (`PhobosTrajectory.h:5-11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TrajectoryFlag {
    Straight,
    Bombard,
    Parabola,
}

/// One BulletType's `Trajectory=` selection and its type data.
#[derive(Debug, Clone, PartialEq)]
pub struct PhobosTrajectoryRules {
    /// `Trajectory=`: `None` keeps the stock `BulletClass::AI` arms.
    pub flag: Option<TrajectoryFlag>,
    /// `Trajectory.Speed=`, `Valueable<double>`, default 100.0.
    pub speed: f64,
    /// `Trajectory.Straight.*`; dormant unless `flag` is `Straight`.
    pub straight: StraightTrajectoryRules,
    /// `Trajectory.Bombard.*`; dormant unless `flag` is `Bombard`.
    pub bombard: BombardTrajectoryRules,
    /// `Trajectory.Parabola.*`; dormant unless `flag` is `Parabola`.
    pub parabola: ParabolaTrajectoryRules,
}

impl Default for PhobosTrajectoryRules {
    fn default() -> Self {
        Self {
            flag: None,
            speed: 100.0,
            straight: StraightTrajectoryRules::default(),
            bombard: BombardTrajectoryRules::default(),
            parabola: ParabolaTrajectoryRules::default(),
        }
    }
}

impl PhobosTrajectoryRules {
    pub fn from_section(section: &IniSection) -> Self {
        let mut rules = Self::default();
        if let Some(name) = section.read_name("Trajectory", 0x800) {
            rules.flag = parse_flag(name);
        }
        // `Trajectory_Speed.Read` then `Math::max(0.001, speed)`.
        rules.speed = section
            .read_phobos_double("Trajectory.Speed", rules.speed)
            .max(0.001);
        match rules.flag {
            Some(TrajectoryFlag::Straight) => {
                rules.straight = StraightTrajectoryRules::from_section(section);
            }
            Some(TrajectoryFlag::Bombard) => {
                rules.bombard = BombardTrajectoryRules::from_section(section, rules.speed);
            }
            Some(TrajectoryFlag::Parabola) => {
                rules.parabola = ParabolaTrajectoryRules::from_section(section);
            }
            None => {}
        }
        rules
    }

    /// Whether the stock flight arms stay in charge.
    pub fn is_stock(&self) -> bool {
        self.flag.is_none()
    }
}

fn parse_flag(name: &str) -> Option<TrajectoryFlag> {
    if name.eq_ignore_ascii_case("Straight") {
        Some(TrajectoryFlag::Straight)
    } else if name.eq_ignore_ascii_case("Bombard") {
        Some(TrajectoryFlag::Bombard)
    } else if name.eq_ignore_ascii_case("Parabola") {
        Some(TrajectoryFlag::Parabola)
    } else {
        None
    }
}

/// `StraightTrajectoryType` (`StraightTrajectory.h:5-81`).
#[derive(Debug, Clone, PartialEq)]
pub struct StraightTrajectoryRules {
    /// `Trajectory.Straight.DetonationDistance=`, `Valueable<Leptons>`,
    /// default 102.
    pub detonation_distance: i32,
    /// `Trajectory.Straight.TargetSnapDistance=`, default 128.
    pub target_snap_distance: i32,
    /// `Trajectory.Straight.ApplyRangeModifiers=`, default `no`.
    pub apply_range_modifiers: bool,
    /// `Trajectory.Straight.PassThrough=`, default `no`.
    pub pass_through: bool,
    /// `Trajectory.Straight.PassDetonate=`, default `no`.
    pub pass_detonate: bool,
    /// `Trajectory.Straight.PassDetonateWarhead=`, `Read<true>`.
    pub pass_detonate_warhead: Option<String>,
    /// `Trajectory.Straight.PassDetonateDamage=`, default 0.
    pub pass_detonate_damage: i32,
    /// `Trajectory.Straight.PassDetonateDelay=`, default 1.
    pub pass_detonate_delay: i32,
    /// `Trajectory.Straight.PassDetonateInitialDelay=`, default 0.
    pub pass_detonate_initial_delay: i32,
    /// `Trajectory.Straight.PassDetonateLocal=`, default `no`.
    pub pass_detonate_local: bool,
    /// `Trajectory.Straight.LeadTimeCalculate=`, default `no`.
    pub lead_time_calculate: bool,
    /// `Trajectory.Straight.OffsetCoord=`, default `0,0,0`.
    pub offset_coord: [i32; 3],
    /// `Trajectory.Straight.RotateCoord=`, `Valueable<double>`, default 0.
    pub rotate_coord: f64,
    /// `Trajectory.Straight.MirrorCoord=`, default **`yes`**.
    pub mirror_coord: bool,
    /// `Trajectory.Straight.UseDisperseBurst=`, default `no`.
    pub use_disperse_burst: bool,
    /// `Trajectory.Straight.AxisOfRotation=`, default `0,0,1`.
    pub axis_of_rotation: [i32; 3],
    /// `Trajectory.Straight.ProximityImpact=`, default 0.
    pub proximity_impact: i32,
    /// `Trajectory.Straight.ProximityWarhead=`, `Read<true>`.
    pub proximity_warhead: Option<String>,
    /// `Trajectory.Straight.ProximityDamage=`, default 0.
    pub proximity_damage: i32,
    /// `Trajectory.Straight.ProximityRadius=`, default 179.
    pub proximity_radius: i32,
    /// `Trajectory.Straight.ProximityDirect=`, default `no`.
    pub proximity_direct: bool,
    /// `Trajectory.Straight.ProximityMedial=`, default `no`.
    pub proximity_medial: bool,
    /// `Trajectory.Straight.ProximityAllies=`, default `no`.
    pub proximity_allies: bool,
    /// `Trajectory.Straight.ProximityFlight=`, default `no`.
    pub proximity_flight: bool,
    /// `Trajectory.Straight.ThroughVehicles=`, default **`yes`**.
    pub through_vehicles: bool,
    /// `Trajectory.Straight.ThroughBuilding=`, default **`yes`**.
    pub through_building: bool,
    /// `Trajectory.Straight.SubjectToGround=`, default `no`.
    pub subject_to_ground: bool,
    /// `Trajectory.Straight.ConfineAtHeight=`, default 0.
    pub confine_at_height: i32,
    /// `Trajectory.Straight.EdgeAttenuation=`, default 1.0, clamped to
    /// `>= 0.0` after the read.
    pub edge_attenuation: f64,
    /// `Trajectory.Straight.CountAttenuation=`, default 1.0, clamped to
    /// `>= 0.0` after the read.
    pub count_attenuation: f64,
}

impl Default for StraightTrajectoryRules {
    fn default() -> Self {
        Self {
            detonation_distance: 102,
            target_snap_distance: 128,
            apply_range_modifiers: false,
            pass_through: false,
            pass_detonate: false,
            pass_detonate_warhead: None,
            pass_detonate_damage: 0,
            pass_detonate_delay: 1,
            pass_detonate_initial_delay: 0,
            pass_detonate_local: false,
            lead_time_calculate: false,
            offset_coord: [0; 3],
            rotate_coord: 0.0,
            mirror_coord: true,
            use_disperse_burst: false,
            axis_of_rotation: [0, 0, 1],
            proximity_impact: 0,
            proximity_warhead: None,
            proximity_damage: 0,
            proximity_radius: 179,
            proximity_direct: false,
            proximity_medial: false,
            proximity_allies: false,
            proximity_flight: false,
            through_vehicles: true,
            through_building: true,
            subject_to_ground: false,
            confine_at_height: 0,
            edge_attenuation: 1.0,
            count_attenuation: 1.0,
        }
    }
}

impl StraightTrajectoryRules {
    fn from_section(section: &IniSection) -> Self {
        let mut rules = Self::default();
        let s = &mut rules;
        s.apply_range_modifiers = section.read_bool(
            "Trajectory.Straight.ApplyRangeModifiers",
            s.apply_range_modifiers,
        );
        s.detonation_distance = section.read_phobos_leptons(
            "Trajectory.Straight.DetonationDistance",
            s.detonation_distance,
        );
        s.target_snap_distance = section.read_phobos_leptons(
            "Trajectory.Straight.TargetSnapDistance",
            s.target_snap_distance,
        );
        s.pass_through = section.read_bool("Trajectory.Straight.PassThrough", s.pass_through);
        s.pass_detonate = section.read_bool("Trajectory.Straight.PassDetonate", s.pass_detonate);
        s.pass_detonate_warhead = read_warhead_ref(
            section,
            "Trajectory.Straight.PassDetonateWarhead",
            s.pass_detonate_warhead.take(),
        );
        s.pass_detonate_damage = section.read_int(
            "Trajectory.Straight.PassDetonateDamage",
            s.pass_detonate_damage,
        );
        s.pass_detonate_delay = section.read_int(
            "Trajectory.Straight.PassDetonateDelay",
            s.pass_detonate_delay,
        );
        s.pass_detonate_initial_delay = section.read_int(
            "Trajectory.Straight.PassDetonateInitialDelay",
            s.pass_detonate_initial_delay,
        );
        s.pass_detonate_local = section.read_bool(
            "Trajectory.Straight.PassDetonateLocal",
            s.pass_detonate_local,
        );
        s.lead_time_calculate = section.read_bool(
            "Trajectory.Straight.LeadTimeCalculate",
            s.lead_time_calculate,
        );
        // `Valueable<CoordStruct>`: `Read3Integers` writes only the parsed
        // prefix, so a one- or two-token read keeps the other axes.
        s.offset_coord =
            section.read_int_fields("Trajectory.Straight.OffsetCoord", 64, s.offset_coord);
        s.rotate_coord =
            section.read_phobos_double("Trajectory.Straight.RotateCoord", s.rotate_coord);
        s.mirror_coord = section.read_bool("Trajectory.Straight.MirrorCoord", s.mirror_coord);
        s.use_disperse_burst =
            section.read_bool("Trajectory.Straight.UseDisperseBurst", s.use_disperse_burst);
        s.axis_of_rotation =
            section.read_int_fields("Trajectory.Straight.AxisOfRotation", 64, s.axis_of_rotation);
        s.proximity_impact =
            section.read_int("Trajectory.Straight.ProximityImpact", s.proximity_impact);
        s.proximity_warhead = read_warhead_ref(
            section,
            "Trajectory.Straight.ProximityWarhead",
            s.proximity_warhead.take(),
        );
        s.proximity_damage =
            section.read_int("Trajectory.Straight.ProximityDamage", s.proximity_damage);
        s.proximity_radius =
            section.read_phobos_leptons("Trajectory.Straight.ProximityRadius", s.proximity_radius);
        s.proximity_direct =
            section.read_bool("Trajectory.Straight.ProximityDirect", s.proximity_direct);
        s.proximity_medial =
            section.read_bool("Trajectory.Straight.ProximityMedial", s.proximity_medial);
        s.proximity_allies =
            section.read_bool("Trajectory.Straight.ProximityAllies", s.proximity_allies);
        s.proximity_flight =
            section.read_bool("Trajectory.Straight.ProximityFlight", s.proximity_flight);
        s.through_vehicles =
            section.read_bool("Trajectory.Straight.ThroughVehicles", s.through_vehicles);
        s.through_building =
            section.read_bool("Trajectory.Straight.ThroughBuilding", s.through_building);
        s.subject_to_ground =
            section.read_bool("Trajectory.Straight.SubjectToGround", s.subject_to_ground);
        s.confine_at_height =
            section.read_int("Trajectory.Straight.ConfineAtHeight", s.confine_at_height);
        s.edge_attenuation =
            section.read_phobos_double("Trajectory.Straight.EdgeAttenuation", s.edge_attenuation);
        s.edge_attenuation = s.edge_attenuation.max(0.0);
        s.count_attenuation =
            section.read_phobos_double("Trajectory.Straight.CountAttenuation", s.count_attenuation);
        s.count_attenuation = s.count_attenuation.max(0.0);
        rules
    }
}

/// `BombardTrajectoryType` (`BombardTrajectory.h:5-63`,
/// `BombardTrajectory.cpp:53-80`).
#[derive(Debug, Clone, PartialEq)]
pub struct BombardTrajectoryRules {
    /// `Trajectory.Bombard.Height=`, `Valueable<double>` in leptons above the
    /// target, default 0.0, clamped to `>= 0.0` after the read.
    pub height: f64,
    /// `Trajectory.Bombard.FallPercent=`, default 1.0.
    pub fall_percent: f64,
    /// `Trajectory.Bombard.FallPercentShift=`, default 0.0.
    pub fall_percent_shift: f64,
    /// `Trajectory.Bombard.FallScatter.Max=`, `Leptons`, default 0.
    pub fall_scatter_max: i32,
    /// `Trajectory.Bombard.FallScatter.Min=`, `Leptons`, default 0.
    pub fall_scatter_min: i32,
    /// `Trajectory.Bombard.FallScatter.Linear=`, default `no`.
    pub fall_scatter_linear: bool,
    /// `Trajectory.Bombard.FallSpeed=`, default `Trajectory.Speed` when the
    /// read value is `|x| < 1e-10` (`BombardTrajectory.cpp:65`).
    pub fall_speed: f64,
    /// `Trajectory.Bombard.DetonationDistance=`, `Leptons`, default 102.
    pub detonation_distance: i32,
    /// `Trajectory.Bombard.DetonationHeight=`, default -1 (disabled).
    pub detonation_height: i32,
    /// `Trajectory.Bombard.EarlyDetonation=`, default `no`.
    pub early_detonation: bool,
    /// `Trajectory.Bombard.TargetSnapDistance=`, `Leptons`, default 128.
    pub target_snap_distance: i32,
    /// `Trajectory.Bombard.FreeFallOnTarget=`, default **`yes`**.
    pub free_fall_on_target: bool,
    /// `Trajectory.Bombard.LeadTimeCalculate=`, default `no`.
    pub lead_time_calculate: bool,
    /// `Trajectory.Bombard.NoLaunch=`, default `no`.
    pub no_launch: bool,
    /// `Trajectory.Bombard.TurningPointAnims=`, `ValueableVector`; the
    /// turning-point animation spawn is presentation-only and stays unported.
    pub turning_point_anims: Vec<String>,
    /// `Trajectory.Bombard.OffsetCoord=`, default `0,0,0`.
    pub offset_coord: [i32; 3],
    /// `Trajectory.Bombard.RotateCoord=`, default 0.
    pub rotate_coord: f64,
    /// `Trajectory.Bombard.MirrorCoord=`, default **`yes`**.
    pub mirror_coord: bool,
    /// `Trajectory.Bombard.UseDisperseBurst=`, default `no`.
    pub use_disperse_burst: bool,
    /// `Trajectory.Bombard.AxisOfRotation=`, default `0,0,1`.
    pub axis_of_rotation: [i32; 3],
    /// `Trajectory.Bombard.SubjectToGround=`, default `no`.
    pub subject_to_ground: bool,
}

impl Default for BombardTrajectoryRules {
    fn default() -> Self {
        Self {
            height: 0.0,
            fall_percent: 1.0,
            fall_percent_shift: 0.0,
            fall_scatter_max: 0,
            fall_scatter_min: 0,
            fall_scatter_linear: false,
            fall_speed: 0.0,
            detonation_distance: 102,
            detonation_height: -1,
            early_detonation: false,
            target_snap_distance: 128,
            free_fall_on_target: true,
            lead_time_calculate: false,
            no_launch: false,
            turning_point_anims: Vec::new(),
            offset_coord: [0; 3],
            rotate_coord: 0.0,
            mirror_coord: true,
            use_disperse_burst: false,
            axis_of_rotation: [0, 0, 1],
            subject_to_ground: false,
        }
    }
}

impl BombardTrajectoryRules {
    fn from_section(section: &IniSection, trajectory_speed: f64) -> Self {
        let mut rules = Self::default();
        let r = &mut rules;
        r.height = section
            .read_phobos_double("Trajectory.Bombard.Height", r.height)
            .max(0.0);
        r.fall_percent =
            section.read_phobos_double("Trajectory.Bombard.FallPercent", r.fall_percent);
        r.fall_percent_shift =
            section.read_phobos_double("Trajectory.Bombard.FallPercentShift", r.fall_percent_shift);
        r.fall_scatter_max =
            section.read_phobos_leptons("Trajectory.Bombard.FallScatter.Max", r.fall_scatter_max);
        r.fall_scatter_min =
            section.read_phobos_leptons("Trajectory.Bombard.FallScatter.Min", r.fall_scatter_min);
        r.fall_scatter_linear = section.read_bool(
            "Trajectory.Bombard.FallScatter.Linear",
            r.fall_scatter_linear,
        );
        r.fall_speed = section.read_phobos_double("Trajectory.Bombard.FallSpeed", r.fall_speed);
        if r.fall_speed.abs() < 1e-10 {
            r.fall_speed = trajectory_speed;
        }
        r.detonation_distance = section.read_phobos_leptons(
            "Trajectory.Bombard.DetonationDistance",
            r.detonation_distance,
        );
        r.detonation_height =
            section.read_int("Trajectory.Bombard.DetonationHeight", r.detonation_height);
        r.early_detonation =
            section.read_bool("Trajectory.Bombard.EarlyDetonation", r.early_detonation);
        r.target_snap_distance = section.read_phobos_leptons(
            "Trajectory.Bombard.TargetSnapDistance",
            r.target_snap_distance,
        );
        r.free_fall_on_target =
            section.read_bool("Trajectory.Bombard.FreeFallOnTarget", r.free_fall_on_target);
        r.lead_time_calculate = section.read_bool(
            "Trajectory.Bombard.LeadTimeCalculate",
            r.lead_time_calculate,
        );
        r.no_launch = section.read_bool("Trajectory.Bombard.NoLaunch", r.no_launch);
        if let Some(list) = section.read_list("Trajectory.Bombard.TurningPointAnims", 0x100) {
            r.turning_point_anims = list.into_iter().map(str::to_owned).collect();
        }
        r.offset_coord =
            section.read_int_fields("Trajectory.Bombard.OffsetCoord", 64, r.offset_coord);
        r.rotate_coord =
            section.read_phobos_double("Trajectory.Bombard.RotateCoord", r.rotate_coord);
        r.mirror_coord = section.read_bool("Trajectory.Bombard.MirrorCoord", r.mirror_coord);
        r.use_disperse_burst =
            section.read_bool("Trajectory.Bombard.UseDisperseBurst", r.use_disperse_burst);
        r.axis_of_rotation =
            section.read_int_fields("Trajectory.Bombard.AxisOfRotation", 64, r.axis_of_rotation);
        r.subject_to_ground =
            section.read_bool("Trajectory.Bombard.SubjectToGround", r.subject_to_ground);
        rules
    }
}

/// `enum class ParabolaFireMode` (`ParabolaTrajectory.h:5-13`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ParabolaFireMode {
    Speed,
    Height,
    Angle,
    SpeedAndHeight,
    HeightAndAngle,
    SpeedAndAngle,
}

/// `ParabolaTrajectoryType` (`ParabolaTrajectory.h:15-67`,
/// `ParabolaTrajectory.cpp:50-88`).
#[derive(Debug, Clone, PartialEq)]
pub struct ParabolaTrajectoryRules {
    /// `Trajectory.Parabola.DetonationDistance=`, `Leptons`, default 102.
    pub detonation_distance: i32,
    /// `Trajectory.Parabola.TargetSnapDistance=`, `Leptons`, default 128.
    pub target_snap_distance: i32,
    /// `Trajectory.Parabola.OpenFireMode=`, default `Speed`; unknown names
    /// also fall back to `Speed`.
    pub open_fire_mode: ParabolaFireMode,
    /// `Trajectory.Parabola.ThrowHeight=`, default 600.
    pub throw_height: i32,
    /// `Trajectory.Parabola.LaunchAngle=`, default 30.0.
    pub launch_angle: f64,
    /// `Trajectory.Parabola.LeadTimeCalculate=`, default `no`.
    pub lead_time_calculate: bool,
    /// `Trajectory.Parabola.DetonationAngle=`, default -90.0.
    pub detonation_angle: f64,
    /// `Trajectory.Parabola.DetonationHeight=`, default -1 (disabled).
    pub detonation_height: i32,
    /// `Trajectory.Parabola.BounceTimes=`, default 0.
    pub bounce_times: i32,
    /// `Trajectory.Parabola.BounceOnWater=`, default `no`.
    pub bounce_on_water: bool,
    /// `Trajectory.Parabola.BounceDetonate=`, default `no`.
    pub bounce_detonate: bool,
    /// `Trajectory.Parabola.BounceAttenuation=`, default 0.8.
    pub bounce_attenuation: f64,
    /// `Trajectory.Parabola.BounceCoefficient=`, default 0.8.
    pub bounce_coefficient: f64,
    /// `Trajectory.Parabola.OffsetCoord=`, default `0,0,0`.
    pub offset_coord: [i32; 3],
    /// `Trajectory.Parabola.RotateCoord=`, default 0.
    pub rotate_coord: f64,
    /// `Trajectory.Parabola.MirrorCoord=`, default **`yes`**.
    pub mirror_coord: bool,
    /// `Trajectory.Parabola.UseDisperseBurst=`, default `no`.
    pub use_disperse_burst: bool,
    /// `Trajectory.Parabola.AxisOfRotation=`, default `0,0,1`.
    pub axis_of_rotation: [i32; 3],
}

impl Default for ParabolaTrajectoryRules {
    fn default() -> Self {
        Self {
            detonation_distance: 102,
            target_snap_distance: 128,
            open_fire_mode: ParabolaFireMode::Speed,
            throw_height: 600,
            launch_angle: 30.0,
            lead_time_calculate: false,
            detonation_angle: -90.0,
            detonation_height: -1,
            bounce_times: 0,
            bounce_on_water: false,
            bounce_detonate: false,
            bounce_attenuation: 0.8,
            bounce_coefficient: 0.8,
            offset_coord: [0; 3],
            rotate_coord: 0.0,
            mirror_coord: true,
            use_disperse_burst: false,
            axis_of_rotation: [0, 0, 1],
        }
    }
}

impl ParabolaTrajectoryRules {
    fn from_section(section: &IniSection) -> Self {
        let mut rules = Self::default();
        let r = &mut rules;
        r.detonation_distance = section.read_phobos_leptons(
            "Trajectory.Parabola.DetonationDistance",
            r.detonation_distance,
        );
        r.target_snap_distance = section.read_phobos_leptons(
            "Trajectory.Parabola.TargetSnapDistance",
            r.target_snap_distance,
        );
        r.open_fire_mode = match section.read_name("Trajectory.Parabola.OpenFireMode", 0x800) {
            Some(name) if name.eq_ignore_ascii_case("height") => ParabolaFireMode::Height,
            Some(name) if name.eq_ignore_ascii_case("angle") => ParabolaFireMode::Angle,
            Some(name) if name.eq_ignore_ascii_case("speedandheight") => {
                ParabolaFireMode::SpeedAndHeight
            }
            Some(name) if name.eq_ignore_ascii_case("heightandangle") => {
                ParabolaFireMode::HeightAndAngle
            }
            Some(name) if name.eq_ignore_ascii_case("speedandangle") => {
                ParabolaFireMode::SpeedAndAngle
            }
            _ => ParabolaFireMode::Speed,
        };
        r.throw_height = section.read_int("Trajectory.Parabola.ThrowHeight", r.throw_height);
        r.launch_angle =
            section.read_phobos_double("Trajectory.Parabola.LaunchAngle", r.launch_angle);
        r.lead_time_calculate = section.read_bool(
            "Trajectory.Parabola.LeadTimeCalculate",
            r.lead_time_calculate,
        );
        r.detonation_angle =
            section.read_phobos_double("Trajectory.Parabola.DetonationAngle", r.detonation_angle);
        r.detonation_height =
            section.read_int("Trajectory.Parabola.DetonationHeight", r.detonation_height);
        r.bounce_times = section.read_int("Trajectory.Parabola.BounceTimes", r.bounce_times);
        r.bounce_on_water =
            section.read_bool("Trajectory.Parabola.BounceOnWater", r.bounce_on_water);
        r.bounce_detonate =
            section.read_bool("Trajectory.Parabola.BounceDetonate", r.bounce_detonate);
        r.bounce_attenuation = section.read_phobos_double(
            "Trajectory.Parabola.BounceAttenuation",
            r.bounce_attenuation,
        );
        r.bounce_coefficient = section.read_phobos_double(
            "Trajectory.Parabola.BounceCoefficient",
            r.bounce_coefficient,
        );
        r.offset_coord =
            section.read_int_fields("Trajectory.Parabola.OffsetCoord", 64, r.offset_coord);
        r.rotate_coord =
            section.read_phobos_double("Trajectory.Parabola.RotateCoord", r.rotate_coord);
        r.mirror_coord = section.read_bool("Trajectory.Parabola.MirrorCoord", r.mirror_coord);
        r.use_disperse_burst =
            section.read_bool("Trajectory.Parabola.UseDisperseBurst", r.use_disperse_burst);
        r.axis_of_rotation =
            section.read_int_fields("Trajectory.Parabola.AxisOfRotation", 64, r.axis_of_rotation);
        rules
    }
}

/// `Valueable<WarheadTypeClass*>::Read<true>`: `ReadString(0x800)` and the
/// warhead factory; `none`/`<none>` clears and an absent or empty key keeps
/// the current name. The factory allocation runs in the ordered rules pass
/// (`allocate_trajectory_warhead_references`), so the name resolves at use.
fn read_warhead_ref(section: &IniSection, key: &str, current: Option<String>) -> Option<String> {
    let Some(name) = section.read_name(key, 0x800) else {
        return current;
    };
    if crate::rules::ini_parser::is_native_none_type_name(name) {
        None
    } else {
        Some(name.to_owned())
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_trajectory_tests.rs"]
mod tests;
