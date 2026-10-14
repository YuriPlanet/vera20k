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
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::InternedId;
#[cfg(test)]
use crate::sim::mission::MissionType;
use crate::sim::timer::CdTimer;
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
}

#[cfg(test)]
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
pub(crate) fn add_credits(sim: &mut Simulation, owner: InternedId, amount: i32) {
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.economy.add_credits(amount);
    }
}

/// [`Economy::spend_money`](crate::sim::economy::Economy::spend_money) on
/// `owner`'s wallet (`HouseClass::Spend_Money @ 0x004F9790`).
pub(crate) fn spend_money(sim: &mut Simulation, owner: InternedId, amount: i32) {
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.economy.spend_money(amount);
    }
}

/// [`Economy::available_money`](crate::sim::economy::Economy::available_money)
/// of `owner`'s wallet (the money interface's slot `+0x18`,
/// `Available_Money @ 0x004F6990`); 0 for a house that does not exist.
pub(crate) fn available_money(sim: &Simulation, owner: InternedId) -> i32 {
    sim.houses
        .get(&owner)
        .map_or(0, |house| house.economy.available_money())
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
    sim: &mut Simulation,
    stable_id: u64,
    old_owner: InternedId,
    new_owner: InternedId,
    rules: &RuleSet,
) {
    let Some(entity) = sim.substrate.entities.get(stable_id) else {
        return;
    };
    if entity.category() != EntityCategory::Structure {
        return;
    }
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return;
    };
    let old_passive = sim
        .houses
        .get(&old_owner)
        .is_some_and(|house| house.multiplay_passive);
    if !old_passive || obj.produce_cash_startup == 0 {
        return;
    }
    let startup = obj.produce_cash_startup;
    let delay = obj.produce_cash_delay;
    let frame = sim.session.binary_frame;
    add_credits(sim, new_owner, startup);
    if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
        entity.produce_cash.arm(frame, delay);
    }
}

