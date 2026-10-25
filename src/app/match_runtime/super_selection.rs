//! The local client's selected Super (`0x008809A0`, VERA's SuperWeapon
//! targeting mode) as the player's Supers move it, and the ChronoPlacement
//! anims the client hides.
//!
//! The selection's writers are the sidebar (`SelectClass::Action`), building
//! placement (`0x004FB8A9`), `SuperClass::Launch` and the revoke/suspend
//! pass (the Ghidra xrefs to `0x008809A0`), and DisplayClass through its own
//! field (`+0x11B8`): the constructor and `Init_Clear` (`0x004A87D6`,
//! `0x004A88E5`) and the right-button release (`DisplayClass::RightUp`,
//! `0x004AAE06`). The map click that fires a Super only queues its event
//! (`DisplayClass::BandBox_LeftUp @ 0x004AB9B0`), so the selection stays
//! until Launch's tail. None drops it because the selected Super is not
//! charged or not granted, so the Chrono Warp case 3 selects stays selected
//! although retail grants it to no house.

use std::collections::BTreeSet;

use crate::app::types::TargetingMode;
use crate::audio::events::{GameSoundEvent, SoundEventQueue};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::anim_class::AnimId;
use crate::sim::world::{SimSoundEvent, Simulation};

/// The `[SuperWeaponTypes]` entry Launch case 3 selects for the player's next
/// click (`0x006CC46E`, the literal index 4: retail ChronoWarpSpecial).
fn chrono_warp_selection(rules: &RuleSet) -> Option<&str> {
    rules
        .super_weapon_order
        .get(crate::sim::superweapon::CHRONO_WARP_SELECTION_INDEX)
        .map(String::as_str)
}

/// Whether the local selection is the Chrono Warp (`0x008809A0 == 4`).
pub(crate) fn chrono_warp_selected(targeting: Option<&TargetingMode>, rules: &RuleSet) -> bool {
    let selected = targeting.and_then(TargetingMode::as_super_weapon);
    selected.is_some_and(|name| {
        chrono_warp_selection(rules).is_some_and(|warp| warp.eq_ignore_ascii_case(name))
            || rules.super_weapon(name).is_some_and(|warp| {
                warp.kind == SuperWeaponKind::ChronoWarp
                    && crate::sim::superweapon::paid_pair_source(rules, warp).is_some()
            })
    })
}

