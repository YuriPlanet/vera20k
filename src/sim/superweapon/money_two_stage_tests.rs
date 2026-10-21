use super::*;
use superweapon::PaidSuperWeaponSelection;

fn complete(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    selection: PaidSuperWeaponSelection,
    cell: (u16, u16),
) -> bool {
    let name = sim.interner.resolve(owner).to_owned();
    sim.apply_command_with_overlays(
        &name,
        &Command::CompletePaidSuperWeapon {
            selection,
            target_rx: cell.0,
            target_ry: cell.1,
        },
        rules,
        None,
        FrameEffects::default(),
    )
}

fn cancel(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    selection: PaidSuperWeaponSelection,
) -> bool {
    let name = sim.interner.resolve(owner).to_owned();
    sim.apply_command_with_overlays(
        &name,
        &Command::CancelPaidSuperWeapon { selection },
        rules,
        None,
        FrameEffects::default(),
    )
}

#[test]
fn superweapon_money_two_stage_settles_source_once_not_destination() {
    for amount in [-500, 500] {
        let rules = rules(&format!(
            "[Sphere]\nMoney.Amount={amount}\n[Warp]\nMoney.Amount=-9000\n"
        ));
        let (mut sim, owner) = world(&rules, 1000, true);
        let source = ready(&mut sim, &rules, owner, "Sphere", false);
        assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000);
        let ticket = sim.paid_super_weapon_selection(owner).unwrap();
        sim.session
            .set_binary_frame_for_test(sim.session.binary_frame() + 1);
        superweapon::tick_house_superweapon_instances(&mut sim, &rules, owner);
        assert!(complete(&mut sim, &rules, owner, ticket, (15, 15)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000 + amount);
        assert_eq!(
            sim.houses[&owner].economy().spent_credits(),
            if amount < 0 { 500 } else { 0 }
        );
        assert!(sim.paid_super_weapon_selection(owner).is_none());
        let hash = sim.state_hash();
        sim.sound_events.clear_for_test();
        assert!(!complete(&mut sim, &rules, owner, ticket, (16, 16)));
        assert_eq!(sim.state_hash(), hash);
        assert!(sim.sound_events.is_empty());
        assert!(!sim.super_weapons[&owner][&source].is_ready);
        assert_eq!(
            sim.super_weapons[&owner][&source].charge_start_tick,
            sim.session.binary_frame() as i32
        );
    }
}

