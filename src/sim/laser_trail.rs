//! Phobos `LaserTrailClass` update — the one owner for every trail consumer.
//!
//! Native source: `src/New/Entity/LaserTrailClass.cpp:7..84` (`Update`). The
//! same `Update` serves three callers:
//! - Techno attach effects, `TechnoExt::UpdateLaserTrails`
//!   (`Ext/Techno/Body.Update.cpp:463..507`);
//! - projectiles, the `BulletClass::AI` head hook
//!   (`Ext/Bullet/Hooks.cpp:99..126`), which predicts the drawn coordinate;
//! - voxel debris, `VoxelAnimClass_AI_Additional`
//!   (`Ext/VoxelAnim/Hooks.cpp:5..27`).
//!
//! Phobos `LaserTrail.Types` (`BulletTypeExt` ART, `VoxelAnimTypeExt` rules)
//! creates one `LaserTrailClass` per index; a projectile initializes its list
//! in `BulletExt::InitializeState` only when the type is not `Inviso`
//! (`Ext/Bullet/Body.cpp:121..133`), and a voxel anim in its constructor
//! (`Ext/VoxelAnim/Body.cpp:5..18`).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, util/ and the rest of sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use serde::{Deserialize, Serialize};

use crate::rules::phobos_laser_trail::{LaserTrailDrawType, LaserTrailType};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::electric_bolt::ElectricBoltBirth;
use crate::sim::combat::laser::{self, LaserColor};
use crate::sim::combat::radiation_beam::{BeamHouseColor, RadiationBeamBirth};
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::MainRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::{FrameChannel, LifecycleOutput, Simulation};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// One `LaserTrailClass`: its type name and `LastLocation`.
///
/// The type name keeps the `[LaserTrailTypes]` entry the type reader saw; the
/// catalog lives in `ArtRegistry`, so resolution happens at update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LaserTrailState {
    /// `LaserTrailClass::Type`, by `[LaserTrailTypes]` name.
    pub(crate) trail_type: String,
    /// `LaserTrailClass::LastLocation`; unset until the first update.
    #[serde(default)]
    pub(crate) last_location: Option<[i32; 3]>,
}

impl LaserTrailState {
    pub(crate) fn new(trail_type: &str) -> Self {
        Self {
            trail_type: trail_type.to_string(),
            last_location: None,
        }
    }

    /// One instance per `LaserTrail.Types` index, in list order.
    pub(crate) fn from_names(names: &[String]) -> Vec<Self> {
        names.iter().map(|name| Self::new(name)).collect()
    }
}