/// The selection writes of the local player's Supers: Launch case 3 selects
/// the Chrono Warp (`0x006CC46E`), every other case clears the selection on
/// its player's tail (`0x006CDE16`, `0x006CD04F`, `0x006CCD9A`, `0x006CCD1C`,
/// `0x006CD50F` for both paradrops, `0x006CCE41`, `0x006CD6F8`,
/// `0x006CDA53`, `0x006CD2CB`, `0x006CD7D3`), and the revoke/suspend pass
/// clears it when the selected Super's hold changes or it is lost
/// (`HouseClass @ 0x0050AF10`, `0x0050B181..0x0050B190`). Other houses'
/// Supers leave it alone. Case 0's tail also follows a launch that finds no
/// silo, which VERA does not report (`superweapon/nuke.rs` RESIDUAL).
pub(super) fn follow_selection_writes(
    targeting: &mut Option<TargetingMode>,
    events: &[SimSoundEvent],
    sim: &Simulation,
    rules: &RuleSet,
    local_owner_name: Option<&str>,
) {
    for event in events {
        let (SimSoundEvent::SuperWeaponLaunched { owner, sw_type, .. }
        | SimSoundEvent::SuperWeaponStatusChanged { owner, sw_type }) = event
        else {
            continue;
        };
        if !local_owner_name
            .is_some_and(|local| local.eq_ignore_ascii_case(sim.interner().resolve(*owner)))
        {
            continue;
        }
        let selected = targeting.as_ref().and_then(TargetingMode::as_super_weapon);
        if let SimSoundEvent::SuperWeaponStatusChanged { .. } = event {
            // An old cancel/hold can precede a new Begin in one slow-frame
            // batch. The saved current ticket wins over that transient event;
            // do not clear the newly selected destination of the same type.
            if sim
                .paid_super_weapon_selection(*owner)
                .is_some_and(|selection| selection.destination == *sw_type)
            {
                continue;
            }
            if selected
                .is_some_and(|name| name.eq_ignore_ascii_case(sim.interner().resolve(*sw_type)))
            {
                *targeting = None;
            }
            continue;
        }
        if sim
            .next_paid_super_weapon_selection(rules, *owner, *sw_type)
            .is_some()
        {
            // Extension source selection is saved and may have been cancelled
            // later in this same command batch. Never revive that old event.
            if let Some(selection) = sim
                .paid_super_weapon_selection(*owner)
                .filter(|selection| selection.source == *sw_type)
                && selected
                    .is_some_and(|name| name.eq_ignore_ascii_case(sim.interner().resolve(*sw_type)))
            {
                *targeting = Some(TargetingMode::SuperWeapon(
                    sim.interner().resolve(selection.destination).to_owned(),
                ));
            }
            continue;
        }
        match rules
            .super_weapon(sim.interner().resolve(*sw_type))
            .map(|sw| sw.kind)
        {
            #[cfg(feature = "ares")]
            Some(SuperWeaponKind::GenericWarhead | SuperWeaponKind::UnitDelivery) => {
                // Ares new-type cleanup only clears the fired selection:
                // an automatic launch must not disarm a different SW.
                if selected
                    .is_some_and(|name| name.eq_ignore_ascii_case(sim.interner().resolve(*sw_type)))
                {
                    *targeting = None;
                }
            }
            Some(SuperWeaponKind::ChronoSphere) => {
                *targeting = chrono_warp_selection(rules)
                    .map(|name| TargetingMode::SuperWeapon(name.to_string()));
            }
            Some(
                SuperWeaponKind::MultiMissile
                | SuperWeaponKind::IronCurtain
                | SuperWeaponKind::LightningStorm
                | SuperWeaponKind::ChronoWarp
                | SuperWeaponKind::ParaDrop
                | SuperWeaponKind::AmerParaDrop
                | SuperWeaponKind::PsychicDominator
                | SuperWeaponKind::SpyPlane
                | SuperWeaponKind::GeneticConverter
                | SuperWeaponKind::ForceShield
                | SuperWeaponKind::PsychicReveal,
            ) if selected.is_some() => {
                *targeting = None;
            }
            _ => {}
        }
    }
}

