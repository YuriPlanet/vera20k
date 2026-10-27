//! A computer house's strategy tick (`HouseClass::AI_Building_Strategy @
//! 0x004FD500`), the IHouse `Fire_Sale @ 0x005013A0` and `All_To_Hunt @
//! 0x00501400` it calls, and the anger nodes it shares with the rest of the
//! house.
//!
//! Native owner: `HouseClass`. `HouseClass::Update` (`0x004F8FBE..0x004F9032`),
//! after the defeat gate and before the building choice, runs Strategy for a
//! house that is neither human-controlled nor `MultiplayPassive=` once its
//! timer (`+0x5634`/`+0x563C`) has expired, and restarts the timer with the
//! delay Strategy returns: `RandomRanged(1, 7) + 105` on the Scenario stream
//! (`0x004FD913..0x004FD928`), Strategy's only draw of its own. The
//! constructor leaves the timer expired, so every computer house draws at its
//! first update, and a defeated one keeps drawing.
//!
//! Strategy, in order:
//! 1. The enemy search (`0x004FD538..0x004FD71E`), in a nonzero game mode for
//!    a house without an enemy (`+0x5600 == -1`) that is not passive and has
//!    a base centre ([`HouseState::base_origin`]). It reads every candidate's
//!    centre from the searching house itself (`0x004FD635..0x004FD657`), so
//!    all distances are zero and the first house in HouseClass::Array that is
//!    not the house, not passive and not defeated takes one anger point,
//!    ally or not. The anger rescan ([`update_anger_nodes`]) picks the enemy.
//!    Its own timer (`+0x5640`/`+0x5648`, [`ENEMY_SEARCH_TIMER`]) has no
//!    writer after the constructor.
//! 2. A defeated enemy's anger is cancelled and the enemy forgotten
//!    (`0x004FD723..0x004FD772`).
//! 3. AI_TryFireSW (`0x005098F0`, `superweapon::ai_fire`) in a nonzero game
//!    mode or from `[IQ] SuperWeapons=` (`0x004FD77C..0x004FD79B`): the
//!    house fires its charged superweapons.
//! 4. The emergency block
//!    ([`crate::sim::house_state::HouseStrategyEmergencyState::advance`]);
//!    state four sells everything and sends everyone hunting.
//! 5. In a nonzero game mode, a house outside state three with no live
//!    building whose type has `Factory=` sells everything and sends everyone
//!    hunting as well (`0x004FD879..0x004FD904`); then a house that must
//!    rebuild its economy (Check_Build_Need, asked first) runs
//!    Manage_Build_Queue (`sim::ai_build_need`). Phobos
//!    `[General] AIFireSale=`/`AIAllToHunt=` gate only these two last-legs
//!    calls; `AIFireSaleDelay=` arms a per-house timer on the first reach.
//!
//! Evidence: `tools/ai_strategy_oracle.py` runs the Update block, whole
//! Strategy with the original UpdateAngerNodes, Fire_Sale and All_To_Hunt;
//! `house_strategy_tests.rs` replays every row.
//!
//! All_To_Hunt draws nothing itself. The damage its Dominator arm deals
//! ([`all_to_hunt`]) goes through the shared ReceiveDamage receiver, where a
//! kill's random draws, anims and detach run as for any other damage.
//! The All-To-Hunt latch is consumed by the shared greatest_threat candidate
//! scoring (`TechnoClass::Evaluate_Candidate @ 0x006F8765`).
//!
//! RESIDUALS:
//! - The own-coordinate test against the empty coordinate
//!   (`0x004FD5B9..0x004FD5E5`) is dormant: no cell's coordinate is zero.

use crate::sim::world::FrameEffects;

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::house_state::HouseState;
use crate::sim::houses::Houses;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::production::{SellOrder, sell_back};
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;

/// `0x004FD918..0x004FD928`: `RandomRanged(1, 7) + 105`.
const RESCHEDULE_BASE_FRAMES: i32 = 105;
/// The enemy search's timer (`+0x5640`/`+0x5648`) as the constructor leaves
/// it (`0x004F5BAE..0x004F5BBA`), started at the construction frame, 0, with
/// no delay; nothing writes it later. It is expired until the signed frame
/// wraps.
const ENEMY_SEARCH_TIMER: CdTimer = CdTimer::started(0, 0);