/// `LaserTrailClass::Update(CoordStruct location)`
/// (`New/Entity/LaserTrailClass.cpp:7..84`).
///
/// The first update only seeds `LastLocation`. A crossed `SegmentLength`
/// advances it and, when the trail is visible and not cloaked, constructs one
/// segment with `FadeDuration`, `Thickness` and `IsIntense`; every draw system
/// shares the same `LastLocation` rule. `IgnoreVertical` suppresses the
/// segment when both horizontal axes moved 16 leptons or less.
///
/// `visible`/`cloaked` are the caller's `LaserTrailClass` fields: techno
/// trails derive them from cloak state, projectile trails are always visible,
/// and voxel trails mirror `VoxelAnimClass::IsVisible`.
///
/// Every draw system is built from the copied segment:
/// - `Laser` constructs a `LaserDraw` with the owner House's colour;
/// - `EBolt` emits the constructor's phase draw and its per-strand colours
///   (`LaserTrailClass.cpp:38..64`);
/// - `RadBeam` emits the copied beam with `Beam.Color.Get(RadColor)`,
///   `FadeDuration.Get(15)` and `Beam.Amplitude` (`:65..75`).
///
/// RESIDUAL: `IsHideable` is parsed but the global
/// `HideLaserTrailEffects` switch has no reader (its default is off, so the
/// native gate is behaviourally identical). A trail with no owner House keeps
/// the type's `Color`, as `CurrentColor` is not replaced without a House.
pub(crate) fn update(
    last_location: &mut Option<[i32; 3]>,
    trail: &LaserTrailType,
    location: [i32; 3],
    visible: bool,
    cloaked: bool,
    owner: Option<InternedId>,
    rad_color: (u8, u8, u8),
    session: &ScenarioSession,
    main_rng: &MainRng,
    outputs: &mut FrameChannel<LifecycleOutput>,
) -> bool {
    let Some(last) = *last_location else {
        *last_location = Some(location);
        return false;
    };
    let dx = i64::from(location[0]) - i64::from(last[0]);
    let dy = i64::from(location[1]) - i64::from(last[1]);
    let dz = i64::from(location[2]) - i64::from(last[2]);
    let segment = f64::from(trail.segment_length);
    if (dx * dx + dy * dy + dz * dz) as f64 <= segment * segment {
        return false;
    }
    *last_location = Some(location);
    if !visible || cloaked {
        return false;
    }
    if trail.ignore_vertical && dx.abs() <= 16 && dy.abs() <= 16 {
        return false;
    }
    let from = ProjectileCoord::new(last[0], last[1], last[2]);
    let to = ProjectileCoord::new(location[0], location[1], location[2]);
    match trail.draw_type {
        LaserTrailDrawType::Laser => {
            // `LaserTrailClass` sets `CurrentColor` to the type's `Color`
            // unless the type is `IsHouseColor` and a House exists; the
            // LaserDraw is forced to `IsHouseColor = true` but receives that
            // copied colour (`New/Entity/LaserTrailClass.cpp:24..37`).
            let color = match owner {
                Some(owner) if trail.is_house_color => LaserColor::House(owner),
                _ => LaserColor::HouseStyle(trail.color),
            };
            let birth = laser::construct(
                session,
                outputs,
                from,
                to,
                0,
                trail.fade_duration(),
                color,
                NativeF32Bits::POSITIVE_ZERO,
            );
            birth.width = trail.thickness;
            birth.supported = trail.is_intense;
        }
        LaserTrailDrawType::EBolt => {
            // `LaserTrailClass.cpp:43..63`: a disabled strand keeps the
            // constructor's black colour, and the bolt constructor's
            // phase draw is part of Main.
            let colors: [Option<[u8; 3]>; 3] = std::array::from_fn(|index| {
                if trail.bolt_disable[index] {
                    Some([0, 0, 0])
                } else {
                    trail.bolt_color[index]
                }
            });
            outputs.emit(LifecycleOutput::ElectricBoltCreated(ElectricBoltBirth {
                frame: session.binary_frame() as i32,
                from,
                to,
                z_adjust: trail.bolt_z_adjust,
                phase: main_rng.next_range_i32_inclusive(0, 256),
                alternate_color: trail.is_alternate_color,
                colors,
                arcs: trail.bolt_arcs,
                owner: None,
                decay: 1_i32 << (trail.fade_duration().clamp(1, 31) - 1),
            }));
        }
        LaserTrailDrawType::RadBeam => {
            let (red, green, blue) = rad_color;
            outputs.emit(LifecycleOutput::RadiationBeamCreated(RadiationBeamBirth {
                frame: session.binary_frame() as i32,
                from,
                to,
                z_adjust: 0,
                color: trail.beam_color.unwrap_or([red, green, blue]),
                house: None,
                house_color: BeamHouseColor::None,
                duration: trail.fade_duration(),
                amplitude: NativeF64Bits::from_bits(trail.beam_amplitude.to_bits()),
                eruption: false,
            }));
        }
    }
    true
}

impl Simulation {
    /// The `BulletClass::AI` head hook (`Ext/Bullet/Hooks.cpp:117..123`): a
    /// bullet without a Phobos trajectory seeds each unset `LastLocation` at
    /// its raw `Location`, then advances to `Location + Velocity`, one frame
    /// ahead of the stock move. `Inviso` bullets never initialized a list.
    pub(crate) fn update_projectile_laser_trails(&mut self, id: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(id) else {
            return;
        };
        if projectile.has_phobos_trajectory() {
            return;
        }
        let position = projectile.position();
        let velocity = projectile.velocity();
        let owner = projectile.phobos_firer_house();
        let raw = [position.x, position.y, position.z];
        let location = [
            predicted_coord(position.x, velocity.x),
            predicted_coord(position.y, velocity.y),
            predicted_coord(position.z, velocity.z),
        ];
        self.advance_projectile_laser_trails(id, rules, raw, location, owner);
    }

