//! `TemporalClass`: the Chrono Legionnaire's erase beam (and the IFV's chrono
//! gunner weapon).
//!
//! Native owner: one `TemporalClass` (`operator new(0x50)`, vtable
//! `0x007F5180`) per Techno whose rookie weapon 0 warhead is `Temporal=`
//! (`TechnoClass::Init_Managers @ 0x006F4154..0x006F41BD`, `TechnoClass+0x274`
//! TemporalImUsing). The firer's Inviso shot reaches the Temporal arm of
//! `BulletClass::DetonateAtCoord` (`0x00469423..0x004694C6`), which calls
//! [`Simulation::temporal_initiate_warp`] on the bullet's target. The TARGET
//! owns the tick: each class's AI calls its chain head's `Update` (vtable
//! `+0x5C`) first (Unit `0x00736204`, Infantry `0x0051BB6E`, Aircraft
//! `0x00414BDB`, Building `0x0043FCF9`) and then, while warped, returns
//! before FootClass::AI or TechnoClass::AI_Update: none of its missions,
//! managers, repair, stance, deploy or animation work runs. VERA runs parts of
//! that AI in their own frame phases, and each consults
//! [`GameEntity::ai_frozen`]: idle actions, fear, deploy, building repair and
//! the AI's low-credit sale, depot service, spawn managers, boarding and
//! unloading, construction, facing and turrets, the sprite stage, the legacy
//! order intents, engineer, bridge-repair and C4 orders, gates, aircraft docks
//! and bunker installs. The head subtracts its own weapon's `Damage=` and every
//! chained attacker's from `WarpRemaining`, which the first attacker set to the
//! target's `Strength=` times ten; at zero the target is erased: `WarpAway=`,
//! no death, no survivors.
//!
//! State: the firer holds its link ([`TemporalState`]'s `link`, `+0x274`:
//! target `+0x28`, the chain's `+0x40` Prev and `+0x44` Next, `+0x48`
//! WarpRemaining); the target holds only the chain head (`head`, `+0x278`),
//! and the temporal half of `+0x270` BeingWarpedOut is exactly
//! `head.is_some()`. Attacker entity ids stand for their TemporalClass. Only
//! this module writes either.
//!
//! Releases (`TemporalClass::LetGo @ 0x0071ABC0`): the attacker retargets
//! (`0x0071AF61`); a new warp's victim lets its own victim go (`0x0071B162`);
//! the attacker enters a new cell (the PerCellProcess tail `0x006F5090` at
//! `0x006F50B4`), enters idle mode (`TechnoClass::Enter_Idle_Mode @
//! 0x00709A54`, reached from the Unit and Infantry leaves through
//! `0x004D82D9` and from the Aircraft leaf only past its mission checks,
//! `0x00417782`; the Attack mission's idle exit after a Stop is one such
//! entry), expires (`0x0071AB7A`), changes house through CaptureManager
//! (`0x004723F3`), strays beyond `OpenToppedWarpDistance=` from an open
//! transport (`0x0071A841`), or leaves an IFV's gunner seat (`0x00746557`).
//! Nothing heals: the victim resumes at full health and a later attacker
//! starts again at `Strength * 10`.
//!
//! Scenario draws: none in the Temporal functions themselves (InitiateWarp,
//! CanWarpTarget, Update, SumChainDamage, LetGo, ClearLinkedList and the
//! pointer-expiry forward call no RNG). A warp's start can draw through the
//! spawns it kills and the captives it frees, its erase through the target's
//! UnInit and the attackers' idle selection.
//!
//! Evidence: control flow and ordering read from the disassembly at the
//! addresses cited here. Native execution, compared in the `native_` tests:
//! `tools/spatial_oracle/temporal_update.py` runs the original Update with
//! SumChainDamage, LetGo, ClearLinkedList, Sqrt_Approx and ftol (the step, the
//! chain sum and its depth cap, the erase test and its callees in order, the
//! no-target erase, the corrupt-head release, the open-topped release
//! boundary); `tools/spatial_oracle/temporal_initiate_warp.py` runs the
//! original InitiateWarp with CanWarpTarget, LetGo and Contact_With_Whom
//! (`Strength * 10` and its wrap, the insert after the head, the refusals, the
//! releases, the notices). Read, not executed: the per-class prologues and
//! the sparkle cadence, the pointer-expiry forward, the gunner hand-over and
//! the release sites. Retail data: `[ChronoBeam]
//! Temporal=yes` on `[NeutronRifle]` (8), `[NeutronRifleE]` (16) and the IFV's
//! `[CRNeutronRifle]` (5), all Inviso; `[General] WarpAway=`,
//! `ChronoSparkle1=`; `[CombatDamage] OpenToppedWarpDistance=`.
//!
//! RESIDUALS:
//! - TeleportLocomotionClass also writes `+0x270` (warp-out start
//!   `0x007197D0`) and owns `+0x271`. [`GameEntity::is_warped_out`] folds it in
//!   for every reader, but the per-class AI prologue (the sparkle and the
//!   frozen AI, Unit `0x00736217..0x0073634D` and its Infantry, Aircraft and
//!   Building twins) and the phase gates ([`GameEntity::ai_frozen`]) cover the
//!   temporal writer only: VERA's teleport state machine keeps its own
//!   destination, which the frozen branch's `Set_Destination(0, 1)` would
//!   cancel. Trigger: a Chrono Legionnaire or Chrono Miner teleporting.
//!   Effect: no ChronoSparkle1 during the teleport and the object's own AI
//!   keeps running. Frequency: every Chrono teleport.
//! - A warped object's locomotor processes only when `+0x271`, or `+0x270`
//!   with `+0x27C` (written 1 by `SuperClass::Launch @ 0x006CCC3D` and two
//!   unnamed Foot sites `0x004DF9EA`/`0x005231C1`), is set; VERA has no
//!   `+0x27C` and never processes a temporal victim's locomotor. Trigger: a
//!   Chronosphere launch on an object being erased. Effect: none observed.
//! - The target's gattling spin-down at warp start (`0x0070E000(1)`, gated on
//!   `IsGattling=`): VERA has no gattling stages.
//! - `Mark(2)` at warp start and release (vtable `+0x124`) and the building's
//!   paused animation slots (`0x004521C0`/`0x00452210` pause and resume the 21
//!   slots) are presentation; the online latch they share is
//!   [`GameEntity::building_online`].
//! - House `+0x1FC` (set at a building's warp start, release and erase; read
//!   for the player house at `0x004F926C`) is not identified.
//! - The online latch's readers VERA wires are Is_Operational, power drain,
//!   radar, the refinery's and an absorber's CanEnter (`0x0043C422`) and the
//!   depot probe (`0x0043C7FB`). Not wired:
//!   - `TechnoTypeClass::FindFactory @ 0x005F7900` with its online argument
//!     (`(1,1,1)`, `production_tech::revalidate_eligibility`): production of
//!     a category whose every factory is warped suspends natively; VERA's
//!     `BuildEligibility::TemporarilyBlocked` seam has no consumer. Trigger:
//!     a warp on a house's only factory of a kind. Effect: VERA keeps
//!     producing during the warp.
//!   - `HouseClass::CanBuild`'s upgrade-prerequisite scan
//!     (`0x004F7DE6..0x004F7E4E`: an upgrade prerequisite counts only on an
//!     online, unsold host; plain prerequisites use the house counters), the
//!     AI's AI_ManageProduction (`0x0050B020`), CheckDockArrayOccupancy
//!     (`0x0044E855`) and PowerCheck_Upgrade (`0x00450605`). Effect: an
//!     option enabled by a warped upgrade host stays available.
//!   - The player-only BuildingClass virtual `+0x4E0` (`0x004456D0`,
//!     unidentified) and the sensor-range circle (`0x00456750`,
//!     presentation). `0x0044017E` lies past BuildingClass::Update's frozen
//!     jump, so no warp reaches it.
//! - A building's erase kills its garrison through `0x004585C0(0)` and deletes
//!   absorbed passengers outright before Record_The_Kill; VERA's carrier
//!   UnInit purges both inside the building's UnInit (each at health 0 with no
//!   killer, booking a loss). Draws inside `0x004585C0` are not established.
//! - SlaveManager release with the attacker as liberator (`0x006B0AE0`):
//!   VERA models no slave release on any master death.
//! - ReceiveGunner's weapon-timer hand-over (`0x0074646E..0x007464B8`, and the
//!   mirror in RemoveGunner) is not ported; only the TemporalClass moves.
//! - `WarpPerStep` (`+0x4C`, written each step, read by no sim function) and
//!   the `+0x2C` timer and `+0x38`/`+0x3C` fields (no writer) are not kept.
//! - VERA fires in the combat phase after the live-object pass, so a warp
//!   started this frame takes its first step on the target's next AI visit,
//!   where native steps a target later in the logic vector the same frame.
//!   Trigger: a warp start on a target after its attacker in the vector.
//!   Effect: the erase lands one frame late, the target gets one more
//!   unfrozen AI and locomotor turn on the shot's frame, and the power and
//!   online recomputation follows a frame later.
//! - VERA's Stop assigns a Stop mission where the native IDLE event
//!   (`0x004C74CB..0x004C76BB`) assigns none, so a legionnaire on Attack lets
//!   go with the event instead of on its Attack mission's next dispatch (up
//!   to one Attack cadence later); on any other mission the beam holds, as
//!   natively.
//! - An engineer turned away from a warped building (`0x00519EF2`) is not
//!   given `Set_Destination(0, 1)` and the scatter (`vtable+0x174`): VERA's
//!   adjacent capture simply waits and captures on release. The Selling arm
//!   before it (`0x00519EB2`) has no VERA state: a sale completes at once.
//! - `UnitClass::Receive_Radio` answers WANT_RIDE (`0x24`) with 0 while
//!   warped (`0x0073745C..0x00737473`); the message is dormant in stock YR
//!   and not represented.
//! - A transport's death or grinding ejects its first passenger through
//!   RemoveGunner (`0x00737FD4`, `0x0073A0C8`/`0x0073A0DF`); VERA's passenger
//!   escape residual (`crew_survival`) covers those paths.
//! - Open-topped passengers do not fire yet, so the open-topped release
//!   (`0x0071A841`) has no production producer; the corpus pins its
//!   boundary.
//! - The cursor readers of `+0x270` (What_Action: Techno `0x006FFF07`,
//!   `0x007005CD`, `0x00700731`; Infantry `0x0051E59B`; Unit `0x00740165`,
//!   `0x007402CA`) belong to the app; the sim accepts an Attack order on a
//!   warped target that the native cursor would not offer, and fire
//!   admission drops it.
//! - Voxel animation and harvest-overlay frames keep stepping for a warped
//!   object; their native owners are not established (the sprite stage steps
//!   in TechnoClass::AI_Update at `0x006FAC4D` and holds).

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::{Simulation, UninitContext};