/// The client's hiding of a Super's ChronoPlacement anim: case 3 hides
/// another house's at once and stops its sound (`0x006CC485..0x006CC4A0`),
/// and every SuperClass::AI hides a held anim while the selection is not
/// the Chrono Warp and stops its sound (`0x006CBCD4..0x006CBCF9`). The byte
/// (AnimClass `+0x19D`) has no other writer for this anim, so a hidden one
/// stays hidden after its Super lets it go. `hidden` keeps the client's
/// bytes; anims that are gone leave it.
pub(super) fn hide_placement_anims(
    hidden: &mut BTreeSet<AnimId>,
    sim: &Simulation,
    chrono_warp_selected: bool,
    local_owner_name: Option<&str>,
    output: &mut SoundEventQueue,
) {
    hidden.retain(|anim| sim.anim(*anim).is_some());
    for (owner, anim) in sim.super_placement_anims() {
        let local = local_owner_name
            .is_some_and(|local| local.eq_ignore_ascii_case(sim.interner().resolve(owner)));
        if (local && chrono_warp_selected) || !hidden.insert(anim) {
            continue;
        }
        output.push(GameSoundEvent::AnimationStopped {
            anim_id: anim,
            stop_sound_id: None,
            source: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::superweapon::chronosphere_tests::{
        SOURCE, charge_chronosphere, click, retail_world, step,
    };

    fn warp_selected() -> Option<TargetingMode> {
        Some(TargetingMode::SuperWeapon("ChronoWarpSpecial".to_string()))
    }

    #[cfg(feature = "ares")]
    #[test]
    fn paid_superweapon_selection_tracks_the_ticket_and_never_revives_cancelled_source() {
        use crate::sim::command::Command;
        let Some((mut rules, mut sim, owner)) = retail_world() else {
            return;
        };
        let source_type = rules
            .super_weapons
            .values_mut()
            .find(|sw| sw.kind == SuperWeaponKind::ChronoSphere)
            .unwrap();
        source_type.money_amount = 500;
        source_type.is_powered = false;
        sim.spawn_object("GACSPH", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let source = charge_chronosphere(&mut sim, owner);
        let ticket = sim
            .next_paid_super_weapon_selection(&rules, owner, source)
            .unwrap();
        assert!(sim.apply_command(
            "Americans",
            &Command::BeginPaidSuperWeapon {
                selection: ticket,
                target_rx: SOURCE.0,
                target_ry: SOURCE.1,
            },
            Some(&rules),
        ));
        let source_events = sim.sound_events().to_vec();
        let mut targeting = Some(TargetingMode::SuperWeapon("ChronoSphereSpecial".into()));
        follow_selection_writes(
            &mut targeting,
            &source_events,
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, warp_selected());
        sim.take_sound_events_for_test();
        assert!(sim.apply_command(
            "Americans",
            &Command::CancelPaidSuperWeapon { selection: ticket },
            Some(&rules),
        ));
        follow_selection_writes(
            &mut targeting,
            sim.sound_events(),
            &sim,
            &rules,
            Some("Americans"),
        );
        assert!(targeting.is_none());
        follow_selection_writes(
            &mut targeting,
            &source_events,
            &sim,
            &rules,
            Some("Americans"),
        );
        assert!(targeting.is_none());
        assert!(sim.paid_super_weapon_selection(owner).is_none());
        assert_eq!(sim.houses()[&owner].economy().credits(), 0);
        // Source -> cancel -> same source again before presentation catches
        // up: the older cancel event must not erase the new destination.
        sim.take_sound_events_for_test();
        let first = sim
            .next_paid_super_weapon_selection(&rules, owner, source)
            .unwrap();
        for command in [
            Command::BeginPaidSuperWeapon {
                selection: first,
                target_rx: SOURCE.0,
                target_ry: SOURCE.1,
            },
            Command::CancelPaidSuperWeapon { selection: first },
            Command::BeginPaidSuperWeapon {
                selection: crate::sim::superweapon::PaidSuperWeaponSelection {
                    sequence: first.sequence + 1,
                    ..first
                },
                target_rx: SOURCE.0,
                target_ry: SOURCE.1,
            },
        ] {
            assert!(sim.apply_command("Americans", &command, Some(&rules),));
        }
        targeting = Some(TargetingMode::SuperWeapon("ChronoSphereSpecial".into()));
        follow_selection_writes(
            &mut targeting,
            sim.sound_events(),
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, warp_selected());
    }

    #[cfg(feature = "ares")]
    #[test]
    fn extensions_only_clear_the_matching_local_selection() {
        for kind in ["GenericWarhead", "UnitDelivery"] {
            let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!(
            "[SuperWeaponTypes]\n0=Strike\n1=Other\n[Strike]\nType={kind}\n[Other]\nType=GenericWarhead\n",
        ))).unwrap();
            let mut sim = Simulation::with_seed(0);
            let local = sim.intern("Local");
            let remote = sim.intern("Remote");
            let sw_type = sim.intern("Strike");
            let launched = |owner| SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type,
                world: crate::sim::components::DriveCoord::cell(10, 10, 0).into(),
            };
            let mut selected = Some(TargetingMode::SuperWeapon("Other".to_owned()));
            follow_selection_writes(
                &mut selected,
                &[launched(local)],
                &sim,
                &rules,
                Some("Local"),
            );
            assert_eq!(
                selected,
                Some(TargetingMode::SuperWeapon("Other".to_owned()))
            );
            selected = Some(TargetingMode::SuperWeapon("Strike".to_owned()));
            follow_selection_writes(
                &mut selected,
                &[launched(remote)],
                &sim,
                &rules,
                Some("Local"),
            );
            assert!(selected.is_some());
            follow_selection_writes(
                &mut selected,
                &[launched(local)],
                &sim,
                &rules,
                Some("Local"),
            );
            assert!(selected.is_none());
        }
    }

    /// Case 3 selects the Chrono Warp for its player (`0x006CC46E`), case 4
    /// clears the selection (`0x006CCD1C`); another house's launches leave
    /// the local selection alone.
    #[test]
    fn the_local_chronosphere_launches_move_the_selection() {
        let Some((rules, mut sim, local)) = retail_world() else {
            return;
        };
        let remote = sim.intern("Russians");
        let sphere = sim.intern("ChronoSphereSpecial");
        let warp = sim.intern("ChronoWarpSpecial");
        let launched = |owner, sw_type| SimSoundEvent::SuperWeaponLaunched {
            owner,
            sw_type,
            world: crate::sim::components::DriveCoord::cell(21, 21, 0).into(),
        };
        let mut targeting = None;
        follow_selection_writes(
            &mut targeting,
            &[launched(remote, sphere)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, None);
        follow_selection_writes(
            &mut targeting,
            &[launched(local, sphere)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, warp_selected());
        assert!(chrono_warp_selected(targeting.as_ref(), &rules));
        follow_selection_writes(
            &mut targeting,
            &[launched(remote, warp)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, warp_selected());
        follow_selection_writes(
            &mut targeting,
            &[launched(local, warp)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, None);
        // A building placement is not a Super selection: case 4 keeps it.
        // Supplied input prior only; this test does not construct a factory.
        let mut input = crate::app::input::state::MatchInputState::new(Default::default());
        assert!(input.begin_building_placement(
            "GAPOWR",
            7,
            crate::sim::intern::InternedId::from_index(0)
        ));
        let mut placing = input.targeting_mode;
        follow_selection_writes(
            &mut placing,
            &[launched(local, warp)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert!(placing.is_some());
    }

    /// Case 7 clears the local player's Super selection (`0x006CCE41`);
    /// another house's Dominator leaves it.
    #[test]
    fn the_local_dominator_launch_clears_the_selection() {
        let Some((rules, mut sim, local)) = retail_world() else {
            return;
        };
        let remote = sim.intern("Russians");
        let dominator = sim.intern("PsychicDominatorSpecial");
        let launched = |owner| SimSoundEvent::SuperWeaponLaunched {
            owner,
            sw_type: dominator,
            world: crate::sim::components::DriveCoord::cell(21, 21, 0).into(),
        };
        let selected = || {
            Some(TargetingMode::SuperWeapon(
                "PsychicDominatorSpecial".to_string(),
            ))
        };
        let mut targeting = selected();
        follow_selection_writes(
            &mut targeting,
            &[launched(remote)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, selected());
        follow_selection_writes(
            &mut targeting,
            &[launched(local)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, None);
    }

    /// Cases 1 and 9 clear the local player's Super selection on their tails
    /// (`0x006CD04F`, `0x006CDA53`), whichever Super is selected: in
    /// `tools/superweapon_oracle.json` `iron_curtain_launch` and
    /// `genetic_launch`, each charged row of the player's ends with the
    /// selection at -1 and every other row leaves it, and VERA's launches
    /// report every charged row once (`iron_curtain_tests`,
    /// `genetic_converter_tests`); so for case 11 (`0x006CD7D3`,
    /// `psychic_launch`, `psychic_reveal_tests`) and for case 0's launches
    /// with a silo (`0x006CDE16`, `nuke_launch`, `nuke_tests`). The player's
    /// charged case 0 rows without one pin the `nuke.rs` RESIDUAL.
    #[test]
    fn the_local_launch_tails_of_cases_0_1_9_and_11_clear_the_selection() {
        let Some((rules, mut sim, local)) = retail_world() else {
            return;
        };
        let remote = sim.intern("Russians");
        let selected = || Some(TargetingMode::SuperWeapon("NukeSpecial".to_string()));
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json"))
                .unwrap();
        for (section, name) in [
            ("nuke_launch", "NukeSpecial"),
            ("iron_curtain_launch", "IronCurtainSpecial"),
            ("genetic_launch", "GeneticConverterSpecial"),
            ("psychic_launch", "PsychicRevealSpecial"),
        ] {
            let sw_type = sim.intern(name);
            let rows = oracle[section].as_array().unwrap();
            assert!(rows.len() > 10);
            for row in rows {
                let owner = if row["player"] == true { local } else { remote };
                let silo = section != "nuke_launch"
                    || row["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|event| event[0] == "queue_mission");
                // ClickFire admits only a charged Super, and case 0 launches
                // only with a silo: no launch, no report.
                let events = if row["charged"] == true && silo {
                    vec![SimSoundEvent::SuperWeaponLaunched {
                        owner,
                        sw_type,
                        world: crate::sim::components::DriveCoord::cell(40, 40, 0).into(),
                    }]
                } else {
                    Vec::new()
                };
                let mut targeting = selected();
                follow_selection_writes(&mut targeting, &events, &sim, &rules, Some("Americans"));
                let native = if row["selected_super"] == -1 {
                    None
                } else {
                    assert_eq!(row["selected_super"], 9, "{row}");
                    selected()
                };
                if row["charged"] == true && !silo && row["player"] == true {
                    // nuke.rs RESIDUAL: native's tail clears the selection
                    // without a silo; VERA reports no launch and keeps it.
                    assert_eq!((targeting, native), (selected(), None), "{row}");
                } else {
                    assert_eq!(targeting, native, "{section} {row}");
                }
            }
        }
    }

    /// Cases 5, 6 and 8 clear the local player's Super selection on their
    /// shared tail (`0x006CD50F`, `0x006CD6F8`), whichever Super is selected;
    /// another house's launch leaves it. `tools/superweapon_oracle.json`
    /// `spy_plane_launch` runs case 8: each charged row of the player's ends
    /// with the selection at -1 and every other row leaves it, and VERA's
    /// launch reports every charged row once (`spy_plane_tests`).
    #[test]
    fn the_local_paradrop_and_spy_plane_launches_clear_the_selection() {
        let Some((rules, mut sim, local)) = retail_world() else {
            return;
        };
        let remote = sim.intern("Russians");
        let launched = |owner, sw_type| SimSoundEvent::SuperWeaponLaunched {
            owner,
            sw_type,
            world: crate::sim::components::DriveCoord::cell(21, 21, 0).into(),
        };
        let selected = || Some(TargetingMode::SuperWeapon("NukeSpecial".to_string()));
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json"))
                .unwrap();
        let rows = oracle["spy_plane_launch"].as_array().unwrap();
        assert!(rows.len() > 10);
        let spy_plane = sim.intern("SpyPlaneSpecial");
        for row in rows {
            let owner = if row["player"] == true { local } else { remote };
            // ClickFire admits only a charged Super: no launch, no report.
            let events = if row["charged"] == true {
                vec![launched(owner, spy_plane)]
            } else {
                Vec::new()
            };
            let mut targeting = selected();
            follow_selection_writes(&mut targeting, &events, &sim, &rules, Some("Americans"));
            let expected = if row["selected_super"] == -1 {
                None
            } else {
                assert_eq!(row["selected_super"], 9, "{row}");
                selected()
            };
            assert_eq!(targeting, expected, "{row}");
        }
        for name in ["ParaDropSpecial", "AmericanParaDropSpecial"] {
            let sw_type = sim.intern(name);
            let mut targeting = selected();
            follow_selection_writes(
                &mut targeting,
                &[launched(remote, sw_type)],
                &sim,
                &rules,
                Some("Americans"),
            );
            assert_eq!(targeting, selected(), "{name}");
            follow_selection_writes(
                &mut targeting,
                &[launched(local, sw_type)],
                &sim,
                &rules,
                Some("Americans"),
            );
            assert_eq!(targeting, None, "{name}");
        }
    }

    /// The revoke/suspend pass drops the player's selection when the selected
    /// Super's hold changes or it is lost (`0x0050B181..0x0050B190`); another
    /// Super's or another house's report leaves it.
    #[test]
    fn the_selected_supers_hold_change_or_loss_drops_the_selection() {
        let Some((rules, mut sim, local)) = retail_world() else {
            return;
        };
        let remote = sim.intern("Russians");
        let sphere = sim.intern("ChronoSphereSpecial");
        let warp = sim.intern("ChronoWarpSpecial");
        let changed = |owner, sw_type| SimSoundEvent::SuperWeaponStatusChanged { owner, sw_type };
        let mut targeting = warp_selected();
        follow_selection_writes(
            &mut targeting,
            &[changed(local, sphere), changed(remote, warp)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, warp_selected());
        follow_selection_writes(
            &mut targeting,
            &[changed(local, warp)],
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(targeting, None);
    }

    /// The local player's ChronoPlacement anim shows while the Chrono Warp is
    /// selected and hides, with one sound stop, once it is not
    /// (`0x006CBCD4..0x006CBCF9`); another house's hides at once
    /// (`0x006CC485..0x006CC4A0`); a gone anim leaves the set.
    #[test]
    fn placement_anims_hide_off_the_chrono_warp_selection() {
        let Some((rules, mut sim, americans)) = retail_world() else {
            return;
        };
        charge_chronosphere(&mut sim, americans);
        click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
        let anims = sim.super_placement_anims();
        assert_eq!(anims.len(), 1);
        let own = anims[0].1;

        let mut hidden = BTreeSet::new();
        let mut output = SoundEventQueue::new();
        hide_placement_anims(&mut hidden, &sim, true, Some("Americans"), &mut output);
        assert!(hidden.is_empty());
        assert!(output.drain().is_empty());

        hide_placement_anims(&mut hidden, &sim, false, Some("Americans"), &mut output);
        hide_placement_anims(&mut hidden, &sim, false, Some("Americans"), &mut output);
        assert_eq!(hidden, BTreeSet::from([own]));
        let stops = output.drain();
        assert!(
            matches!(
                stops.as_slice(),
                [GameSoundEvent::AnimationStopped { anim_id, .. }] if *anim_id == own
            ),
            "{stops:?}"
        );

        // Seen from the Russians' client the anim is another house's.
        let mut theirs = BTreeSet::new();
        hide_placement_anims(&mut theirs, &sim, true, Some("Russians"), &mut output);
        assert_eq!(theirs, BTreeSet::from([own]));
        assert_eq!(output.drain().len(), 1);

        // Released by the warp, the anim plays out and leaves.
        click(&mut sim, &rules, americans, "ChronoWarpSpecial", (40, 40));
        for _ in 0..200 {
            if sim.anim(own).is_none() {
                break;
            }
            step(&mut sim, &rules);
        }
        assert!(sim.anim(own).is_none());
        hide_placement_anims(&mut hidden, &sim, false, Some("Americans"), &mut output);
        assert!(hidden.is_empty());
    }

    /// The map click only queues its event (`0x004AB9B0`), so the selection
    /// holds until Launch's tail: the Chronosphere until case 3 selects the
    /// Chrono Warp, and that until case 4 clears it. The player's
    /// ChronoPlacement anim shows throughout, then plays out unhidden.
    #[test]
    fn the_selection_holds_from_the_click_to_the_launch_tail() {
        use crate::sim::command::{Command, CommandEnvelope};
        let Some((rules, mut sim, americans)) = retail_world() else {
            return;
        };
        charge_chronosphere(&mut sim, americans);
        let selected = |name: &str| Some(TargetingMode::SuperWeapon(name.to_string()));
        // A click whose event executes three frames on, as input delay does.
        let click = |sim: &mut Simulation, name: &str, (rx, ry): (u16, u16)| {
            let sw_type_id = sim.intern(name);
            sim.queue_command(CommandEnvelope::new(
                americans,
                sim.session().tick() + 3,
                Command::LaunchSuperWeapon {
                    sw_type_id,
                    target_rx: rx,
                    target_ry: ry,
                },
            ));
        };
        let mut targeting = selected("ChronoSphereSpecial");
        let mut hidden = BTreeSet::new();
        let mut output = SoundEventQueue::new();
        // The app's frame (`sim_tick`): the frame's events move the
        // selection, then the client hides its anims.
        let mut frame = |sim: &mut Simulation, targeting: &mut Option<TargetingMode>| {
            step(sim, &rules);
            let events = sim.take_sound_events_for_test();
            follow_selection_writes(targeting, &events, sim, &rules, Some("Americans"));
            let warp = chrono_warp_selected(targeting.as_ref(), &rules);
            hide_placement_anims(&mut hidden, sim, warp, Some("Americans"), &mut output);
        };

        click(&mut sim, "ChronoSphereSpecial", SOURCE);
        for _ in 0..2 {
            frame(&mut sim, &mut targeting);
            assert_eq!(targeting, selected("ChronoSphereSpecial"));
        }
        frame(&mut sim, &mut targeting);
        assert_eq!(targeting, warp_selected());
        let anims = sim.super_placement_anims();
        assert_eq!(anims.len(), 1);
        let own = anims[0].1;

        click(&mut sim, "ChronoWarpSpecial", (40, 40));
        for _ in 0..2 {
            frame(&mut sim, &mut targeting);
            assert_eq!(targeting, warp_selected());
        }
        frame(&mut sim, &mut targeting);
        assert_eq!(targeting, None);
        assert!(sim.super_placement_anims().is_empty());
        let mut frames = 0;
        while sim.anim(own).is_some() {
            frame(&mut sim, &mut targeting);
            frames += 1;
            assert!(frames < 200, "the anim never plays out");
        }
        assert!(frames > 1, "the anim plays out after the warp");
        assert!(hidden.is_empty());
        assert!(output.drain().is_empty());
    }
}