///445FA5..445FC8: every admitted Grand_Opening rearms a nonzero delay,
/// including the already-placed capture arm. Zero leaves the prior timer.
pub(crate) fn produce_cash_on_grand_opening(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let delay = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
        .map_or(0, |object| object.produce_cash_delay);
    if delay != 0
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        entity.produce_cash.arm(sim.session.binary_frame, delay);
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
/// `0x0043FD0C`, `IsBeingWarped` `+0x271` at `0x0043FD1E`) jump past it; VERA
/// has no chrono-warp bytes on a building, so the block always runs.
/// Residual trigger: a captured derrick inside a Chronosphere warp — the
/// native timer dies on the skipped frame, VERA's keeps paying.
pub(crate) fn produce_cash_step(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get(stable_id) else {
        return;
    };
    if entity.category() != EntityCategory::Structure {
        return;
    }
    let frame = sim.session.binary_frame;
    if !produce_cash_fires(entity.produce_cash.timer(), frame) {
        return;
    }
    let owner = entity.owner();
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
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
        .is_some_and(|house| house.multiplay_passive)
    {
        return;
    }
    if sim.building_operational_state(stable_id, rules) != Some(true) {
        return;
    }
    if amount > 0 {
        add_credits(sim, owner, amount);
    } else {
        spend_money(sim, owner, amount.wrapping_neg());
    }
}

// ---------------------------------------------------------------------------
// Floating Disc money drain
// ---------------------------------------------------------------------------

/// `MapClass::Look_up_building_in_cell`-equivalent for the drain link: the
/// live structure whose foundation covers `(rx, ry)`.
pub(crate) fn building_at_cell(sim: &Simulation, rx: u16, ry: u16) -> Option<u64> {
    building_at_cell_in_store(&sim.substrate.entities, rx, ry)
}

/// [`building_at_cell`] over a bare entity store (the combat host holds no
/// `Simulation` while it applies fire emissions).
pub(crate) fn building_at_cell_in_store(entities: &EntityStore, rx: u16, ry: u16) -> Option<u64> {
    entities.values().find_map(|entity| {
        if entity.category() != EntityCategory::Structure || entity.death.is_dying() {
            return None;
        }
        let (width, height) =
            crate::rules::foundation::foundation_dimensions(entity.type_facts.foundation());
        let (bx, by) = (entity.position.rx, entity.position.ry);
        (rx >= bx && rx < bx.saturating_add(width) && ry >= by && ry < by.saturating_add(height))
            .then_some(entity.stable_id())
    })
}

/// `0x0070FD70` (DB label `BuildingClass__EnterTransport` is a misnomer; the
/// sole caller is `TechnoClass::Fire_At @ 0x006FDF8C` on the `DrainWeapon`
/// arm): when the building in the drainer's own cell is the target, set
/// `target+0x1D0 = drainer` (`DrainingMe`), `drainer+0x1CC = target`
/// (`DrainTarget`), flag the victim house's power recalc (`+0x5778`), free any
/// mind-controlled captives, create the `DrainAnim` and drop the drainer from
/// its team. This owner installs the pointers and dirties House power; the
/// captives and team belong to the caller. The reached House assessment
/// suppresses the drained output. DrainAnim remains a separate residual.
///
/// Returns whether the link was made.
pub(crate) fn install_drain_link(sim: &mut Simulation, drainer_id: u64, victim_id: u64) -> bool {
    let Some((rx, ry)) = sim
        .substrate
        .entities
        .get(drainer_id)
        .map(|entity| (entity.position.rx, entity.position.ry))
    else {
        return false;
    };
    if building_at_cell_in_store(&sim.substrate.entities, rx, ry) != Some(victim_id) {
        return false;
    }
    if let Some(victim) = sim.substrate.entities.get_mut(victim_id) {
        victim.drain.draining_me = Some(drainer_id);
    }
    if let Some(drainer) = sim.substrate.entities.get_mut(drainer_id) {
        drainer.drain.target = Some(victim_id);
    }
    if let Some(owner) = sim
        .substrate
        .entities
        .get(victim_id)
        .map(|victim| victim.owner())
    {
        //70FDAC, before capture release/animation/team callbacks.
        sim.invalidate_house_power(owner, false);
    }
    true
}

/// `0x0070FE50` (DB label `BuildingClass__ExitTransport`, misnomer): remove
/// the `DrainAnim`, clear the victim's `DrainingMe`, flag its house's power
/// recalc, clear the drainer's `DrainTarget`. Also the inline copy in
/// `TechnoClass::AI_Update @ 0x006FA1DF..0x006FA21E`.
pub(crate) fn stop_drain(sim: &mut Simulation, drainer_id: u64) {
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
        sim.invalidate_house_power(owner, false); //70FE93 /6FA217
    }
    if let Some(drainer) = sim.substrate.entities.get_mut(drainer_id) {
        drainer.drain.target = None;
    }
}