/// `HouseClass::Update @ 0x004F8FBE..0x004F9032`: a computer house whose
/// Strategy timer has expired runs [`building_strategy`], and the timer
/// restarts at this frame with the delay it returns.
pub(crate) fn update_strategy(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    let frame = sim.session.binary_frame() as i32;
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if !house.strategy_timer().expired(frame)
        || house.is_controlled_by_human(sim.session.game_mode_nonzero())
        || house.multiplay_passive()
    {
        return;
    }
    let delay = building_strategy(sim, rules, owner, registry, frame_effects);
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.restart_strategy_timer(frame, delay);
    }
}

/// `HouseClass::AI_Building_Strategy @ 0x004FD500` for an existing house:
/// the module doc's steps; returns the timer's next delay.
fn building_strategy(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> i32 {
    pick_enemy(
        &sim.interner,
        &sim.session,
        &sim.house_alliances,
        &mut sim.houses,
        owner,
    );
    forget_defeated_enemy(&sim.interner, &sim.house_alliances, &mut sim.houses, owner);
    // `0x004FD77C..0x004FD79B`: a signed IQ (`+0x24C`) compare.
    if sim.session.game_mode_nonzero()
        || sim
            .houses
            .get(&owner)
            .is_some_and(|house| house.current_iq() >= rules.general.iq_super_weapons)
    {
        crate::sim::superweapon::ai_fire::try_fire(sim, rules, owner, registry, frame_effects);
    }

    // Available_Money (IHouse vt+0x18) is a pure read; the block's second
    // query sees the same value.
    let money = crate::sim::credit_income::available_money(&sim.houses, owner);
    let frame = sim.session.binary_frame() as i32;
    let emergency = sim
        .houses
        .get_mut(&owner)
        .is_some_and(|house| house.strategy_emergency_mut().advance(frame, || money));
    if emergency {
        sell_off_and_hunt(sim, rules, owner, "state four", registry, frame_effects);
    }

    // `0x004FD848..0x004FD911`: urgency slot 0 is the missing factory (4),
    // slot 1 Check_Build_Need's answer (0 or 1), both asked before either
    // acts; from level 4 down, slot 0's sells off and hunts and slot 1's
    // runs Manage_Build_Queue.
    if sim.session.game_mode_nonzero() {
        let suppressed = sim
            .houses
            .get(&owner)
            .is_none_or(|house| house.strategy_emergency().mode() == 3);
        let no_factory = !suppressed && !has_live_factory(sim, rules, owner);
        let build_need = crate::sim::ai_build_need::check_build_need(sim, rules, owner);
        if no_factory {
            last_legs_actions(sim, rules, owner, registry, frame_effects);
        }
        if build_need {
            crate::sim::ai_build_need::manage_build_queue(
                sim,
                rules,
                owner,
                registry,
                frame_effects,
            );
        }
    }

    sim.scenario_rng
        .next_range_i32_inclusive(1, 7)
        .wrapping_add(RESCHEDULE_BASE_FRAMES)
}

/// IHouse Fire_Sale (vt+0x34) then All_To_Hunt (vt+0x38).
fn sell_off_and_hunt(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    why: &str,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    log::debug!(
        "{} sells off and hunts ({why})",
        sim.interner.resolve(owner)
    );
    fire_sale(sim, rules, owner, registry, frame_effects);
    all_to_hunt(sim, rules, owner, registry, frame_effects);
}

/// `0x004FD8F7..0x004FD907`, the no-factory block's calls, as Phobos
/// `[General] AIFireSale=`, `AIFireSaleDelay=` and `AIAllToHunt=` gate them
/// (`src/Ext/House/Hooks.cpp`). All_To_Hunt still runs while the delayed sale
/// waits, and the timer is armed once and then only read.
fn last_legs_actions(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    #[cfg(feature = "phobos")]
    phobos_last_legs_actions(sim, rules, owner, registry, frame_effects);
    #[cfg(not(feature = "phobos"))]
    sell_off_and_hunt(sim, rules, owner, "no factory", registry, frame_effects);
}

#[cfg(feature = "phobos")]
fn phobos_last_legs_actions(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if rules.general.ai_fire_sale {
        let frame = sim.session.binary_frame() as i32;
        let delay = rules.general.ai_fire_sale_delay;
        let sale_due = if delay <= 0 {
            true
        } else {
            match sim
                .houses
                .get(&owner)
                .map(|house| house.strategy_emergency().ai_fire_sale_delay_timer())
            {
                Some(Some(timer)) => timer.expired(frame),
                Some(None) => {
                    if let Some(house) = sim.houses.get_mut(&owner) {
                        house
                            .strategy_emergency_mut()
                            .start_ai_fire_sale_delay(frame, delay);
                    }
                    false
                }
                None => false,
            }
        };
        if sale_due {
            log::debug!("{} sells off on its last legs", sim.interner.resolve(owner));
            fire_sale(sim, rules, owner, registry, frame_effects);
        }
    }
    if rules.general.ai_all_to_hunt {
        all_to_hunt(sim, rules, owner, registry, frame_effects);
    }
}

/// `0x004FD538..0x004FD71E`: see the module doc.
fn pick_enemy(
    interner: &StringInterner,
    session: &ScenarioSession,
    house_alliances: &HouseAlliances,
    houses: &mut Houses,
    owner: InternedId,
) {
    if !ENEMY_SEARCH_TIMER.expired(session.binary_frame() as i32) || !session.game_mode_nonzero() {
        return;
    }
    let Some(house) = houses.get(&owner) else {
        return;
    };
    if house.enemy_house().is_some() || house.multiplay_passive() || house.base_origin() == (0, 0) {
        return;
    }
    let first = houses.order().iter().copied().find(|&peer| {
        peer != owner
            && houses
                .get(&peer)
                .is_some_and(|peer| !peer.multiplay_passive() && !peer.is_defeated())
    });
    if let Some(peer) = first {
        update_anger_nodes(houses, house_alliances.as_map(), interner, owner, peer, 1);
    }
}

/// `0x004FD723..0x004FD772`: a defeated enemy's anger node is cancelled
/// (the anger rescan runs) and the enemy forgotten.
fn forget_defeated_enemy(
    interner: &StringInterner,
    house_alliances: &HouseAlliances,
    houses: &mut Houses,
    owner: InternedId,
) {
    let Some(enemy) = houses.get(&owner).and_then(|house| house.enemy_house()) else {
        return;
    };
    if !houses.get(&enemy).is_some_and(|house| house.is_defeated()) {
        return;
    }
    let anger = houses[&owner]
        .grudge_scores()
        .get(&enemy)
        .copied()
        .unwrap_or(0);
    update_anger_nodes(
        houses,
        house_alliances.as_map(),
        interner,
        owner,
        enemy,
        anger.wrapping_neg(),
    );
    if let Some(house) = houses.get_mut(&owner) {
        house.forget_enemy();
    }
}

/// `0x004FD879..0x004FD8C3`: a building in the house's list (House+0x68)
/// that is alive (`+0x90`), not in limbo (`+0x81`) and whose type has
/// `Factory=` (`+0xEB8`).
fn has_live_factory(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> bool {
    sim.houses.get(&owner).is_some_and(|house| {
        house.base_projection().buildings().iter().any(|&id| {
            sim.substrate.entities.get(id).is_some_and(|building| {
                building.is_ai_alive()
                    && !building.lifecycle.in_limbo()
                    && sim
                        .object_type(building.type_ref(), rules)
                        .is_some_and(|ty| ty.has_factory())
            })
        })
    })
}

/// IHouse `Fire_Sale @ 0x005013A0`: with any building counted (`+0x2F0`),
/// the computer sale (`Sell_Back(1)`, vt+0x1A0) of every building in the
/// house's list that is not in limbo and has Health above zero, in list
/// order. The list's length is read once; only a `FirestormWall=` sale
/// leaves it at once, and no retail type sets that key.
pub(crate) fn fire_sale(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if house.tracking().buildings() <= 0 {
        return;
    }
    for slot in 0..house.base_projection().buildings().len() {
        let Some(id) = sim
            .houses
            .get(&owner)
            .and_then(|house| house.base_projection().buildings().get(slot).copied())
        else {
            break;
        };
        let sells = sim.substrate.entities.get(id).is_some_and(|building| {
            !building.lifecycle.in_limbo() && building.health.current() > 0
        });
        if sells {
            sell_back(sim, rules, id, SellOrder::Computer, registry, frame_effects);
        }
    }
}

/// IHouse `All_To_Hunt @ 0x00501400`: from the last Techno to the first
/// (TechnoClass::Array, stable-id order, its length read once), each of the
/// house's objects that is on the map (`+0x74`) and not in limbo, read at
/// its turn:
/// - one the Psychic Dominator holds (`+0x2C4`) whose type is
///   `Insignificant=` (`+0x232`) takes its type's `Strength=` as
///   `C4Warhead=` damage, with no attacker, ignoring defences and keeping
///   its passengers in (`ReceiveDamage`, vt+0x16C, `0x0050144B..0x005014AA`),
///   unless the house's IsHuman byte (`+0x1EC`) is set (on retail rules:
///   the civilians, their vehicles and the animals the computer's Dominator
///   took);
/// - otherwise a Foot (`+0x14 & 4`) leaves its team (`0x005014C5..
///   0x005014D4`, with its idle order) and queues Hunt (vt+0x1E8,
///   `Queue_Mission(Hunt, 0)`).
///
/// An occupied building calls the shared SellBuilding457DE0(1,0) owner;
/// its occupants Scatter, leave their teams and queue Hunt in that order.
/// Then the All-To-Hunt latch (`+0x249`) is set.
pub(crate) fn all_to_hunt(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    // Steam gamemd:501400 captures count once;50141D/501423
    // re-read the array and its indexed object after callbacks. Storage
    // snapshots hide intervening removals from this existing registry.
    let count = sim.substrate.entities.techno_registry_len();
    let human = sim.houses.get(&owner).is_some_and(|house| house.is_human());
    let hunt = MissionId::from_known(MissionType::Hunt);
    for index in (0..count).rev() {
        let Some(id) = sim.substrate.entities.techno_registry_at(index) else {
            continue;
        };
        let Some(techno) = sim.substrate.entities.get(id) else {
            continue;
        };
        if techno.owner() != owner || !techno.lifecycle.cell_marked() || techno.lifecycle.in_limbo()
        {
            continue;
        }
        let dominated_strength = (techno.mind_control.permanent() && !human)
            .then(|| sim.interner.object_type(techno.type_ref(), rules))
            .flatten()
            .filter(|object| object.insignificant)
            .map(|object| object.strength);
        if let Some(strength) = dominated_strength {
            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                strength,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                sim.rule_handles().c4,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            );
            sim.commit_direct_damage_receiver(rules, registry, event, frame_effects);
            continue;
        }
        if techno.category() == EntityCategory::Structure {
            crate::sim::passenger::garrison_eject::sell_building_occupants(
                sim,
                rules,
                registry,
                id,
                crate::sim::passenger::garrison_eject::GarrisonEjectMode::Hunt,
                frame_effects,
            );
            continue;
        }
        sim.leave_team(id, false, rules, frame_effects);
        let _ = sim.mission_queue_exact(
            id,
            hunt,
            0,
            sim.session.binary_frame(),
            &LiveReadyInputProvider { rules },
        );
    }
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.strategy_emergency_mut().set_all_to_hunt_bias();
    }
}