/// `SumChainDamage @ 0x0071AB10` recurses while the depth is at most this
/// (`0x0071AB22 CMP EAX,0x32; JG`).
const CHAIN_DEPTH_LIMIT: i32 = 0x32;
/// `AnimClass(type, coord, 0, 1, 0x600, 0, 0)` for WarpAway and the sparkle.
const TEMPORAL_ANIM_DRAW_FLAGS: u32 = 0x600;
/// The sparkle's frame phase: `frame % 24 == 0` (`0x0073622F..0x0073623E`).
const SPARKLE_PERIOD_FRAMES: i32 = 24;
/// A Foot's sparkle sits 0x78 leptons east and south of its Location
/// (`0x00736274`, `0x00736280`).
const SPARKLE_OFFSET_LEPTONS: i32 = 0x78;
/// An occupied building's port sparkle ZAdjust (`anim+0x100 = -200`,
/// `0x004404CF`).
const PORT_SPARKLE_Z_ADJUST: i32 = -200;
/// A building's `vtable+0xAC` (`0x00459EF0`): its Location less 128 leptons on
/// X and Y.
const BUILDING_RENDER_SHIFT_LEPTONS: i32 = 128;

/// One `TemporalClass`, owned by the attacker that fires it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TemporalLink {
    /// `+0x28` the object being warped.
    target: Option<u64>,
    /// `+0x40` the attacker whose link precedes this one on the target.
    prev: Option<u64>,
    /// `+0x44` the attacker whose link follows.
    next: Option<u64>,
    /// `+0x48` the warp left; read on the chain head only.
    warp_remaining: i32,
}