    /// The shared trajectory hook (`Ext/Bullet/Trajectories/PhobosTrajectory.cpp:502..531`):
    /// after `OnAI` each trail seeds at the bullet's current `Location` and
    /// advances to `Velocity + Location`.
    pub(crate) fn update_trajectory_laser_trails(&mut self, id: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(id) else {
            return;
        };
        let position = projectile.position();
        let velocity = projectile.velocity();
        let owner = projectile.phobos_firer_house();
        let raw = [position.x, position.y, position.z];
        let location = [
            predicted_coord(position.x, velocity.x),
            predicted_coord(position.y, velocity.y),
            predicted_coord(position.z, velocity.z),
        ];
        self.advance_projectile_laser_trails(id, rules, raw, location, owner);
    }

    fn advance_projectile_laser_trails(
        &mut self,
        id: u64,
        rules: &RuleSet,
        raw: [i32; 3],
        location: [i32; 3],
        owner: Option<InternedId>,
    ) {
        let mut trails = std::mem::take(
            self.projectiles
                .get_mut(id)
                .expect("the live Bullet has trail state")
                .phobos_laser_trails_mut(),
        );
        if !trails.is_empty() {
            let art = rules.art();
            let session = &self.session;
            let outputs = &mut self.lifecycle_outputs;
            for trail in &mut trails {
                let Some(kind) = art.laser_trail(&trail.trail_type) else {
                    continue;
                };
                if trail.last_location.is_none() {
                    trail.last_location = Some(raw);
                }
                update(
                    &mut trail.last_location,
                    kind,
                    location,
                    true,
                    false,
                    owner,
                    rules.radiation.color,
                    session,
                    &self.main_rng,
                    outputs,
                );
            }
        }
        *self
            .projectiles
            .get_mut(id)
            .expect("the live Bullet has trail state")
            .phobos_laser_trails_mut() = trails;
    }

    /// `VoxelAnimClass_AI_Additional` (`Ext/VoxelAnim/Hooks.cpp:5..27`): every
    /// flight visit advances each trail to the retained `Object.Location`,
    /// before the stock bounce update.
    ///
    /// RESIDUAL: VERA has no separate `VoxelAnimClass::IsVisible` flag, so the
    /// trail is always visible while the debris is alive.
    pub(crate) fn update_voxel_anim_laser_trails(&mut self, id: u64, rules: &RuleSet) {
        let Some(piece) = self.substrate.voxel_anims.get(id) else {
            return;
        };
        let location = piece.world_coord().to_array();
        let owner = piece.owner_house();
        let mut trails = std::mem::take(
            self.substrate
                .voxel_anims
                .get_mut(id)
                .expect("the live VoxelAnim has trail state")
                .phobos_laser_trails_mut(),
        );
        if !trails.is_empty() {
            let art = rules.art();
            let session = &self.session;
            let outputs = &mut self.lifecycle_outputs;
            for trail in &mut trails {
                let Some(kind) = art.laser_trail(&trail.trail_type) else {
                    continue;
                };
                update(
                    &mut trail.last_location,
                    kind,
                    location,
                    true,
                    false,
                    owner,
                    rules.radiation.color,
                    session,
                    &self.main_rng,
                    outputs,
                );
            }
        }
        *self
            .substrate
            .voxel_anims
            .get_mut(id)
            .expect("the live VoxelAnim has trail state")
            .phobos_laser_trails_mut() = trails;
    }
}

