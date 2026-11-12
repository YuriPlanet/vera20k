//! The launch source retained on a HunterSeeker, independently of later capture.

use crate::sim::intern::InternedId;

/// A Super as Ares TechnoExt::SuperWeapon (+0xA0) holds it: assigned
/// before Put by SW_HunterSeeker::Activate's callback (installed Ares
/// 0x10072190), and by SW_EMPulse::Activate (10071200) on each cannon it
/// fires. House supers retain their identity for the scenario's lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AttachedSuper {
    pub owner: InternedId,
    pub sw_type: InternedId,
}

#[cfg(feature = "ares")]
impl crate::sim::world::Simulation {
    /// Fly Acquire_Hunter_Seeker_Target (Ares 0x1004D770), delegated to
    /// TechnoExt::AcquireHunterSeekerTarget 0x10042E40..0x10043171. Installed
    /// 3.0 also excludes zero HP, out-of-playfield, invulnerable/warped targets;
    /// older public Ares source lacks those gates.
    pub(crate) fn acquire_hunter_seeker_target(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> bool {
        use crate::map::entities::EntityCategory;
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::rules::superweapon_targeting::{HouseMask, TargetMask};
        use crate::sim::combat::TargetKind;
        let Some(hunter) = self.substrate.entities.get(id) else {
            return false;
        };
        // Other locomotors' virtual method is a no-op, even when the SW uses
        // them as a helper. Do not turn stationary helper units into seekers.
        if hunter.attack_target.target().is_some()
            || hunter
                .locomotor
                .as_ref()
                .is_none_or(|l| l.active_kind() != LocomotorKind::Fly)
        {
            return false;
        }
        let origin = hunter.ares_super_weapon();
        let source = origin.map_or(hunter.owner(), |origin| origin.owner);
        let Some(house) = self.houses.get(&source) else {
            return false;
        };
        let settings = origin
            .and_then(|origin| rules.super_weapon(self.interner.resolve(origin.sw_type)))
            .and_then(|sw| sw.hunter_seeker.as_ref());
        let policy = origin
            .and_then(|origin| rules.super_weapon_metadata(self.interner.resolve(origin.sw_type)))
            .map(|metadata| metadata.targeting());
        let multiplayer = self.session.game_mode_nonzero();
        let human = house.is_controlled_by_human(multiplayer);
        let favourite_only = multiplayer && !human && house.enemy_house().is_some();
        let prefer_players = multiplayer && human && settings.is_none_or(|s| !s.random_only);
        let mut preferred = false;
        let mut candidates = Vec::new();
        // EntityStore preserves TechnoClass::Array's construction order.
        for candidate in self.substrate.entities.values() {
            if if favourite_only {
                Some(candidate.owner()) != house.enemy_house()
            } else {
                !policy
                    .map_or(HouseMask::ENEMIES, |p| p.affects_house())
                    .intersects(self.ares_house_relation(source, candidate.owner()))
            } {
                continue;
            }
            if candidate.health.current() <= 0
                || candidate.lifecycle.in_limbo()
                || !candidate.is_object_alive()
                || !candidate.in_playfield()
                || candidate.temporal.is_warped()
                || crate::sim::superweapon::invulnerability::is_invulnerable(
                    candidate.invulnerability.as_ref(),
                    self.session.binary_frame(),
                )
            {
                continue;
            }
            let Some(ty) = self.interner.object_type(candidate.type_ref(), rules) else {
                continue;
            };
            if ty.invisible
                || !ty.legal_target
                || ty.hunter_seeker_ignored()
                || (self.session.harvester_immune()
                    && candidate.category() == EntityCategory::Unit
                    && ty.harvester)
            {
                continue;
            }
            if let Some(policy) = policy {
                let mask = policy.affects_target();
                if mask.intersects(TargetMask::ALL_CELLS) {
                    let water = self
                        .resolved_terrain
                        .as_ref()
                        .and_then(|terrain| {
                            terrain.cell(candidate.position.rx(), candidate.position.ry())
                        })
                        .is_some_and(|cell| {
                            cell.land_type
                                == crate::rules::terrain_rules::LandType::Water.as_index()
                        });
                    if !mask.intersects(if water {
                        TargetMask::WATER
                    } else {
                        TargetMask::LAND
                    }) {
                        continue;
                    }
                }
                let category = match candidate.category() {
                    EntityCategory::Infantry => TargetMask::INFANTRY,
                    EntityCategory::Structure => TargetMask::BUILDING,
                    EntityCategory::Unit | EntityCategory::Aircraft => TargetMask::UNIT,
                };
                if mask.intersects(TargetMask::ALL_CONTENTS) && !mask.intersects(category) {
                    continue;
                }
            }
            let player = prefer_players
                && self
                    .houses
                    .get(&candidate.owner())
                    .is_some_and(|h| !h.multiplay_passive());
            if player && !preferred {
                candidates.clear();
                preferred = true;
            }
            if player || !preferred {
                candidates.push(candidate.stable_id());
            }
        }
        if candidates.is_empty() {
            return false;
        }
        // One inclusive Scenario RandomRanged (65C7E0); a one-element pool
        // returns at min == max (je 65C889) without drawing.
        let picked = self
            .scenario_rng
            .next_range_i32_inclusive(0, candidates.len() as i32 - 1);
        self.assign_target_represented(
            id,
            Some(TargetKind::Entity(candidates[picked as usize])),
            rules,
            registry,
            effects,
        )
        .expect("live Fly target setter");
        true
    }
}

/// What the Ares Horizontal_Step hook (1004DC30, hook 0x4CF3D0) leaves the
/// stock height block of `0x004CEFB0`.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HunterSeekerHeight {
    /// Return 0: the stock height computation runs.
    Stock,
    /// Return 0x4CF4D2 with this FlightLevel (Fly `+0x38`).
    Level(i32),
    /// The detonation arm: return 0x4CF5F2 with the step's result 0.
    Detonate,
}

