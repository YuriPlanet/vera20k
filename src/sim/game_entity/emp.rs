//! Ordinary ground-victim Ares EMP. The Techno+504 counter and held Anim
//! have one private owner; the existing powered-unit owner keeps +1C8.
//! Installed Bullet46920B ->100536E0/10062320 and AI6FAF0D ->10053DF0.
//! Evidence/transport bounds: tools/rules_oracle/ares_emp.{py,json,meta.json};
//! full chain ownership and residuals: docs/ares-emp.md.
use super::GameEntity;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct EmpState {
    remaining: i32,
    sparkle: Option<u64>,
    last_mission: i32,
}
impl Default for EmpState {
    fn default() -> Self {
        // Installed TechnoExt constructor10042D20/10042D27.
        Self {
            remaining: 0,
            sparkle: None,
            last_mission: -1,
        }
    }
}
impl GameEntity {
    pub(crate) fn emp_remaining(&self) -> i32 {
        self.emp.remaining
    }
    pub(crate) fn emp_sparkle(&self) -> Option<u64> {
        self.emp.sparkle
    }
    pub(crate) fn emp_anim_pointer_expired(&mut self, expired: u64) {
        if self.emp.sparkle == Some(expired) {
            self.emp.sparkle = None;
        }
    }
    pub(crate) fn hash_emp_state<H: Hasher>(&self, hasher: &mut H) {
        if self.emp != EmpState::default() {
            b"techno-emp-v1".hash(hasher);
            self.emp.hash(hasher);
        }
    }
}