#[test]
fn superweapon_money_two_stage_cancel_reselect_and_old_commands_cannot_replay() {
    let rules = rules("[Sphere]\nMoney.Amount=500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let old = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(cancel(&mut sim, &rules, owner, old));
    assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    assert!(!complete(&mut sim, &rules, owner, old, (15, 15)));
    assert!(fire(&mut sim, &rules, owner, source, (11, 11)));
    let new = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(new.sequence > old.sequence);
    assert!(!cancel(&mut sim, &rules, owner, old));
    assert!(!complete(&mut sim, &rules, owner, old, (15, 15)));
    assert_eq!(sim.paid_super_weapon_selection(owner), Some(new));
    assert!(complete(&mut sim, &rules, owner, new, (15, 15)));
    assert_eq!(sim.houses[&owner].economy().credits(), 1500);
}

#[test]
fn superweapon_money_two_stage_unaffordable_final_is_atomic_and_retryable() {
    let rules = rules("[Sphere]\nMoney.Amount=-500\nEVA.InsufficientFunds=CustomFunds\n");
    let (mut sim, owner) = world(&rules, 500, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let ticket = sim.paid_super_weapon_selection(owner).unwrap();
    crate::sim::credit_income::spend_money(&mut sim.houses, owner, 1);
    sim.sound_events.clear_for_test();
    let hash = sim.state_hash();
    assert!(!complete(&mut sim, &rules, owner, ticket, (15, 15)));
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.houses[&owner].economy().credits(), 499);
    assert_eq!(sim.houses[&owner].economy().spent_credits(), 1);
    assert!(matches!(sim.sound_events.as_slice(),
        [SimSoundEvent::SuperWeaponInsufficientFunds { owner: actual, sw_type }]
        if *actual == owner && *sw_type == source));
    crate::sim::credit_income::add_credits(&mut sim.houses, owner, 1);
    assert!(complete(&mut sim, &rules, owner, ticket, (15, 15)));
    assert_eq!(sim.houses[&owner].economy().credits(), 0);
    assert!(!complete(&mut sim, &rules, owner, ticket, (15, 15)));
}

#[test]
fn superweapon_money_two_stage_postclick_fee_uses_even_zero_source_settings() {
    let rules = rules("[Warp]\nMoney.Amount=500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let ticket = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(complete(&mut sim, &rules, owner, ticket, (15, 15)));
    assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    assert!(!complete(&mut sim, &rules, owner, ticket, (15, 15)));
}

#[test]
fn superweapon_money_two_stage_wrong_owner_target_type_and_old_ticket_are_inert() {
    let rules = rules("[Sphere]\nMoney.Amount=500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let ticket = sim.paid_super_weapon_selection(owner).unwrap();
    let other = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(other, 1, None, true, 2000, 10));
    superweapon::construct_house_supers(&mut sim, &rules, other, false);
    let hash = sim.state_hash();
    assert!(!complete(&mut sim, &rules, other, ticket, (15, 15)));
    assert!(!complete(
        &mut sim,
        &rules,
        owner,
        ticket,
        (u16::MAX, u16::MAX)
    ));
    let wrong = PaidSuperWeaponSelection {
        destination: source,
        ..ticket
    };
    assert!(!complete(&mut sim, &rules, owner, wrong, (15, 15)));
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.paid_super_weapon_selection(owner), Some(ticket));
}

#[test]
fn superweapon_money_two_stage_one_time_consumption_waits_for_final_activation() {
    let rules = rules("[Sphere]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", true);
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let ticket = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(sim.super_weapons[&owner][&source].is_active);
    assert!(cancel(&mut sim, &rules, owner, ticket));
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let ticket = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(complete(&mut sim, &rules, owner, ticket, (15, 15)));
    assert!(!sim.super_weapons[&owner][&source].is_active);
    assert_eq!(sim.houses[&owner].economy().credits(), 500);
}

#[test]
fn superweapon_money_two_stage_reset_and_defeat_invalidate_pending_selection() {
    for defeat in [false, true] {
        let rules = rules("[Sphere]\nMoney.Amount=500\n");
        let (mut sim, owner) = world(&rules, 1000, true);
        let source = ready(&mut sim, &rules, owner, "Sphere", false);
        assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
        let ticket = sim.paid_super_weapon_selection(owner).unwrap();
        if defeat {
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_is_defeated_for_test(true);
            superweapon::update_owned_supers(&mut sim, &rules, owner);
        } else {
            superweapon::reset_super(
                &sim.interner,
                &sim.session,
                &mut sim.substrate,
                &mut sim.sound_events,
                &mut sim.super_weapons,
                &rules,
                owner,
                "Sphere",
            );
        }
        assert!(sim.paid_super_weapon_selection(owner).is_none());
        assert!(!complete(&mut sim, &rules, owner, ticket, (15, 15)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    }
}

fn restore(sim: &Simulation, rules: &RuleSet) -> Simulation {
    use crate::sim::snapshot::GameSnapshot;
    let bytes =
        GameSnapshot::save_validated(sim, 0, rules.simulation_config_hash(), "two-stage", 0);
    let saved: GameSnapshot = save_codec::deserialize(&bytes).unwrap();
    let mut restored = saved.sim;
    crate::sim::arena_fixture::flat_ground(&mut restored, rules);
    restored.resolve_type_handles(rules);
    restored.restore_after_snapshot_load().unwrap();
    restored
}

#[test]
fn superweapon_money_two_stage_save_pending_consumed_and_cancelled_keep_identity() {
    for amount in [-500, 500] {
        let rules = rules(&format!("[Sphere]\nMoney.Amount={amount}\n"));
        let (mut sim, owner) = world(&rules, 1000, true);
        let source = ready(&mut sim, &rules, owner, "Sphere", true);
        assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
        let ticket = sim.paid_super_weapon_selection(owner).unwrap();
        let mut loaded = restore(&sim, &rules);
        // Native load intentionally rebuilds navigation history (world_hash.rs)
        // so compare the complete financial/Super authority, not a whole-world
        // hash that also covers that unrelated load-time navigation history.
        assert_eq!(
            save_codec::serialize(&loaded.super_weapons).unwrap(),
            save_codec::serialize(&sim.super_weapons).unwrap()
        );
        assert_eq!(
            save_codec::serialize(&loaded.houses[&owner].economy()).unwrap(),
            save_codec::serialize(&sim.houses[&owner].economy()).unwrap()
        );
        assert_eq!(loaded.paid_super_weapon_selection(owner), Some(ticket));
        assert!(complete(&mut loaded, &rules, owner, ticket, (15, 15)));
        let mut consumed = restore(&loaded, &rules);
        assert_eq!(
            save_codec::serialize(&consumed.super_weapons).unwrap(),
            save_codec::serialize(&loaded.super_weapons).unwrap()
        );
        assert_eq!(
            save_codec::serialize(&consumed.houses[&owner].economy()).unwrap(),
            save_codec::serialize(&loaded.houses[&owner].economy()).unwrap()
        );
        assert!(!complete(&mut consumed, &rules, owner, ticket, (15, 15)));
        assert_eq!(consumed.houses[&owner].economy().credits(), 1000 + amount);
        assert!(cancel(&mut sim, &rules, owner, ticket));
        let mut cancelled = restore(&sim, &rules);
        assert!(!complete(&mut cancelled, &rules, owner, ticket, (15, 15)));
        assert!(fire(&mut cancelled, &rules, owner, source, (11, 11)));
        assert!(
            cancelled
                .paid_super_weapon_selection(owner)
                .unwrap()
                .sequence
                > ticket.sequence
        );
    }
}

#[test]
fn superweapon_money_two_stage_provider_loss_and_power_hold_retire_ticket() {
    for held in [false, true] {
        let rules = rules(
            "[BuildingTypes]\n0=PROVIDER\n[PROVIDER]\nStrength=200\nSuperWeapon=Sphere\n\
            [Sphere]\nMoney.Amount=500\nIsPowered=yes\n",
        );
        let (mut sim, owner) = world(&rules, 1000, true);
        let provider = sim
            .spawn_object("PROVIDER", "Americans", 6, 6, 0, &rules)
            .unwrap();
        let source = ready(&mut sim, &rules, owner, "Sphere", false);
        sim.super_weapons
            .instance_mut(&owner, &source)
            .unwrap()
            .can_lose = true;
        assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
        let ticket = sim.paid_super_weapon_selection(owner).unwrap();
        if held {
            sim.power_states
                .house_mut(owner)
                .set_is_low_power_for_test(true);
        } else {
            sim.substrate.entities.remove(provider);
        }
        // Recheck at final activation as well as at the ordinary dirty tail.
        assert!(!complete(&mut sim, &rules, owner, ticket, (15, 15)));
        superweapon::update_owned_supers(&mut sim, &rules, owner);
        assert!(sim.paid_super_weapon_selection(owner).is_none());
        sim.power_states
            .house_mut(owner)
            .set_is_low_power_for_test(false);
        ready(&mut sim, &rules, owner, "Sphere", false);
        assert!(!complete(&mut sim, &rules, owner, ticket, (15, 15)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    }
}

#[test]
fn superweapon_money_two_stage_general_prepost_and_new_source_replace_pending() {
    let rules = rules(
        "[SuperWeaponTypes]\n5=OtherSource\n6=OtherDestination\n\
        [Sphere]\nMoney.Amount=100\n\
        [OtherSource]\nType=PsychicReveal\nAction=PsychicReveal\nPreClick=yes\nIsPowered=no\nMoney.Amount=300\n\
        [OtherDestination]\nType=PsychicReveal\nPostClick=yes\nPreDependent=ParaDrop\nIsPowered=no\nMoney.Amount=900\n",
    );
    // PreDependent is the native kind/index spelling: ParaDrop = list index5.
    let (mut sim, owner) = world(&rules, 1000, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    let other = ready(&mut sim, &rules, owner, "OtherSource", false);
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let old = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(fire(&mut sim, &rules, owner, other, (11, 11)));
    let new = sim.paid_super_weapon_selection(owner).unwrap();
    assert_eq!(new.source, other);
    assert!(!complete(&mut sim, &rules, owner, old, (15, 15)));
    assert!(complete(&mut sim, &rules, owner, new, (15, 15)));
    assert_eq!(sim.houses[&owner].economy().credits(), 1300);
}

#[test]
fn superweapon_money_two_stage_begin_cancel_reselect_in_one_saved_command_batch() {
    use crate::sim::command::CommandEnvelope;
    let rules = rules("[Sphere]\nMoney.Amount=500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    let first = sim
        .next_paid_super_weapon_selection(&rules, owner, source)
        .unwrap();
    let second = PaidSuperWeaponSelection {
        sequence: first.sequence + 1,
        ..first
    };
    let commands = [
        Command::BeginPaidSuperWeapon {
            selection: first,
            target_rx: 10,
            target_ry: 10,
        },
        Command::CancelPaidSuperWeapon { selection: first },
        Command::BeginPaidSuperWeapon {
            selection: second,
            target_rx: 11,
            target_ry: 11,
        },
        Command::CancelPaidSuperWeapon { selection: second },
        Command::CompletePaidSuperWeapon {
            selection: second,
            target_rx: 15,
            target_ry: 15,
        },
    ];
    for command in &commands {
        sim.queue_command(CommandEnvelope::new(owner, 0, command.clone()));
    }
    assert!(sim.projected_paid_super_weapon_selection(owner).is_none());
    let mut loaded = restore(&sim, &rules);
    let commands = loaded.take_due_commands();
    assert_eq!(commands.len(), 5);
    for (index, command) in commands.iter().enumerate() {
        assert_eq!(
            loaded.apply_command_with_overlays(
                "Americans",
                &command.payload,
                &rules,
                None,
                FrameEffects::default()
            ),
            index != 4
        );
    }
    assert_eq!(loaded.houses[&owner].economy().credits(), 1000);
    assert!(!complete(&mut loaded, &rules, owner, first, (15, 15)));
    assert!(!complete(&mut loaded, &rules, owner, second, (15, 15)));
    let hash = loaded.state_hash();
    assert!(!loaded.apply_command_with_overlays(
        "Americans",
        &commands[0].payload,
        &rules,
        None,
        FrameEffects::default()
    ));
    assert_eq!(loaded.state_hash(), hash);
}

#[test]
fn superweapon_money_two_stage_retail_tank_warp_uses_existing_payload_once() {
    let Some((mut rules, mut sim, owner)) = superweapon::chronosphere_tests::retail_world() else {
        return;
    };
    rules
        .super_weapons
        .values_mut()
        .find(|sw| sw.kind == crate::rules::superweapon_type::SuperWeaponKind::ChronoSphere)
        .unwrap()
        .money_amount = -500;
    crate::sim::credit_income::add_credits(&mut sim.houses, owner, 1000);
    let tank = sim
        .spawn_object("MTNK", "Americans", 21, 21, 0, &rules)
        .unwrap();
    let source = ready(&mut sim, &rules, owner, "ChronoSphereSpecial", false);
    assert!(fire(&mut sim, &rules, owner, source, (21, 21)));
    assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    assert_eq!(sim.super_placement_anims().len(), 1);
    let ticket = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(complete(&mut sim, &rules, owner, ticket, (40, 40)));
    let unit = sim.substrate.entities.get(tank).unwrap();
    assert!(unit.chrono_warp_latch());
    assert_eq!(
        unit.locomotor.as_ref().unwrap().active_kind(),
        crate::rules::locomotor_type::LocomotorKind::Teleport
    );
    assert_eq!(sim.houses[&owner].economy().credits(), 500);
    assert!(sim.super_placement_anims().is_empty());
    let hash = sim.state_hash();
    assert!(!complete(&mut sim, &rules, owner, ticket, (41, 41)));
    assert_eq!(sim.state_hash(), hash);
}

#[test]
fn superweapon_money_two_stage_other_power_and_sequence_exhaustion_cannot_reuse_ticket() {
    let rules = rules("[Sphere]\nMoney.Amount=500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    let other = ready(&mut sim, &rules, owner, "Reveal", false);
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let old = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(fire(&mut sim, &rules, owner, other, (11, 11)));
    assert!(sim.paid_super_weapon_selection(owner).is_none());
    assert!(!complete(&mut sim, &rules, owner, old, (15, 15)));
    sim.super_weapons
        .instance_mut(&owner, &source)
        .unwrap()
        .paid_sequence = u64::MAX;
    let hash = sim.state_hash();
    assert!(!fire(&mut sim, &rules, owner, source, (10, 10)));
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.houses[&owner].economy().credits(), 1000);
}

#[test]
fn superweapon_money_two_stage_refused_source_request_id_is_never_reused() {
    let rules = rules("[Sphere]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 499, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    let old = sim
        .next_paid_super_weapon_selection(&rules, owner, source)
        .unwrap();
    let request = Command::BeginPaidSuperWeapon {
        selection: old,
        target_rx: 10,
        target_ry: 10,
    };
    assert!(!sim.apply_command_with_overlays(
        "Americans",
        &request,
        &rules,
        None,
        FrameEffects::default()
    ));
    assert!(sim.paid_super_weapon_selection(owner).is_none());
    assert!(sim.super_weapons[&owner][&source].is_ready);
    assert_eq!(sim.houses[&owner].economy().credits(), 499);
    let mut sim = restore(&sim, &rules);
    crate::sim::credit_income::add_credits(&mut sim.houses, owner, 1);
    let new = sim
        .next_paid_super_weapon_selection(&rules, owner, source)
        .unwrap();
    assert!(new.sequence > old.sequence);
    let request = Command::BeginPaidSuperWeapon {
        selection: new,
        target_rx: 10,
        target_ry: 10,
    };
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &request,
        &rules,
        None,
        FrameEffects::default()
    ));
    assert!(!cancel(&mut sim, &rules, owner, old));
    assert!(!complete(&mut sim, &rules, owner, old, (15, 15)));
    assert!(complete(&mut sim, &rules, owner, new, (15, 15)));
    assert_eq!(sim.houses[&owner].economy().credits(), 0);
}

#[test]
fn superweapon_money_two_stage_restore_rejects_corrupt_lifecycle_records() {
    use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
    for corruption in 0..5 {
        let rules = rules("[Sphere]\nMoney.Amount=500\n");
        let (mut sim, owner) = world(&rules, 1000, true);
        let source = ready(&mut sim, &rules, owner, "Sphere", false);
        assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
        let ticket = sim.paid_super_weapon_selection(owner).unwrap();
        let other = sim.interner.get("Reveal").unwrap();
        let missing = sim.interner.intern("NotARetainedSuper");
        match corruption {
            0 => {
                sim.super_weapons
                    .instance_mut(&owner, &source)
                    .unwrap()
                    .paid_sequence = 0
            }
            1 => {
                sim.super_weapons
                    .instance_mut(&owner, &source)
                    .unwrap()
                    .paid_selection = Some(PaidSuperWeaponSelection {
                    source: other,
                    ..ticket
                })
            }
            2 => {
                sim.super_weapons
                    .instance_mut(&owner, &source)
                    .unwrap()
                    .paid_selection = Some(PaidSuperWeaponSelection {
                    destination: missing,
                    ..ticket
                })
            }
            3 => {
                let instance = sim.super_weapons.instance_mut(&owner, &other).unwrap();
                instance.paid_sequence = 1;
                instance.paid_selection = Some(PaidSuperWeaponSelection {
                    source: other,
                    ..ticket
                });
            }
            4 => {
                sim.houses.remove_for_test(&owner);
            }
            _ => unreachable!(),
        }
        let bytes =
            GameSnapshot::save_validated(&sim, 0, rules.simulation_config_hash(), "corrupt", 0);
        let mut saved: GameSnapshot = save_codec::deserialize(&bytes).unwrap();
        assert!(
            matches!(
                saved.sim.restore_after_snapshot_load(),
                Err(SnapshotRestoreError::InvalidPaidSuperWeaponSelection(_))
            ),
            "corruption {corruption}"
        );
    }
}

#[test]
fn superweapon_money_two_stage_views_and_refusal_settings_use_source_not_destination() {
    let rules = rules(
        "[Sphere]\nMoney.Amount=-500\nEVA.InsufficientFunds=CustomFunds\n\
        [Warp]\nMoney.Amount=-10000\n",
    );
    let (mut sim, owner) = world(&rules, 500, true);
    let source = ready(&mut sim, &rules, owner, "Sphere", false);
    let destination = ready(&mut sim, &rules, owner, "Warp", false);
    let financial = superweapon::financial_type(&rules, rules.super_weapon("Warp").unwrap());
    assert_eq!(financial.money_amount, -500);
    assert_eq!(
        financial.insufficient_funds_eva.as_deref(),
        Some("CustomFunds")
    );
    assert!(fire(&mut sim, &rules, owner, source, (10, 10)));
    let view = superweapon::superweapon_views_for_owner(&sim, &rules, &owner)
        .into_iter()
        .find(|view| view.type_id == destination)
        .unwrap();
    assert!(view.can_fire && view.can_afford);
    crate::sim::credit_income::spend_money(&mut sim.houses, owner, 1);
    let view = superweapon::superweapon_views_for_owner(&sim, &rules, &owner)
        .into_iter()
        .find(|view| view.type_id == destination)
        .unwrap();
    assert!(view.can_fire && !view.can_afford);
}
