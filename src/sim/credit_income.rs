//! Per-object money producers/consumers that live in the object AI loop:
//! oil-derrick `ProduceCash` and the Floating Disc money drain (GSI-09.01).
//! This module owns, and alone writes, each object's [`ProduceCash`] timer
//! and [`Drain`] links.
//!
//! Both mechanisms move credits through the house's wallet
//! ([`Economy`](crate::sim::economy::Economy)), whose money primitives these
//! look up by house: `HouseClass::Add_Credits @ 0x004F9950` and
//! `HouseClass::Spend_Money @ 0x004F9790`.
//!
//! Building AI runs ProduceCash43FD2C before shared Techno43FE56 promotion
//! and drain transfer, matching the implemented Structure prelude order.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and sim/ only. NEVER on render/, ui/,
//!   audio/ or net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::mission::MissionType;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::power_system::PowerStates;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;

/// `BuildingClass+0x6D0`/`+0x6D8` ProduceCash timer (oil derricks).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProduceCash {
    /// Seeded to the constructor's dead state (`start = construction frame`,
    /// `duration = 0`, `BuildingClass::Constructor @ 0x0043B92B..0x0043B937`);
    /// only a capture from a `MultiplayPassive` house (`BuildingClass::
    /// ChangeOwner @ 0x004482DB..0x004482F9`), Grand_Opening
    /// (`0x00445FA5..0x00445FC8`) and the re-arm in `BuildingClass::Update @
    /// 0x0043FD5B..0x0043FD86` give it a duration. `+0x6D4`, the middle dword,
    /// is scratch the fire test never reads. Zero-duration on every
    /// non-derrick object. Hashed (v135) and persisted.
    timer: CdTimer,
}

impl ProduceCash {
    /// The constructor's dead timer, started at `construction_frame`.
    pub(crate) fn new(construction_frame: u32) -> Self {
        Self {
            timer: CdTimer::started(construction_frame as i32, 0),
        }
    }

    /// The ProduceCash timer.
    pub(crate) fn timer(&self) -> CdTimer {
        self.timer
    }

    /// Start the timer at `frame` with `delay` (`ProduceCashDelay`).
    fn arm(&mut self, frame: u32, delay: i32) {
        self.timer = CdTimer::started(frame as i32, delay);
    }
}

/// The Floating Disc drain link pair, `TechnoClass+0x1CC`/`+0x1D0`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Drain {
    /// `TechnoClass+0x1CC DrainTarget`: the building this object is draining
    /// (Floating Disc). Set by `Fire_At`'s `DrainWeapon` arm
    /// ([`install_drain_link`]), cleared by `UnitClass::AI`'s cell recheck,
    /// the ally check in `AI_Update`, pointer expiry and the death arm of
    /// `TechnoClass::ReceiveDamage` (`0x007020B1`, `0x00702102`). Hashed
    /// (v135) and persisted.
    target: Option<u64>,
    /// `TechnoClass+0x1D0 DrainingMe`: the object draining this one, the
    /// reciprocal of `target`. The victim's `AI_Update` reads it for the money
    /// transfer. Hashed (v135) and persisted.
    draining_me: Option<u64>,
    /// `TechnoClass+0x1D4 DrainAnim`: the drainer's `DrainAnimationType=`
    /// anim ([`attach_drain_anim`]), un-initialized by every native break of
    /// the link. Persisted (v422); not hashed, since the anim and its owner
    /// are hashed with the anims.
    anim: Option<crate::sim::anim_class::AnimId>,
}

impl Drain {
    /// The building this object is draining.
    pub(crate) fn target(&self) -> Option<u64> {
        self.target
    }

    /// The object draining this one.
    pub(crate) fn draining_me(&self) -> Option<u64> {
        self.draining_me
    }