#[cfg(feature = "ares")]
mod receiver {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::{
        locomotor_type::LocomotorKind, ruleset::RuleSet, warhead_type::WarheadType,
    };
    use crate::sim::{
        anim_class::AnimWorldCoord,
        components::AnimClassSpawnDescriptor,
        mission::MissionType,
        world::{FrameEffects, Simulation},
    };
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum EmpDelivery {
        Rejected,
        Applied,
        /// Deliberate later mechanisms; never represented by a generic offline bool.
        Unsupported,
    }
    /// Installed10062476..100624B4; signed32 addition wraps before max0.
    fn capped_duration(old: i32, delta: i32, cap: i32) -> i32 {
        let sum = old.wrapping_add(delta);
        // Preserve10062476..A7's separate signed branches: moving max/min
        // across this wrapping add changes overflow and zero-cap removal.
        let new = if delta > 0 {
            if cap < 0 {
                old.max(delta)
            } else if cap == 0 {
                sum
            } else {
                old.max(sum.min(cap))
            }
        } else if cap < 0 {
            sum
        } else {
            sum.min(cap)
        };
        new.max(0)
    }
    fn duration_delta(duration: i32, modifier: NativeF64Bits) -> i32 {
        X87::fistp_i32_masked(X87::mul(
            X87::load_i32(duration),
            X87::load_f64(if duration > 0 {
                modifier
            } else {
                NativeF64Bits::ONE
            }),
        ))
    }
    fn advisable(entity: &GameEntity) -> bool {
        !matches!(
            entity.mission.current().known(),
            Some(MissionType::Unload | MissionType::Selling | MissionType::Construction)
        )
    }
    /// Installed10035C26..10035D74 ring/row cursor. The first ring differs
    /// from gamemd's static AreaDamage table, and this cursor extends to256.
    fn visit_emp_cells(center: (i16, i16), radius: i32, mut visit: impl FnMut((i16, i16))) {
        visit(center);
        for ring in 1..=radius {
            let (mut x, mut y) = (-1i32, -ring);
            let (mut advance, mut mirror_advance) = (true, true);
            loop {
                visit((
                    center.0.wrapping_add(x as i16),
                    center.1.wrapping_add(y as i16),
                ));
                if x == 1 && y == ring {
                    break;
                }
                if advance {
                    x += 1;
                    advance = x == 0;
                } else if x < 0 {
                    x = -x;
                    advance = mirror_advance;
                } else {
                    y += 1;
                    let magnitude = y.abs();
                    x = -2 * (ring - magnitude) - 1;
                    let edge = x + magnitude;
                    advance = edge >= 0;
                    if edge < 0 {
                        x = if edge == -1 {
                            x + 1
                        } else {
                            -ring + magnitude / 2
                        };
                    }
                    mirror_advance = advance;
                }
            }
        }
    }
    /// Native100767F2 squares signed wrapped XYZ differences, FSQRTs and
    /// compares against CellSpread*256. Compare squared nonnegative lengths
    /// instead: ordinary map deltas have exact integer squares in binary64,
    /// avoid integer sqrt truncation, and are deterministic on every target.
    /// Native FSQRT rounding at arbitrary fractional boundaries is a residual;
    /// native corpus pins exact zero, half-cell and nonzero-Z controls.
    fn emp_ground_in_range(
        impact: crate::sim::projectile::ProjectileCoord,
        coord: [i32; 3],
        spread: f64,
    ) -> bool {
        let delta = [
            coord[0].wrapping_sub(impact.x),
            coord[1].wrapping_sub(impact.y),
            coord[2].wrapping_sub(impact.z),
        ];
        let cutoff = spread * 256.0;
        if cutoff < 0.0 || cutoff.is_nan() {
            return false;
        }
        let square = delta.map(|value| {
            let value = f64::from(value);
            value * value
        });
        square[0] + square[1] + square[2] <= cutoff * cutoff
    }
    impl Simulation {
        /// Installed Bullet46920B calls this pre-effect even when Damage=0.
        /// The bounded producer collects the ground CellClass lists. Airborne
        /// victims and their half-distance collector remain a later mechanism.
        pub(crate) fn apply_ares_emp_detonation(
            &mut self,
            impact: crate::sim::projectile::ProjectileCoord,
            source: Option<u64>,
            warhead: &WarheadType,
            rules: &RuleSet,
            effects: FrameEffects<'_>,
        ) {
            if !warhead.emp_rules().is_some_and(|emp| emp.duration() != 0) {
                return;
            }
            for target in self.ares_emp_ground_targets(impact, warhead.cell_spread_f64, rules) {
                let result = self.deliver_ares_emp(target, source, warhead, rules, effects);
                if result == EmpDelivery::Unsupported {
                    log::debug!("Ares EMP victim {target} requires a later dependency chain");
                }
            }
        }
        fn ares_emp_ground_targets(
            &self,
            impact: crate::sim::projectile::ProjectileCoord,
            spread: f64,
            rules: &RuleSet,
        ) -> Vec<u64> {
            use crate::map::{cell_index::NativeCellIdentity, resolved_terrain::NativeCellQuery};
            use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
            let radius = (X87::fistp_i32_masked(X87::add(
                X87::load_f64(NativeF64Bits::from_bits(spread.to_bits())),
                X87::load_f64(NativeF64Bits::from_bits(0.99f64.to_bits())),
            )) as u32)
                .min(256) as i32;
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(NativeCellQuery::canonical);
            let center = crate::sim::movement::coord_cell(crate::sim::components::DriveCoord {
                x: impact.x,
                y: impact.y,
                z: impact.z,
            });
            let mut candidates = Vec::new();
            visit_emp_cells(center, radius, |requested| {
                let (rx, ry, layer) = if let Some(cells) = cells.as_ref() {
                    //10035B2A..10035B48 indexes the fixed cell array and
                    // skips null, rather than stamping MapClass's Dummy.
                    let Some(index) = cells
                        .terrain()
                        .native_fixed_cell_index(requested.0, requested.1)
                    else {
                        return;
                    };
                    let cell = NativeCellIdentity::Real(index);
                    let (x, y) = cells.coord(cell);
                    (
                        x as u16,
                        y as u16,
                        if cells.flags(cell) & 0x100 != 0 {
                            MovementLayer::Bridge
                        } else {
                            MovementLayer::Ground
                        },
                    )
                } else {
                    (
                        requested.0 as u16,
                        requested.1 as u16,
                        MovementLayer::Ground,
                    )
                };
                if let Some(occupancy) = self.substrate.occupancy.get(rx, ry) {
                    for occupant in occupancy.iter_layer(layer) {
                        let Some(entity) = self.substrate.entities.get(occupant.entity_id) else {
                            continue;
                        };
                        if entity.category() == EntityCategory::Structure
                            && self
                                .interner
                                .object_type(entity.type_ref(), rules)
                                .is_some_and(|kind| kind.invisible_in_game)
                        {
                            continue;
                        }
                        let Some(coord) = ground_pose::target_get_coords(
                            crate::sim::combat::TargetKind::Entity(occupant.entity_id),
                            self,
                            cells.as_ref(),
                        ) else {
                            continue;
                        };
                        if emp_ground_in_range(impact, [coord.x, coord.y, coord.z], spread) {
                            candidates.push((entity.native_unique_id(), occupant.entity_id));
                        }
                    }
                }
            });
            // Native set comparator10035BA8/100765BA uses signed Abstract+10,
            // including equality: two references to the same UID deliver once.
            candidates.sort_by_key(|(uid, _)| *uid);
            candidates.dedup_by_key(|(uid, _)| *uid);
            candidates.into_iter().map(|(_, id)| id).collect()
        }
        /// One actual10062320 delivery. The initial supported chain is human
        /// Drive/Walk, with no Team/managers/harvester/positive death threshold.
        /// Their distinct detach/AI/aircraft/building paths remain explicit residuals.
        fn deliver_ares_emp(
            &mut self,
            target: u64,
            source: Option<u64>,
            warhead: &WarheadType,
            rules: &RuleSet,
            effects: FrameEffects<'_>,
        ) -> EmpDelivery {
            let Some(settings) = warhead
                .emp_rules()
                .filter(|settings| settings.duration() != 0)
            else {
                return EmpDelivery::Rejected;
            };
            let Some(victim) = self.substrate.entities.get(target) else {
                return EmpDelivery::Rejected;
            };
            let Some(object) = self.interner.object_type(victim.type_ref(), rules) else {
                return EmpDelivery::Rejected;
            };
            let Some(kind) = object.emp_rules() else {
                return EmpDelivery::Rejected;
            };
            let source_owner = source
                .and_then(|source| self.substrate.entities.get(source))
                .map(|source| source.owner());
            let allied = source_owner.is_some_and(|owner| {
                crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(self.house_alliances.as_map()),
                    &self.interner,
                    owner,
                    victim.owner(),
                )
            });
            if !settings.can_affect_house(
                source_owner.is_some(),
                source_owner == Some(victim.owner()),
                allied,
                warhead.affects_allies,
            ) || victim.is_warped_out()
                || crate::sim::superweapon::invulnerability::is_invulnerable(
                    victim.invulnerability.as_ref(),
                    self.session.binary_frame(),
                )
                || victim.lifecycle.in_limbo()
                || !victim.lifecycle.object_alive()
            {
                return EmpDelivery::Rejected;
            }
            let default_immune = match victim.category() {
                EntityCategory::Infantry => !object.cyborg,
                EntityCategory::Unit => object.organic,
                _ => return EmpDelivery::Unsupported,
            };
            if kind.immune().unwrap_or(default_immune) || kind.rank_immune(victim.veterancy.rank())
            {
                return EmpDelivery::Rejected;
            }
            if source_owner == Some(victim.owner()) && object.type_immune {
                // Reuse every represented rank-specific weapon slot. Native
                // scans20; semantics/configuration of18/19 are a separate
                // unresolved getter/reader boundary, not guessed new slots.
                let elite = victim.veterancy.rank() >= 200;
                if (0..crate::rules::object_type::WEAPON_SLOT_COUNT).any(|index| {
                    (if elite {
                        object.elite_weapon_at(index)
                    } else {
                        object.weapon_at(index)
                    })
                    .and_then(|name| rules.weapon(name))
                    .and_then(|weapon| weapon.warhead.as_deref())
                    .and_then(|name| rules.warhead(name))
                    .and_then(|wh| wh.emp_rules())
                    .is_some_and(|emp| emp.duration() != 0)
                }) {
                    return EmpDelivery::Rejected;
                }
            }
            let verses = f64::from_bits(warhead.interaction(object.armor_id()).multiplier().bits());
            if verses.abs() < 0.001 {
                return EmpDelivery::Rejected;
            }
            // Restrict the new producer by actual dependency support, not a
            // fabricated immunity. An unsupported victim is observable to tests.
            if !matches!(
                victim.locomotor.as_ref().map(|loco| loco.active_kind()),
                Some(LocomotorKind::Drive | LocomotorKind::Walk)
            ) || !self.owner_is_human(victim.owner())
                || victim.is_harvester()
                || victim.is_falling_down()
                || victim.spawn_manager.is_some()
                || victim.slave_manager.is_some()
                || victim.capture_manager.is_some()
                || victim.mind_control.controller().is_some()
                || victim.temporal.warp_target().is_some()
                || self.team_script_vm.team_for_member(target).is_some()
                || kind.threshold() > 0
            {
                return EmpDelivery::Unsupported;
            }
            if self.unit_in_contact_war_factory(target, rules) {
                return EmpDelivery::Rejected;
            }
            let delta = duration_delta(settings.duration(), kind.modifier());
            let old = victim.emp.remaining;
            let remaining = capped_duration(old, delta, settings.cap());
            //10062625 first activation may select Type/General fallback;
            //1006256D refresh only supplies the explicit WH pointer. Preserve
            // that provenance when a prior held Anim has expired.
            let first_activation = old <= 0 && remaining > 0;
            let sparkle_name = settings
                .sparkles()
                .or_else(|| {
                    if first_activation {
                        kind.sparkles().unwrap_or_else(|| rules.emp_sparkles())
                    } else {
                        None
                    }
                })
                .map(str::to_owned);
            self.substrate
                .entities
                .get_mut(target)
                .expect("live EMP target")
                .emp
                .remaining = remaining;
            if old > 0 && remaining == 0 {
                self.disable_ares_emp(target, rules, effects);
                // Ares3.0 10062526..10062563: healing EMP raises65 then64.
                // Natural expiry calls Disable directly and raises neither.
                // Both calls resolve the current Tag; actions may remove it.
                let context =
                    crate::sim::world::UninitContext::new(rules, effects.overlay_registry())
                        .with_effects(effects);
                self.dispatch_object_tag_event(target, 65, Some(target), source, context);
                if self
                    .substrate
                    .entities
                    .get(target)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
                {
                    self.dispatch_object_tag_event(target, 64, Some(target), None, context);
                }
            } else if old <= 0 && remaining > 0 {
                let selected = self
                    .substrate
                    .entities
                    .get(target)
                    .is_some_and(|entity| entity.selection.is_selected());
                self.enable_ares_emp(target, rules, effects);
                if selected {
                    if let Some(entity) = self.substrate.entities.get_mut(target) {
                        entity.selection.select();
                    }
                }
                // 100624C5..10062509 checks Alive before63 and again before
                //62. The generic raise has no source; retain the current Tag
                //rather than a pre-callback attachment. Neither check skips
                //the common threshold/sparkle tail in the native body.
                let context =
                    crate::sim::world::UninitContext::new(rules, effects.overlay_registry())
                        .with_effects(effects);
                if self
                    .substrate
                    .entities
                    .get(target)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
                {
                    self.dispatch_object_tag_event(target, 63, Some(target), source, context);
                    if self
                        .substrate
                        .entities
                        .get(target)
                        .is_some_and(|entity| entity.lifecycle.object_alive())
                    {
                        self.dispatch_object_tag_event(target, 62, Some(target), None, context);
                    }
                }
            } else if old == remaining {
                return EmpDelivery::Applied;
            }
            if first_activation || settings.sparkles().is_some() {
                self.ares_emp_sparkle(target, sparkle_name.as_deref(), rules);
            }
            EmpDelivery::Applied
        }
        fn enable_ares_emp(&mut self, target: u64, rules: &RuleSet, effects: FrameEffects<'_>) {
            if let Some(entity) = self.substrate.entities.get_mut(target) {
                entity.emp.last_mission = entity.mission.current().raw();
            }
            if self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| !entity.deactivated.is_offline() && advisable(entity))
            {
                self.techno_deactivate(target, rules, effects);
            }
        }
        fn disable_ares_emp(&mut self, target: u64, rules: &RuleSet, effects: FrameEffects<'_>) {
            if self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| entity.deactivated.is_offline())
            {
                self.techno_reactivate(target, rules, effects);
            }
            self.retire_ares_emp_sparkle(target);
        }
        /// Installed10053DF0 is reached once per actual Techno AI visit.
        pub(crate) fn tick_ares_emp(
            &mut self,
            target: u64,
            rules: &RuleSet,
            effects: FrameEffects<'_>,
        ) {
            let Some(entity) = self.substrate.entities.get_mut(target) else {
                return;
            };
            if entity.emp.remaining == 0 {
                return;
            }
            entity.emp.remaining = entity.emp.remaining.wrapping_sub(1);
            let remaining = entity.emp.remaining;
            let delayed = !entity.deactivated.is_offline() && advisable(entity);
            if remaining == 0 {
                self.disable_ares_emp(target, rules, effects);
            } else if delayed {
                self.enable_ares_emp(target, rules, effects);
            }
        }
        fn ares_emp_sparkle(&mut self, target: u64, name: Option<&str>, rules: &RuleSet) {
            let Some(entity) = self.substrate.entities.get(target) else {
                return;
            };
            // UpdateSparkle10062FE0 reads the live counter after tag callbacks.
            // A nested effect can replace the state used by the outer delivery.
            if entity.emp.remaining <= 0 {
                self.retire_ares_emp_sparkle(target);
                return;
            }
            if entity.emp.sparkle.is_some() {
                return;
            }
            let Some(name) = name else {
                return;
            };
            let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            let world = AnimWorldCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            };
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let name = self.interner.intern(name);
            // Native10063012..10063029 constructor pushes loop1,flags600.
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(name, rx, ry, sub_x, sub_y, z)
            };
            if let Ok(anim) = self.spawn_anim_at_world(rules, descriptor, world) {
                self.set_anim_owner_object(anim, Some(target), rules);
                if let Some(entity) = self.substrate.entities.get_mut(target) {
                    entity.emp.sparkle = Some(anim);
                }
            }
        }
        fn retire_ares_emp_sparkle(&mut self, target: u64) {
            let anim = self
                .substrate
                .entities
                .get_mut(target)
                .and_then(|entity| entity.emp.sparkle.take());
            if let Some(anim) = anim.and_then(|id| self.substrate.anims.get_mut(id)) {
                anim.end_loops();
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::CellListInsertion;
        use crate::sim::projectile::ProjectileCoord;
        fn corpus() -> serde_json::Value {
            serde_json::from_str(crate::test_fixture::text(
                "tools/rules_oracle/ares_emp.json",
            ))
            .unwrap()
        }
        fn int(value: &serde_json::Value) -> i32 {
            value.as_i64().unwrap() as i32
        }
        fn rules(extra: &str) -> RuleSet {
            let ini = IniFile::from_str(
                "[VehicleTypes]\n0=TANK\n[Warheads]\n0=EMPWH\n[Animations]\n0=SPARK\n[TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n[EMPWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nEMP.Duration=2\nEMP.Cap=2\nEMP.Sparkles=SPARK\n",
            );
            let art = IniFile::from_str("[SPARK]\nImage=SPARK\nLoopCount=-1\nRate=1\nEnd=4\n");
            let mut layers = crate::rules::native_processing::RulesLayerStack::new(ini);
            if !extra.is_empty() {
                // Native INI duplicate keys within one physical file are
                // first-wins. Apply corpus priors through a real later Rules
                // layer, not duplicate keys after the two-tick scene defaults.
                layers.push(
                    crate::rules::native_processing::RulesLayerKind::Scenario,
                    IniFile::from_str(&format!("[EMPWH]\n{extra}")),
                );
            }
            let processed = layers.process_with_fixed_art(&art).unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
            // Synthetic frame-count asset boundary, not an art extraction.
            registry.bind_anim_frame_count_for_test("SPARK", 4);
            rules.install_art_data(registry);
            rules
        }
        fn install_emp_trigger_chain(sim: &mut Simulation, source_house: i32, repeat: i32) {
            let map = crate::map::map_file::MapFile::from_bytes(
                format!(
                    "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
                     [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
                     [Basic]\nNewINIFormat=4\n\
                     [Triggers]\nE62=Neutral,E63,E62,0,1,1,1,0\n\
                     E63=Neutral,E64,E63,0,1,1,1,0\nE64=Neutral,E65,E64,0,1,1,1,0\n\
                     E65=Neutral,<none>,E65,0,1,1,1,0\n\
                     [Events]\nE62=1,62,0,0\nE63=1,63,0,{source_house}\n\
                     E64=1,64,0,0\nE65=1,65,0,{source_house}\n\
                     [Actions]\nE62=1,56,0,0,0,0,0,0,A\nE63=1,56,0,1,0,0,0,0,A\n\
                     E64=1,56,0,2,0,0,0,0,A\nE65=1,56,0,3,0,0,0,0,A\n\
                     [Tags]\nEMP={repeat},EMP,E62\n"
                )
                .as_bytes(),
            )
            .unwrap();
            sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
        }
        fn tagged_emp_scene(
            source_house_delta: i32,
            repeat: i32,
        ) -> (Simulation, RuleSet, u64, u64) {
            let rules = rules("");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                .unwrap();
            let source_house = sim
                .base_reservation_house_index(sim.entities().get(source).unwrap().owner())
                .unwrap();
            install_emp_trigger_chain(&mut sim, source_house + source_house_delta, repeat);
            sim.attach_object_trigger_tag(target, Some("EMP"));
            (sim, rules, target, source)
        }
        fn emp_event_kinds(sim: &Simulation) -> Vec<i32> {
            sim.tag_dispatch_trace_for_test()
                .iter()
                .map(|(_, kind)| *kind)
                .collect()
        }

        /// Ares3.0 deliver100624C5..10062509 and10062526..10062563 raise
        /// house-specific events before generic events. Natural expiry does not.
        #[test]
        fn ares_emp_tagged_delivery_orders_events_and_natural_expiry_does_not_emit_them() {
            let (mut sim, rules, target, source) = tagged_emp_scene(0, 2);
            let wh = rules.warhead("EMPWH").unwrap();
            sim.deliver_ares_emp(target, Some(source), wh, &rules, FrameEffects::empty());
            assert_eq!(emp_event_kinds(&sim), [63, 62]);
            assert!(sim.trigger_local_is_set_for_test(0));
            assert!(sim.trigger_local_is_set_for_test(1));
            assert!(!sim.trigger_local_is_set_for_test(2));
            assert!(!sim.trigger_local_is_set_for_test(3));
            sim.clear_tag_dispatch_trace_for_test();
            sim.deliver_ares_emp(target, Some(source), wh, &rules, FrameEffects::empty());
            assert!(
                emp_event_kinds(&sim).is_empty(),
                "capped repeat is not a transition"
            );
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert!(
                emp_event_kinds(&sim).is_empty(),
                "timer expiry has no removal events"
            );
            sim.deliver_ares_emp(target, Some(source), wh, &rules, FrameEffects::empty());
            sim.clear_tag_dispatch_trace_for_test();
            let healing = self::rules("EMP.Duration=-3\nEMP.Cap=-1\n");
            sim.deliver_ares_emp(
                target,
                Some(source),
                healing.warhead("EMPWH").unwrap(),
                &healing,
                FrameEffects::empty(),
            );
            assert_eq!(emp_event_kinds(&sim), [65, 64]);
            assert!(sim.trigger_local_is_set_for_test(2));
            assert!(sim.trigger_local_is_set_for_test(3));
        }

        #[test]
        fn ares_emp_tag_event_house_filter_uses_raw_source_index_and_requires_source() {
            for (delta, source_present) in [(10, true), (0, false)] {
                let (mut sim, rules, target, source) = tagged_emp_scene(delta, 2);
                sim.deliver_ares_emp(
                    target,
                    source_present.then_some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty(),
                );
                assert_eq!(emp_event_kinds(&sim), [63, 62]);
                assert!(sim.trigger_local_is_set_for_test(0));
                assert!(!sim.trigger_local_is_set_for_test(1));
                let healing = self::rules("EMP.Duration=-3\nEMP.Cap=-1\n");
                sim.deliver_ares_emp(
                    target,
                    source_present.then_some(source),
                    healing.warhead("EMPWH").unwrap(),
                    &healing,
                    FrameEffects::empty(),
                );
                assert!(sim.trigger_local_is_set_for_test(2));
                assert!(!sim.trigger_local_is_set_for_test(3));
            }
        }

        #[test]
        fn ares_emp_tag_predicates_require_matching_kind_actor_and_live_counter() {
            let (mut sim, rules, target, source) = tagged_emp_scene(0, 2);
            let context = crate::sim::world::UninitContext::with_rules(&rules);
            for kind in [62, 63] {
                sim.dispatch_object_tag_event(target, kind, Some(target), Some(source), context);
            }
            assert!(!sim.trigger_local_is_set_for_test(0));
            assert!(!sim.trigger_local_is_set_for_test(1));
            // A source alone cannot replace the affected actor argument.
            for kind in [64, 65] {
                sim.dispatch_object_tag_event(target, kind, None, Some(source), context);
            }
            assert!(!sim.trigger_local_is_set_for_test(2));
            assert!(!sim.trigger_local_is_set_for_test(3));
            sim.deliver_ares_emp(
                target,
                Some(source),
                rules.warhead("EMPWH").unwrap(),
                &rules,
                FrameEffects::empty(),
            );
            for kind in [64, 65] {
                sim.dispatch_object_tag_event(target, kind, Some(target), Some(source), context);
            }
            assert!(!sim.trigger_local_is_set_for_test(2));
            assert!(!sim.trigger_local_is_set_for_test(3));
        }

        #[test]
        fn ares_emp_first_tag_event_detachment_suppresses_the_generic_raise() {
            for healing in [false, true] {
                let (mut sim, rules, target, source) = tagged_emp_scene(0, 0);
                if healing {
                    sim.attach_object_trigger_tag(target, None);
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        rules.warhead("EMPWH").unwrap(),
                        &rules,
                        FrameEffects::empty(),
                    );
                    sim.attach_object_trigger_tag(target, Some("EMP"));
                }
                let rules = if healing {
                    self::rules("EMP.Duration=-3\nEMP.Cap=-1\n")
                } else {
                    rules
                };
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty(),
                );
                let (specific, specific_flag, generic_flag) =
                    if healing { (65, 3, 2) } else { (63, 1, 0) };
                assert_eq!(emp_event_kinds(&sim), [specific]);
                assert!(sim.trigger_local_is_set_for_test(specific_flag));
                assert!(!sim.trigger_local_is_set_for_test(generic_flag));
                assert!(!sim.object_has_trigger_tag(target));
            }
        }

        #[test]
        fn ares_emp_first_tag_event_can_kill_the_victim_before_generic_raise() {
            for healing in [false, true] {
                let (mut sim, _, target, source) = tagged_emp_scene(0, 2);
                sim.attach_object_trigger_tag(target, None);
                if healing {
                    let initial = rules("");
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        initial.warhead("EMPWH").unwrap(),
                        &initial,
                        FrameEffects::empty(),
                    );
                }
                let rules = rules(&format!(
                    "EMP.Duration={}\nEMP.Cap=-1\n[Warheads]\n1=C4\n[C4]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[CombatDamage]\nC4Warhead=C4\n",
                    if healing { -3 } else { 2 }
                ));
                assert_eq!(rules.bridge_warheads.c4_name, "C4");
                let source_house = sim
                    .base_reservation_house_index(sim.entities().get(source).unwrap().owner())
                    .unwrap();
                let (specific, generic) = if healing { (65, 64) } else { (63, 62) };
                let map = crate::map::map_file::MapFile::from_bytes(
                    format!(
                        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
                     [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
                     [Basic]\nNewINIFormat=4\n\
                     [Triggers]\nKILL=Neutral,GENERIC,KILL,0,1,1,1,0\n\
                     GENERIC=Neutral,<none>,GENERIC,0,1,1,1,0\n\
                     [Events]\nKILL=1,{specific},0,{source_house}\nGENERIC=1,{generic},0,0\n\
                     [Actions]\nKILL=1,32,0,0,0,0,0,0,A\nGENERIC=1,56,0,0,0,0,0,0,A\n\
                     [Tags]\nEMP=2,EMP,KILL\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
                sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
                sim.attach_object_trigger_tag(target, Some("EMP"));
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty(),
                );
                assert!(
                    sim.entities()
                        .get(target)
                        .is_none_or(|entity| !entity.lifecycle.object_alive()),
                    "the configured C4 callback must kill its actor, healing={healing}"
                );
                assert!(!sim.trigger_local_is_set_for_test(0));
                assert!(!emp_event_kinds(&sim).contains(&generic));
            }
        }
        #[test]
        fn ares_emp_native_cap_and_conversion_goldens() {
            let corpus = corpus();
            let defaults = EmpState::default();
            assert_eq!(
                defaults.last_mission,
                int(&corpus["constructor_defaults"][1]["result"]["last_mission"])
            );
            assert_eq!(
                defaults.sparkle.is_some(),
                corpus["constructor_defaults"][1]["result"]["sparkle_present"]
                    .as_bool()
                    .unwrap()
            );
            for row in corpus["caps"].as_array().unwrap() {
                let f = &row["fixture"];
                assert_eq!(
                    capped_duration(int(&f["old"]), int(&f["delta"]), int(&f["cap"])),
                    int(&row["result"]),
                    "{row}"
                );
            }
            for row in corpus["deliver"].as_array().unwrap() {
                if row["converted_deltas"].as_array().unwrap().is_empty() {
                    continue;
                }
                let f = &row["fixture"];
                let duration = f["duration"].as_i64().unwrap_or(120) as i32;
                let modifier = f["modifier"].as_f64().unwrap_or(1.0);
                assert_eq!(
                    duration_delta(duration, NativeF64Bits::from_bits(modifier.to_bits())),
                    int(&row["converted_deltas"][0]),
                    "{}",
                    row["name"]
                );
            }
        }
        fn mcv_emp_scene() -> (Simulation, RuleSet, u64, u64) {
            use crate::rules::{art_data::ArtRegistry, native_processing::RulesLayerStack};
            let ini = IniFile::from_str(
                "[VehicleTypes]\n0=MCV\n1=SOURCE\n[BuildingTypes]\n0=YARD\n[Warheads]\n0=EMPWH\n[Animations]\n0=SPARK\n[AI]\nBuildConst=YARD\n[MCV]\nStrength=1000\nArmor=heavy\nSpeed=5\nROT=5\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nDeploysInto=YARD\n[SOURCE]\nStrength=400\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n[YARD]\nStrength=1000\nConstructionYard=yes\nDeployFacing=4\n[EMPWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nEMP.Duration=3\nEMP.Cap=3\nEMP.Sparkles=SPARK\n[Clear]\nBuildable=yes\n[Hunt]\nRate=0.016\n[Guard]\nRate=0.016\n[Unload]\nRate=0.016\n",
            );
            let art =
                IniFile::from_str("[YARD]\nFoundation=4x3\n[SPARK]\nLoopCount=-1\nRate=1\nEnd=4\n");
            let mut rules = RuleSet::from_processed_rules(
                &RulesLayerStack::new(ini)
                    .process_with_fixed_art(&art)
                    .unwrap(),
            )
            .unwrap();
            let mut art = ArtRegistry::from_ini(&art);
            art.bind_anim_frame_count_for_test("SPARK", 4);
            rules.install_art_data(art);
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.session
                .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object_at_height("MCV", "Americans", 20, 22, 128, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("SOURCE", "Russians", 5, 5, 0, 0, &rules)
                .unwrap();
            (sim, rules, target, source)
        }
        #[test]
        fn ares_emp_mcv_shared_admission_and_pending_unload_respect_emp() {
            use crate::sim::{
                mission::{MissionId, authority::LiveReadyInputProvider},
                world::ObjectAiCtx,
            };
            let native = crate::test_fixture::json("tools/mcv_deploy_oracle.json");
            for row in native["deploy_admission"].as_array().unwrap() {
                // Unit DeathFrame's independent virtual override is not a
                // represented runtime state of this ordinary live MCV fixture.
                if int(&row["death_frame"]) != -1 {
                    continue;
                }
                let (mut sim, rules, target, _) = mcv_emp_scene();
                sim.substrate
                    .entities
                    .get_mut(target)
                    .unwrap()
                    .emp
                    .remaining = int(&row["emp_counter"]);
                assert_eq!(
                    sim.can_deploy_slash_unload(target, &rules),
                    row["result"]["allowed"].as_bool().unwrap(),
                    "{row}"
                );
                if row["entry"] == "unit_deploy" && !row["result"]["allowed"].as_bool().unwrap() {
                    sim.substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .mcv_deploy_pending
                        .set_for_test(true);
                    assert!(!sim.deploy_mcv(target, &rules, None, FrameEffects::empty()));
                    assert_eq!(
                        u8::from(
                            sim.substrate
                                .entities
                                .get(target)
                                .unwrap()
                                .mcv_deploy_pending
                                .is_pending()
                        ),
                        int(&row["result"]["deploy_retry"]) as u8
                    );
                }
            }
            let (mut sim, rules, target, source) = mcv_emp_scene();
            // A deploy already commenced before the hit. Unload defers EMP
            // Deactivate, so the shared CanDeploy owner must protect its AI.
            sim.mission_queue_exact(
                target,
                MissionId::from_known(MissionType::Unload),
                1,
                0,
                &LiveReadyInputProvider { rules: &rules },
            )
            .unwrap();
            assert_eq!(
                sim.substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .mission
                    .current()
                    .known(),
                Some(MissionType::Unload)
            );
            assert_eq!(
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty()
                ),
                EmpDelivery::Applied
            );
            assert!(!sim.can_deploy_slash_unload(target, &rules));
            sim.session.set_binary_frame_for_test(1);
            assert!(sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default()));
            let mcv = sim
                .substrate
                .entities
                .get(target)
                .expect("pending Unload must not convert during EMP");
            assert!(mcv.lifecycle.object_alive());
            assert_eq!(mcv.emp_remaining(), 2);
            assert!(
                !sim.substrate
                    .entities
                    .values()
                    .any(|entity| sim.interner.resolve(entity.type_ref()) == "YARD")
            );
        }
        #[test]
        fn ares_emp_shared_activation_matches_native_hook_gates() {
            for row in corpus()["activation"].as_array().unwrap() {
                let fixture = &row["fixture"];
                let rules = rules(&format!(
                    "[TANK]\nPoweredUnit={}\n[BuildingTypes]\n0=CENTER\n[CENTER]\nStrength=400\nPowersUnit=TANK\n[Clear]\nBuildable=yes\n",
                    if fixture["powered_unit"].as_bool().unwrap() {
                        "yes"
                    } else {
                        "no"
                    }
                ));
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                sim.resolve_type_handles(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                if let Some(counter) = fixture["center_counter"].as_i64() {
                    let center = sim
                        .spawn_object_at_height("CENTER", "Americans", 16, 16, 0, 0, &rules)
                        .unwrap();
                    let center = sim.substrate.entities.get_mut(center).unwrap();
                    center.building_powered_center.set_for_test(true);
                    center.emp.remaining = counter as i32;
                }
                let victim = sim.substrate.entities.get_mut(target).unwrap();
                victim.emp.remaining = int(&fixture["counter"]);
                victim
                    .deactivated
                    .set_for_test(fixture["deactivated"].as_bool().unwrap());
                victim
                    .mission
                    .set_current_for_test(crate::sim::mission::MissionId::from_raw(2), 7);
                let rng = sim.rng_state();
                sim.techno_reactivate(target, &rules, FrameEffects::empty());
                let victim = sim.substrate.entities.get(target).unwrap();
                let remains_offline = row["after"]["deactivated"].as_bool().unwrap();
                assert_eq!(
                    victim.deactivated.is_offline(),
                    remains_offline,
                    "{}",
                    row["name"]
                );
                if remains_offline {
                    assert_eq!(
                        victim.mission.current().raw(),
                        2,
                        "veto before ResetOrders: {}",
                        row["name"]
                    );
                    assert_eq!(victim.mission.handler_state(), 7, "{}", row["name"]);
                }
                assert_eq!(sim.rng_state(), rng, "{}", row["name"]);
            }
        }
        #[test]
        fn ares_emp_power_center_return_cannot_reactivate_an_emp_victim() {
            let rules = rules(
                "[TANK]\nPoweredUnit=yes\n[BuildingTypes]\n0=CENTER\n[CENTER]\nStrength=400\nPowersUnit=TANK\n[Clear]\nBuildable=yes\n",
            );
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                .unwrap();
            let center = sim
                .spawn_object_at_height("CENTER", "Americans", 16, 16, 0, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .selection
                .select();
            assert_eq!(
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty()
                ),
                EmpDelivery::Applied
            );
            let owner = sim.substrate.entities.get(target).unwrap().owner();
            assert!(!sim.has_powered_center(owner));
            sim.powered_center_edge(center, true, &rules, FrameEffects::empty());
            assert!(sim.has_powered_center(owner));
            let victim = sim.substrate.entities.get(target).unwrap();
            assert!(
                victim.deactivated.is_offline(),
                "House50E010 must pass the shared EMP veto"
            );
            assert!(victim.selection.is_selected());
            assert_eq!(victim.emp_remaining(), 2);
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert!(
                sim.substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .deactivated
                    .is_offline()
            );
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert!(
                !sim.substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .deactivated
                    .is_offline()
            );
        }
        #[test]
        fn ares_emp_expired_sparkle_refresh_preserves_native_fallback_provenance() {
            for row in corpus()["sparkle_refresh"].as_array().unwrap() {
                let fixture = &row["fixture"];
                let mut extra = format!(
                    "EMP.Duration={}\nEMP.Cap={}\nEMP.Sparkles={}\n",
                    int(&fixture["duration"]),
                    int(&fixture["cap"]),
                    if fixture["sparkle"].as_bool().unwrap_or(false) {
                        "SPARK"
                    } else {
                        "none"
                    }
                );
                if fixture["type_sparkle"].as_bool().unwrap_or(false) {
                    extra.push_str("[TANK]\nEMP.Sparkles=SPARK\n");
                }
                if fixture["default_sparkle"].as_bool().unwrap_or(false) {
                    extra.push_str("[General]\nEMPulseSparkles=SPARK\n");
                }
                let rules = rules(&extra);
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                sim.resolve_type_handles(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                let source = sim
                    .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                    .unwrap();
                let warhead = rules.warhead("EMPWH").unwrap();
                assert_eq!(
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        warhead,
                        &rules,
                        FrameEffects::empty()
                    ),
                    EmpDelivery::Applied
                );
                let held = sim
                    .substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .emp_sparkle()
                    .unwrap();
                assert_eq!(
                    sim.substrate.entities.get(target).unwrap().emp_remaining(),
                    int(&row["first"]["counter"])
                );
                sim.notify_anim_pointer_expired(held);
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(target)
                        .unwrap()
                        .emp_sparkle()
                        .is_some(),
                    row["expired"]["sparkle_present"].as_bool().unwrap()
                );
                let rng = sim.rng_state();
                let anim_count = sim.substrate.anims.len();
                assert_eq!(
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        warhead,
                        &rules,
                        FrameEffects::empty()
                    ),
                    EmpDelivery::Applied
                );
                let victim = sim.substrate.entities.get(target).unwrap();
                assert_eq!(
                    victim.emp_remaining(),
                    int(&row["after"]["counter"]),
                    "{}",
                    row["name"]
                );
                let constructed = row["after"]["sparkle_present"].as_bool().unwrap();
                assert_eq!(
                    victim.emp_sparkle().is_some(),
                    constructed,
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    sim.substrate.anims.len(),
                    anim_count + usize::from(constructed),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    sim.rng_state(),
                    rng,
                    "synthetic fixed-rate Anim: {}",
                    row["name"]
                );
            }
        }
        #[test]
        fn ares_emp_native_collector_uses_existing_cells_coordinates_and_uid() {
            let rules = rules("");
            for row in corpus()["collectors"].as_array().unwrap() {
                let f = &row["fixture"];
                let impact = ProjectileCoord {
                    x: int(&f["impact"][0]),
                    y: int(&f["impact"][1]),
                    z: int(&f["impact"][2]),
                };
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                let bridge = f["bridge"].as_bool().unwrap();
                if bridge {
                    use crate::map::{
                        cell_index::NativeCellIdentity, resolved_terrain::NativeCellQuery,
                    };
                    let terrain = sim.resolved_terrain.as_mut().unwrap();
                    let NativeCellIdentity::Real(index) =
                        NativeCellQuery::canonical(terrain).lookup((10, 10))
                    else {
                        panic!("real fixture cell");
                    };
                    let mut flags = terrain.capture_real_cell_bridge_flags_0x1180();
                    flags.set_allocated_cell(index, 0x100);
                    assert!(terrain.restore_real_cell_bridge_flags_0x1180(&flags));
                }
                for (index, object) in f["objects"].as_array().unwrap().iter().enumerate() {
                    let id = index as u64 + 100; // Stable order differs from native UID order.
                    let mut entity = GameEntity::test_default(id, "TANK", "Americans", 10, 10);
                    entity.set_type_ref_for_test(sim.interner.intern("TANK"));
                    entity.set_native_unique_id_for_test(int(&object["uid"]));
                    entity
                        .position
                        .put_world_xy([int(&object["coords"][0]), int(&object["coords"][1])]);
                    entity
                        .position
                        .set_exact_z_leptons_for_test(Some(int(&object["coords"][2])));
                    sim.substrate.entities.insert(entity);
                    let layer = if bridge {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    };
                    sim.substrate.occupancy.add(
                        10,
                        10,
                        id,
                        layer,
                        None,
                        CellListInsertion::PrependNonBuilding,
                    );
                    if f["duplicate_neighbor"].as_bool().unwrap() {
                        sim.substrate.occupancy.add(
                            11,
                            10,
                            id,
                            layer,
                            None,
                            CellListInsertion::PrependNonBuilding,
                        );
                    }
                }
                let ids =
                    sim.ares_emp_ground_targets(impact, f["spread"].as_f64().unwrap(), &rules);
                let uids: Vec<_> = ids
                    .iter()
                    .map(|id| sim.substrate.entities.get(*id).unwrap().native_unique_id())
                    .collect();
                let expected: Vec<_> = row["result"]["ordered_uids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(int)
                    .collect();
                assert_eq!(uids, expected, "{}", row["name"]);
                let radius = (f["spread"].as_f64().unwrap() + 0.99) as i32;
                let mut visited = Vec::new();
                visit_emp_cells((10, 10), radius, |cell| {
                    visited.push([i32::from(cell.0), i32::from(cell.1)])
                });
                let expected: Vec<_> = row["result"]["visited_cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|cell| [int(&cell[0]), int(&cell[1])])
                    .collect();
                assert_eq!(visited, expected, "{}", row["name"]);
            }
        }
        #[test]
        fn ares_emp_native_ground_delivery_admission_and_transitions() {
            for row in corpus()["deliver"].as_array().unwrap() {
                let f = &row["fixture"];
                // These native controls establish later actor/getter branches,
                // not the ordinary Drive victim's production configuration.
                if f.get("actor_kind").is_some()
                    || f["warping"].as_bool().unwrap_or(false)
                    || f["normal_emp_slot"].as_i64().unwrap_or(0) >= 18
                    || f["elite_emp_slot"].as_i64().unwrap_or(0) >= 18
                {
                    continue;
                }
                let yesno = |key: &str, default: bool| {
                    if f[key].as_bool().unwrap_or(default) {
                        "yes"
                    } else {
                        "no"
                    }
                };
                let owner = f["affects_owner"].as_bool().map_or(String::new(), |value| {
                    format!("AffectsOwner={}\n", if value { "yes" } else { "no" })
                });
                let immune = f["immune"].as_bool().map_or(String::new(), |value| {
                    format!("ImmuneToEMP={}\n", if value { "yes" } else { "no" })
                });
                let weapon = if f["normal_emp_slot"].as_i64() == Some(1) {
                    "Secondary=EMPWeapon\n[EMPWeapon]\nDamage=0\nWarhead=EMPWH\n"
                } else {
                    ""
                };
                let verses = vec![f["verses"].as_f64().unwrap_or(1.0).to_string(); 11].join(",");
                let rules = rules(&format!(
                    "Verses={verses}\nEMP.Duration={}\nEMP.Cap={}\nEMP.Sparkles=none\nAffectsAllies={}\nAffectsEnemies={}\n{owner}[TANK]\nEMP.Modifier={}\nOrganic={}\nTypeImmune={}\n{immune}VeteranAbilities={}\n{weapon}",
                    f["duration"].as_i64().unwrap_or(120),
                    f["cap"].as_i64().unwrap_or(120),
                    yesno("affects_allies", false),
                    yesno("affects_enemies", true),
                    f["modifier"].as_f64().unwrap_or(1.0),
                    yesno("organic", false),
                    yesno("type_immune", false),
                    if f["veteran_immune"].as_bool().unwrap_or(false) {
                        "EMPIMMUNE"
                    } else {
                        "RADIMMUNE"
                    },
                ));
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                let source_owner = if f["same_owner"].as_bool().unwrap_or(false) {
                    "Americans"
                } else {
                    "Russians"
                };
                let source = sim
                    .spawn_object_at_height("TANK", source_owner, 12, 10, 0, 0, &rules)
                    .unwrap();
                if f["allied"].as_bool().unwrap_or(false) {
                    sim.house_alliances_mut_for_test()
                        .entry(source_owner.to_ascii_uppercase())
                        .or_default()
                        .insert("AMERICANS".into());
                }
                {
                    let victim = sim.substrate.entities.get_mut(target).unwrap();
                    victim.emp.remaining = f["old"].as_i64().unwrap_or(0) as i32;
                    victim.emp.last_mission = 0;
                    victim
                        .deactivated
                        .set_for_test(f["deactivated"].as_bool().unwrap_or(false));
                    victim.selection.select();
                    victim
                        .mission
                        .set_current_for_test(crate::sim::mission::MissionId::from_raw(5), 0);
                    victim
                        .veterancy
                        .set_rank_for_test((f["rank"].as_f64().unwrap_or(0.0) * 100.0) as u16);
                    if f["iron_curtain"].as_bool().unwrap_or(false) {
                        crate::sim::superweapon::invulnerability::apply_invulnerability(
                            victim,
                            0,
                            100,
                            crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
                        );
                    }
                }
                let source = (!f["no_firer"].as_bool().unwrap_or(false)).then_some(source);
                for expected in row["result"].as_array().unwrap() {
                    let verdict = sim.deliver_ares_emp(
                        target,
                        source,
                        rules.warhead("EMPWH").unwrap(),
                        &rules,
                        FrameEffects::empty(),
                    );
                    assert_ne!(verdict, EmpDelivery::Unsupported, "{}", row["name"]);
                    let actual = sim.substrate.entities.get(target).unwrap();
                    assert_eq!(
                        actual.emp_remaining(),
                        int(&expected["counter"]),
                        "{}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.emp.last_mission,
                        int(&expected["saved_mission"]),
                        "{}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.deactivated.is_offline(),
                        expected["deactivated"].as_bool().unwrap(),
                        "{}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.selection.is_selected(),
                        expected["selected"].as_bool().unwrap(),
                        "{}",
                        row["name"]
                    );
                }
            }
        }
        #[test]
        fn ares_emp_native_ai_visits_defer_unload_and_restore_without_reselect() {
            let rules = rules("EMP.Sparkles=none\n");
            for row in corpus()["ticks"].as_array().unwrap() {
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                {
                    let victim = sim.substrate.entities.get_mut(target).unwrap();
                    victim.emp.remaining = int(&row["fixture"]["old"]);
                    victim.emp.last_mission = 0; // Explicit prior in native fixture, not constructor.
                    victim
                        .deactivated
                        .set_for_test(row["fixture"]["deactivated"].as_bool().unwrap_or(false));
                    victim.selection.select();
                }
                let rng = sim.scenario_rng.logical_state();
                for (index, mission) in row["missions"].as_array().unwrap().iter().enumerate() {
                    sim.substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .mission
                        .set_current_for_test(
                            crate::sim::mission::MissionId::from_raw(int(mission)),
                            0,
                        );
                    sim.tick_ares_emp(target, &rules, FrameEffects::empty());
                    let actual = sim.substrate.entities.get(target).unwrap();
                    let expected = &row["result"][index];
                    assert_eq!(
                        actual.emp_remaining(),
                        int(&expected["counter"]),
                        "{} visit{index}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.emp.last_mission,
                        int(&expected["saved_mission"]),
                        "{} visit{index}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.deactivated.is_offline(),
                        expected["deactivated"].as_bool().unwrap(),
                        "{} visit{index}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.selection.is_selected(),
                        expected["selected"].as_bool().unwrap(),
                        "{} visit{index}",
                        row["name"]
                    );
                }
                assert_eq!(sim.scenario_rng.logical_state(), rng, "{}", row["name"]);
            }
        }
        #[test]
        fn ares_emp_ground_hit_save_expiry_and_anim_pointer_lifecycle() {
            let rules = rules("");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            sim.session
                .set_current_house_for_test(sim.interner.get("Americans"));
            let target = sim
                .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                .unwrap();
            {
                let victim = sim.substrate.entities.get_mut(target).unwrap();
                victim
                    .mission
                    .set_current_for_test(crate::sim::mission::MissionId::from_raw(5), 0);
                victim.selection.select();
            }
            let coord = crate::sim::movement::ground_pose::position_world_coord(
                &sim.substrate.entities.get(target).unwrap().position,
            );
            let impact = ProjectileCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            };
            let rng = sim.scenario_rng.logical_state();
            sim.apply_ares_emp_detonation(
                impact,
                Some(source),
                rules.warhead("EMPWH").unwrap(),
                &rules,
                FrameEffects::empty(),
            );
            let expected = &corpus()["connected"][0]["result"];
            let assert_state = |sim: &Simulation, phase: usize| {
                let victim = sim.substrate.entities.get(target).unwrap();
                assert_eq!(victim.emp_remaining(), int(&expected[phase]["counter"]));
                assert_eq!(
                    victim.deactivated.is_offline(),
                    expected[phase]["deactivated"].as_bool().unwrap()
                );
                assert_eq!(
                    victim.selection.is_selected(),
                    expected[phase]["selected"].as_bool().unwrap()
                );
                assert_eq!(
                    victim.emp.last_mission,
                    int(&expected[phase]["saved_mission"])
                );
                assert_eq!(
                    victim.emp_sparkle().is_some(),
                    expected[phase]["sparkle_present"].as_bool().unwrap()
                );
            };
            assert_state(&sim, 0);
            assert!(!sim.techno_player_controllable(target, &rules));
            let held = sim
                .substrate
                .entities
                .get(target)
                .unwrap()
                .emp_sparkle()
                .unwrap();
            assert_eq!(
                sim.substrate
                    .anims
                    .get(held)
                    .unwrap()
                    .runtime()
                    .loop_remaining,
                255
            );
            // Actual hit and Drive/Walk transition draw no RNG; the Anim's
            // constructor uses its own established allocation/initialization owner.
            assert_eq!(sim.scenario_rng.logical_state(), rng);
            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "EMP", 0);
            for row in corpus()["pointer_expiry"].as_array().unwrap() {
                let mut probe = crate::sim::snapshot::GameSnapshot::load(&bytes)
                    .unwrap()
                    .sim;
                let expired = if row["fixture"]["matching"].as_bool().unwrap() {
                    held
                } else {
                    held + 1
                };
                probe.notify_anim_pointer_expired(expired);
                let victim = probe.substrate.entities.get(target).unwrap();
                assert_eq!(victim.emp_remaining(), int(&row["result"]["counter"]));
                assert_eq!(
                    victim.emp_sparkle().is_some(),
                    row["result"]["sparkle_present"].as_bool().unwrap()
                );
                assert_eq!(
                    probe
                        .substrate
                        .anims
                        .get(held)
                        .unwrap()
                        .runtime()
                        .loop_remaining,
                    int(&row["result"]["animation_iterations"][0]) as u8
                );
            }
            sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            sim.resolve_type_handles(&rules);
            assert_state(&sim, 0);
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert_state(&sim, 1);
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert_state(&sim, 2);
            assert!(sim.techno_player_controllable(target, &rules));
            assert_eq!(
                sim.substrate
                    .anims
                    .get(held)
                    .unwrap()
                    .runtime()
                    .loop_remaining,
                0
            );
            let victim = sim.substrate.entities.get_mut(target).unwrap();
            victim.emp.sparkle = Some(held);
            victim.emp_anim_pointer_expired(held + 1);
            assert_eq!(victim.emp_sparkle(), Some(held));
            sim.notify_anim_pointer_expired(held);
            assert_eq!(
                sim.substrate.entities.get(target).unwrap().emp_sparkle(),
                None
            );
        }
    }
}