/// `(int)(location.X + velocity.X)`: the C cast truncates a bullet's binary64
/// velocity before the trail compares it.
fn predicted_coord(location: i32, velocity: NativeF64Bits) -> i32 {
    (f64::from(location) + f64::from_bits(velocity.bits())).trunc() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::projectile::{
        ProjectileCollisionPolicy, ProjectilePayload, ProjectileSpawn, ProjectileTarget,
        ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
    };
    use crate::sim::world::FrameEffects;

    fn rules(rules: &str, art: &str) -> RuleSet {
        let art = IniFile::from_str(art);
        let mut rules =
            RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(rules), &art).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        rules
    }

    fn lasers(sim: &Simulation) -> Vec<crate::sim::combat::laser::LaserBirth> {
        sim.lifecycle_outputs
            .as_slice()
            .iter()
            .filter_map(|fact| match fact {
                LifecycleOutput::LaserCreated(laser) => Some(*laser),
                _ => None,
            })
            .collect()
    }

    /// `LaserTrailClass::Update`: seed, one segment per `SegmentLength`, and
    /// nothing while cloaked or invisible.
    #[test]
    fn a_trail_draws_one_segment_per_segment_length() {
        let mut sim = Simulation::with_seed(3);
        let mut last = None;
        let mut trail = LaserTrailType {
            thickness: 3,
            is_intense: true,
            ..LaserTrailType::default()
        };
        trail.segment_length = 64;
        let owner = InternedId::from_index(0);
        let draw = |sim: &mut Simulation,
                    last: &mut Option<[i32; 3]>,
                    trail: &LaserTrailType,
                    location,
                    visible,
                    cloaked,
                    owner| {
            update(
                last,
                trail,
                location,
                visible,
                cloaked,
                owner,
                (0, 0, 0),
                &sim.session,
                &sim.main_rng,
                &mut sim.lifecycle_outputs,
            )
        };
        assert!(!draw(
            &mut sim,
            &mut last,
            &trail,
            [0, 0, 0],
            true,
            false,
            Some(owner)
        ));
        assert_eq!(last, Some([0, 0, 0]), "the first update only seeds");
        assert!(!draw(
            &mut sim,
            &mut last,
            &trail,
            [32, 0, 0],
            true,
            false,
            Some(owner)
        ));
        assert!(draw(
            &mut sim,
            &mut last,
            &trail,
            [65, 0, 0],
            true,
            false,
            Some(owner)
        ));
        assert_eq!(last, Some([65, 0, 0]));
        let births = lasers(&sim);
        assert_eq!(births.len(), 1);
        assert_eq!(births[0].width, 3);
        assert!(births[0].supported);
        assert_eq!((births[0].from.x, births[0].to.x), (0, 65));

        // A cloaked trail still advances LastLocation but draws nothing.
        assert!(draw(
            &mut sim,
            &mut last,
            &trail,
            [131, 0, 0],
            true,
            false,
            Some(owner)
        ));
        assert!(!draw(
            &mut sim,
            &mut last,
            &trail,
            [197, 0, 0],
            true,
            true,
            Some(owner)
        ));
        assert_eq!(last, Some([197, 0, 0]));
        assert!(!draw(
            &mut sim,
            &mut last,
            &trail,
            [263, 0, 0],
            false,
            false,
            Some(owner)
        ));
        assert_eq!(last, Some([263, 0, 0]));
    }

    /// A projectile with `LaserTrail.Types` seeds at its raw `Location`, then
    /// advances at the drawn `Location + Velocity`; an `Inviso=` type gets no
    /// list at all (`Ext/Bullet/Hooks.cpp:117..123`).
    #[test]
    fn projectile_trails_use_the_predicted_drawn_coordinate() {
        let rules = rules(
            "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\nSpeed=5\nPrimary=GUN\n\
             [Weapons]\n0=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nImage=SHOT\n",
            "[LaserTrailTypes]\n0=TRAIL\n[TRAIL]\nSegmentLength=1\nFadeDuration=3\n\
             [SHOT]\nLaserTrail.Types=TRAIL\n",
        );
        let mut sim = Simulation::with_seed(3);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let source = sim
            .spawn_object_at_height("TANK", "Neutral", 1, 1, 0, 0, &rules)
            .expect("TANK spawns");
        let projectile_type = sim.interner.intern("SHOT");
        let warhead = sim.interner.intern("SA");
        let id = sim.substrate.allocate_stable_id();
        sim.admit_projectile(
            id,
            ProjectileSpawn {
                native_unique_id: 1,
                line_trail: None,
                flat: false,
                source_id: source,
                origin: ProjectileCoord::new(0, 0, 0),
                target: ProjectileTarget::Cell { rx: 1, ry: 0 },
                initial_target_position: ProjectileCoord::new(256, 0, 0),
                payload: ProjectilePayload::new(1, warhead, Some(projectile_type), false),
                speed_leptons_per_frame: 64,
                velocity: ProjectileVelocity::new(64, 0, 0),
                trajectory: ProjectileTrajectory::Straight,
                guidance: None,
                visual: ProjectileVisualState::new(0, 0, 0),
                arm_frames: 0,
                fuse_frames: None,
                ranged_fuse: false,
                tracks_target: false,
                target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
                collision: ProjectileCollisionPolicy::NONE,
            },
        );
        Simulation::construct_bullet_scheme(
            &sim.interner,
            &sim.substrate,
            &sim.houses,
            &mut sim.projectiles,
            id,
            &rules,
        );
        // Native seeds at the raw Location, then updates to Location +
        // Velocity: the first frame already draws from the launch point.
        sim.update_projectile_laser_trails(id, &rules);
        let first = lasers(&sim);
        assert_eq!(
            first.len(),
            1,
            "the first frame draws from the raw Location"
        );
        assert_eq!((first[0].from.x, first[0].to.x), (0, 64));
        sim.projectiles
            .get_mut(id)
            .unwrap()
            .set_position_for_test(ProjectileCoord::new(64, 0, 0));
        sim.update_projectile_laser_trails(id, &rules);
        let births = lasers(&sim);
        assert_eq!(births.len(), 2, "one SegmentLength crossing per frame");
        assert_eq!(births[1].duration, 3);
        assert_eq!((births[1].from.x, births[1].to.x), (64, 128));
    }

    /// Voxel debris with `LaserTrail.Types` (rules layer) trails its retained
    /// `Object.Location` on every flight visit.
    #[test]
    fn voxel_debris_trails_follow_the_retained_location() {
        let rules = rules(
            "[VoxelAnims]\n0=PIECE\n[PIECE]\nDuration=30\nLaserTrail.Types=TRAIL\n",
            "[LaserTrailTypes]\n0=TRAIL\n[TRAIL]\nSegmentLength=1\nFadeDuration=3\n",
        );
        let mut sim = Simulation::with_seed(3);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let type_id = rules.voxel_anim_type_id_by_name("PIECE").unwrap();
        let owner = InternedId::from_index(0);
        let mut packets = Vec::new();
        sim.emit_debris_voxel(
            type_id,
            rules.voxel_anim_type(type_id),
            Some(owner),
            glam::IVec3::new(256, 256, 0),
            false,
            &mut packets,
            &rules,
        )
        .unwrap();
        sim.admit_death_debris(packets, &rules);
        let id = sim.voxel_anims().ids()[0];
        sim.visit_voxel_anim(id, &rules, FrameEffects::default())
            .unwrap();
        assert!(lasers(&sim).is_empty(), "the first update only seeds");
        sim.visit_voxel_anim(id, &rules, FrameEffects::default())
            .unwrap();
        assert_eq!(lasers(&sim).len(), 1, "the bounce moved the location");
    }

    /// `LaserTrailClass`'s `CurrentColor`: the type's `Color` without a
    /// House (or without `IsHouseColor`), the House's laser colour when both
    /// exist. The LaserDraw still takes DrawBeamSpecial.
    #[test]
    fn laser_trail_colour_follows_current_color() {
        let mut sim = Simulation::with_seed(3);
        let trail = LaserTrailType {
            segment_length: 1,
            color: [1, 2, 3],
            fade_duration: Some(5),
            ..LaserTrailType::default()
        };
        let mut last = None;
        assert!(!update(
            &mut last,
            &trail,
            [0, 0, 0],
            true,
            false,
            None,
            (0, 0, 0),
            &sim.session,
            &sim.main_rng,
            &mut sim.lifecycle_outputs,
        ));
        assert!(update(
            &mut last,
            &trail,
            [10, 0, 0],
            true,
            false,
            None,
            (0, 0, 0),
            &sim.session,
            &sim.main_rng,
            &mut sim.lifecycle_outputs,
        ));
        assert_eq!(lasers(&sim)[0].color, LaserColor::HouseStyle([1, 2, 3]));

        let owner = InternedId::from_index(0);
        let house_trail = LaserTrailType {
            is_house_color: true,
            ..trail.clone()
        };
        last = None;
        assert!(!update(
            &mut last,
            &house_trail,
            [0, 0, 0],
            true,
            false,
            Some(owner),
            (0, 0, 0),
            &sim.session,
            &sim.main_rng,
            &mut sim.lifecycle_outputs,
        ));
        assert!(update(
            &mut last,
            &house_trail,
            [10, 0, 0],
            true,
            false,
            Some(owner),
            (0, 0, 0),
            &sim.session,
            &sim.main_rng,
            &mut sim.lifecycle_outputs,
        ));
        assert_eq!(lasers(&sim)[1].color, LaserColor::House(owner));
    }

    /// `LaserTrailClass::Update`'s EBolt and RadBeam arms: the bolt carries
    /// `Bolt.ZAdjust`, `Bolt.Arcs`, alternate colour, per-strand colours and
    /// `FadeDuration`'s lifetime exponent; the beam carries
    /// `Beam.Color.Get(RadColor)`, `FadeDuration.Get(15)` and
    /// `Beam.Amplitude`.
    #[test]
    fn ebolt_and_radbeam_trails_emit_their_visuals() {
        let mut sim = Simulation::with_seed(3);
        let mut last = None;
        let mut bolt = LaserTrailType {
            draw_type: LaserTrailDrawType::EBolt,
            bolt_z_adjust: -7,
            bolt_arcs: 3,
            is_alternate_color: true,
            bolt_disable: [false, true, false],
            bolt_color: [Some([1, 2, 3]), None, None],
            fade_duration: Some(4),
            ..LaserTrailType::default()
        };
        bolt.segment_length = 1;
        let mut beam = LaserTrailType {
            draw_type: LaserTrailDrawType::RadBeam,
            beam_color: Some([4, 5, 6]),
            beam_amplitude: 12.5,
            fade_duration: Some(9),
            ..LaserTrailType::default()
        };
        beam.segment_length = 1;
        let mut fallback = LaserTrailType {
            draw_type: LaserTrailDrawType::RadBeam,
            ..LaserTrailType::default()
        };
        fallback.segment_length = 1;
        let rad_color = (9, 8, 7);

        assert!(!update(
            &mut last,
            &bolt,
            [0, 0, 0],
            true,
            false,
            None,
            rad_color,
            &sim.session,
            &sim.main_rng,
            &mut sim.lifecycle_outputs,
        ));
        assert!(update(
            &mut last,
            &bolt,
            [10, 0, 0],
            true,
            false,
            None,
            rad_color,
            &sim.session,
            &sim.main_rng,
            &mut sim.lifecycle_outputs,
        ));
        for trail in [&beam, &fallback] {
            last = None;
            assert!(!update(
                &mut last,
                trail,
                [0, 0, 0],
                true,
                false,
                None,
                rad_color,
                &sim.session,
                &sim.main_rng,
                &mut sim.lifecycle_outputs,
            ));
            assert!(update(
                &mut last,
                trail,
                [10, 0, 0],
                true,
                false,
                None,
                rad_color,
                &sim.session,
                &sim.main_rng,
                &mut sim.lifecycle_outputs,
            ));
        }

        let facts = sim.lifecycle_outputs.as_slice();
        let bolt_birth = facts
            .iter()
            .find_map(|fact| match fact {
                LifecycleOutput::ElectricBoltCreated(birth) => Some(birth),
                _ => None,
            })
            .expect("the EBolt trail emits one bolt");
        assert_eq!(bolt_birth.z_adjust, -7);
        assert_eq!(bolt_birth.arcs, 3);
        assert!(bolt_birth.alternate_color);
        assert_eq!(
            bolt_birth.colors,
            [Some([1, 2, 3]), Some([0, 0, 0]), None],
            "a disabled strand keeps the constructor's black colour"
        );
        assert_eq!(bolt_birth.decay, 1 << 3, "FadeDuration=4");
        assert_eq!((bolt_birth.from.x, bolt_birth.to.x), (0, 10));
        let beams: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact {
                LifecycleOutput::RadiationBeamCreated(birth) => Some(birth),
                _ => None,
            })
            .collect();
        assert_eq!(beams.len(), 2);
        assert_eq!(beams[0].color, [4, 5, 6], "Beam.Color wins");
        assert_eq!(beams[0].duration, 9);
        assert_eq!(
            f64::from_bits(beams[0].amplitude.bits()),
            12.5,
            "Beam.Amplitude"
        );
        assert_eq!(beams[1].color, [9, 8, 7], "unset takes RadColor");
        assert_eq!(beams[1].duration, 15, "RadBeam's FadeDuration default");
    }
}