    /// The drainer's DrainAnim.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn anim(&self) -> Option<crate::sim::anim_class::AnimId> {
        self.anim
    }

    /// The anim's own expiry clears the matching `+0x1D4`: its owner
    /// callback (`0x00710427..0x0071042F`) and PointerExpired
    /// (`0x00707930..0x00707938`).
    pub(crate) fn expire_anim(&mut self, anim: crate::sim::anim_class::AnimId) {
        if self.anim == Some(anim) {
            self.anim = None;
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drain {
    pub(crate) fn set_target_for_test(&mut self, target: Option<u64>) {
        self.target = target;
    }

    pub(crate) fn set_draining_me_for_test(&mut self, draining_me: Option<u64>) {
        self.draining_me = draining_me;
    }
}

/// The ProduceCash fire test inlined in `BuildingClass::Update @
/// 0x0043FD2C..0x0043FD59` on the `+0x6D0`/`+0x6D8` timer ([`ProduceCash`]):
/// it fires when exactly one frame remains. An armed timer of duration `D`
/// fires on the frame where `frame - start == D - 1`, i.e. every `D - 1`
/// frames after each re-arm, and a timer whose expiry frame was skipped (the
/// two warp gates at `0x0043FD0C`/`0x0043FD1E` jump past the block) is dead
/// until re-armed.
fn produce_cash_fires(timer: CdTimer, frame: u32) -> bool {
    timer.remaining(frame as i32) == 1
}

/// [`Economy::add_credits`](crate::sim::economy::Economy::add_credits) on
/// `owner`'s wallet (`HouseClass::Add_Credits @ 0x004F9950`).
pub fn add_credits(houses: &mut Houses, owner: InternedId, amount: i32) {
    if let Some(house) = houses.get_mut(&owner) {
        house.economy_mut().add_credits(amount);
    }
}

/// [`Economy::spend_money`](crate::sim::economy::Economy::spend_money) on
/// `owner`'s wallet (`HouseClass::Spend_Money @ 0x004F9790`).
pub(crate) fn spend_money(houses: &mut Houses, owner: InternedId, amount: i32) {
    if let Some(house) = houses.get_mut(&owner) {
        house.economy_mut().spend_money(amount);
    }
}

/// [`Economy::available_money`](crate::sim::economy::Economy::available_money)
/// of `owner`'s wallet (the money interface's slot `+0x18`,
/// `Available_Money @ 0x004F6990`); 0 for a house that does not exist.
pub(crate) fn available_money(houses: &Houses, owner: InternedId) -> i32 {
    houses
        .get(&owner)
        .map_or(0, |house| house.economy().available_money())
}

// ---------------------------------------------------------------------------
// Oil derrick ProduceCash
// ---------------------------------------------------------------------------

/// `BuildingClass::ChangeOwner @ 0x004482AA..0x004482F9`, run BEFORE the
/// Techno owner swap while `this->Owner` is still the OLD house: when the old
/// owner's HouseType has `MultiplayPassive` (`HouseType+0x1A6`) set and
/// `ProduceCashStartup` (`Type+0x1558`) is non-zero, `Add_Credits(startup)`
/// on the NEW owner (`ECX = EBX = [ESP+0x58]`, the first stack argument), then
/// arm the ProduceCash timer with `ProduceCashDelay` (`Type+0x1560`).
///
/// Capture by a non-passive house from a non-passive house (an engineer
/// retaking a derrick) grants nothing and leaves the timer running as it was.
pub(crate) fn produce_cash_on_owner_change(
    interner: &StringInterner,
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    houses: &mut Houses,
    stable_id: u64,
    old_owner: InternedId,
    new_owner: InternedId,
    rules: &RuleSet,
) {
    let Some(entity) = substrate.entities.get(stable_id) else {
        return;
    };
    if entity.category() != EntityCategory::Structure {
        return;
    }
    let Some(obj) = interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    let old_passive = houses
        .get(&old_owner)
        .is_some_and(|house| house.multiplay_passive());
    if !old_passive || obj.produce_cash_startup == 0 {
        return;
    }
    let startup = obj.produce_cash_startup;
    let delay = obj.produce_cash_delay;
    let frame = session.binary_frame();
    add_credits(houses, new_owner, startup);
    if let Some(entity) = substrate.entities.get_mut(stable_id) {
        entity.produce_cash.arm(frame, delay);
    }
}

///445FA5..445FC8: every admitted Grand_Opening rearms a nonzero delay,
/// including the already-placed capture arm. Zero leaves the prior timer.
pub(crate) fn produce_cash_on_grand_opening(
    interner: &StringInterner,
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    id: u64,
    rules: &RuleSet,
) {
    let delay = substrate
        .entities
        .get(id)
        .and_then(|entity| interner.object_type(entity.type_ref(), rules))
        .map_or(0, |object| object.produce_cash_delay);
    if delay != 0
        && let Some(entity) = substrate.entities.get_mut(id)
    {
        entity.produce_cash.arm(session.binary_frame(), delay);
    }
}

/// `BuildingClass::Update @ 0x0043FD2C..0x0043FDD6`, the ProduceCash block:
/// fire test on the `+0x6D0` timer (see [`produce_cash_fires`]),
/// re-arm with `ProduceCashDelay`, skip when the owner's HouseType is
/// `MultiplayPassive` (`0x0043FD89..0x0043FD9A`), skip unless operational
/// (`vtable+0x350`, `0x0043FDA0`), then `amount > 0 → Add_Credits(amount)`
/// else `Spend_Money(-amount)` (`0x0043FDAA..0x0043FDD1`).
///
/// The two warp gates ahead of the block (`IsWarpingOut` `+0x270` at
/// `0x0043FD0C`, `IsBeingWarped` `+0x271` at `0x0043FD1E`) jump past it. In
/// VERA the object turn's temporal prologue (`temporal_ai_prologue`) returns
/// for a warped building before this step, so a timer whose expiry frame
/// falls inside the warp dies as natively.
pub(crate) fn produce_cash_step(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get(stable_id) else {
        return;
    };
    if entity.category() != EntityCategory::Structure {
        return;
    }
    let frame = sim.session.binary_frame();
    if !produce_cash_fires(entity.produce_cash.timer(), frame) {
        return;
    }
    let owner = entity.owner();
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    let amount = obj.produce_cash_amount;
    let delay = obj.produce_cash_delay;
    if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
        entity.produce_cash.arm(frame, delay);
    }
    if sim
        .houses
        .get(&owner)
        .is_some_and(|house| house.multiplay_passive())
    {
        return;
    }
    if sim.building_operational_state(stable_id, rules) != Some(true) {
        return;
    }
    if amount > 0 {
        add_credits(&mut sim.houses, owner, amount);
    } else {
        spend_money(&mut sim.houses, owner, amount.wrapping_neg());
    }
}

// ---------------------------------------------------------------------------
// Floating Disc money drain
// ---------------------------------------------------------------------------

/// `0x0070FD70` (DB label `BuildingClass__EnterTransport` is a misnomer; the
/// sole caller is `TechnoClass::Fire_At @ 0x006FDF8C` on the `DrainWeapon`
/// arm): when the building in the drainer's own cell is the target, set
/// `target+0x1D0 = drainer` (`DrainingMe`), `drainer+0x1CC = target`
/// (`DrainTarget`), flag the victim house's power recalc (`+0x5778`), free any
/// mind-controlled captives, create the `DrainAnim` and drop the drainer from
/// its team. This owner installs the pointers and dirties House power; the
/// caller then frees the captives, attaches the anim ([`attach_drain_anim`])
/// and drops the team, in that order. The reached House assessment
/// suppresses the drained output.
///
/// Returns whether the link was made.
pub(crate) fn install_drain_link(
    substrate: &mut ObjectSubstrate,
    houses: &Houses,
    power_states: &mut PowerStates,
    drainer_id: u64,
    victim_id: u64,
) -> bool {
    let Some((rx, ry)) = substrate
        .entities
        .get(drainer_id)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
    else {
        return false;
    };
    // 70FD8D: first ground Building, before any reciprocal-link writes.
    if substrate
        .occupancy
        .first_building_on_layer(rx, ry, MovementLayer::Ground)
        != Some(victim_id)
    {
        return false;
    }
    if let Some(victim) = substrate.entities.get_mut(victim_id) {
        victim.drain.draining_me = Some(drainer_id);
    }
    if let Some(drainer) = substrate.entities.get_mut(drainer_id) {
        drainer.drain.target = Some(victim_id);
    }
    if let Some(owner) = substrate
        .entities
        .get(victim_id)
        .map(|victim| victim.owner())
    {
        //70FDAC, before capture release/animation/team callbacks.
        Simulation::invalidate_house_power(houses, power_states, owner, false);
    }
    true
}

/// A fixture's drain link pair, without Install_Drain_Link's cell gate.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn link_for_test(sim: &mut Simulation, drainer_id: u64, victim_id: u64) {
    sim.substrate
        .entities
        .get_mut(drainer_id)
        .unwrap()
        .drain
        .target = Some(victim_id);
    sim.substrate
        .entities
        .get_mut(victim_id)
        .unwrap()
        .drain
        .draining_me = Some(drainer_id);
}

/// Install_Drain_Link's anim (`0x0070FDC2..0x0070FE14`): an AnimClass of
/// `[CombatDamage] DrainAnimationType=` at the drainer's GetCoords (vt+0x48),
/// no delay, one loop, flags `0x600`, no Z adjust, forward, which
/// SetOwnerObject (`0x00424B50`) then attaches to the drainer; kept in its
/// `+0x1D4`. No type, or a refused construction, keeps nothing.
pub(crate) fn attach_drain_anim(sim: &mut Simulation, drainer_id: u64, rules: &RuleSet) {
    let Some(anim_name) = rules.combat_damage.drain_animation_type.as_deref() else {
        return;
    };
    let Some(coord) = sim.anim_owner_coords(drainer_id) else {
        return;
    };
    let type_id = sim.interner.intern(anim_name);
    let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
        delay: 0,
        loop_count: 1,
        draw_flags: 0x600,
        z_adjust: 0,
        reverse: false,
        owner: Some(drainer_id),
        ..crate::sim::components::AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
    };
    match sim.spawn_anim_at_world(rules, descriptor, coord) {
        Ok(anim_id) => {
            if let Some(drainer) = sim.substrate.entities.get_mut(drainer_id) {
                drainer.drain.anim = Some(anim_id);
            }
        }
        Err(error) => log::debug!("drain anim [{anim_name}] did not construct: {error}"),
    }
}