/// An object's Temporal state: its own link when it fires a Temporal weapon,
/// and the head of the chain warping it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TemporalState {
    /// `TechnoClass+0x274` TemporalImUsing.
    link: Option<TemporalLink>,
    /// `TechnoClass+0x278` TemporalTargetingMe: the chain head's attacker.
    head: Option<u64>,
}

impl TemporalState {
    /// The temporal half of `TechnoClass+0x270` BeingWarpedOut.
    pub fn is_warped(&self) -> bool {
        self.head.is_some()
    }

    /// `TechnoClass+0x278`: the attacker heading the chain warping this
    /// object.
    pub(crate) fn chain_head(&self) -> Option<u64> {
        self.head
    }

    /// `vtable+0x1DC` (`0x0070C5D0`): a link that holds a target.
    pub fn is_warping_someone(&self) -> bool {
        self.warp_target().is_some()
    }

    /// The object this one is warping.
    pub fn warp_target(&self) -> Option<u64> {
        self.link.as_ref().and_then(|link| link.target)
    }

    /// `TechnoClass+0x274` is set.
    pub fn has_link(&self) -> bool {
        self.link.is_some()
    }

    /// Every object id this state names, for the snapshot reference check.
    pub(crate) fn references(&self) -> impl Iterator<Item = u64> + '_ {
        self.head.into_iter().chain(
            self.link
                .iter()
                .flat_map(|link| link.target.into_iter().chain(link.prev).chain(link.next)),
        )
    }

    /// A target warped by `head`, for fixtures outside this module.
    #[cfg(test)]
    pub(crate) fn warped_by_for_test(head: u64) -> Self {
        Self {
            link: None,
            head: Some(head),
        }
    }
}

impl GameEntity {
    /// `TechnoClass+0x270` BeingWarpedOut (vtable `+0x1D4`, `0x0070C5B0`):
    /// written by TemporalClass and by TeleportLocomotionClass's warp-out.
    pub fn is_warped_out(&self) -> bool {
        self.temporal.is_warped()
            || self
                .teleport_state
                .as_ref()
                .is_some_and(|teleport| teleport.warp_out_active())
    }

    /// The frozen branch of every class's AI while a Temporal chain warps the
    /// object: the leaf returns (Unit `0x0073635A`, Infantry `0x0051BC17`,
    /// Aircraft `0x00414D2B`, Building `0x0043FD14` -> `0x0044057A`) before
    /// FootClass::AI (`0x0073647B`, `0x0051BC9F`, `0x00414DA3`) or
    /// TechnoClass::AI_Update (`0x0043FE56`), so none of its missions,
    /// managers, repair, stance, deploy or animation work runs. VERA runs
    /// parts of that AI in their own frame phases; each consults this.
    pub fn ai_frozen(&self) -> bool {
        self.temporal.is_warped()
    }

    /// `TechnoClass+0x271` (vtable `+0x1D8`, `0x0070C5C0`): the teleport's
    /// warp-in.
    pub fn is_warping_in(&self) -> bool {
        self.teleport_state
            .as_ref()
            .is_some_and(|teleport| teleport.warp_in_active())
    }

    /// `BuildingClass+0x660`, the online latch: cleared at a warp's start
    /// (`0x004521C0`, reached from InitiateWarp for a building) and set at its
    /// release (`0x00452210`, from LetGo). Read by `Is_Operational @
    /// 0x004555D0`, GetPowerDrain `0x0044E88F` and the radar scan `0x00508EA3`.
    /// VERA represents only this writer: the player and trigger power toggle
    /// (`GoOffline @ 0x00452360`, `GoOnline @ 0x00452260`) is not ported, and
    /// a corrupt-head release (ClearLinkedList) that natively leaves the latch
    /// cleared cannot arise here.
    pub fn building_online(&self) -> bool {
        !self.temporal.is_warped()
    }
}

/// `TechnoClass::Init_Managers @ 0x006F4154..0x006F41BD`: a link for every
/// class whose rookie `GetWeapon(0)` warhead is Temporal (the managers are
/// built before a map reader sets veterancy, as for the CaptureManager).
pub(crate) fn init_temporal(object_type: &ObjectType, rules: &RuleSet) -> TemporalState {
    let temporal = crate::sim::combat::combat_weapon::weapon_for_index(object_type, 0, 0)
        .and_then(|(weapon_name, _)| rules.weapon(weapon_name))
        .and_then(|weapon| weapon.warhead.as_deref())
        .and_then(|warhead| rules.warhead(warhead))
        .is_some_and(|warhead| warhead.temporal);
    TemporalState {
        link: temporal.then(TemporalLink::default),
        head: None,
    }
}

/// The bullet's target as the Temporal arm reads it (`bullet+0x10C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporalShotTarget {
    /// No target: the arm ends before InitiateWarp.
    None,
    /// A cell: InitiateWarp runs with no Techno, which only drops the
    /// firer's current victim.
    Cell,
    /// A Techno.
    Object(u64),
}

impl Simulation {
    fn temporal_link(&self, attacker: u64) -> Option<&TemporalLink> {
        self.substrate
            .entities
            .get(attacker)
            .and_then(|entity| entity.temporal.link.as_ref())
    }

    fn temporal_link_mut(&mut self, attacker: u64) -> Option<&mut TemporalLink> {
        self.substrate
            .entities
            .get_mut(attacker)
            .and_then(|entity| entity.temporal.link.as_mut())
    }