/// Apply one signed anger delta and recompute the designated enemy.
///
/// gamemd-derived: `HouseClass__UpdateAngerNodes @ 0x00504790`. Native updates
/// only a constructor-registered peer node, then forward-scans every peer with
/// a strict-positive/strict-greater winner rule. Rust's sparse map represents
/// untouched constructor-zero nodes without materializing serialized state.
/// With no winner the enemy is cleared (`0x0050484F`).
pub(crate) fn update_anger_nodes(
    houses: &mut crate::sim::houses::Houses,
    alliances: &HouseAllianceMap,
    interner: &StringInterner,
    owner: InternedId,
    peer: InternedId,
    delta: i32,
) {
    let peer_is_registered = peer != owner && houses.contains_key(&peer);
    if peer_is_registered && let Some(house) = houses.get_mut(&owner) {
        house.add_anger(peer, delta);
    }

    let Some(house) = houses.get(&owner) else {
        return;
    };
    let mut best_score = 0;
    let mut best_house = None;
    for &candidate_id in houses.order() {
        if candidate_id == owner {
            continue;
        }
        let Some(candidate) = houses.get(&candidate_id) else {
            continue;
        };
        let score = house
            .grudge_scores()
            .get(&candidate_id)
            .copied()
            .unwrap_or(0);
        if score > best_score
            && !candidate.is_defeated()
            && !crate::map::houses::is_allied_with(
                alliances,
                interner.resolve(owner),
                interner.resolve(candidate_id),
            )
        {
            best_score = score;
            best_house = Some(candidate_id);
        }
    }
    if let Some(house) = houses.get_mut(&owner) {
        house.assign_enemy(best_house);
    }
}

/// Exact `House+0x249` decision consumed by native
/// `TechnoClass__Evaluate_Candidate @ 0x006F875F..0x006F878B`: under the
/// All-To-Hunt bias, a candidate the house's current enemy does not own
/// scores 1 ([`crate::sim::combat::greatest_threat`]).
pub(crate) fn all_to_hunt_score_override(
    attacker_house: &HouseState,
    candidate_owner: InternedId,
) -> Option<i32> {
    if attacker_house.strategy_emergency().all_to_hunt_bias()
        && attacker_house
            .enemy_house()
            .is_some_and(|enemy| candidate_owner != enemy)
    {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "house_strategy_tests.rs"]
mod tests;
