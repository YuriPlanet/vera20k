//! Headless checks for the Phobos activation/link/sale presentation arms of
//! the production dispatcher. The sim already decided *what* happened; these
//! cover the client-local column pick and the label/silence split
//! (`Ext/SWType/FireSuperWeapon.cpp:491-526`, `Ext/Building/Hooks.Selling.cpp:18,73`).

use super::dispatch_sim_sound_events;
use crate::app::input::messages::HudMessage;
use crate::audio::events::{GameSoundEvent, SoundEventQueue};
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};
use std::sync::Arc;

const RULES: &str = "[InfantryTypes]\n0=E1\n[AircraftTypes]\n[VehicleTypes]\n\
[BuildingTypes]\n0=STRUCT\n1=PLAIN\n\
[E1]\nStrength=100\nSpeed=4\nSight=5\n\
[STRUCT]\nStrength=500\nEVA.Sold=EVA_LinkedSW\n\
[PLAIN]\nStrength=500\n\
[SuperWeaponTypes]\n0=RootSpecial\n\
[RootSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
Message.Activated.Owner=\n\
Message.Activated.Enemies=Msg:Enemy\n\
EVA.Activated.Enemies=EVA_UnitSold\n";

/// The fixed EVAMD catalog the process pass resolves `EVA.Activated.*` against
/// (`SWTypeExt::LoadFromINIFile`'s `ValueableIdx<VoxClass>` reads, which native
/// runs after EVAMD init), plus the `EVA.Sold` identity the bind pass needs.
const EVAMD: &str = "[DialogList]\n0=EVA_UnitSold\n1=EVA_StructureSold\n2=EVA_LinkedSW\n";

struct World {
    sim: Simulation,
    rules: RuleSet,
    americans: InternedId,
    russians: InternedId,
    sw: InternedId,
}

fn world() -> World {
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(RULES),
        None,
        IniFile::empty().clone(),
        Arc::new(SoundRegistry::default()),
        Arc::new(EvaRegistry::from_ini(&IniFile::from_str(EVAMD))),
    )
    .expect("rules parse");
    let (rules, ..) = owner
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
        .expect("rules load")
        .into_parts();
    let mut sim = Simulation::new();
    let americans = sim.interner_mut_for_test().intern("Americans");
    let russians = sim.interner_mut_for_test().intern("Russians");
    sim.register_house(HouseState::new(americans, 0, None, true, 0, 10));
    sim.register_house(HouseState::new(russians, 1, None, false, 0, 10));
    let sw = sim.interner_mut_for_test().intern("RootSpecial");
    World {
        sim,
        rules,
        americans,
        russians,
        sw,
    }
}

/// Dispatch one event as the `Americans` client and answer the produced sound
/// events plus the HUD rows.
fn dispatch(world: &World, event: SimSoundEvent) -> (Vec<GameSoundEvent>, Vec<HudMessage>) {
    let mut output = SoundEventQueue::new();
    let mut hud = Vec::new();
    dispatch_sim_sound_events(
        [event],
        &world.sim,
        &world.rules,
        Some("Americans"),
        &mut |_| true,
        &mut output,
        Some(&mut hud),
    );
    (output.drain(), hud)
}

/// A House the client does not own and does not treat as an ally takes the
/// enemy column: its message becomes a HUD row in the launcher's colour and
/// its EVA line reaches the queue.
#[test]
fn a_foreign_launch_takes_the_enemy_column() {
    let world = world();
    let (events, hud) = dispatch(
        &world,
        SimSoundEvent::SuperWeaponActivatedMessage {
            owner: world.russians,
            sw_type: world.sw,
        },
    );
    assert!(
        matches!(
            hud.as_slice(),
            [HudMessage::PhobosSuperWeaponMessage {
                audience: crate::rules::phobos_sw_messages::PhobosActivatedAudience::Enemies,
                firer,
                ..
            }] if *firer == world.russians
        ),
        "the enemy column is printed with the launching House"
    );
    assert!(
        matches!(
            events.as_slice(),
            [GameSoundEvent::Eva { event, .. }] if event == "EVA_UnitSold"
        ),
        "the enemy column's EVA line is admitted"
    );
}