    fn set_temporal_head(&mut self, target: u64, head: Option<u64>) {
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.temporal.head = head;
        }
    }

    /// The Temporal arm of `BulletClass::DetonateAtCoord` (`0x00469423..
    /// 0x004694C6`): no firer or no target ends it; a Foot outside the ground
    /// layer is not warped (`0x0046946C`, InWhichLayer `!= 2`); a Unit in a
    /// Tank Bunker hands the firer its bunker, which the firer retargets and
    /// warps instead (`0x0046946B..0x004694A5`).
    ///
    /// RESIDUALS:
    /// - The layer test forwards to the Foot's locomotor (`vt+0x78` ->
    ///   locomotor `+0x74`), which VERA does not model; `IsHighFlying` stands
    ///   in, as for GetFireError's Foot-layer gate (`combat_weapon`). Trigger:
    ///   a Rocketeer or Kirov just after lift-off. Effect: VERA warps it where
    ///   native misses. Frequency: brief windows per flight.
    /// - `0x004694C1` calls through `+0x274` with no null test, so a Temporal
    ///   warhead on a firer without a link would crash natively; no stock
    ///   firer lacks one, and VERA does nothing.
    pub(crate) fn temporal_detonation(
        &mut self,
        firer: u64,
        target: TemporalShotTarget,
        rules: &RuleSet,
    ) {
        if !self.substrate.entities.contains(firer) {
            return;
        }
        let victim = match target {
            TemporalShotTarget::None => return,
            TemporalShotTarget::Cell => None,
            TemporalShotTarget::Object(id) => {
                let Some(entity) = self.substrate.entities.get(id) else {
                    return;
                };
                let foot = matches!(
                    entity.category,
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                );
                if foot && crate::sim::combat::combat_weapon::target_is_high_flying(entity) {
                    return;
                }
                match entity.bunker_link.installed_in() {
                    Some(bunker) if entity.category == EntityCategory::Unit => {
                        // vtable +0x3C8 Assign_Target(bunker) on the firer.
                        let requested = Some(crate::sim::combat::TargetKind::Entity(bunker));
                        let commits = crate::sim::mission::concrete_effects::assign_target_commits(
                            &self.substrate.entities,
                            requested,
                        );
                        if let Some(firer_entity) = self.substrate.entities.get_mut(firer) {
                            crate::sim::mission::concrete_effects::represented_assign_target_admitted(
                                firer_entity,
                                requested,
                                commits,
                            );
                        }
                        Some(bunker)
                    }
                    _ => Some(id),
                }
            }
        };
        if self.temporal_link(firer).is_none() {
            return;
        }
        self.temporal_initiate_warp(firer, victim, rules);
    }

    /// `TemporalClass::InitiateWarp @ 0x0071AF20`.
    pub(crate) fn temporal_initiate_warp(
        &mut self,
        attacker: u64,
        target: Option<u64>,
        rules: &RuleSet,
    ) {
        if let Some(target) = target {
            // 0x0071AF2F..0x0071AF48: the target's spawns die and its captives
            // go free before anything else is decided.
            if self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| entity.spawn_manager.is_some())
            {
                crate::sim::spawn_manager::kill_all_spawns_with_context(
                    self,
                    target,
                    UninitContext::with_rules(rules),
                );
            }
            if self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| entity.capture_manager.is_some())
            {
                self.free_all_captures(target, rules);
            }
        }
        // 0x0071AF4D..0x0071AF61: the attacker drops its previous victim.
        self.temporal_release_if_warping(attacker);
        let Some(target) = target else {
            return;
        };
        if !self.can_warp_target(target, rules) {
            return;
        }
        // 0x0071AF76..0x0071AF81: an attacker being warped cannot start.
        if self
            .substrate
            .entities
            .get(attacker)
            .is_none_or(|entity| entity.temporal.head.is_some())
        {
            return;
        }
        let Some((existing_head, strength)) = self.substrate.entities.get(target).map(|entity| {
            (
                entity.temporal.head,
                self.object_type(entity.type_ref(), rules)
                    .map_or(0, |object| object.strength),
            )
        }) else {
            return;
        };
        if let Some(link) = self.temporal_link_mut(attacker) {
            link.target = Some(target);
        }
        match existing_head {
            None => {
                // 0x0071AF98..0x0071AFB9: the first attacker heads the chain
                // with Strength * 10 (`LEA x5; SHL 1`).
                self.set_temporal_head(target, Some(attacker));
                if let Some(link) = self.temporal_link_mut(attacker) {
                    link.warp_remaining = strength.wrapping_mul(10);
                }
                self.temporal_warp_start_notice(target, rules);
            }
            Some(head) => {
                // 0x0071B0D1..0x0071B0E4: insert right after the head.
                let head_next = self.temporal_link(head).and_then(|link| link.next);
                if let Some(link) = self.temporal_link_mut(attacker) {
                    link.prev = Some(head);
                    link.next = head_next;
                }
                if let Some(link) = self.temporal_link_mut(head) {
                    link.next = Some(attacker);
                }
                if let Some(next) = head_next
                    && let Some(link) = self.temporal_link_mut(next)
                {
                    link.prev = Some(attacker);
                }
            }
        }
        // 0x0071B0EA sets `+0x270`, which the head above stands for; a
        // building's online latch follows it (`0x004521C0`).
        // 0x0071B14E..0x0071B162: a victim that was itself warping lets go.
        self.temporal_release_if_warping(target);
        // 0x0071B16D: ObjectClass::Deselect.
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.selected = false;
        }
    }

    /// The first attacker's notice (`0x0071AFBC..0x0071B0CF`): a harvester
    /// raises the ore-miner alert (`CreateRadarEvent(4)` and the EVA, both
    /// gated on the player's own house, which the app applies); a building
    /// that is not `Insignificant=` and not a 1x1 undeployer (`vtable+0x80`,
    /// `0x00465D40`, as on the damage path) notifies its house it is under
    /// attack.
    fn temporal_warp_start_notice(&mut self, target: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(target) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let structure = match entity.category {
            EntityCategory::Unit if object.harvester => false,
            EntityCategory::Structure
                if !object.insignificant && !object.is_1x1_with_undeploy() =>
            {
                true
            }
            _ => return,
        };
        let event = crate::sim::combat::UnderAttackEvent {
            rx: entity.position.rx,
            ry: entity.position.ry,
            owner: entity.owner(),
            miner: !structure,
            structure,
        };
        self.dispatch_under_attack_events(&[event]);
    }

    /// `TemporalClass::CanWarpTarget @ 0x0071AE50`: a `Warpable=` type, not
    /// under the Iron Curtain or a Force Shield (vtable `+0x160`), and not a
    /// Unit still standing in the war factory its radio contact slot 0 names
    /// (`0x0065AD30(0)`: the slot itself, not the first filled one).
    fn can_warp_target(&self, target: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(target) else {
            return false;
        };
        if !self
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.warpable)
        {
            return false;
        }
        if crate::sim::superweapon::invulnerability::is_invulnerable(
            entity.invulnerability.as_ref(),
            self.session.binary_frame,
        ) {
            return false;
        }
        if entity.category == EntityCategory::Unit
            && let Some(contact) = entity.radio_contacts.slot(0)
            && let Some(factory) = self.substrate.entities.get(contact)
            && factory.category == EntityCategory::Structure
            && self
                .object_type(factory.type_ref(), rules)
                .is_some_and(|object| object.weapons_factory)
            && crate::sim::credit_income::building_at_cell(
                self,
                entity.position.rx,
                entity.position.ry,
            ) == Some(contact)
        {
            return false;
        }
        true
    }

    /// `TemporalClass::Update @ 0x0071A760`, run for `head` by its target.
    fn temporal_update_head(&mut self, head: u64, rules: &RuleSet) {
        let Some(link) = self.temporal_link(head).cloned() else {
            return;
        };
        // 0x0071A76B..0x0071A790: a head with a predecessor releases everyone.
        if let Some(target) = link.target
            && link.prev.is_some()
            && self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| entity.temporal.head == Some(head))
        {
            self.set_temporal_head(target, None);
            self.temporal_clear_linked_list(head, rules);
            return;
        }
        // 0x0071A79D..0x0071A846: from an open transport, a target beyond
        // OpenToppedWarpDistance cells lets go.
        if let Some(target) = link.target
            && self.in_open_transport(head, rules)
            && let (Some(owner), Some(victim)) =
                (self.anim_owner_coords(head), self.anim_owner_coords(target))
        {
            let distance = crate::util::native_x87::distance_3d_leptons(
                [victim.x, victim.y, victim.z],
                [owner.x, owner.y, owner.z],
            );
            if distance
                > rules
                    .combat_damage
                    .open_topped_warp_distance
                    .wrapping_mul(256)
            {
                self.temporal_let_go(head);
                return;
            }
        }
        // 0x0071A84E..0x0071A88A: the chain first, then the head's own weapon.
        let chain = link
            .next
            .map_or(0, |next| self.temporal_chain_damage(next, 1, rules));
        let damage = self.temporal_owner_damage(head, rules);
        let remaining = link.warp_remaining.wrapping_sub(damage.wrapping_add(chain));
        if let Some(stored) = self.temporal_link_mut(head) {
            stored.warp_remaining = remaining;
        }
        if remaining > 0 {
            return;
        }
        self.temporal_erase(head, link.target, rules);
    }

    /// `SumChainDamage @ 0x0071AB10`: this attacker's damage plus the chain
    /// below it, recursing while the depth is at most [`CHAIN_DEPTH_LIMIT`].
    fn temporal_chain_damage(&self, attacker: u64, depth: i32, rules: &RuleSet) -> i32 {
        let rest = match self.temporal_link(attacker).and_then(|link| link.next) {
            Some(next) if depth <= CHAIN_DEPTH_LIMIT => {
                self.temporal_chain_damage(next, depth.wrapping_add(1), rules)
            }
            _ => 0,
        };
        self.temporal_owner_damage(attacker, rules)
            .wrapping_add(rest)
    }

    /// `Owner->GetWeapon(Owner->SelectWeapon(0))->Damage` (vtable `+0x2E4`
    /// then `+0x3F8`, `0x0071A860..0x0071A87B`): the current weapon's raw
    /// `Damage=`, the elite weapon when elite, with no firepower bonus, armor
    /// or Verses.
    fn temporal_owner_damage(&self, attacker: u64, rules: &RuleSet) -> i32 {
        let Some(entity) = self.substrate.entities.get(attacker) else {
            return 0;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return 0;
        };
        let facts = crate::sim::combat::combat_weapon::attacker_facts(entity, object);
        let index = crate::sim::combat::combat_weapon::what_weapon_should_i_use(
            rules, object, &facts, None,
        );
        crate::sim::combat::combat_weapon::weapon_for_index(object, entity.veterancy, index)
            .and_then(|(weapon, _)| rules.weapon(weapon))
            .map_or(0, |weapon| weapon.damage)
    }

    /// `TechnoClass+0x82` InOpenTransport: inside an `OpenTopped=` transport.
    fn in_open_transport(&self, attacker: u64, rules: &RuleSet) -> bool {
        self.substrate
            .entities
            .get(attacker)
            .and_then(|entity| entity.passenger_role.inside_transport_id())
            .and_then(|transport| self.substrate.entities.get(transport))
            .and_then(|transport| self.object_type(transport.type_ref(), rules))
            .is_some_and(|object| object.open_topped)
    }

    /// The erase (`0x0071A895..0x0071AB02`).
    fn temporal_erase(&mut self, head: u64, target: Option<u64>, rules: &RuleSet) {
        let Some(target) = target.filter(|&target| self.substrate.entities.contains(target)) else {
            // 0x0071A895..0x0071A8B5: no target — clear and idle; then the
            // `0x0071A90E` retest jumps to the common tail, which clears and
            // idles again (`0x0071AAE7..0x0071AB02`).
            self.temporal_clear_fields(head);
            self.temporal_owner_idle(head, rules);
            self.temporal_clear_fields(head);
            self.temporal_owner_idle(head, rules);
            return;
        };
        // 0x0071A8BD..0x0071A909: WarpAway at the target's Location.
        if let Some(entity) = self.substrate.entities.get(target) {
            let location = location_coord(entity);
            self.spawn_temporal_anim(&rules.general.warp_away.name, location, None, rules);
        }
        // 0x0071A917..0x0071A978: a Trainable attacker gains
        // `VeterancyStruct::Add(ownerCost, targetCost)` here, and again in
        // Record_The_Kill below.
        self.temporal_veterancy(head, target, rules);
        let Some(category) = self
            .substrate
            .entities
            .get(target)
            .map(|entity| entity.category)
        else {
            return;
        };
        let killer_owner = self.substrate.entities.get(head).map(GameEntity::owner);
        if category == EntityCategory::Structure {
            // 0x0071A9EE..0x0071AA15: a Tank Bunker releases its occupant
            // (`0x004593A0`). Garrison occupants and absorbed passengers go
            // in the UnInit below (module residual).
            crate::sim::docking::bunker_link::release_sell_destroy(self, target);
        } else if let Some(entity) = self.substrate.entities.get(target)
            && let Some(object) = self.object_type(entity.type_ref(), rules)
            && let Some(event) = crate::sim::combat::death_announcement_event(
                object,
                category,
                entity.position.rx,
                entity.position.ry,
                entity.owner(),
            )
        {
            // 0x0071AAB5 vtable +0x3B8: Death_Announcement. A bunkered unit's
            // own release (`0x00459470`) is left to UnInit: the detonation
            // redirects to the bunker, so no stock warp holds one.
            self.dispatch_unit_lost_events(&[event]);
        }
        // vtable +0xE0 Record_The_Kill(Owner): experience, kill and score.
        if let Some(victim) = self.substrate.entities.get_mut(target) {
            crate::sim::combat::record_kill_credit(victim, killer_owner, rules, &self.interner);
        }
        crate::sim::combat::award_kill_experience(
            &mut self.substrate.entities,
            rules,
            &self.interner,
            &self.house_alliances,
            head,
            target,
        );
        // vtable +0xF8 UnInit: no death effects, no survivors.
        self.uninit_with_context(target, UninitContext::with_rules(rules));
        // 0x0071AAD5..0x0071AB02: idle, clear, idle again (the UnInit's
        // pointer expiry already cleared the link and idled once).
        self.temporal_owner_idle(head, rules);
        self.temporal_clear_fields(head);
        self.temporal_owner_idle(head, rules);
    }

    /// `VeterancyStruct::Add(OwnerType cost, TargetType cost)` for a
    /// `Trainable=` attacker (`0x0071A917..0x0071A978`).
    ///
    /// RESIDUAL: both costs are `TechnoTypeClass::vt+0x84` evaluated with the
    /// owning house, as in Record_The_Kill; VERA reads the bare `Cost=`
    /// (equal for stock countries).
    fn temporal_veterancy(&mut self, attacker: u64, target: u64, rules: &RuleSet) {
        let (Some(attacker_entity), Some(target_entity)) = (
            self.substrate.entities.get(attacker),
            self.substrate.entities.get(target),
        ) else {
            return;
        };
        let (Some(owner_type), Some(target_type)) = (
            self.object_type(attacker_entity.type_ref(), rules),
            self.object_type(target_entity.type_ref(), rules),
        ) else {
            return;
        };
        if !owner_type.trainable {
            return;
        }
        let (owner_cost, target_cost) = (owner_type.cost, target_type.cost);
        if let Some(attacker_entity) = self.substrate.entities.get_mut(attacker) {
            crate::sim::combat::veterancy::award_kill(
                attacker_entity,
                owner_cost,
                target_cost,
                true,
                rules.general.veteran_ratio,
                rules.general.veteran_cap,
            );
        }
    }

    /// `Enter_Idle_Mode(0, 1)` (vtable `+0x484`) on an attacker. Every stock
    /// Temporal firer is a Foot; a building or aircraft firer has no VERA
    /// idle selector.
    fn temporal_owner_idle(&mut self, attacker: u64, rules: &RuleSet) {
        #[cfg(test)]
        IDLE_TRACE.with(|trace| trace.borrow_mut().push(attacker));
        if self.substrate.entities.get(attacker).is_some_and(|entity| {
            matches!(
                entity.category,
                EntityCategory::Unit | EntityCategory::Infantry
            )
        }) {
            crate::sim::world::queue_foot_enter_idle_mode(self, attacker, rules);
        }
    }

    fn temporal_clear_fields(&mut self, attacker: u64) {
        if let Some(link) = self.temporal_link_mut(attacker) {
            link.target = None;
            link.next = None;
            link.prev = None;
        }
    }

    /// The `if (+0x274 && +0x274->Target) LetGo()` every release site
    /// shares: retargeting, idle mode (`0x00709A43..0x00709A54`), the cell
    /// entry tail (`0x006F50A3..0x006F50B4`) and CaptureManager's
    /// DecideUnitFate (`0x004723E4..0x004723F3`).
    pub(crate) fn temporal_release_if_warping(&mut self, attacker: u64) {
        if self
            .substrate
            .entities
            .get(attacker)
            .is_some_and(|entity| entity.temporal.is_warping_someone())
        {
            self.temporal_let_go(attacker);
        }
    }

    /// `TemporalClass::LetGo @ 0x0071ABC0`: a head hands the chain and its
    /// progress to the next attacker, a lone head frees the target (a building
    /// comes back online), a chained link unlinks. LetGo never idles its own
    /// attacker.
    pub(crate) fn temporal_let_go(&mut self, attacker: u64) {
        let Some(link) = self.temporal_link(attacker).cloned() else {
            return;
        };
        match link.prev {
            None => {
                if let Some(next) = link.next {
                    if let Some(target) = link.target {
                        self.set_temporal_head(target, Some(next));
                    }
                    if let Some(next_link) = self.temporal_link_mut(next) {
                        next_link.prev = None;
                        next_link.warp_remaining = link.warp_remaining;
                    }
                } else if let Some(target) = link.target {
                    self.set_temporal_head(target, None);
                }
            }
            Some(prev) => {
                if let Some(next) = link.next
                    && let Some(next_link) = self.temporal_link_mut(next)
                {
                    next_link.prev = Some(prev);
                }
                if let Some(prev_link) = self.temporal_link_mut(prev) {
                    prev_link.next = link.next;
                }
            }
        }
        self.temporal_clear_fields(attacker);
    }

    /// `ClearLinkedList @ 0x0071ADE0`: frees the target, then walks Next and
    /// Prev, unlinking a neighbour's back-link only where it points here
    /// (`0x0071AE02`, `0x0071AE19`), and idles each attacker last.
    fn temporal_clear_linked_list(&mut self, attacker: u64, rules: &RuleSet) {
        // A link without a target faults natively (`0x0071ADE9` writes
        // through it); VERA stops, which also ends any walk of a cycle.
        let Some(target) = self.temporal_link(attacker).and_then(|link| link.target) else {
            return;
        };
        self.set_temporal_head(target, None);
        if let Some(stored) = self.temporal_link_mut(attacker) {
            stored.target = None;
        }
        if let Some(next) = self.temporal_link(attacker).and_then(|link| link.next) {
            if let Some(next_link) = self.temporal_link_mut(next)
                && next_link.prev == Some(attacker)
            {
                next_link.prev = None;
            }
            self.temporal_clear_linked_list(next, rules);
        }
        // Prev is read again after the Next walk (`0x0071AE12`).
        if let Some(prev) = self.temporal_link(attacker).and_then(|link| link.prev) {
            if let Some(prev_link) = self.temporal_link_mut(prev)
                && prev_link.next == Some(attacker)
            {
                prev_link.next = None;
            }
            self.temporal_clear_linked_list(prev, rules);
        }
        self.temporal_clear_fields(attacker);
        self.temporal_owner_idle(attacker, rules);
    }

    /// `0x0071AD40`, the house blow-up's release of the chain warping one of
    /// its objects (`HouseClass::Blowup_All @ 0x004FC6D0`, at `0x004FC742`),
    /// run on the head: frees the target, then walks Next and Prev through
    /// ClearLinkedList, which idles those attackers, but never idles its own.
    pub(crate) fn temporal_release_chain_no_idle(&mut self, attacker: u64, rules: &RuleSet) {
        let Some(target) = self.temporal_link(attacker).map(|link| link.target) else {
            return;
        };
        if let Some(target) = target {
            self.set_temporal_head(target, None);
            if let Some(stored) = self.temporal_link_mut(attacker) {
                stored.target = None;
            }
        }
        if let Some(next) = self.temporal_link(attacker).and_then(|link| link.next) {
            if let Some(next_link) = self.temporal_link_mut(next)
                && next_link.prev == Some(attacker)
            {
                next_link.prev = None;
            }
            self.temporal_clear_linked_list(next, rules);
        }
        if let Some(prev) = self.temporal_link(attacker).and_then(|link| link.prev) {
            if let Some(prev_link) = self.temporal_link_mut(prev)
                && prev_link.next == Some(attacker)
            {
                prev_link.next = None;
            }
            self.temporal_clear_linked_list(prev, rules);
        }
        self.temporal_clear_fields(attacker);
    }

    /// `TechnoClass::PointerExpired`'s forward (`0x00707B34` ->
    /// `0x0071AB60`), outside the removal gate: the attacker's own expiry lets
    /// go; its target's expiry clears the link and idles the attacker without
    /// relinking the chain. `rules` is absent only for a rules-less UnInit
    /// (fixtures; production passes the frame's rules), which has no idle
    /// selector, so the idle is skipped there.
    pub(crate) fn temporal_pointer_expired(
        &mut self,
        listener: u64,
        expired: u64,
        rules: Option<&RuleSet>,
    ) {
        let Some(target) = self.temporal_link(listener).map(|link| link.target) else {
            return;
        };
        if expired == listener {
            self.temporal_let_go(listener);
        } else if target == Some(expired) {
            self.temporal_clear_fields(listener);
            if let Some(rules) = rules {
                self.temporal_owner_idle(listener, rules);
            }
        }
    }

    /// `UnitClass` vtable `+0x4D4` (`0x00746420`): a gunner's TemporalClass
    /// moves to the IFV, which becomes its owner.
    pub(crate) fn temporal_receive_gunner(&mut self, ifv: u64, gunner: u64) {
        self.temporal_move_link(gunner, ifv);
    }

    /// `UnitClass` vtable `+0x4D8` (`0x007464E0`), when the last passenger
    /// leaves a `Gunner=` transport (`0x004DE738..0x004DE742`): the
    /// TemporalClass returns to the gunner, which lets go of the IFV's victim
    /// (`0x00746557`).
    pub(crate) fn temporal_remove_gunner(&mut self, ifv: u64, gunner: u64) {
        if self.temporal_move_link(ifv, gunner) {
            self.temporal_release_if_warping(gunner);
        }
    }

    /// Move a TemporalClass to another owner, re-pointing the chain and the
    /// target's head to the new owner's id. Returns false when `from` holds
    /// none (the native null tests at `0x00746434`/`0x007464FC`).
    fn temporal_move_link(&mut self, from: u64, to: u64) -> bool {
        if !self.substrate.entities.contains(to) {
            return false;
        }
        let Some(link) = self
            .substrate
            .entities
            .get_mut(from)
            .and_then(|entity| entity.temporal.link.take())
        else {
            return false;
        };
        if let Some(target) = link.target
            && self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| entity.temporal.head == Some(from))
        {
            self.set_temporal_head(target, Some(to));
        }
        if let Some(prev) = link.prev
            && let Some(prev_link) = self.temporal_link_mut(prev)
        {
            prev_link.next = Some(to);
        }
        if let Some(next) = link.next
            && let Some(next_link) = self.temporal_link_mut(next)
        {
            next_link.prev = Some(to);
        }
        if let Some(entity) = self.substrate.entities.get_mut(to) {
            entity.temporal.link = Some(link);
        }
        true
    }

    fn spawn_temporal_anim(
        &mut self,
        anim_name: &str,
        coord: AnimWorldCoord,
        z_adjust: Option<i32>,
        rules: &RuleSet,
    ) {
        if anim_name.is_empty() {
            return;
        }
        let type_id = self.interner.intern(anim_name);
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: TEMPORAL_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_id, rx, ry, sub_x, sub_y, z,
            )
        };
        let anim = match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(anim) => anim,
            Err(error) => {
                log::debug!("temporal anim [{anim_name}] did not construct: {error}");
                return;
            }
        };
        if let Some(z_adjust) = z_adjust {
            self.set_anim_z_adjust(anim, z_adjust);
        }
    }

    /// The warped object's AI prologue: its chain head's Update, the sparkle,
    /// and the frozen body (Unit `0x00736204..0x0073635A`, Infantry
    /// `0x0051BADE..0x0051BC17`, Aircraft `0x00414BDB..0x00414D2B`, Building
    /// `0x0043FCF9..0x0043FD26` then `0x004403D4..0x00440573`). Returns true
    /// when the rest of the object's AI must not run this frame. An erase
    /// leaves the dead target's head in place, so it still sparkles and
    /// returns frozen, as native does.
    pub(crate) fn temporal_ai_prologue(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(category) = self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.category)
        else {
            return true;
        };
        // Infantry sparkles before its head's Update; the others after.
        if category == EntityCategory::Infantry {
            self.temporal_foot_sparkle(id, rules);
        }
        if let Some(head) = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.temporal.head)
        {
            self.temporal_update_head(head, rules);
        }
        if matches!(category, EntityCategory::Unit | EntityCategory::Aircraft) {
            self.temporal_foot_sparkle(id, rules);
        }
        if !self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.temporal.is_warped())
        {
            return false;
        }
        if category == EntityCategory::Structure {
            self.temporal_building_sparkle(id, rules);
        }
        // Frozen: TarCom drops, and a Foot's NavCom (`Set_Destination(0, 1)`),
        // which also ends its path (`UnitClass::Set_Destination @ 0x00741970`
        // resets the path head): VERA's path executor stops with it, as for
        // every concrete null destination (`mission::authority`). For a
        // Drive/Ship Unit that setter also reaches the locomotor Stop, which
        // nulls +34 with no warp gate (Drive `0x004AFE00`, Ship `0x0069F510`;
        // none in `0x00741970` or Foot `0x004D94B0` either), so a released
        // unit only finishes its current track. The locomotor's Move_To
        // refuses while warped (Drive `0x004AFD71`, Walk `0x0075ACD0`), and it
        // does not process.
        let track_unit = category == EntityCategory::Unit
            && self
                .substrate
                .entities
                .get(id)
                .and_then(|entity| entity.locomotor.as_ref())
                .is_some_and(|loco| {
                    matches!(
                        loco.active_kind(),
                        crate::rules::locomotor_type::LocomotorKind::Drive
                            | crate::rules::locomotor_type::LocomotorKind::Ship
                    )
                });
        let mut clears_destination = false;
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            if entity.attack_target.is_some() {
                crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
            }
            clears_destination = category != EntityCategory::Structure
                && (entity.navigation.nav_com.is_some() || entity.movement_target.is_some());
        }
        if clears_destination {
            if track_unit {
                self.set_unit_null_destination(id, Some(rules));
            }
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                if !track_unit {
                    crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                        entity, None,
                    );
                }
                entity.movement_target = None;
            }
        }
        true
    }

    /// A Foot's `ChronoSparkle1=` every 24th frame while warped, 0x78 leptons
    /// east and south of its Location.
    fn temporal_foot_sparkle(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !entity.temporal.is_warped() || !sparkle_frame(self.session.binary_frame, 0) {
            return;
        }
        let location = location_coord(entity);
        let coord = AnimWorldCoord {
            x: location.x.wrapping_add(SPARKLE_OFFSET_LEPTONS),
            y: location.y.wrapping_add(SPARKLE_OFFSET_LEPTONS),
            z: location.z,
        };
        let name = rules.general.chrono_sparkle1.name.clone();
        self.spawn_temporal_anim(&name, coord, None, rules);
    }

    /// A frozen building's sparkle (`0x004403D4..0x0044055D`): a type with
    /// `MaxNumberOccupants=` sparkles at each port `i` on frames where
    /// `(frame + i) % 24 == 0`, at its `vtable+0xAC` coordinate plus
    /// `IsometricPixelToWorld(MuzzleFlash<i>)` with ZAdjust -200; any other
    /// building sparkles at its Location every 24th frame.
    fn temporal_building_sparkle(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let frame = self.session.binary_frame;
        let location = location_coord(entity);
        let name = rules.general.chrono_sparkle1.name.clone();
        let ports = object.max_number_occupants;
        if ports == 0 {
            if sparkle_frame(frame, 0) {
                self.spawn_temporal_anim(&name, location, None, rules);
            }
            return;
        }
        let muzzle_flashes = rules
            .art_registry
            .get(&object.image)
            .or_else(|| rules.art_registry.get(&object.id))
            .map(|art| art.muzzle_flash_positions.clone())
            .unwrap_or_default();
        for port in 0..ports {
            if !sparkle_frame(frame, port) {
                continue;
            }
            let (px, py) = muzzle_flashes.get(port as usize).copied().unwrap_or((0, 0));
            let (dx, dy) =
                crate::util::pixel_conversion::PixelConversionBounds::isometric_pixel_to_leptons(
                    px, py,
                );
            let coord = AnimWorldCoord {
                x: location
                    .x
                    .wrapping_sub(BUILDING_RENDER_SHIFT_LEPTONS)
                    .wrapping_add(dx),
                y: location
                    .y
                    .wrapping_sub(BUILDING_RENDER_SHIFT_LEPTONS)
                    .wrapping_add(dy),
                z: location.z,
            };
            self.spawn_temporal_anim(&name, coord, Some(PORT_SPARKLE_Z_ADJUST), rules);
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Every `Enter_Idle_Mode` this module issues, in order, for the native
    /// corpus.
    static IDLE_TRACE: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Drain the idle trace (see `IDLE_TRACE`).
#[cfg(test)]
fn take_idle_trace() -> Vec<u64> {
    IDLE_TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

/// `(frame + phase) % 24 == 0` under the native signed IDIV.
fn sparkle_frame(frame: u32, phase: u32) -> bool {
    (frame as i32).wrapping_add(phase as i32) % SPARKLE_PERIOD_FRAMES == 0
}

/// `ObjectClass+0x9C` Location, the coordinate WarpAway and the sparkle use.
fn location_coord(entity: &GameEntity) -> AnimWorldCoord {
    let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    }
}

#[cfg(test)]
#[path = "temporal_tests.rs"]
mod tests;