/// The `+0x1D4` DrainAnim un-init (vt+0xF8) that opens every native break
/// of a drain link, for `drainer_id`.
fn remove_drain_anim(sim: &mut Simulation, drainer_id: u64, rules: &RuleSet) {
    if let Some(anim) = sim
        .substrate
        .entities
        .get_mut(drainer_id)
        .and_then(|drainer| drainer.drain.anim.take())
    {
        sim.destroy_anim_with_context(anim, rules);
    }
}

/// `0x0070FE50` (DB label `BuildingClass__ExitTransport`, misnomer): remove
/// the `DrainAnim`, clear the victim's `DrainingMe`, flag its house's power
/// recalc, clear the drainer's `DrainTarget`. Also the inline copy in
/// `TechnoClass::AI_Update @ 0x006FA1DF..0x006FA21E`.
pub(crate) fn stop_drain(sim: &mut Simulation, drainer_id: u64, rules: &RuleSet) {
    remove_drain_anim(sim, drainer_id, rules);
    let Some(victim_id) = sim
        .substrate
        .entities
        .get(drainer_id)
        .and_then(|entity| entity.drain.target)
    else {
        return;
    };
    if let Some(victim) = sim.substrate.entities.get_mut(victim_id)
        && victim.drain.draining_me == Some(drainer_id)
    {
        victim.drain.draining_me = None;
        let owner = victim.owner();
        Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, owner, false); //70FE93 /6FA217
    }
    if let Some(drainer) = sim.substrate.entities.get_mut(drainer_id) {
        drainer.drain.target = None;
    }
}

/// `TechnoClass::PointerExpired @ 0x0070785F..0x0070792A` and the death arm
/// of `TechnoClass::ReceiveDamage @ 0x0070206A..0x00702102`: an expiring or
/// dying object drops both halves of any drain link it is part of, the
/// drainer's DrainAnim first on each side. The killing hit reaches it through
/// its Destroy's Detach_All broadcast (`0x005F5280`, the receiver's
/// `detach_all`), in the same ReceiveDamage and before any draw
/// the death arm makes, so a falling wreck no longer drains or shows its ray.
pub(crate) fn clear_drain_links_on_expiry(sim: &mut Simulation, expired_id: u64, rules: &RuleSet) {
    let Some((drain_target, draining_me)) = sim
        .substrate
        .entities
        .get(expired_id)
        .map(|entity| (entity.drain.target, entity.drain.draining_me))
    else {
        return;
    };
    if drain_target.is_some() {
        remove_drain_anim(sim, expired_id, rules);
    }
    if let Some(victim_id) = drain_target
        && let Some(victim) = sim.substrate.entities.get_mut(victim_id)
        && victim.drain.draining_me == Some(expired_id)
    {
        victim.drain.draining_me = None;
        let owner = victim.owner();
        Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, owner, false); //7020AA /70789F
    }
    if let Some(drainer_id) = draining_me
        && sim
            .substrate
            .entities
            .get(drainer_id)
            .is_some_and(|drainer| drainer.drain.target == Some(expired_id))
    {
        remove_drain_anim(sim, drainer_id, rules);
        if let Some(drainer) = sim.substrate.entities.get_mut(drainer_id) {
            drainer.drain.target = None;
        }
        let owner = sim
            .substrate
            .entities
            .get(expired_id)
            .map(|entity| entity.owner());
        if let Some(owner) = owner {
            Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, owner, false);
        } //7020FB /70791D
    }
    if let Some(entity) = sim.substrate.entities.get_mut(expired_id) {
        entity.drain = Drain::default();
    }
}

/// The two drain blocks of the common `TechnoClass::AI_Update` body, in
/// native order, right after the veterancy promotion sample:
///
/// 1. Victim side `0x006FA14B..0x006FA1C5`: with `DrainingMe` (`+0x1D0`) set
///    and the type byte `+0x5ED` set (`0x006FA15D`), on frames where
///    `g_CurrentFrameCounter % DrainMoneyFrameDelay == 0` (signed `IDIV`):
///    `amt = DrainMoneyAmount; if (Available_Money(owner) < amt) amt =
///    Available_Money(owner); Spend_Money(owner, amt);
///    Add_Credits(drainer->Owner, amt)`.
///
///    `+0x5ED` is NOT `Drainable`: `TechnoTypeClass::ReadINI @ 0x007143EA..
///    0x007143FE` stores it from the key `"ResourceDestination"` (string
///    `0x00843CA4`); `"Drainable"` (`0x00843CD8`) lands in `+0x5EF`, which
///    only the `Fire_At` link gate at `0x006FDF7B` reads. So a drained
///    building loses money only when its type is a resource destination (the
///    refineries `GAREFN`/`NAREFN`/`YAREFN`); a drained `GAPOWR` loses power
///    (the `+0x5778` recalc, retained House assessment) but no credits.
/// 2. Drainer side `0x006FA1C5..0x006FA224`: with `DrainTarget` (`+0x1CC`)
///    set and the target's house allied with this object's
///    (`HouseClass::Is_Ally_ByObject @ 0x004F9A90`, asked by the target's
///    house at `0x006FA1CF`), stop the drain inline.
pub(crate) fn drain_common_step(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get(stable_id) else {
        return;
    };
    let owner = entity.owner();
    let draining_me = entity.drain.draining_me;
    let drain_target = entity.drain.target;
    let type_ref = entity.type_ref();

    if let Some(drainer_id) = draining_me
        && sim
            .interner
            .object_type(type_ref, rules)
            .is_some_and(|obj| obj.resource_destination)
    {
        let delay = rules.general.drain_money_frame_delay;
        // Native divides by the raw dword; a zero delay would fault there.
        if delay != 0
            && (sim.session.binary_frame() as i32).wrapping_rem(delay) == 0
            && let Some(drainer_owner) = sim
                .substrate
                .entities
                .get(drainer_id)
                .map(|drainer| drainer.owner())
        {
            let mut amount = rules.general.drain_money_amount;
            let available = available_money(&sim.houses, owner);
            if available < amount {
                amount = available;
            }
            spend_money(&mut sim.houses, owner, amount);
            add_credits(&mut sim.houses, drainer_owner, amount);
        }
    }

    if let Some(victim_id) = drain_target
        && let Some(target_owner) = sim
            .substrate
            .entities
            .get(victim_id)
            .map(|victim| victim.owner())
        && crate::map::houses::is_allied_with(
            sim.house_alliances.as_map(),
            sim.interner.resolve(target_owner),
            sim.interner.resolve(owner),
        )
    {
        stop_drain(sim, stable_id, rules);
    }
}