/// `TechnoClass::PointerExpired @ 0x0070785F..0x0070792A` and the death arm
/// of `TechnoClass::ReceiveDamage @ 0x0070206A..0x00702102`: an expiring
/// object drops both halves of any drain link it is part of.
pub(crate) fn clear_drain_links_on_expiry(sim: &mut Simulation, expired_id: u64) {
    let Some((drain_target, draining_me)) = sim
        .substrate
        .entities
        .get(expired_id)
        .map(|entity| (entity.drain.target, entity.drain.draining_me))
    else {
        return;
    };
    if let Some(victim_id) = drain_target
        && let Some(victim) = sim.substrate.entities.get_mut(victim_id)
        && victim.drain.draining_me == Some(expired_id)
    {
        victim.drain.draining_me = None;
        let owner = victim.owner();
        sim.invalidate_house_power(owner, false); //7020AA /70789F
    }
    if let Some(drainer_id) = draining_me
        && let Some(drainer) = sim.substrate.entities.get_mut(drainer_id)
        && drainer.drain.target == Some(expired_id)
    {
        drainer.drain.target = None;
        let owner = sim
            .substrate
            .entities
            .get(expired_id)
            .map(|entity| entity.owner());
        if let Some(owner) = owner {
            sim.invalidate_house_power(owner, false);
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
///    set and `HouseClass::IsAlliedWith(target->Owner, this)` true, stop the
///    drain inline.
pub(crate) fn drain_common_step(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get(stable_id) else {
        return;
    };
    let victim_owner = entity.owner();
    let draining_me = entity.drain.draining_me;
    let drain_target = entity.drain.target;
    let type_ref = entity.type_ref();

    if let Some(drainer_id) = draining_me
        && rules
            .object(sim.interner.resolve(type_ref))
            .is_some_and(|obj| obj.resource_destination)
    {
        let delay = rules.general.drain_money_frame_delay;
        // Native divides by the raw dword; a zero delay would fault there.
        if delay != 0
            && (sim.session.binary_frame as i32).wrapping_rem(delay) == 0
            && let Some(drainer_owner) = sim
                .substrate
                .entities
                .get(drainer_id)
                .map(|drainer| drainer.owner())
        {
            let mut amount = rules.general.drain_money_amount;
            let available = available_money(sim, victim_owner);
            if available < amount {
                amount = available;
            }
            spend_money(sim, victim_owner, amount);
            add_credits(sim, drainer_owner, amount);
        }
    }

    if let Some(victim_id) = drain_target
        && let Some(target_owner) = sim
            .substrate
            .entities
            .get(victim_id)
            .map(|victim| victim.owner())
        && crate::map::houses::are_houses_friendly(
            &sim.house_alliances,
            sim.interner.resolve(target_owner),
            sim.interner.resolve(victim_owner),
        )
    {
        stop_drain(sim, stable_id);
    }
}

/// `UnitClass::AI @ 0x007361A9..0x007361E9`: with `DrainTarget` set, on frames
/// where `g_CurrentFrameCounter % 16 == 2` (signed, `0x007361B8..0x007361C7`)
/// re-look-up the building in the unit's own cell and stop the drain when it
/// is no longer the target. (The second arm at `0x007361E9` only tidies a
/// `DrainAnim` left behind by a cleared target; there is no anim here.)
pub(crate) fn drain_unit_ai_step(sim: &mut Simulation, stable_id: u64) {
    let Some((target, rx, ry)) = sim.substrate.entities.get(stable_id).and_then(|entity| {
        entity
            .drain
            .target
            .map(|target| (target, entity.position.rx, entity.position.ry))
    }) else {
        return;
    };
    if (sim.session.binary_frame as i32).wrapping_rem(16) != 2 {
        return;
    }
    if building_at_cell(sim, rx, ry) != Some(target) {
        stop_drain(sim, stable_id);
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
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(
            "[General]\nDrainMoneyFrameDelay=30\nDrainMoneyAmount=30\n\n\
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
        ), &IniFile::from_str("[CAOILD]\nFoundation=2x2\n[CAOILDP]\nFoundation=2x2\n[GAPOWR]\nFoundation=2x2\n[NAREFN]\nFoundation=2x2\n"))
        .expect("credit-income test rules parse")
    }

    fn cmd(sim: &Simulation, owner: &str, tick: u64, payload: Command) -> CommandEnvelope {
        let owner_id = sim.interner.get(owner).expect("owner interned");
        CommandEnvelope::new(owner_id, tick, payload)
    }

    fn house(sim: &mut Simulation, name: &str, human: bool, credits: i32) -> InternedId {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, 0, None, human, credits, 10));
        sim.session.house_order.push(id);
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
        let (old_rx, old_ry) = (entity.position.rx, entity.position.ry);
        entity.position.rx = rx;
        entity.position.ry = ry;
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
        sim.houses[&owner].economy.credits()
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
        sim.houses.get_mut(&neutral).unwrap().multiplay_passive = true;
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
        assert_eq!(armed.start_frame(), sim.session.binary_frame as i32);

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
                payments.push((tick, sim.session.binary_frame));
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
            sim.session.binary_frame = 99;
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

    #[test]
    fn derrick_stops_paying_while_not_operational() {
        let rules = rules();
        let mut sim = Simulation::with_seed(0x0011_0902);
        let neutral = house(&mut sim, "Neutral", false, 0);
        sim.houses.get_mut(&neutral).unwrap().multiplay_passive = true;
        let americans = house(&mut sim, "Americans", true, 0);
        let oil = sim
            .spawn_object("CAOILDP", "Neutral", 10, 10, 0, &rules)
            .expect("powered derrick spawns");
        sim.change_owner_with_rules(
            oil,
            americans,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(credits(&sim, americans), 1_000);
        // No power plant: output 0 < drain 10 → low power → not operational.
        run_ticks(&mut sim, &rules, 300);
        assert!(
            sim.power_states[&americans].is_low_power,
            "fixture must be in low power"
        );
        assert_eq!(
            credits(&sim, americans),
            1_000,
            "no ProduceCashAmount while offline"
        );
        // The timer still re-armed on every expiry (the re-arm precedes the gate).
        let timer = sim.substrate.entities.get(oil).unwrap().produce_cash.timer;
        assert_eq!(timer.duration(), 100);
        assert!(
            (sim.session.binary_frame as i32).wrapping_sub(timer.start_frame()) < 100,
            "timer keeps cycling while offline"
        );
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
        assert_eq!(building_at_cell(&sim, 21, 21), Some(refinery));
        assert!(install_drain_link(&mut sim, disk, refinery));
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
        assert!(install_drain_link(&mut sim, disk, plant));
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
        assert!(!disc.passively_acquired_target);
        assert_eq!(
            sim.substrate
                .entities
                .get(refinery)
                .unwrap()
                .drain
                .draining_me,
            Some(disk)
        );

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
        assert!(
            credits(&sim, americans) < 5_000,
            "the link keeps draining after the mission drops"
        );
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
        assert!(!install_drain_link(&mut sim, disk, plant));
        assert_eq!(
            sim.substrate.entities.get(plant).unwrap().drain.draining_me,
            None
        );

        move_disc(&mut sim, disk, 20, 21);
        assert!(install_drain_link(&mut sim, disk, plant));
        run_ticks(&mut sim, &rules, 40);
        assert_eq!(
            sim.substrate.entities.get(disk).unwrap().drain.target,
            Some(plant)
        );

        move_disc(&mut sim, disk, 30, 21);
        run_ticks(&mut sim, &rules, 17);
        assert_eq!(sim.substrate.entities.get(disk).unwrap().drain.target, None);
        assert_eq!(
            sim.substrate.entities.get(plant).unwrap().drain.draining_me,
            None
        );

        // Pointer expiry drops both halves.
        move_disc(&mut sim, disk, 20, 21);
        assert!(install_drain_link(&mut sim, disk, plant));
        sim.uninit_with_rules(disk, &rules);
        assert_eq!(
            sim.substrate.entities.get(plant).unwrap().drain.draining_me,
            None
        );
    }

    #[test]
    fn produce_cash_timer_fires_once_at_delay_minus_one_then_needs_rearm() {
        let timer = CdTimer::started(100, 100);
        for frame in 100..199 {
            assert!(
                !produce_cash_fires(timer, frame),
                "frame {frame} must not fire"
            );
        }
        assert!(produce_cash_fires(timer, 199));
        // Past its duration the native timer is dead: `elapsed >= duration`
        // jumps over the block until something re-arms it.
        assert!(!produce_cash_fires(timer, 200));
        assert!(!produce_cash_fires(timer, 5_000));
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