/// The lowest descending FlightLevel 1004DC30 keeps.
#[cfg(feature = "ares")]
const DESCEND_MIN_LEVEL: i32 = 10;
/// 1004DC30's look-ahead: the current cell and up to 10 more steps.
#[cfg(feature = "ares")]
const LOOK_AHEAD_STEPS: i32 = 10;
/// `CellClass::GetCoordsWithBridge`: a bridge cell (flag 0x100) adds 0x1A0.
#[cfg(feature = "ares")]
const BRIDGE_DECK_Z: i32 = 0x1A0;

#[cfg(feature = "ares")]
impl crate::sim::world::Simulation {
    fn is_hunter_seeker(&self, id: u64, rules: &crate::rules::ruleset::RuleSet) -> bool {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.interner.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.hunter_seeker)
    }

    /// The hunter's Target when it is a Techno (`generic_cast<TechnoClass*>`).
    fn hunter_seeker_techno_target(&self, id: u64) -> Option<u64> {
        match self.substrate.entities.get(id)?.attack_target.target()? {
            crate::sim::combat::TargetKind::Entity(target)
                if self.substrate.entities.contains(target) =>
            {
                Some(target)
            }
            _ => None,
        }
    }

    /// Ares 3.0 FlyLocomotionClass_ILocomotion_Process_HunterSeeker
    /// (1004D7A0, hook 0x4CCB84), before Process's queued-mission check: a
    /// `HunterSeeker=` Fly drops a Techno Target that left the playfield
    /// (`+0x3D5`), is iron-curtained (vt+0x160) or is being warped out
    /// (vt+0x1D4) through Assign_Target(NULL) (vt+0x3C8); without a Target
    /// it reacquires as its landing callback does. True for a hunter.
    pub(crate) fn ares_hunter_seeker_process(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> bool {
        if !self.is_hunter_seeker(id, rules) {
            return false;
        }
        let frame = self.session.binary_frame();
        let lost = self
            .hunter_seeker_techno_target(id)
            .and_then(|target| self.substrate.entities.get(target))
            .is_some_and(|target| {
                !target.in_playfield()
                    || crate::sim::superweapon::invulnerability::is_invulnerable(
                        target.invulnerability.as_ref(),
                        frame,
                    )
                    || target.is_warped_out()
            });
        if lost {
            let _ = self.assign_target_represented(id, None, rules, registry, effects);
        }
        self.ares_hunter_seeker_reacquire(id, rules, registry, effects);
        true
    }

    /// Ares 3.0 FlyLocomotionClass_sub_4CD600_HunterSeeker_UpdateTarget
    /// (1004DA90, hook 0x4CD9C8, in UpdateFlightMotion before its Guard to
    /// Move queue): a hunter with a Target flies at the Target's GetCoords
    /// (a Unit or Infantry in a tube at its tube coordinate, which VERA does
    /// not represent), raised to the cell floor (`0x00578080`), as the Fly's
    /// destination (`+0x1C`), and snaps its body (`+0x388`) and turret
    /// (`+0x3A0`) facings toward it.
    pub(crate) fn ares_hunter_seeker_update_target(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        if !self.is_hunter_seeker(id, rules) {
            return;
        }
        let Some(target) = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.attack_target.target())
        else {
            return;
        };
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let Some(mut at) =
            crate::sim::movement::ground_pose::target_get_coords(target, self, cells.as_ref())
        else {
            return;
        };
        let floor = crate::sim::projectile::projectile_ground_z(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            crate::sim::projectile::ProjectileCoord::new(at.x, at.y, at.z),
        );
        at.z = at.z.max(floor);
        let frame = self.session.binary_frame();
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let source = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
        // Math::arctanfoo(source.Y - Y, X - source.X) into a DirStruct.
        let facing =
            crate::util::direction_tables::facing16_between([source.x, source.y], [at.x, at.y]);
        if let Some(fly) = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.fly_runtime_mut())
        {
            fly.hunt_destination(at);
        }
        entity.body_facing.snap(facing, frame);
        entity.ensure_fly_secondary_facing();
        if let Some(turret) = entity.barrel_facing.as_mut() {
            turret.snap(facing, frame);
        }
    }

    /// Ares 3.0 FlyLocomotionClass_sub_4CEFB0_HunterSeeker (1004DC30, hook
    /// 0x4CF3D0), the height block of Horizontal_Step for a hunter with a
    /// Target. Its XY distance to the destination (CRT sqrt, truncated)
    /// below `HunterSeeker.DetonateProximity` detonates on a Techno Target;
    /// below `.DescendProximity` FlightLevel blends from the type's flight
    /// height above the floor (`ratio = distance / proximity`) down to the
    /// Target's Z, less the floor, at least 10. Farther, with a positive
    /// Apparent_Speed it looks along the body facing (the game's sine table
    /// as YRpp Math::cos/sin call it) one speed per step through the next
    /// 10 legal cells; a cell higher (with bridge decks) than the current
    /// one restores the type's FlightLevel.
    pub(crate) fn ares_hunter_seeker_height(
        &self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> HunterSeekerHeight {
        use crate::util::native_x87::X87Chop53 as X;
        let Some(entity) = self.substrate.entities.get(id) else {
            return HunterSeekerHeight::Stock;
        };
        let Some(object) = self
            .interner
            .object_type(entity.type_ref(), rules)
            .filter(|object| object.hunter_seeker)
        else {
            return HunterSeekerHeight::Stock;
        };
        let Some(target) = entity.attack_target.target() else {
            return HunterSeekerHeight::Stock;
        };
        let Some(fly) = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
        else {
            return HunterSeekerHeight::Stock;
        };
        let (detonate, descend) = object
            .techno_combat()
            .hunter_seeker_proximities(rules.ares_general());
        let terrain = self.resolved_terrain.as_ref();
        let own = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
        let destination = fly.destination();
        let (dx, dy) = (
            f64::from(own.x.wrapping_sub(destination.x)),
            f64::from(own.y.wrapping_sub(destination.y)),
        );
        // CoordStruct::Magnitude's CRT sqrt; its truncation to an integer
        // is the same under either x87 rounding mode for these sums.
        let distance = (dx * dx + dy * dy).sqrt() as i32;
        if distance < detonate {
            return if self.hunter_seeker_techno_target(id).is_some() {
                HunterSeekerHeight::Detonate
            } else {
                HunterSeekerHeight::Stock
            };
        }
        let flight_level = crate::sim::movement::air_movement::type_flight_level(
            entity,
            Some((rules, &self.interner)),
        );
        let floor_at = |x: i32, y: i32, z: i32| {
            crate::sim::projectile::projectile_ground_z(
                terrain,
                &self.effective_shared_cell_dummy(),
                crate::sim::projectile::ProjectileCoord::new(x, y, z),
            )
        };
        if distance < descend {
            let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            let target_z =
                crate::sim::movement::ground_pose::target_get_coords(target, self, cells.as_ref())
                    .map_or(0, |at| at.z);
            let floor = floor_at(own.x, own.y, own.z);
            let height = floor.wrapping_add(flight_level);
            let ratio = X::div(X::load_i32(distance), X::load_i32(descend))
                .expect("a positive DescendProximity divides");
            let one = X::load_i32(1);
            let lerp = X::add(
                X::mul(X::load_i32(target_z), X::sub(one, ratio)),
                X::mul(X::load_i32(height), ratio),
            );
            let level = (X::ftol_i64(lerp).unwrap_or(0) as i32).wrapping_sub(floor);
            return HunterSeekerHeight::Level(level.max(DESCEND_MIN_LEVEL));
        }
        // Apparent_Speed (`0x004CFE20`).
        let speed = crate::sim::movement::air_movement::current_fly_speed(
            crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed),
            fly.current_speed(),
        );
        if speed <= 0 {
            return HunterSeekerHeight::Stock;
        }
        let frame = self.session.binary_frame();
        let radians =
            crate::util::direction_tables::radians_from_facing16(entity.body_facing.current(frame));
        let trig = crate::util::retail_trig::TrigTable::embedded();
        let radians = X::load_f64(radians).expect("a facing's radians are finite");
        let step = |value| X::ftol_i64(X::mul(value, X::load_i32(speed))).unwrap_or(0) as i32;
        let (step_x, step_y) = (
            step(trig.cos_from_table(radians)),
            step(trig.sin_from_table(radians)).wrapping_neg(),
        );
        let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let (mut x, mut y) = (own.x, own.y);
        let (mut highest, mut current) = (0, 0);
        for step_index in 0..=LOOK_AHEAD_STEPS {
            // MapClass::GetCellAt, then CellClass::GetCoordsWithBridge's Z.
            let (rx, ry) = ((x / 256) as i16, (y / 256) as i16);
            let mut z = crate::sim::movement::navcom::target_cell_coord(
                rx as u16,
                ry as u16,
                cells.as_ref(),
            )
            .z;
            if let Some(terrain) = terrain
                && terrain.native_cell_flags(terrain.native_cell_identity((rx, ry))) & 0x100 != 0
            {
                z = z.wrapping_add(BRIDGE_DECK_Z);
            }
            highest = highest.max(z);
            if step_index == 0 {
                current = z;
            }
            x = x.wrapping_add(step_x);
            y = y.wrapping_add(step_y);
            if !self.map_coordinates_legal(x, y) {
                break;
            }
        }
        if highest > current {
            HunterSeekerHeight::Level(flight_level)
        } else {
            HunterSeekerHeight::Stock
        }
    }

    /// 1004DC30's detonation arm: the Techno Target and then the hunter take
    /// its weapon 0's Damage with its Warhead through ReceiveDamage (vt+0x16C,
    /// distance 0, IgnoreDefenses, the hunter as the Target's attacker and
    /// none for its own), then the C4 flash (`0x0048A620`) and
    /// Apply_area_damage of the weapon's Damage and Warhead at the hunter.
    pub(crate) fn ares_hunter_seeker_detonate(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) {
        let Some((damage, warhead)) = self.hunter_seeker_weapon(id, rules) else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let at = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        if let Some(target) = self.hunter_seeker_techno_target(id) {
            self.hunter_seeker_strike(target, id, damage, warhead, rules, registry, effects);
        }
        self.hunter_seeker_strike(
            id,
            crate::sim::combat::RAD_NO_ATTACKER,
            damage,
            warhead,
            rules,
            registry,
            effects,
        );
        // The hunter's GetCoords; it is read before the two receivers, as no
        // Fly owner moves inside them.
        let coord = crate::sim::projectile::ProjectileCoord::new(at.x, at.y, at.z);
        self.combat_light_requests
            .emit(crate::sim::combat::CombatLightRequest::Impact {
                target_id: None,
                damage,
                warhead_ref: self.interner.intern(&rules.bridge_warheads.c4_name),
                coord,
                force_create: false,
                flags: 0,
            });
        if let Some(warhead_type) = self.interner.warhead_type(warhead, rules) {
            let _ = crate::sim::combat::world_receiver::apply_area_damage(
                self,
                rules,
                registry,
                coord,
                damage,
                warhead_type,
                true,
                (id, None, warhead),
                effects,
            );
        }
    }

    /// Ares 3.0 FootClass_UpdatePosition_HunterSeeker (1004E050, hook
    /// 0x4D8D95, PerCellProcess's off-playfield exit before its UnInit): a
    /// hunter leaving the map hits its Techno Target with weapon 0 so it
    /// does not get away.
    pub(crate) fn ares_hunter_seeker_parting_strike(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) {
        if !self.is_hunter_seeker(id, rules) {
            return;
        }
        let Some(target) = self.hunter_seeker_techno_target(id) else {
            return;
        };
        if let Some((damage, warhead)) = self.hunter_seeker_weapon(id, rules) {
            self.hunter_seeker_strike(target, id, damage, warhead, rules, registry, effects);
        }
    }

    /// GetWeapon(0) (vt+0x3F8) at the hunter's veterancy: Damage and Warhead.
    fn hunter_seeker_weapon(
        &self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<(i32, InternedId)> {
        let entity = self.substrate.entities.get(id)?;
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        let weapon =
            crate::sim::combat::combat_weapon::primary_for_tier(object, entity.veterancy.rank())
                .and_then(|weapon| rules.weapon(weapon))?;
        let warhead = self.interner.get(weapon.warhead.as_deref()?)?;
        Some((weapon.damage, warhead))
    }

    /// `ReceiveDamage(&damage, 0, warhead, attacker, true, true, NULL)`.
    #[allow(clippy::too_many_arguments)]
    fn hunter_seeker_strike(
        &mut self,
        victim: u64,
        attacker: u64,
        damage: i32,
        warhead: InternedId,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) {
        self.commit_direct_damage_receiver(
            rules,
            registry,
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                victim,
                damage,
                0,
                attacker,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
            effects,
        );
    }
}

