//! `FootClass::ClickedAction_Cell`'s MoveFlash (`0x004D7EB5..0x004D8065`)
//! through the production command drain, with retail rules and art: the
//! constructor arguments and coordinate come from instruction reading, not
//! from a native capture.

use super::*;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::scenario_session::ScenarioGameMode;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

fn retail_arena(mode: ScenarioGameMode) -> Option<(Simulation, RuleSet)> {
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    assert_eq!(rules.general.move_flash.name, "RING");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4D7EB5);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_game_mode(mode);
    Some((sim, rules))
}

fn set_cell(sim: &mut Simulation, cell: (u16, u16), level: u8, flags: u32) {
    let terrain = sim.resolved_terrain_mut_for_test().as_mut().unwrap();
    let cell = terrain.cell_mut_for_test(cell.0, cell.1).unwrap();
    cell.level = level;
    cell.bridge_facts.raw_flags = flags;
}

/// The anims one MoveFlash command adds in its tick: type, coordinate and
/// draw flags.
fn flash(
    sim: &mut Simulation,
    rules: &RuleSet,
    cell: (u16, u16),
) -> Vec<(String, AnimWorldCoord, u32)> {
    let before: std::collections::HashSet<AnimId> = sim.anims().map(|(id, _)| *id).collect();
    let owner = sim.interner.intern("Americans");
    let tick = sim.session.tick();
    sim.queue_command(CommandEnvelope::new(
        owner,
        tick,
        Command::MoveFlash {
            rx: cell.0,
            ry: cell.1,
        },
    ));
    let commands = sim.take_due_commands();
    assert_eq!(commands.len(), 1);
    sim.advance_tick(&commands, Some(rules), None, None, 33);
    sim.anims()
        .filter(|(id, _)| !before.contains(id))
        .map(|(_, anim)| {
            (
                sim.interner.resolve(anim.type_id()).to_owned(),
                anim.world_coord(),
                anim.draw_flags(),
            )
        })
        .collect()
}

#[test]
fn retail_move_flash_spawns_ring_on_the_clicked_cells_ground_centre() {
    let Some((mut sim, rules)) = retail_arena(ScenarioGameMode::Skirmish) else {
        return;
    };
    set_cell(&mut sim, (12, 9), 2, 0);
    assert_eq!(
        flash(&mut sim, &rules, (12, 9)),
        vec![(
            "RING".to_owned(),
            AnimWorldCoord {
                x: 12 * 256 + 128,
                y: 9 * 256 + 128,
                // Two levels of 104 leptons (`0x00578080` on a flat cell).
                z: 208,
            },
            0x600,
        )]
    );
}

#[test]
fn a_bridge_cell_raises_the_move_flash_to_the_deck() {
    let Some((mut sim, rules)) = retail_arena(ScenarioGameMode::Skirmish) else {
        return;
    };
    set_cell(&mut sim, (12, 9), 2, 0x100);
    set_cell(&mut sim, (14, 9), 2, 0x400);
    let z = |flashes: Vec<(String, AnimWorldCoord, u32)>| {
        assert_eq!(flashes.len(), 1);
        flashes[0].1.z
    };
    assert_eq!(
        z(flash(&mut sim, &rules, (12, 9))),
        208 + BRIDGE_DECK_HEIGHT_LEPTONS
    );
    // Only CellClass+0x140 bit 0x100 reaches the deck add (`0x004D7F13`).
    assert_eq!(z(flash(&mut sim, &rules, (14, 9))), 208);
}

#[test]
fn network_modes_keep_the_move_flash_off_the_synchronized_anims() {
    for (mode, spawns) in [
        (ScenarioGameMode::Campaign, true),
        (ScenarioGameMode::Skirmish, true),
        (ScenarioGameMode::Lan, false),
        (ScenarioGameMode::Wol, false),
    ] {
        let Some((mut sim, rules)) = retail_arena(mode) else {
            return;
        };
        assert_eq!(
            flash(&mut sim, &rules, (12, 9)).len(),
            usize::from(spawns),
            "{mode:?}"
        );
    }
}