/// `UnitClass::AI @ 0x007361A9..0x00736204`, two arms in native order:
///
/// 1. With `DrainTarget` set, on frames where `g_CurrentFrameCounter % 16 == 2`
///    (signed, `0x007361B8..0x007361C7`), re-look-up the building in the
///    unit's own cell and stop the drain when it is no longer the target.
/// 2. `0x007361E9..0x00736204`: a `DrainAnim` (`+0x1D4`) left with no
///    `DrainTarget` is removed through Stop_Drain.
pub(crate) fn drain_unit_ai_step(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) {
    let Some((target, anim, rx, ry)) = sim.substrate.entities.get(stable_id).map(|entity| {
        (
            entity.drain.target,
            entity.drain.anim,
            entity.position.rx(),
            entity.position.ry(),
        )
    }) else {
        return;
    };
    if let Some(target) = target
        && (sim.session.binary_frame() as i32).wrapping_rem(16) == 2
        && sim
            .substrate
            .occupancy
            .first_building_on_layer(rx, ry, MovementLayer::Ground)
            != Some(target)
    {
        stop_drain(sim, stable_id, rules);
    }
    if anim.is_some()
        && sim
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.drain.target.is_none())
    {
        stop_drain(sim, stable_id, rules);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::TargetKind;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;

    /// Retail shape: `GAPOWR` is `Drainable=yes` only; the refineries carry
    /// both `ResourceDestination=yes` and `Drainable=yes` (rulesmd.ini
    /// `[NAREFN]`). The disc carries retail's laser/drain pair with the
    /// retail `DiskDrain` range and retail `Sight=9`: `Sight` defaults to 0
    /// here, and a sightless disc never reveals the victim's cell, so the
    /// first Attack dispatch drops the target on the fog-visibility retarget
    /// (`combat::resolve_attacker_fire`) before any shot is resolved.
    fn rules() -> RuleSet {
        rules_with_refinery_foundation("2x2")
    }

    fn rules_with_refinery_foundation(foundation: &str) -> RuleSet {
        // Retail `[DISKRAY]` art (ARTMD.INI), five frames bound.
        let art_text = format!(
            "[CAOILD]\nFoundation=2x2\n[CAOILDP]\nFoundation=2x2\n[GAPOWR]\nFoundation=2x2\n\
             [NAREFN]\nFoundation={foundation}\n\
             [DISKRAY]\nLoopCount=-1\nRate=200\nLoopStart=0\nLoopEnd=4\n"
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(
            "[General]\nDrainMoneyFrameDelay=30\nDrainMoneyAmount=30\n\n\
             [CombatDamage]\nDrainAnimationType=DISKRAY\n\n\
             [InfantryTypes]\n[AircraftTypes]\n\
             [VehicleTypes]\n0=DISK\n\
             [BuildingTypes]\n0=CAOILD\n1=CAOILDP\n2=GAPOWR\n3=NAREFN\n\n\
             [CAOILD]\nStrength=1000\nArmor=wood\nCapturable=true\n\
             ProduceCashStartup=1000\nProduceCashAmount=20\nProduceCashDelay=100\n\n\
             [CAOILDP]\nStrength=1000\nArmor=wood\nCapturable=true\n\
             Powered=yes\nPower=-10\n\
             ProduceCashStartup=1000\nProduceCashAmount=20\nProduceCashDelay=100\n\n\
             [GAPOWR]\nStrength=750\nArmor=wood\nPower=100\nDrainable=yes\n\n\
             [NAREFN]\nStrength=1000\nArmor=wood\n\
             ResourceDestination=yes\nDrainable=yes\n\n\
             [DISK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=500\nArmor=heavy\nSpeed=6\n\
             Sight=9\nPrimary=DiskLaser\nSecondary=DiskDrain\n\n\
             [DiskLaser]\nDamage=90\nROF=80\nRange=7\nProjectile=Invisible\nWarhead=DiskWH\n\n\
             [DiskDrain]\nDamage=1\nBurst=1\nROF=50\nRange=1.5\nProjectile=Invisible\nWarhead=AntiB\n\
             DrainWeapon=yes\nOmniFire=yes\n\n\
             [DiskWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
             [AntiB]\nVerses=0%,0%,0%,0%,0%,0%,100%,100%,100%,100%,100%\n",
        ), &IniFile::from_str(&art_text))
        .expect("credit-income test rules parse");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(&art_text));
        art.bind_anim_frame_count_for_test("DISKRAY", 5);
        rules.replace_art_registry_for_test(art);
        rules
    }

    /// The anim's un-init ran: out of the logic vector, awaiting deletion
    /// (or already deleted by a later tick).
    fn anim_uninited(sim: &Simulation, id: crate::sim::anim_class::AnimId) -> bool {
        sim.anim(id).is_none_or(|anim| {
            !anim.in_logic_vector() && sim.substrate.pending_delete.contains_object(&id)
        })
    }

    /// The disc's live DrainAnim: DISKRAY, owned by the disc.
    fn assert_drain_anim(sim: &Simulation, disk: u64) -> crate::sim::anim_class::AnimId {
        let id = sim
            .entities()
            .get(disk)
            .unwrap()
            .drain
            .anim
            .expect("drain anim");
        let anim = sim.anim(id).expect("live drain anim");
        assert_eq!(sim.interner.resolve(anim.type_id()), "DISKRAY");
        assert_eq!(anim.owner_entity(), Some(disk));
        id
    }

    fn cmd(sim: &Simulation, owner: &str, tick: u64, payload: Command) -> CommandEnvelope {
        let owner_id = sim.interner.get(owner).expect("owner interned");
        CommandEnvelope::new(owner_id, tick, payload)
    }

    fn house(sim: &mut Simulation, name: &str, human: bool, credits: i32) -> InternedId {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, None, human, credits, 10));
        id
    }

    /// A far-away building so Short Game's defeat scan (no buildings, no
    /// base unit → lose → sole survivor wins → frames stop after SavourDelay)
    /// never terminates the fixture match.
    fn base(sim: &mut Simulation, rules: &RuleSet, owner: &str, rx: u16, ry: u16) {
        sim.spawn_object("GAPOWR", owner, rx, ry, 0, rules)
            .expect("base building spawns");
    }

    fn run_ticks(sim: &mut Simulation, rules: &RuleSet, ticks: u32) {
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        for _ in 0..ticks {
            let _ = sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        }
    }

    /// Test-only relocation that keeps the transient occupancy index coherent.
    fn move_disc(sim: &mut Simulation, disk: u64, rx: u16, ry: u16) {
        let entity = sim.substrate.entities.get_mut(disk).expect("disc present");
        let (old_rx, old_ry) = (entity.position.rx(), entity.position.ry());
        entity.position.set_rx_for_test(rx);
        entity.position.set_ry_for_test(ry);
        let layer = sim.substrate.occupancy.listed_layer(old_rx, old_ry, disk);
        if let Some(layer) = layer {
            sim.substrate.occupancy.move_entity(
                old_rx,
                old_ry,
                rx,
                ry,
                disk,
                layer,
                None,
                crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
            );
        }
    }

    fn credits(sim: &Simulation, owner: InternedId) -> i32 {
        sim.houses[&owner].economy().credits()
    }

    /// Original 70FD8D/47C520 queries the actual ground list, not the enclosing
    /// rectangle. Explicit authored ART selects the native special foundation;
    /// stock GAREFN/NAREFN instead use rectangular 4x3 foundations.
    #[test]
    fn cell_building_query_disc_refuses_an_authored_refinery_foundation_hole() {
        let rules = rules_with_refinery_foundation("3x3Refinery");
        let mut sim = Simulation::new();
        let refinery = sim
            .spawn_object("NAREFN", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let disc = sim
            .spawn_object("DISK", "YuriCountry", 22, 21, 0, &rules)
            .unwrap();
        assert!(!install_drain_link(
            &mut sim.substrate,
            &sim.houses,
            &mut sim.power_states,
            disc,
            refinery
        ));
        assert_eq!(sim.entities().get(disc).unwrap().drain.target(), None);
        assert_eq!(
            sim.entities().get(refinery).unwrap().drain.draining_me(),
            None
        );
    }

    /// Original 47C520 has no dying predicate; Mark removal, not death state,
    /// changes the query result. Exercise both install and the existing signed
    /// frame%16==2 recheck through the drain owner, with explicit lifetime priors.
    #[test]
    fn cell_building_query_disc_follows_marked_lifetime_and_recheck_phase() {
        let rules = rules();
        let mut sim = Simulation::new();
        let refinery = sim
            .spawn_object("NAREFN", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let disc = sim
            .spawn_object("DISK", "YuriCountry", 21, 21, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(refinery)
            .unwrap()
            .death
            .set_dying_for_test(true);
        assert!(
            install_drain_link(
                &mut sim.substrate,
                &sim.houses,
                &mut sim.power_states,
                disc,
                refinery
            ),
            "still on the ground list"
        );
        sim.session.set_binary_frame_for_test(2);
        drain_unit_ai_step(&mut sim, disc, &rules);
        assert_eq!(
            sim.entities().get(disc).unwrap().drain.target(),
            Some(refinery)
        );
        assert!(sim.unmark_entity_remove(
            refinery,
            crate::sim::world::UninitContext::with_rules(&rules)
        ));
        sim.session.set_binary_frame_for_test(17);
        drain_unit_ai_step(&mut sim, disc, &rules);
        assert_eq!(
            sim.entities().get(disc).unwrap().drain.target(),
            Some(refinery),
            "not a recheck frame"
        );
        sim.session.set_binary_frame_for_test(18);
        drain_unit_ai_step(&mut sim, disc, &rules);
        assert_eq!(sim.entities().get(disc).unwrap().drain.target(), None);
        assert_eq!(
            sim.entities().get(refinery).unwrap().drain.draining_me(),
            None
        );
        assert!(
            !install_drain_link(
                &mut sim.substrate,
                &sim.houses,
                &mut sim.power_states,
                disc,
                refinery
            ),
            "stored but unlisted"
        );
    }

    /// §2.13: capture from a `MultiplayPassive` house grants
    /// `ProduceCashStartup` and arms the timer; the timer then pays
    /// `ProduceCashAmount` every `ProduceCashDelay - 1` frames (the native
    /// `remaining == 1` fire test), and re-capture by another non-passive
    /// house grants nothing.
    #[test]
    fn derrick_capture_grant_then_twenty_per_ninety_nine_frames() {
        let rules = rules();
        let mut sim = Simulation::with_seed(0x0011_0901);
        let neutral = house(&mut sim, "Neutral", false, 0);
        sim.houses
            .get_mut(&neutral)
            .unwrap()
            .set_multiplay_passive_for_test(true);
        let americans = house(&mut sim, "Americans", true, 5_000);
        let soviets = house(&mut sim, "Soviet", true, 5_000);
        base(&mut sim, &rules, "Americans", 30, 30);
        base(&mut sim, &rules, "Soviet", 40, 40);
        let oil = sim
            .spawn_object("CAOILD", "Neutral", 10, 10, 0, &rules)
            .expect("derrick spawns");
        let opened = sim.substrate.entities.get(oil).unwrap().produce_cash.timer;
        // spawn_object includes Unlimbo/Grand_Opening. Original445F80 arms
        // the type's delay even for a passive house; ProduceCash gates payout.
        assert_eq!(opened.duration(), 100, "opening arms the delay");

        // Owned by the passive house nothing is ever paid.
        run_ticks(&mut sim, &rules, 120);
        assert_eq!(credits(&sim, neutral), 0);

        sim.change_owner_with_rules(
            oil,
            americans,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            credits(&sim, americans),
            6_000,
            "ProduceCashStartup on capture"
        );
        let armed = sim.substrate.entities.get(oil).unwrap().produce_cash.timer;
        assert_eq!(armed.duration(), 100);
        assert_eq!(armed.start_frame(), sim.session.binary_frame() as i32);

        // Arming frame semantics: `change_owner` armed the timer at frame
        // `F = start_frame` with duration `Delay = 100`. `advance_tick` runs
        // the object AI on the CURRENT `binary_frame` and commits `+1` only at
        // its end, so the k-th tick after arming evaluates the fire test at
        // frame `F + k - 1`; `remaining = Delay - (frame - F) == 1` holds at
        // frame `F + 99`, i.e. on tick 100 exactly. Each re-arm then repeats
        // the period of `Delay - 1 = 99` frames.
        let start = armed.start_frame() as u32;
        let mut payments: Vec<(u32, u32)> = Vec::new();
        let mut last = credits(&sim, americans);
        for tick in 1..=400 {
            run_ticks(&mut sim, &rules, 1);
            let now = credits(&sim, americans);
            if now != last {
                assert_eq!(now - last, 20, "one ProduceCashAmount per fire");
                payments.push((tick, sim.session.binary_frame()));
                last = now;
            }
        }
        assert_eq!(payments.len(), 4, "payments at (tick, frame) {payments:?}");
        assert_eq!(
            payments[0].0, 100,
            "first fire on the 100th tick after arming"
        );
        assert_eq!(
            payments[0].1,
            start + 100,
            "the fire ran at frame F + 99; the tick then committed F + 100"
        );
        for pair in payments.windows(2) {
            assert_eq!(pair[1].0 - pair[0].0, 99, "re-armed period is Delay - 1");
        }

        // Re-capture from a non-passive owner grants no startup cash.
        // Grand_Opening(captured) re-arms the delay for the new owner.
        let before = credits(&sim, soviets);
        sim.change_owner_with_rules(
            oil,
            soviets,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(credits(&sim, soviets), before);
        run_ticks(&mut sim, &rules, 99 * 2);
        assert!(
            credits(&sim, soviets) > before,
            "income continues for the captor"
        );
        assert_eq!(credits(&sim, americans), last, "old owner stops earning");
    }

    /// §2.13 gate: `vtable+0x350` (`0x004555D0`) refuses an unpowered
    /// building, so a `Powered=yes` derrick under low power re-arms without
    /// paying.
    #[test]
    fn cash_uses_shared_powered_special_outage_and_effective_mission_gate() {
        use crate::sim::mission::state::MissionTestFixture;
        use crate::sim::mission::{MissionDispatchTimer, MissionId};
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=OIL\n[OIL]\nStrength=100\nPoweredSpecial=yes\nProduceCashAmount=20\nProduceCashDelay=100\n"
        )).unwrap();
        let mut sim = Simulation::new();
        let owner = house(&mut sim, "A", true, 0);
        let id = sim
            .spawn_object_at_height("OIL", "A", 10, 10, 0, 0, &rules)
            .unwrap();
        for (outage, current, queued, pays) in [
            (true, 5, -1, false),
            (false, -1, 0x12, false),
            (false, -1, 0x13, false),
            (false, -1, 5, true),
        ] {
            sim.power_states
                .map_mut_for_test()
                .entry(owner)
                .or_default()
                .start_blackout(0, if outage { 1000 } else { 0 });
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.produce_cash.timer = CdTimer::started(0, 100);
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(current),
                queued: MissionId::from_raw(queued),
                suspended: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
            sim.session.set_binary_frame_for_test(99);
            let before = credits(&sim, owner);
            produce_cash_step(&mut sim, id, &rules);
            assert_eq!(credits(&sim, owner) - before, if pays { 20 } else { 0 });
            assert_eq!(
                sim.entities()
                    .get(id)
                    .unwrap()
                    .produce_cash
                    .timer
                    .start_frame(),
                99
            );
        }
    }

    /// §2.17: with the drain link installed on a `ResourceDestination=yes`
    /// victim (a refinery) the victim loses `min(DrainMoneyAmount, Available)`
    /// on every `DrainMoneyFrameDelay`-th frame and the drainer's house gains
    /// the same amount; a poor victim is drained only to zero.
    #[test]
    fn disc_drain_thirty_per_thirty_frames_capped_at_available() {
        let rules = rules();
        let mut sim = Simulation::with_seed(0x0011_0903);
        let yuri = house(&mut sim, "YuriCountry", false, 500);
        let americans = house(&mut sim, "Americans", true, 75);
        base(&mut sim, &rules, "YuriCountry", 40, 40);
        base(&mut sim, &rules, "Americans", 44, 44);
        let refinery = sim
            .spawn_object("NAREFN", "Americans", 20, 20, 0, &rules)
            .expect("refinery spawns");
        let disk = sim
            .spawn_object("DISK", "YuriCountry", 21, 21, 0, &rules)
            .expect("disc spawns");
        assert_eq!(
            sim.occupancy()
                .first_building_on_layer(21, 21, MovementLayer::Ground),
            Some(refinery)
        );
        assert!(install_drain_link(
            &mut sim.substrate,
            &sim.houses,
            &mut sim.power_states,
            disk,
            refinery
        ));
        assert_eq!(
            sim.substrate.entities.get(disk).unwrap().drain.target,
            Some(refinery)
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(refinery)
                .unwrap()
                .drain
                .draining_me,
            Some(disk)
        );

        // 75 available: 30, 30, then 15, then nothing.
        let mut transfers: Vec<i32> = Vec::new();
        let mut victim_last = 75;
        for _ in 0..150 {
            run_ticks(&mut sim, &rules, 1);
            let now = credits(&sim, americans);
            if now != victim_last {
                transfers.push(victim_last - now);
                victim_last = now;
            }
            assert_eq!(
                credits(&sim, americans) + credits(&sim, yuri),
                575,
                "money is moved, never created or destroyed"
            );
        }
        assert_eq!(transfers, vec![30, 30, 15]);
        assert_eq!(credits(&sim, americans), 0);
        assert_eq!(credits(&sim, yuri), 575);
        // The link survives an empty wallet (native keeps calling with 0).
        assert_eq!(
            sim.substrate.entities.get(disk).unwrap().drain.target,
            Some(refinery)
        );
    }

    /// The drainer side stops the link once the drained building's house is
    /// allied with the disc's, as the building's house asks it (`0x006FA1CF`,
    /// `HouseClass::Is_Ally_ByObject @ 0x004F9A90`): the disc's house allying
    /// the building's alone keeps the link.
    #[test]
    fn the_drained_buildings_house_asks_whether_the_drain_stops() {
        let rules = rules();
        let linked = |asker: &str, other: &str| {
            let mut sim = Simulation::with_seed(0x006F_A1CF);
            house(&mut sim, "YuriCountry", false, 500);
            house(&mut sim, "Americans", true, 75);
            let plant = sim
                .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
                .expect("power plant spawns");
            let disk = sim
                .spawn_object("DISK", "YuriCountry", 21, 21, 0, &rules)
                .expect("disc spawns");
            assert!(install_drain_link(
                &mut sim.substrate,
                &sim.houses,
                &mut sim.power_states,
                disk,
                plant
            ));
            *sim.house_alliances_mut_for_test() = crate::map::houses::HouseAllianceMap::from([(
                asker.to_ascii_uppercase(),
                [other.to_ascii_uppercase()].into(),
            )]);
            drain_common_step(&mut sim, disk, &rules);
            sim.substrate.entities.get(disk).unwrap().drain.target == Some(plant)
        };
        assert!(linked("YuriCountry", "Americans"));
        assert!(!linked("Americans", "YuriCountry"));
    }

    /// `TechnoClass::AI_Update @ 0x006FA15D` reads `Type+0x5ED` =
    /// `ResourceDestination`, not `Drainable` (`+0x5EF`): a drained `GAPOWR`
    /// keeps its link (the `Fire_At` gate at `0x006FDF7B` is the `Drainable`
    /// one) but moves no credits.
    #[test]
    fn drained_power_plant_keeps_the_link_but_moves_no_credits() {
        let rules = rules();
        let mut sim = Simulation::with_seed(0x0011_0906);
        let yuri = house(&mut sim, "YuriCountry", false, 500);
        let americans = house(&mut sim, "Americans", true, 75);
        base(&mut sim, &rules, "YuriCountry", 40, 40);
        let plant = sim
            .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
            .expect("power plant spawns");
        let disk = sim
            .spawn_object("DISK", "YuriCountry", 21, 21, 0, &rules)
            .expect("disc spawns");
        assert!(install_drain_link(
            &mut sim.substrate,
            &sim.houses,
            &mut sim.power_states,
            disk,
            plant
        ));
        run_ticks(&mut sim, &rules, 150);
        assert_eq!(credits(&sim, americans), 75, "no credits leave a GAPOWR");
        assert_eq!(credits(&sim, yuri), 500);
        assert_eq!(
            sim.substrate.entities.get(disk).unwrap().drain.target,
            Some(plant),
            "the link itself is the Drainable-gated half and persists"
        );
        assert_eq!(
            sim.substrate.entities.get(plant).unwrap().drain.draining_me,
            Some(disk)
        );
    }

    /// `TechnoClass::Fire_At @ 0x006FDF8C..0x006FDF97`: the DrainWeapon arm
    /// installs the link through `0x0070FD70` and then calls
    /// `Assign_Target(NULL)` (`[vtable+0x3C8]` = `0x006FCDB0`), so the disc
    /// leaves the shot with no Target while `DrainTarget` persists, and its
    /// Attack mission takes the no-target exit into idle mode on the next
    /// dispatch instead of holding Attack on the building it drains.
    #[test]
    fn drain_shot_clears_the_attack_target_and_drops_the_attack_mission() {
        let rules = rules();
        let mut sim = Simulation::with_seed(0x0011_0907);
        let _yuri = house(&mut sim, "YuriCountry", true, 500);
        let americans = house(&mut sim, "Americans", true, 5_000);
        base(&mut sim, &rules, "YuriCountry", 40, 40);
        base(&mut sim, &rules, "Americans", 44, 44);
        let refinery = sim
            .spawn_object("NAREFN", "Americans", 20, 20, 0, &rules)
            .expect("refinery spawns");
        let disk = sim
            .spawn_object("DISK", "YuriCountry", 21, 21, 0, &rules)
            .expect("disc spawns");
        let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
        let order = cmd(
            &sim,
            "YuriCountry",
            1,
            Command::Attack {
                attacker_id: disk,
                target_id: refinery,
            },
        );
        let result = sim.advance_tick(&[order], Some(&rules), Some(&grid), None, 67);
        assert_eq!(result.executed_commands, 1, "the Attack order is admitted");
        let disc = sim.substrate.entities.get(disk).unwrap();
        // The MEGAMISSION arm queues Attack behind the Guard the disc took at
        // Unlimbo (`UnitClass::Enter_Idle_Mode 0x00738970`); the per-object
        // AI's Ready->Commence promotes it on a later visit
        // (`mission_host_promote`).
        assert_eq!(
            disc.mission.queued().known(),
            Some(MissionType::Attack),
            "the order queues Attack"
        );
        assert_eq!(
            disc.attack_target.target(),
            Some(TargetKind::Entity(refinery)),
            "the order installs the Target"
        );

        // Run until the drain shot lands and installs the link.
        let mut linked_at = None;
        for tick in 2..=120u32 {
            run_ticks(&mut sim, &rules, 1);
            if sim.substrate.entities.get(disk).unwrap().drain.target == Some(refinery) {
                linked_at = Some(tick);
                break;
            }
        }
        let linked_at = linked_at.expect("the DrainWeapon shot links the disc to the refinery");
        let disc = sim.substrate.entities.get(disk).unwrap();
        // The object-AI stage (promotion + handler dispatch) precedes the
        // combat phase within a tick, so on the linking tick Attack is the
        // committed mission and its no-target exit has not yet run.
        assert_eq!(
            disc.mission.current().known(),
            Some(MissionType::Attack),
            "Attack is committed on the linking tick (tick {linked_at})"
        );
        assert!(
            disc.attack_target.target().is_none(),
            "Assign_Target(NULL) at 0x006FDF97 leaves no Target on the same tick (tick {linked_at})"
        );
        assert!(!disc.passive_scan.acquired());
        assert_eq!(
            sim.substrate
                .entities
                .get(refinery)
                .unwrap()
                .drain
                .draining_me,
            Some(disk)
        );
        // Install_Drain_Link's AnimClass (`0x0070FDC2..0x0070FE14`).
        let anim = assert_drain_anim(&sim, disk);

        // The Attack handler's only no-target exit runs the idle-mode selector
        // (`techno_ai::mission_handlers::foot_enter_idle_mode_queue`): no
        // destination, so it queues Guard. The drain link persists and keeps
        // paying out after the mission drops.
        run_ticks(&mut sim, &rules, 120);
        let disc = sim.substrate.entities.get(disk).unwrap();
        assert_eq!(
            disc.mission.current().known(),
            Some(MissionType::Guard),
            "the no-target Attack exit commits Guard"
        );
        assert_eq!(disc.drain.target, Some(refinery));
        assert_eq!(disc.drain.anim, Some(anim), "the ray loops while linked");
        assert!(!anim_uninited(&sim, anim));
        assert!(
            credits(&sim, americans) < 5_000,
            "the link keeps draining after the mission drops"
        );
    }

    #[test]
    fn retail_disc_attack_binds_and_loops_the_attached_drain_ray() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        let mut rules = battle.rules;
        // Use the loader's closure instead of this module's manually bound ART.
        let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        let name = rules.combat_damage.drain_animation_type.as_deref().unwrap();
        assert_eq!(name, "DISKRAY");
        let physical_frames = crate::assets::shp_file::ShpFile::frame_count_from_bytes(
            assets.get_ref("DISKRAY.SHP").unwrap(),
        )
        .unwrap();
        assert_eq!(
            rules
                .art()
                .anim_runtime_config(name)
                .unwrap()
                .raw_shp_frame_count,
            Some(i32::from(physical_frames)),
            "physical DISKRAY.SHP must bind before the drain shot"
        );

        let mut sim = Simulation::with_seed(0x70FD70);
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let yuri = house(&mut sim, "YuriCountry", true, 500);
        house(&mut sim, "Americans", true, 5_000);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.resolve_rule_animation_lists(&rules);
        base(&mut sim, &rules, "YuriCountry", 4, 4);
        let plant = sim
            .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let disk = sim
            .spawn_object("DISK", "YuriCountry", 21, 23, 0, &rules)
            .unwrap();
        // Start over the target's marked cell; approach timing is unrelated to
        // the loader regression. The ordinary Attack still creates the link.
        move_disc(&mut sim, disk, 21, 21);
        sim.fog_mut_for_test().mark_visible_for_owner(yuri, 20, 20);
        let order = cmd(
            &sim,
            "YuriCountry",
            1,
            Command::Attack {
                attacker_id: disk,
                target_id: plant,
            },
        );
        let result = sim.advance_tick(&[order], Some(&rules), Some(&grid), None, 67);
        assert_eq!(result.executed_commands, 1);
        for _ in 0..120 {
            run_ticks(&mut sim, &rules, 1);
            if sim.entities().get(disk).unwrap().drain.target() == Some(plant) {
                break;
            }
        }
        assert_eq!(
            sim.entities().get(disk).unwrap().drain.target(),
            Some(plant)
        );
        let ray = assert_drain_anim(&sim, disk);
        let anim = sim.anim(ray).unwrap();
        assert_eq!(anim.draw_flags(), 0x600);
        assert_eq!(
            anim.world_coord(),
            crate::sim::anim_class::AnimWorldCoord { x: 0, y: 0, z: 0 }
        );
        assert_eq!(sim.anim_absolute_coord(ray), sim.anim_owner_coords(disk));
        assert!(sim.display_layers().layer_of(ray).is_some());
        let mut frames = std::collections::BTreeSet::new();
        for _ in 0..120 {
            run_ticks(&mut sim, &rules, 1);
            let anim = sim.anim(ray).expect("the same attached ray keeps looping");
            frames.insert(anim.runtime().current_frame);
            assert_eq!(sim.anim_absolute_coord(ray), sim.anim_owner_coords(disk));
        }
        // AnimClass::AI4246FC..42470A treats ART LoopEnd as exclusive.
        assert_eq!(frames, (0..4).collect());
        move_disc(&mut sim, disk, 16, 16);
        run_ticks(&mut sim, &rules, 16);
        assert_eq!(sim.entities().get(disk).unwrap().drain.target(), None);
        assert!(
            anim_uninited(&sim, ray),
            "leaving the building removes the ray"
        );
    }

    /// The drain re-check (`0x007361A9..0x00736204`) precedes
    /// `TemporalClass::Update` (`0x00736204`), so a disc being erased still
    /// drops a ray left with no link.
    #[test]
    fn a_warped_disc_still_drops_a_ray_with_no_link() {
        let rules = rules();
        let mut sim = Simulation::with_seed(0x0073_61A9);
        let _yuri = house(&mut sim, "YuriCountry", false, 500);
        let _americans = house(&mut sim, "Americans", true, 500);
        base(&mut sim, &rules, "YuriCountry", 40, 40);
        base(&mut sim, &rules, "Americans", 4, 4);
        let disk = sim
            .spawn_object("DISK", "YuriCountry", 30, 30, 0, &rules)
            .expect("disc spawns");
        attach_drain_anim(&mut sim, disk, &rules);
        let stray = assert_drain_anim(&sim, disk);
        sim.substrate
            .entities
            .get_mut(disk)
            .unwrap()
            .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(
                disk,
            ));
        run_ticks(&mut sim, &rules, 1);
        let disc = sim.entities().get(disk).unwrap();
        assert!(disc.is_warped_out(), "the fixture keeps the disc warped");
        assert_eq!(disc.drain.anim, None);
        assert!(anim_uninited(&sim, stray));
    }

    /// `UnitClass::AI @ 0x007361B3`: every 16th frame the disc re-checks the
    /// building under it and releases the link once it has moved off;
    /// `0x0070FD70` refuses to link a disc that is not over the target.
    #[test]
    fn disc_link_needs_the_cell_and_drops_when_the_disc_leaves() {
        let rules = rules();
        let mut sim = Simulation::with_seed(0x0011_0904);
        let _yuri = house(&mut sim, "YuriCountry", false, 500);
        let _americans = house(&mut sim, "Americans", true, 500);
        base(&mut sim, &rules, "YuriCountry", 40, 40);
        let plant = sim
            .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
            .expect("power plant spawns");
        let disk = sim
            .spawn_object("DISK", "YuriCountry", 30, 30, 0, &rules)
            .expect("disc spawns");
        assert!(!install_drain_link(
            &mut sim.substrate,
            &sim.houses,
            &mut sim.power_states,
            disk,
            plant
        ));
        assert_eq!(
            sim.substrate.entities.get(plant).unwrap().drain.draining_me,
            None
        );

        move_disc(&mut sim, disk, 20, 21);
        assert!(install_drain_link(
            &mut sim.substrate,
            &sim.houses,
            &mut sim.power_states,
            disk,
            plant
        ));
        attach_drain_anim(&mut sim, disk, &rules);
        let anim = assert_drain_anim(&sim, disk);
        run_ticks(&mut sim, &rules, 40);
        assert_eq!(
            sim.substrate.entities.get(disk).unwrap().drain.target,
            Some(plant)
        );
        assert!(!anim_uninited(&sim, anim), "the ray loops while linked");

        // Stop_Drain (`0x0070FE50`) un-inits the ray first.
        move_disc(&mut sim, disk, 30, 21);
        run_ticks(&mut sim, &rules, 17);
        assert_eq!(sim.substrate.entities.get(disk).unwrap().drain.target, None);
        assert_eq!(
            sim.substrate.entities.get(plant).unwrap().drain.draining_me,
            None
        );
        assert_eq!(sim.entities().get(disk).unwrap().drain.anim, None);
        assert!(anim_uninited(&sim, anim));

        // `0x007361E9`: a ray left without a DrainTarget goes on the next
        // UnitClass::AI, whatever the frame.
        attach_drain_anim(&mut sim, disk, &rules);
        let stray = assert_drain_anim(&sim, disk);
        sim.session.set_binary_frame_for_test(5);
        drain_unit_ai_step(&mut sim, disk, &rules);
        assert_eq!(sim.entities().get(disk).unwrap().drain.anim, None);
        assert!(anim_uninited(&sim, stray));

        // The victim's expiry drops both halves and the drainer's ray.
        move_disc(&mut sim, disk, 20, 21);
        assert!(install_drain_link(
            &mut sim.substrate,
            &sim.houses,
            &mut sim.power_states,
            disk,
            plant
        ));
        attach_drain_anim(&mut sim, disk, &rules);
        let anim = assert_drain_anim(&sim, disk);
        sim.uninit_with_rules(plant, &rules);
        assert_eq!(sim.entities().get(disk).unwrap().drain.target, None);
        assert_eq!(sim.entities().get(disk).unwrap().drain.anim, None);
        assert!(anim_uninited(&sim, anim));

        // So does the drainer's.
        let plant = sim
            .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
            .expect("power plant spawns");
        assert!(install_drain_link(
            &mut sim.substrate,
            &sim.houses,
            &mut sim.power_states,
            disk,
            plant
        ));
        attach_drain_anim(&mut sim, disk, &rules);
        let anim = assert_drain_anim(&sim, disk);
        sim.uninit_with_rules(disk, &rules);
        assert_eq!(
            sim.substrate.entities.get(plant).unwrap().drain.draining_me,
            None
        );
        assert!(anim_uninited(&sim, anim));
    }

    #[test]
    fn constructed_timer_never_fires() {
        // Frames only move forward from the construction frame natively.
        let timer = CdTimer::started(7, 0);
        for frame in 7..400 {
            assert!(!produce_cash_fires(timer, frame));
        }
        // The `-1` sentinel reads the raw duration; only duration 1 fires.
        assert!(!produce_cash_fires(CdTimer::from_raw(-1, 0), 0));
        assert!(produce_cash_fires(CdTimer::from_raw(-1, 1), 123));
    }
}
