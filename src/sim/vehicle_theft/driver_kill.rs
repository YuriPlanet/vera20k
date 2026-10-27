//! KillDriver: installed Ares701914/702819 and10052B40/100456D0.
//! The transaction uses Cargo, control, team, spawning and House owners.

use super::*;
use crate::rules::ares_warhead::HouseSelector;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::damage::DamageState;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER};
use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
use crate::sim::passenger::{DepartureRoute, depart_cargo_head};
use crate::sim::world::{FrameEffects, UninitContext};
use crate::util::native_x87::{
    MaskedX87Chop53 as X, MaskedX87Ordering as Ordering, NativeF32Bits, NativeF64Bits,
};

impl Simulation {
    /// TechnoClass_Update1004B65D..1004B6A6 keeps a driverless hull harmless
    /// on every AI visit. Foot+6AD is the locomotor swap, not ordinary motion.
    pub(crate) fn update_ares_driver_killed_mission(
        &mut self,
        target: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        use crate::sim::mission::{MissionId, MissionType};
        let Some(actor) = self.substrate.entities.get(target) else {
            return;
        };
        if !actor.driver_killed()
            || actor.mission.current().known() == Some(MissionType::Harmless)
            || (actor.category().is_foot() && actor.lift.swap_active())
        {
            return;
        }
        let _ = self.assign_target_represented(target, None, rules, registry, effects);
        let _ = self.assign_destination_represented(target, None, rules, registry, effects);
        let _ = self.mission_queue_exact(
            target,
            MissionId::from_known(MissionType::Harmless),
            1,
            self.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
    }

    /// ReceiveDamage_Aftermath1004AB50, after protected-Techno responses and
    /// before the native Uncloak70281D. Unlike EMP this is not a Bullet AoE.
    pub(crate) fn ares_driver_kill_aftermath(
        &mut self,
        event: &EntityDamageEvent,
        state: DamageState,
        initially_positive: bool,
        current_damage: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(warhead) = rules.warhead(self.interner.resolve(event.warhead_ref)) else {
            return;
        };
        let Some(settings) = warhead
            .ares_rules()
            .filter(|rules| rules.kill_driver().enabled())
        else {
            return;
        };
        if state == DamageState::Unaffected
            && !event.receiver_flags.ignore_defenses
            && initially_positive
            && current_damage == 0
            && settings.effects_require_damage()
        {
            return;
        }
        let Some(target) = self.substrate.entities.get(event.target_id) else {
            return;
        };
        let Some(kind) = self.entity_object_type(target, rules) else {
            return;
        };
        //1004AC25: positive Verses cutoff, not an absolute-value test.
        if settings.effects_require_verses()
            && X::compare(
                X::load_f64(NativeF64Bits::from_bits(0x3f1a_36e2_eb1c_432d)),
                X::load_f64(warhead.interaction(kind.armor_id()).multiplier()),
            ) == Ordering::Greater
        {
            return;
        }
        //10052B4D/5F: no invoker or non-Foot target never attempts the draw.
        let Some(invoker) = (event.attacker_id != RAD_NO_ATTACKER)
            .then(|| self.substrate.entities.get(event.attacker_id))
            .flatten()
        else {
            return;
        };
        if !matches!(
            target.category(),
            EntityCategory::Unit | EntityCategory::Aircraft | EntityCategory::Infantry
        ) {
            return;
        }
        let source_owner = self.houses.get(&invoker.owner()).map(|house| house.name());
        let target_owner = self.houses.get(&target.owner()).map(|house| house.name());
        let same_owner = source_owner.is_some() && source_owner == target_owner;
        let allied = same_owner
            || source_owner
                .zip(target_owner)
                .is_some_and(|(source, target)| {
                    crate::sim::combat::combat_weapon::is_ally_by_object(
                        Some(self.house_alliances.as_map()),
                        &self.interner,
                        source,
                        target,
                    )
                });
        if !settings.can_affect_house(
            source_owner.is_some(),
            same_owner,
            allied,
            warhead.affects_allies,
        ) || !self.ares_can_kill_driver(
            event.target_id,
            settings.kill_driver().kill_below_percent(),
            rules,
        ) {
            return;
        }
        //10052BF8..10052C2F: exactly one Scenario draw even at chance0/1.
        // The file-backed scale at100954C8 is binary64 2^-31.
        let draw = self.scenario_rng.next_range_i32_inclusive(1, i32::MAX);
        let scaled = X::mul(
            X::load_i32(draw),
            X::load_f64(NativeF64Bits::from_bits(0x3e00_0000_0000_0000)),
        );
        if X::compare(scaled, X::load_f64(settings.kill_driver().chance())) == Ordering::Greater {
            return;
        }
        let owner = self
            .ares_nonrandom_house_kind(
                settings.kill_driver().owner(),
                None,
                source_owner,
                source_owner,
                target_owner,
                rules,
            )
            .or_else(|| {
                self.ares_nonrandom_house_kind(
                    HouseSelector::Special,
                    None,
                    None,
                    None,
                    None,
                    rules,
                )
            });
        if let Some(owner) = owner {
            self.ares_replace_driver(
                event.target_id,
                event.attacker_id,
                owner,
                settings.kill_driver().remove_veterancy(),
                rules,
                registry,
                effects,
            );
        }
    }

    /// CanKillDriver100434C0: live class/radio/type/XP/health facts. Cell
    /// lookup is canonical here (runtime), unlike cursor-only admission.
    fn ares_can_kill_driver(&self, id: u64, threshold: NativeF64Bits, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(kind) = self.entity_object_type(actor, rules) else {
            return false;
        };
        if !matches!(
            actor.category(),
            EntityCategory::Unit | EntityCategory::Aircraft
        ) || actor.category() == EntityCategory::Unit
            && (kind.crate_goodie || !kind.dock.is_empty())
            || actor.is_warped_out()
            || crate::sim::superweapon::invulnerability::is_invulnerable(
                actor.invulnerability.as_ref(),
                self.session.binary_frame(),
            )
        {
            return false;
        }
        if actor.category() == EntityCategory::Unit {
            //1004352A..1004355F compares the cell's building with RADIO
            //slot0 (+E4 items[0]), not the head of its object list.
            let building = self.resolved_terrain.as_ref().and_then(|terrain| {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                let cell = ground_pose::query_object_cell(
                    &cells,
                    ground_pose::object_location(actor, Some(terrain)),
                );
                let (rx, ry) = cells.coord(cell);
                u16::try_from(rx)
                    .ok()
                    .zip(u16::try_from(ry).ok())
                    .and_then(|(rx, ry)| {
                        self.substrate.occupancy.first_building_on_layer(
                            rx,
                            ry,
                            MovementLayer::Ground,
                        )
                    })
            });
            if let Some(building) =
                building.filter(|building| actor.radio_contacts.slot(0) == Some(*building))
                && self
                    .substrate
                    .entities
                    .get(building)
                    .and_then(|building| self.entity_object_type(building, rules))
                    .is_some_and(|kind| kind.weapons_factory && !kind.naval)
            {
                return false;
            }
        }
        if kind.natural
            || kind.organic
            || kind
                .promotion_abilities()
                .is_some_and(|abilities| abilities.driver_protected(actor.veterancy.raw()))
        {
            return false;
        }
        let protected = kind.driver_rules().protected_min_health().unwrap_or(
            if kind.driver_rules().protected() {
                NativeF64Bits::POSITIVE_ZERO
            } else {
                NativeF64Bits::ONE
            },
        );
        let limit =
            if X::compare(X::load_f64(protected), X::load_f64(threshold)) == Ordering::Greater {
                threshold
            } else {
                protected
            };
        //10044C60 is the existing Object5F5C60 ratio, including masked NaN.
        X::compare(actor.health.ratio(kind.strength), X::load_f64(limit)) != Ordering::Greater
    }

    /// TechnoExt::KillDriver100456D0: the admitted vehicle changes driver,
    /// preserving the distinct operator-death, ejection and cleanup order.
    fn ares_replace_driver(
        &mut self,
        target: u64,
        invoker: u64,
        owner: crate::sim::intern::InternedId,
        remove_veterancy: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(passive) = self.houses.get(&owner).map(|house| {
            rules.country_multiplay_passive(self.interner.resolve(house.house_type_id()))
        }) else {
            return;
        };
        let Some(actor) = self.substrate.entities.get_mut(target) else {
            return;
        };
        actor.vehicle_driver.set_killed_from_owner(passive);
        if actor.owner() == owner {
            return;
        }
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let operator = self
            .substrate
            .entities
            .get(target)
            .and_then(|actor| self.entity_object_type(actor, rules))
            .map(|kind| kind.driver_rules().operator().clone())
            .unwrap_or_default();
        let passengers = self
            .substrate
            .entities
            .get(target)
            .and_then(|actor| actor.passenger_role.cargo())
            .map(|cargo| cargo.passengers().to_vec())
            .unwrap_or_default();
        match operator {
            //1004572C..10045756: _ANY_ removes ONE operator, the head.
            crate::rules::ares_driver::OperatorRequirement::Any if !passengers.is_empty() => {
                let _ = depart_cargo_head(
                    self,
                    rules,
                    registry,
                    target,
                    DepartureRoute::DeathEscape,
                    |sim, id| {
                        sim.record_kill_and_uninit(id, Some(invoker), context);
                        Ok(())
                    },
                    effects,
                );
            }
            crate::rules::ares_driver::OperatorRequirement::Types(types) => {
                for passenger in passengers {
                    if self
                        .substrate
                        .entities
                        .get(passenger)
                        .and_then(|actor| self.entity_object_type(actor, rules))
                        .is_some_and(|kind| {
                            types.iter().any(|required| {
                                required.category == kind.category
                                    && required.id.eq_ignore_ascii_case(&kind.id)
                            })
                        })
                    {
                        self.remove_cargo_passenger(target, Some(passenger), rules, effects);
                        self.record_kill_and_uninit(passenger, Some(invoker), context);
                        //100457D0 falls through to100457D6 after the first
                        //matching operator; only nonmatches advance the list.
                        break;
                    }
                }
            }
            _ => {}
        }
        let count = self
            .substrate
            .entities
            .get(target)
            .map_or(0, |actor| actor.passenger_role.passenger_count());
        for _ in 0..count {
            let result = depart_cargo_head(
                self,
                rules,
                registry,
                target,
                DepartureRoute::DeathEscape,
                |sim, id| {
                    let point = sim
                        .substrate
                        .entities
                        .get(target)
                        .map(|actor| ground_pose::position_world_coord(&actor.position));
                    if point.is_none_or(|point| {
                        !sim.place_ares_survivor(rules, registry, id, point, 128, false, effects)
                    }) {
                        sim.record_kill_and_uninit(id, None, context);
                    }
                    // Shared placement owns successful carrier/+82 clearing
                    // (native100448A0 ->7104A0), including parachutes.
                    Ok(())
                },
                effects,
            );
            if result.is_err() {
                break;
            }
        }
        if let Some(actor) = self.substrate.entities.get_mut(target) {
            actor.capture_infantry_type_index.record(-1);
            if remove_veterancy {
                actor.veterancy.inherit_raw(NativeF32Bits::from_bits(0));
            }
        }
        if let Some(controller) = self
            .substrate
            .entities
            .get(target)
            .and_then(|actor| actor.mind_control.controller())
        {
            self.free_unit(controller, target, rules, registry, effects);
        }
        self.clear_permanent_control_and_ring(target, rules);
        if let Some(actor) = self.substrate.entities.get_mut(target) {
            actor.mind_control.clear_house_controller();
        }
        self.free_all_captures(target, rules, registry, effects);
        self.leave_team(target, false, rules, effects);
        crate::sim::spawn_manager::kill_all_spawns_with_context(self, target, context);
        crate::sim::spawn_manager::clear_all_spawn_targets(self, target, rules, registry, effects);
        self.free_slaves_for_driver_change(target, invoker, passive, rules, registry, effects);
        self.change_owner_with_rules(target, owner, rules, registry, effects);
        if passive {
            let _ = self.mission_queue_exact(
                target,
                crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Harmless,
                ),
                1,
                self.session.binary_frame(),
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
        }
        let _ = self.assign_target_represented(target, None, rules, registry, effects);
        // ReplaceDriver10045995 passes mode 0; the later AI enforcement uses 1.
        let _ = self.assign_destination_keeping_queue(target, None, rules, registry, effects);
        self.dispatch_object_tag_event(target, 68, Some(target), Some(invoker), context);
        if self
            .substrate
            .entities
            .get(target)
            .is_some_and(|actor| actor.lifecycle.object_alive())
        {
            self.dispatch_object_tag_event(target, 67, Some(target), None, context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::ReceiverCallFlags;
    use crate::sim::house_state::HouseState;
    use crate::sim::passenger::{BoardingArm, board_passenger};

    #[test]
    fn driver_hit_kills_one_operator_ejects_rider_and_allows_a_new_driver() {
        for operator_rule in ["_ANY_", "OP"] {
            exercise_driver_handoff(operator_rule);
        }
    }

    fn exercise_driver_handoff(operator_rule: &str) {
        let (mut sim, rules, overlays) = crate::sim::world::entry_test_fixture::fixture_with_rules(
            &"[Countries]\n0=Americans\n1=Russians\n2=Special\n3=French\n\
             [Special]\nMultiplayPassive=yes\n\
             [InfantryTypes]\n2=OP\n3=DRIVER\n4=CONTROLLER\n[VehicleTypes]\n0=HOST\n\
             [CONTROLLER]\nStrength=100\nPrimary=ControlGun\nSpeedType=Foot\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [ControlGun]\nDamage=1\nRange=6\nProjectile=Invisible\nWarhead=CONTROL_WH\n\
             [OP]\nStrength=100\nSpeedType=Foot\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [DRIVER]\nStrength=100\nCanDrive=yes\nSpeedType=Foot\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [HOST]\nStrength=300\nPassengers=3\nOperator=_ANY_\nSpeed=5\nPrimary=HostGun\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [HostGun]\nDamage=1\nRange=6\nROF=20\nProjectile=Invisible\nWarhead=DRIVER_WH\n\
             [Warheads]\n0=DRIVER_WH\n1=ZERO_CHANCE\n2=CONTROL_WH\n\
             [CONTROL_WH]\nMindControl=yes\n\
             [DRIVER_WH]\nKillDriver=yes\nKillDriver.RemoveVeterancy=yes\n\
             [ZERO_CHANCE]\nKillDriver=yes\nKillDriver.Chance=0\nAffectsOwner=yes\n"
                .replace("Operator=_ANY_", &format!("Operator={operator_rule}")),
        );
        for (name, side) in [
            ("Americans", 0),
            ("Russians", 1),
            ("Special", 0),
            ("French", 0),
        ] {
            let owner = sim.interner.intern(name);
            sim.houses
                .register(HouseState::new(owner, side, Some(owner), false, 0, 10));
        }
        let host = sim
            .spawn_object_at_height("HOST", "Russians", 16, 15, 0, 0, &rules)
            .unwrap();
        let rider = sim
            .spawn_object_at_height("OP", "Russians", 17, 15, 0, 0, &rules)
            .unwrap();
        let operator = sim
            .spawn_object_at_height("OP", "Russians", 17, 16, 0, 0, &rules)
            .unwrap();
        for passenger in [rider, operator] {
            assert!(board_passenger(
                &mut sim,
                &rules,
                passenger,
                host,
                BoardingArm::InfantryTransport,
                Some(&overlays),
                Default::default()
            ));
        }
        let invoker = sim
            .spawn_object_at_height("OP", "Americans", 15, 15, 0, 0, &rules)
            .unwrap();
        let driver = sim
            .spawn_object_at_height("DRIVER", "Americans", 15, 16, 0, 0, &rules)
            .unwrap();
        let controller = sim
            .spawn_object_at_height("CONTROLLER", "French", 14, 16, 0, 0, &rules)
            .unwrap();
        assert!(sim.can_capture(controller, host, &rules));
        assert!(sim.can_be_perma_mind_controlled(host, &rules));
        sim.assign_target_represented(
            host,
            Some(crate::sim::combat::TargetKind::Entity(invoker)),
            &rules,
            Some(&overlays),
            Default::default(),
        )
        .unwrap();
        sim.assign_destination_represented(
            host,
            Some(crate::sim::components::NavTargetRef::cell(18, 15)),
            &rules,
            Some(&overlays),
            Default::default(),
        )
        .unwrap();
        assert!(
            sim.entities()
                .get(host)
                .unwrap()
                .attack_target
                .target()
                .is_some()
        );
        assert!(
            sim.entities()
                .get(host)
                .unwrap()
                .navigation
                .nav_com()
                .is_some()
        );
        sim.substrate
            .entities
            .get_mut(host)
            .unwrap()
            .veterancy
            .set_raw_for_test(NativeF32Bits::from_bits(1.5f32.to_bits()));
        let warhead_ref = sim.interner.intern("DRIVER_WH");
        let hit = EntityDamageEvent::direct_receiver(
            host,
            1,
            0,
            invoker,
            None,
            warhead_ref,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        sim.commit_noncombat_aoe_hits(&rules, Some(&overlays), &[hit], Default::default());
        let vehicle = sim.entities().get(host).unwrap();
        assert_eq!(sim.interner.resolve(vehicle.owner()), "Special");
        assert!(vehicle.driver_killed());
        assert!(vehicle.attack_target.target().is_none());
        assert!(vehicle.navigation.nav_com().is_none());
        assert_eq!(vehicle.passenger_role.passenger_count(), 0);
        assert_eq!(vehicle.veterancy.raw(), NativeF32Bits::from_bits(0));
        assert!(
            sim.entities()
                .get(operator)
                .is_none_or(|actor| !actor.lifecycle.object_alive()),
            "{operator_rule} kills only the first matching operator"
        );
        let passenger = sim.entities().get(rider).unwrap();
        assert!(
            !passenger.lifecycle.in_limbo() && !passenger.passenger_role.is_inside_transport(),
            "{operator_rule} ejects the other matching passenger alive"
        );
        assert!(!sim.can_capture(controller, host, &rules));
        assert!(!sim.can_be_perma_mind_controlled(host, &rules));
        sim.update_ares_operator_operability(host, &rules, Default::default());
        assert!(sim.entities().get(host).unwrap().deactivated.is_offline());
        // A subsequent order cannot reactivate a driverless hull.
        let frame = sim.session.binary_frame();
        let actor = sim.substrate.entities.get_mut(host).unwrap();
        actor.mission = crate::sim::mission::MissionCom::at_frame(frame);
        actor.mission.set_current_for_test(
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
            0,
        );
        assert_eq!(actor.mission.queued(), crate::sim::mission::MissionId::NONE);
        sim.update_ares_driver_killed_mission(host, &rules, Some(&overlays), Default::default());
        assert_eq!(
            sim.entities().get(host).unwrap().mission.queued().known(),
            Some(crate::sim::mission::MissionType::Harmless)
        );
        assert_eq!(
            crate::sim::radio::receive::unit_can_enter_radio(&sim, host, Some(driver), &rules),
            crate::sim::radio::RadioResponse::None
        );
        let captured = sim
            .ares_capture_vehicle(driver, host, &rules, Some(&overlays), Default::default())
            .unwrap();
        assert!(!captured.consumed);
        let vehicle = sim.entities().get(host).unwrap();
        assert_eq!(sim.interner.resolve(vehicle.owner()), "Americans");
        assert!(!vehicle.driver_killed());
        assert_eq!(vehicle.passenger_role.passenger_count(), 1);
        assert!(sim.entities().get(driver).unwrap().lifecycle.in_limbo());
        assert!(sim.can_capture(controller, host, &rules));
        assert!(sim.can_be_perma_mind_controlled(host, &rules));

        // A generic driver satisfies _ANY_; a named operator requirement
        // still needs its matching passenger before either recovery caller
        // may power the hull back on.
        sim.techno_reactivate(host, &rules, Default::default());
        assert_eq!(
            sim.entities().get(host).unwrap().deactivated.is_offline(),
            operator_rule == "OP"
        );
        if operator_rule == "OP" {
            assert!(board_passenger(
                &mut sim,
                &rules,
                rider,
                host,
                BoardingArm::InfantryTransport,
                Some(&overlays),
                Default::default(),
            ));
        }
        sim.update_ares_operator_operability(host, &rules, Default::default());
        assert!(!sim.entities().get(host).unwrap().deactivated.is_offline());

        // Chance zero still spends the native single draw after all gates.
        // Calling the production aftermath isolates it from unrelated damage
        // feedback and scatter draws, which the full receiver above exercised.
        let mut expected = sim.scenario_rng.clone();
        expected.next_range_i32_inclusive(1, i32::MAX);
        let zero = EntityDamageEvent {
            warhead_ref: sim.interner.intern("ZERO_CHANCE"),
            ..hit
        };
        sim.ares_driver_kill_aftermath(
            &zero,
            DamageState::Damaged,
            true,
            1,
            &rules,
            Some(&overlays),
            Default::default(),
        );
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        assert!(!sim.entities().get(host).unwrap().driver_killed());
    }
}