#[cfg(all(test, feature = "ares"))]
mod tests {
    use super::HunterSeekerHeight;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::TargetKind;
    use crate::sim::components::DriveCoord;
    use crate::sim::world::Simulation;

    /// Ares FlyLocomotionClass_sub_4CEFB0_HunterSeeker (1004DC30, hook
    /// 0x4CF3D0): a hunter whose destination is within
    /// `HunterSeekerDetonateProximity` of it detonates on its Techno Target;
    /// within `DescendProximity` it blends down toward the Target, never
    /// below 10; farther over flat ground the stock height block runs.
    #[test]
    fn ares_hunter_seeker_detonates_and_descends_inside_its_proximities() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nHunterSeekerDetonateProximity=100\nHunterSeekerDescendProximity=1000\n\
             [AircraftTypes]\n0=TEST\n[VehicleTypes]\n0=PREY\n\
             [TEST]\nHunterSeeker=yes\nStrength=100\nSpeed=10\n\
             Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
             [PREY]\nStrength=100\nSpeed=4\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let hunter = sim.spawn_object("TEST", "A", 10, 10, 0, &rules).unwrap();
        let prey = sim.spawn_object("PREY", "B", 20, 10, 0, &rules).unwrap();
        let height_at = |sim: &mut Simulation, dx: i32, target: Option<TargetKind>| {
            let entity = sim.substrate.entities.get_mut(hunter).unwrap();
            entity.attack_target.set_for_test(target);
            let at = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            entity
                .locomotor
                .as_mut()
                .and_then(|locomotor| locomotor.fly_runtime_mut())
                .expect("the hunter flies")
                .hunt_destination(DriveCoord {
                    x: at.x + dx,
                    y: at.y,
                    z: 0,
                });
            sim.ares_hunter_seeker_height(hunter, &rules)
        };
        let prey_target = Some(TargetKind::Entity(prey));
        assert_eq!(
            height_at(&mut sim, 50, prey_target),
            HunterSeekerHeight::Detonate
        );
        assert_eq!(
            height_at(&mut sim, 50, Some(TargetKind::Cell(20, 10))),
            HunterSeekerHeight::Stock,
            "only a Techno target detonates"
        );
        assert!(matches!(
            height_at(&mut sim, 500, prey_target),
            HunterSeekerHeight::Level(level) if level >= 10
        ));
        assert_eq!(
            height_at(&mut sim, 5000, prey_target),
            HunterSeekerHeight::Stock
        );
        assert_eq!(
            height_at(&mut sim, 50, None),
            HunterSeekerHeight::Stock,
            "without a Target the hook does nothing"
        );
    }
}