/// The local player's own launch takes the Owner column; this type leaves that
/// column empty and its EVA unset, so neither channel produces anything.
#[test]
fn an_empty_column_produces_nothing_for_either_channel() {
    let world = world();
    let (events, hud) = dispatch(
        &world,
        SimSoundEvent::SuperWeaponActivatedMessage {
            owner: world.americans,
            sw_type: world.sw,
        },
    );
    assert!(events.is_empty(), "no cue for the Owner column");
    assert!(hud.is_empty(), "no row for an empty label");
}

/// `SWTypeExt::ApplyLinkedSW` prints and speaks only on the granting House's
/// own client; this type names neither, so a foreign grant is silent and the
/// local one still prints nothing.
#[test]
fn a_linked_grant_is_gated_on_the_current_player() {
    let world = world();
    let (foreign_events, foreign_hud) = dispatch(
        &world,
        SimSoundEvent::SuperWeaponLinkedAcquired {
            owner: world.russians,
            sw_type: world.sw,
        },
    );
    assert!(foreign_events.is_empty() && foreign_hud.is_empty());

    let (local_events, local_hud) = dispatch(
        &world,
        SimSoundEvent::SuperWeaponLinkedAcquired {
            owner: world.americans,
            sw_type: world.sw,
        },
    );
    assert!(
        local_events.is_empty() && local_hud.is_empty(),
        "an unset linked pair prints/speaks nothing even on the owner's client"
    );
}

/// `BuildingClass_Mi_Selling_EVASold_UndeploysInto` hook 0x449CC1
/// (`Ext/Building/Hooks.Selling.cpp:73-79`): the sold type's `EVA.Sold`
/// replaces `EVA_StructureSold`; a type without it keeps the stock line.
#[test]
fn a_sold_building_prefers_the_types_eva_sold() {
    let mut world = world();
    let typed = world.sim.interner_mut_for_test().intern("STRUCT");
    let (events, _) = dispatch(
        &world,
        SimSoundEvent::StructureSold {
            owner: world.americans,
            type_id: typed,
        },
    );
    assert!(
        matches!(events.as_slice(), [GameSoundEvent::Eva { event, .. }] if event == "EVA_LinkedSW"),
        "the type's EVA.Sold replaced the stock structure line"
    );

    let plain = world.sim.interner_mut_for_test().intern("PLAIN");
    let (events, _) = dispatch(
        &world,
        SimSoundEvent::StructureSold {
            owner: world.americans,
            type_id: plain,
        },
    );
    assert!(
        matches!(
            events.as_slice(),
            [GameSoundEvent::Eva { event, .. }] if event == "EVA_StructureSold"
        ),
        "an absent EVA.Sold keeps the stock line"
    );
}

/// `FootClass_Sell`'s type-authored cue reaches the queue under the same
/// local-owner test the stock `HouseEva` arm applies.
#[test]
fn a_sold_unit_cue_is_forwarded_for_the_local_owner_only() {
    let world = world();
    let (events, _) = dispatch(
        &world,
        SimSoundEvent::HouseEvaNamed {
            owner: world.americans,
            event: "EVA_LinkedSW".to_owned(),
        },
    );
    assert!(matches!(
        events.as_slice(),
        [GameSoundEvent::Eva { event, .. }] if event == "EVA_LinkedSW"
    ));

    let (events, _) = dispatch(
        &world,
        SimSoundEvent::HouseEvaNamed {
            owner: world.russians,
            event: "EVA_LinkedSW".to_owned(),
        },
    );
    assert!(events.is_empty(), "another House's sale stays silent");
}
