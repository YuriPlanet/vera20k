//! `SlaveManagerClass` (Techno+0x2D8): the slaves of a Yuri Slave Miner or its
//! deployed refinery, and their harvest cycle.
//!
//! Evidence: `tools/spatial_oracle/slave_manager.{py,json,meta.json}` runs the
//! per-slave machine, the manager machine for a building owner (with the
//! refinery's relocation test) and for a Slave Miner, DeploySlaves (through
//! the original Unlimbo and Scatter), the slave's Mission_Harvest and its
//! deposit, the Slave Miner's recall rules, its Guard/AreaGuard kick and the
//! placement hand-off; `world/slave_manager_oracle_tests.rs` replays them
//! through this owner.
//!
//! - `TechnoClass::Init_Managers` (`0x006F4020..0x006F4077`) builds the manager
//!   for an `Enslaves=` type (constructor `0x006AF1A0`): `SlavesNumber` slaves,
//!   each constructed in limbo with its node in state 0.
//! - `SlaveManagerClass::AI @ 0x006AF5F0` runs from `TechnoClass::AI` after the
//!   passive block and the bomb (`0x006FA717`) and acts every 10 frames:
//!   AI_Update over the nodes in order (`0x006AF6C0`), then the manager's own
//!   state (`0x006AFD60`).
//! - A building owner leaves state 0 once it is neither building up nor being
//!   sold, and from then on lets its idle slaves out on every visit
//!   (DeploySlaves `0x006B04C0`: an infantry spot in the drop cell, Unlimbo,
//!   Scatter away from the building). A refinery deployed from a Slave Miner
//!   (state 4) waits for its BState (`+0x534`), 0 while it builds up. VERA
//!   keeps a building's build-up and build-down in `building_up` and
//!   `building_down` (`sim::building_construction`) without publishing the
//!   Construction or Selling mission or a BState, so both gates read them
//!   (`GameEntity::constructing_or_selling`, `in_construction_bstate`).
//! - A slave looks for ore within `SlaveMinerSlaveScan` (the Foot
//!   Scan_For_Tiberium), walks there, digs one level per `HarvestRate` frames
//!   (`InfantryClass::Mission_Harvest @ 0x00522E70`), carries its full load to
//!   the drop cell, pays it to the master's house (`0x00522D50`) and waits
//!   inside for `SlaveReloadRate` frames.
//! - A lost slave is regrown after `SlaveRegenRate` frames (`0x006AF650`); a
//!   slave that dies leaves its node through RemoveSlave (`0x006B0A20`).
//! - A master that dies frees its slaves (`0x006B0AE0`): the killer's house
//!   takes the ones outside, which reset to Guard and cheer, and the ones
//!   inside die with it.
//! - Deploying a Slave Miner hands its manager to the refinery (SetOwner
//!   `0x006AF580`, with the hand-off `0x006B0D10`); undeploying hands it back
//!   at the conversion (`BuildingClass::Sell @ 0x0044A047`,
//!   `tick_building_down`). A refinery placed from production takes the same
//!   hand-off (`0x006B0D60`) once its Unlimbo succeeds.
//! - A Slave Miner hunts for a field: it sets out as it leaves its war
//!   factory (`UnitClass::PerCellProcess @ 0x0073A9CA`, ahead of the rally
//!   point), and idle on Guard, Sticky or Area Guard it sets out again once
//!   `SlaveMinerKickFrameDelay` has passed since its mission began and
//!   ShouldRecallSlaves (`0x006B1020`) answers yes (a computer house always; a
//!   human's on ore, or with ore within `SlaveMinerShortScan` after the
//!   delay). State 1 finds the nearest field within `SlaveMinerLongScan` and
//!   a cell beside it to deploy on (FindDeployCell `0x006B0300`), state 2
//!   drives there and deploys (`UnitClass::Deploy`, `deploy_mcv`), state 3
//!   retries, state 4 waits for the deploy, and state 6 recalls its idle
//!   slaves and hunts again. A player's Harvest order onto a field runs
//!   HandleReturnedSlaves (`0x006B0DB0`) from the Harvest mission; any other
//!   order but Attack, and Stop, take it off the hunt (`0x006B0C80`).
//! - A refinery with no ore within `SlaveMinerShortScan` of its centre (the
//!   Building's Scan_For_Tiberium, `0x0070F8F0`) moves when a cell beside
//!   the nearest field within `SlaveMinerLongScan` lies
//!   `SlaveMinerScanCorrection` cells closer to it than the refinery does
//!   (state 5, `0x006B0062..0x006B01F5`); with no field in range it moves
//!   too. It archives its own cell and packs up (Selling's UndeploysInto arm,
//!   VERA's `undeploy_building`); the Slave Miner it becomes is sent to that
//!   cell and its manager, in state 6, recalls its idle slaves and hunts.
//!
//! ## Residuals
//! - The pack-up runs on VERA's undeploy owner (`building_down`, Sell's
//!   stages 0..2 and the build-up played in reverse at the type's
//!   `BuildupTime=` rate), but the Selling mission is not published, so the
//!   refinery's turret keeps firing while it packs up. Trigger: every
//!   relocation and undeploy. Effect: the turret. Frequency: every
//!   relocation. Downstream: the mission owner, a separate mechanism.
//! - A player moves a refinery natively by clicking a cell with it selected
//!   (`BuildingClass` cell click `0x004436F0`: SetRallyPoint's ArchiveTarget
//!   event 0x1E, then SELL 0x16 -> `Sell_Back(-1) @ 0x00447110`); Selling
//!   then runs HandleReturnedSlaves' building arm for an archived field
//!   (`0x0044AA3D..0x0044AA9F`: FindDeployCell beside it becomes the
//!   archive) and the Slave Miner drives there. VERA's app has no such click
//!   (it undeploys on a self-click, which retail never offers); the building
//!   arm is unreachable without it. Trigger: a player's move order for a
//!   refinery (or Construction Yard). Effect: no move order. Frequency:
//!   occasional. Downstream: the building's rally point is VERA's
//!   `rally_target`, not the ArchiveTarget it is natively.
//! - The factory-exit hunt start runs at production: VERA hands a produced
//!   unit its rally point there rather than at the factory exit
//!   (`production_queue.rs`), so the Slave Miner's hunt begins a few frames
//!   before native's, whose exit drive precedes it. Trigger: every Slave
//!   Miner built. Effect: the first scan comes a few frames early.
//!   Frequency: every build. Downstream: none beyond timing.
//! - Two recall sites have no VERA anchor: FootClass::Mission_Hunt's reset
//!   when it picks a destination (`0x004D553D..0x004D5547`; VERA's Hunt port
//!   has no destination step) and FootClass::Mission_AreaGuard's hunt start
//!   on its guard-area return path (`0x004D6D69..0x004D6D73`, absent with
//!   that path). Trigger: a Slave Miner on Hunt (an AI's failed Unload
//!   deploy) or on Area Guard past the Unit kick. Effect: retail resets or
//!   starts the hunt, VERA does not. Frequency: rare. Downstream: none.
//! - The Slave Miner carries a Miner component (`MinerKind::Slave`) as
//!   VERA's order marker: input issues HarvestCell for it where native
//!   `UnitClass::What_Action` gives the Harvest action to a
//!   ResourceGatherer/ResourceDestination type. The dispatch treats it as
//!   the plain Unit it is natively (`techno_ai/mission_handlers.rs`).
//! - Of `InfantryClass::DoType_Sequencer` (`0x00520AE0`) VERA runs only the
//!   Cheer's end (`Simulation::infantry_action_completed`). A digging slave
//!   keeps Doing 38 once its mission leaves Harvest, where retail's case 0x26
//!   forces Ready after the Shovel sequence has played, and it walks home
//!   without the Carry action (39, Do_Action's remap of Walk for a loaded
//!   slave, `0x0051D739..0x0051D773`). Trigger: every slave carrying a load
//!   home. Effect: the Doing value and the walk sequence shown; 0, 3, 38 and
//!   39 are all interruptible, so readiness, Scatter and the fire error
//!   answer alike. Frequency: every trip. Downstream: none beyond the Doing
//!   hash.
//! - FreeSlaves callers other than the death arm (`0x00702065`), the Temporal
//!   erase (`0x0071AAA7`) and the destructor (`0x006F4571`): the Teleport
//!   post-warp (`0x00718998`, `0x00718AEF`) and the Jumpjet crash
//!   (`0x0054CEDF`) pass their own killer; VERA's destructor fallback hands
//!   those slaves to the Civilian house instead. No stock Slave Miner warps or
//!   flies.
//!
//! ## Dependency rules
//! - Part of sim/; sim/ never depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::miner::{CargoBale, MinerConfig, ResourceType};
use crate::sim::mission::authority::queue_entity_mission_deferred;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::timer::CdTimer;
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, SimSoundEvent, Simulation,
    UninitContext,
};

/// Infantry actions the slave machine requests (`InfantryClass::Do_Action`).
const DO_READY: i32 = 0;
const DO_CHEER: i32 = 32;
const DO_SHOVEL: i32 = 38;

/// The manager's own state (`SlaveManagerClass+0x5C`, jump table `0x006B0238`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum ManagerState {
    /// 0: idle; a building owner moves on once it has built up.
    Ready = 0,
    /// 1: a Slave Miner looks for a field (chain 6).
    Scanning = 1,
    /// 2: driving to the field (chain 6).
    Travelling = 2,
    /// 3: deploying there (chain 6).
    Deploying = 3,
    /// 4: just deployed; the refinery is still building up.
    Deployed = 4,
    /// 5: working; idle slaves go out on every visit.
    Working = 5,
    /// 6: packing up to move (chain 6).
    PackingUp = 6,
}

/// One slave's state (`SlaveControl+4`, jump table `0x006AFD40`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum SlaveState {
    /// 0: inside, ready to go out.
    Ready = 0,
    /// 1: looking for ore.
    Scanning = 1,
    /// 2: walking to ore.
    Moving = 2,
    /// 3: digging.
    Harvesting = 3,
    /// 4: carrying its load home.
    Returning = 4,
    /// 5: inside after paying, until `SlaveReloadRate` has passed.
    Reloading = 5,
    /// 6: lost; regrown after `SlaveRegenRate`.
    Dead = 6,
}

/// A `SlaveControl` node (0x14 bytes): the slave, its state and its timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct SlaveNode {
    pub(crate) slave: Option<u64>,
    pub(crate) state: SlaveState,
    pub(crate) timer: CdTimer,
}

/// `SlaveManagerClass`, held by its master (Techno+0x2D8). Its owner (`+0x24`)
/// is the entity holding it; FreeSlaves takes it off the master, which stands
/// for the owner's release (`0x006B0C67`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct SlaveManager {
    /// `+0x28`: the `Enslaves=` type.
    slave_type: InternedId,
    /// `+0x30`: `SlaveRegenRate`.
    regen_rate: i32,
    /// `+0x34`: `SlaveReloadRate`.
    reload_rate: i32,
    /// `+0x38`: the nodes, in the order AI_Update walks them.
    nodes: Vec<SlaveNode>,
    /// `+0x50..+0x58`: every 10 frames.
    ai_timer: CdTimer,
    /// `+0x5C`.
    state: ManagerState,
    /// `+0x60`: the frame stamp the unit-owner states keep.
    frame: i32,
}

impl SlaveManager {
    /// The constructor (`0x006AF1A0`) at `now`: a node per slave it tried to
    /// create, in state 0 with its timer at `now` for 0, and the AI timer at
    /// `now` for 10. A refused CreateObject still appends its node
    /// (`0x006AF258..0x006AF2C5`), which AI_Update then loses (state 6); the
    /// heap leaves that node's state unwritten, and 0 stands for it.
    pub(crate) fn new(
        slave_type: InternedId,
        slaves: impl IntoIterator<Item = Option<u64>>,
        regen_rate: i32,
        reload_rate: i32,
        now: i32,
    ) -> Self {
        Self {
            slave_type,
            regen_rate,
            reload_rate,
            nodes: slaves
                .into_iter()
                .map(|slave| SlaveNode {
                    slave,
                    state: SlaveState::Ready,
                    timer: CdTimer::started(now, 0),
                })
                .collect(),
            ai_timer: CdTimer::started(now, 10),
            state: ManagerState::Ready,
            frame: 0,
        }
    }

    pub(crate) fn state(&self) -> ManagerState {
        self.state
    }

    #[cfg(test)]
    pub(crate) fn frame(&self) -> i32 {
        self.frame
    }

    #[cfg(test)]
    pub(crate) fn nodes(&self) -> &[SlaveNode] {
        &self.nodes
    }

    #[cfg(test)]
    pub(crate) fn ai_timer(&self) -> CdTimer {
        self.ai_timer
    }

    /// The slaves the nodes hold, in node order.
    pub(crate) fn slaves(&self) -> impl Iterator<Item = u64> + '_ {
        self.nodes.iter().filter_map(|node| node.slave)
    }

    /// `0x006B09E0`'s membership walk (IsSlaveAtCell), last node first.
    pub(crate) fn holds(&self, slave: u64) -> bool {
        self.nodes
            .iter()
            .rev()
            .any(|node| node.slave == Some(slave))
    }

    #[cfg(test)]
    pub(crate) fn set_for_test(
        &mut self,
        state: ManagerState,
        frame: i32,
        nodes: Vec<SlaveNode>,
        ai_timer: CdTimer,
    ) {
        self.state = state;
        self.frame = frame;
        self.nodes = nodes;
        self.ai_timer = ai_timer;
    }
}

/// The slave side, on the slave itself: SlaveOwner (`TechnoClass+0x2DC`),
/// the master whose manager holds it, and its Storage (`+0x33C`), one bale
/// per level of ore or gems. Only this module writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SlaveLink {
    owner: Option<u64>,
    cargo: Vec<CargoBale>,
}

impl SlaveLink {
    /// SlaveOwner: the master whose manager holds this slave.
    pub(crate) fn owner(&self) -> Option<u64> {
        self.owner
    }

    /// The Storage levels the slave carries, in the order it cut them.
    pub(crate) fn cargo(&self) -> &[CargoBale] {
        &self.cargo
    }

    /// A slave of `master` carrying `cargo`, for fixtures.
    #[cfg(test)]
    pub(crate) fn for_test(master: Option<u64>, cargo: Vec<CargoBale>) -> Self {
        Self {
            owner: master,
            cargo,
        }
    }
}

/// The manager's owner's cell (`vt+0x1B8`): its coordinate's cell.
pub(crate) fn owner_cell(owner: &GameEntity) -> (i16, i16) {
    let coord = crate::sim::movement::ground_pose::position_world_coord(&owner.position);
    ((coord.x / 256) as i16, (coord.y / 256) as i16)
}

/// `SlaveManagerClass::GetDeployCenter @ 0x006B0690` for the manager's
/// owner: its cell ([`owner_cell`]) plus `(FoundationWidth - 1,
/// FoundationHeight / 2)` for a building, whose type's `foundation` the
/// caller supplies, else the owner's own cell. CellStruct shorts; the height
/// halves with `CDQ; SUB EAX,EDX; SAR 1`. `None` for a building whose
/// foundation is unknown.
pub(crate) fn deploy_center(
    owner: &GameEntity,
    foundation: Option<(u16, u16)>,
) -> Option<(i16, i16)> {
    let cell = owner_cell(owner);
    if owner.category != EntityCategory::Structure {
        return Some(cell);
    }
    let (width, height) = foundation?;
    Some((
        cell.0.wrapping_add((i32::from(width) - 1) as i16),
        cell.1.wrapping_add((i32::from(height) / 2) as i16),
    ))
}

/// FindDeployCell's MapClass::Find_Nearby_Passable_Cell request, as pushed
/// at `0x006B03CF..0x006B0417`: Track over MovementZone Normal, not
/// bridge-aware, the foundation as the rectangle, any overlay refused, no
/// height or obstacle gate, bridge cells refused, the field cell as the
/// nearest-to reference, no quadrant skip, occupancy checked; the required
/// zone is MapClass::GetZoneID of the owner's cell for MovementZone Normal
/// without the bridge lookup (`0x006B0400`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeployCellSearch {
    pub(crate) seed: (i32, i32),
    pub(crate) zone_cell: (i16, i16),
    pub(crate) zone_movement_zone: crate::rules::locomotor_type::MovementZone,
    pub(crate) zone_check_bridge: bool,
    pub(crate) speed_type: crate::rules::locomotor_type::SpeedType,
    pub(crate) movement_zone: crate::rules::locomotor_type::MovementZone,
    pub(crate) bridge_aware: bool,
    pub(crate) footprint: (i32, i32),
    pub(crate) reject_any_overlay: bool,
    pub(crate) check_height: bool,
    pub(crate) allow_bridge_cells: bool,
    pub(crate) target: (i32, i32),
    pub(crate) check_occupancy: bool,
}

impl DeployCellSearch {
    pub(crate) fn new(owner_cell: (i16, i16), seed: (u16, u16), foundation: (u16, u16)) -> Self {
        use crate::rules::locomotor_type::{MovementZone, SpeedType};
        let seed = (i32::from(seed.0), i32::from(seed.1));
        Self {
            seed,
            zone_cell: owner_cell,
            zone_movement_zone: MovementZone::Normal,
            zone_check_bridge: false,
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
            bridge_aware: false,
            footprint: (i32::from(foundation.0), i32::from(foundation.1)),
            reject_any_overlay: true,
            check_height: false,
            allow_bridge_cells: false,
            target: seed,
            check_occupancy: true,
        }
    }
}

/// `0x006B1A70`: `ftol(Sqrt_Approx(dx*dx + dy*dy))` over a cell difference,
/// read back as a signed short (`MOVSX EAX,AX` at `0x006AFB10`).
fn cell_distance(dx: i16, dy: i16) -> i32 {
    use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};
    let x = X87Chop53::load_i32(i32::from(dx));
    let y = X87Chop53::load_i32(i32::from(dy));
    let squared = X87Chop53::add(X87Chop53::mul(x, x), X87Chop53::mul(y, y));
    let root_bits =
        sqrt_approx_f32(squared).expect("map-space squared distance stays in finite f32 range");
    let root =
        X87Chop53::load_f32(root_bits).expect("Sqrt_Approx always returns a finite normal or zero");
    i32::from(X87Chop53::ftol_i64(root).expect("map-space distance fits a signed integer") as i16)
}

impl Simulation {
    fn slave_manager(&self, master: u64) -> Option<&SlaveManager> {
        self.substrate.entities.get(master)?.slave_manager.as_ref()
    }

    fn slave_manager_mut(&mut self, master: u64) -> Option<&mut SlaveManager> {
        self.substrate
            .entities
            .get_mut(master)?
            .slave_manager
            .as_mut()
    }

    fn now_frame(&self) -> i32 {
        self.session.binary_frame as i32
    }

    /// `TechnoClass::Init_Managers @ 0x006F4020..0x006F4077`: an `Enslaves=`
    /// type gets its manager (`0x006AF1A0`): `SlavesNumber` slaves of the
    /// `Enslaves=` type for the master's house, each constructed (the
    /// TechnoClass constructor's Scenario draw) and left in limbo with the
    /// master as its SlaveOwner. Runs after the master's own constructor, in
    /// the constructor-owned children order.
    pub(crate) fn create_slave_manager(&mut self, master: u64, rules: &RuleSet) {
        let Some((slave_type, count, regen, reload, owner, cell, facing, z)) =
            self.substrate.entities.get(master).and_then(|parent| {
                let object = self.object_type(parent.type_ref(), rules)?;
                let slave_type = object.enslaves.as_deref()?;
                let slave_object = rules.object_case_insensitive(slave_type)?;
                if slave_object.category != crate::rules::object_type::ObjectCategory::Infantry {
                    return None;
                }
                Some((
                    slave_object.id.clone(),
                    object.slaves_number.max(0),
                    object.slave_regen_rate,
                    object.slave_reload_rate,
                    self.interner.resolve(parent.owner()).to_string(),
                    (parent.position.rx, parent.position.ry),
                    parent.facing,
                    parent.position.z,
                ))
            })
        else {
            return;
        };
        let mut slaves = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let slave = self.construct_object_limbo_at_height(
                &slave_type,
                &owner,
                cell.0,
                cell.1,
                facing,
                z,
                rules,
            );
            if let Some(entity) = slave.and_then(|slave| self.substrate.entities.get_mut(slave)) {
                entity.slave.owner = Some(master);
            }
            slaves.push(slave);
        }
        let slave_type = self.interner.intern(&slave_type);
        let now = self.now_frame();
        if let Some(parent) = self.substrate.entities.get_mut(master) {
            parent.slave_manager = Some(SlaveManager::new(slave_type, slaves, regen, reload, now));
        }
    }

    /// `SlaveManagerClass::AI @ 0x006AF5F0`, from the master's TechnoClass::AI
    /// (`0x006FA717`): every 10 frames, AI_Update then the manager's own state.
    pub(crate) fn slave_manager_ai(
        &mut self,
        master: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let now = self.now_frame();
        let Some(manager) = self.slave_manager_mut(master) else {
            return;
        };
        if !manager.ai_timer.expired(now) {
            return;
        }
        manager.ai_timer.start(now, 10);
        self.slave_ai_update(master, rules, registry);
        self.slave_manager_step(master, rules, registry);
    }

    /// `SlaveManagerClass::AI_Update @ 0x006AF6C0`: each node in order.
    pub(crate) fn slave_ai_update(
        &mut self,
        master: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let count = self.slave_manager(master).map_or(0, |m| m.nodes.len());
        for index in 0..count {
            let Some(manager) = self.slave_manager(master) else {
                return;
            };
            let Some(mut node) = manager.nodes.get(index).copied() else {
                return;
            };
            let regen_rate = manager.regen_rate;
            // 0x006AF6EB..0x006AF724: a node whose slave is gone is lost.
            if node.slave.is_none() && node.state != SlaveState::Dead {
                node.state = SlaveState::Dead;
                node.timer.start(self.now_frame(), regen_rate);
            }
            self.slave_node_step(master, &mut node, rules, registry);
            if let Some(slot) = self
                .slave_manager_mut(master)
                .and_then(|manager| manager.nodes.get_mut(index))
            {
                *slot = node;
            }
        }
    }

    fn slave_node_step(
        &mut self,
        master: u64,
        node: &mut SlaveNode,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let now = self.now_frame();
        match node.state {
            SlaveState::Ready => {}
            SlaveState::Scanning => {
                let Some(slave) = node.slave else { return };
                // 0x006AF73B: the Foot Scan_For_Tiberium over SlaveMinerSlaveScan.
                let range =
                    crate::sim::miner::ore_scan::scan_cells(rules.general.slave_miner_slave_scan);
                match crate::sim::miner::ore_scan::scan_for_tiberium(
                    self, rules, registry, slave, range,
                ) {
                    Some(cell) => {
                        self.send_slave(slave, cell, rules, registry);
                        node.state = SlaveState::Moving;
                    }
                    None => {
                        if let Some(entity) = self.substrate.entities.get_mut(slave) {
                            entity.set_archive_target(None);
                        }
                        if let Some(drop) = self.slave_drop_cell(master, rules) {
                            self.send_slave(slave, drop, rules, registry);
                        }
                        node.state = SlaveState::Returning;
                    }
                }
            }
            SlaveState::Moving => {
                let Some(slave) = node.slave else { return };
                let Some(entity) = self.substrate.entities.get(slave) else {
                    return;
                };
                let cell = (entity.position.rx, entity.position.ry);
                let driving = entity.navigation.nav_com.is_some();
                // 0x006AF7F8: Tiberium land under the slave (0x00487DF0).
                if crate::sim::miner::ore_scan::cell_is_tiberium_land(self, registry, cell) {
                    self.slave_start_harvest(slave);
                    node.state = SlaveState::Harvesting;
                } else if !driving {
                    node.state = SlaveState::Scanning;
                }
            }
            SlaveState::Harvesting => {
                let Some(slave) = node.slave else { return };
                let Some(entity) = self.substrate.entities.get(slave) else {
                    return;
                };
                let cell = (entity.position.rx, entity.position.ry);
                // 0x00522FC0: Get_Mission (vt+0x184 = 0x005B3040, the current
                // mission, else the queued one) is Harvest.
                let harvesting = entity.mission.effective().known() == Some(MissionType::Harvest);
                if self.slave_is_full(slave, rules) {
                    // 0x006AF8F3: archive its cell and carry the load home.
                    if let Some(entity) = self.substrate.entities.get_mut(slave) {
                        entity.set_archive_target(Some(TargetKind::Cell(cell.0, cell.1)));
                    }
                    if let Some(drop) = self.slave_drop_cell(master, rules) {
                        self.send_slave(slave, drop, rules, registry);
                    }
                    node.state = SlaveState::Returning;
                } else if !crate::sim::miner::ore_scan::cell_is_tiberium_land(self, registry, cell)
                {
                    // 0x00522D20: Queue_Mission(Guard, 0).
                    self.queue_slave_mission(slave, MissionType::Guard);
                    node.state = SlaveState::Scanning;
                } else if !harvesting {
                    // 0x00522FC0: the mission is no longer Harvest.
                    node.state = SlaveState::Scanning;
                }
            }
            SlaveState::Returning => {
                let Some(slave) = node.slave else { return };
                self.slave_returning_step(master, slave, node, rules, registry);
            }
            SlaveState::Reloading => {
                if !node.timer.expired(now) {
                    return;
                }
                // 0x006AFCDF..0x006AFCF4: Strength and EstimatedHealth back to
                // the type's Strength, then ready.
                if let Some(slave) = node.slave {
                    let strength = self
                        .substrate
                        .entities
                        .get(slave)
                        .and_then(|e| self.object_type(e.type_ref(), rules))
                        .map(|object| object.strength);
                    if let (Some(strength), Some(entity)) =
                        (strength, self.substrate.entities.get_mut(slave))
                    {
                        entity.health.current = strength;
                        entity.estimated_health.reset(strength);
                    }
                }
                node.state = SlaveState::Ready;
            }
            SlaveState::Dead => {
                if node.timer.expired(now) {
                    self.regrow_slave(master, node, rules);
                }
            }
        }
    }

    /// AI_Update state 4 (`0x006AF9FE..0x006AFCD4`).
    fn slave_returning_step(
        &mut self,
        master: u64,
        slave: u64,
        node: &mut SlaveNode,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(drop) = self.slave_drop_cell(master, rules) else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(slave) else {
            return;
        };
        let cell = (entity.position.rx, entity.position.ry);
        let nav_com = entity.navigation.nav_com;
        // 0x006AFA94..0x006AFB1B: a NavCom whose cell lies more than
        // ApproachTargetResetMultiplier cells from the drop cell is re-issued.
        let drifted = nav_com.is_some_and(|target| {
            let Ok(coord) = crate::sim::movement::nav_target_coordinate(
                target,
                Some(slave),
                &self.substrate.entities,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            ) else {
                return false;
            };
            // CDQ; AND EDX,0xFF; ADD; SAR 8, then 16-bit CellStruct subtraction.
            let to_cell = |leptons: i32| (leptons.wrapping_add((leptons >> 31) & 0xFF) >> 8) as i16;
            let dx = to_cell(coord.x).wrapping_sub(drop.0 as i16);
            let dy = to_cell(coord.y).wrapping_sub(drop.1 as i16);
            cell_distance(dx, dy) > rules.general.approach_target_reset_multiplier
        });
        if cell == drop && nav_com.is_none() {
            // 0x006AFBCC: pay, go inside and reload.
            self.slave_deposit(slave, master, rules);
            self.limbo_slave(slave, rules);
            let reload = self.slave_manager(master).map_or(0, |m| m.reload_rate);
            node.state = SlaveState::Reloading;
            node.timer.start(self.now_frame(), reload);
        } else if nav_com.is_none() || drifted {
            self.send_slave(slave, drop, rules, registry);
        }
    }

    /// The manager's own machine (`0x006AFD60`, jump table `0x006B0238`).
    pub(crate) fn slave_manager_step(
        &mut self,
        master: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let now = self.now_frame();
        let Some(owner) = self.substrate.entities.get(master) else {
            return;
        };
        let building = owner.category == EntityCategory::Structure;
        let unit = owner.category == EntityCategory::Unit;
        let driving = owner.navigation.nav_com.is_some();
        let deploy_pending = owner.mcv_deploy_pending;
        let guarding = owner.mission.current().known() == Some(MissionType::Guard);
        let constructing_or_selling = owner.constructing_or_selling();
        let built = !owner.in_construction_bstate();
        let Some(state) = self.slave_manager(master).map(SlaveManager::state) else {
            return;
        };
        match state {
            ManagerState::Ready => {
                // 0x006AFD7A..0x006AFDA7: a building whose mission is neither
                // Selling nor Construction.
                if building && !constructing_or_selling {
                    self.set_manager_state(master, ManagerState::Working, i32::MAX);
                }
            }
            ManagerState::Scanning => {
                // 0x006AFDC1..0x006AFDE5: a Slave Miner already driving.
                if unit && driving {
                    self.set_manager_state(master, ManagerState::Travelling, i32::MAX);
                    return;
                }
                // 0x006AFDF2..0x006AFEBF: the nearest field within
                // SlaveMinerLongScan (the owner's Scan_For_Tiberium), a cell
                // there to deploy on, and the drive to it.
                let range =
                    crate::sim::miner::ore_scan::scan_cells(rules.general.slave_miner_long_scan);
                let target = crate::sim::miner::ore_scan::scan_for_tiberium(
                    self, rules, registry, master, range,
                )
                .and_then(|field| self.find_deploy_cell(master, field, rules));
                match target {
                    None => self.set_manager_state(master, ManagerState::Ready, now),
                    Some(cell) => {
                        self.send_slave_master(master, cell, rules);
                        self.set_manager_state(master, ManagerState::Travelling, i32::MAX);
                    }
                }
            }
            ManagerState::Travelling => {
                // 0x006AFEC0..0x006AFF42: a Slave Miner that has arrived
                // deploys (`UnitClass::Deploy`); a refused deploy retries in
                // 30 frames.
                if !unit {
                    self.set_manager_state(master, ManagerState::Ready, now);
                } else if !driving && !self.slave_master_deploys(master, rules) {
                    self.set_manager_state(master, ManagerState::Deploying, i32::MAX);
                    if let Some(manager) = self.slave_manager_mut(master) {
                        manager.ai_timer.start(now, 30);
                    }
                }
            }
            ManagerState::Deploying => {
                // 0x006AFF43..0x006AFF8D, 0x006B0223: the retry; a second
                // refusal hunts again.
                if !unit {
                    self.set_manager_state(master, ManagerState::Ready, now);
                } else if !self.slave_master_deploys(master, rules) {
                    self.set_manager_state(master, ManagerState::Scanning, i32::MAX);
                }
            }
            ManagerState::Deployed => {
                // 0x006AFF8E..0x006AFFC0: a building whose BState is not 0.
                if building && built {
                    self.set_manager_state(master, ManagerState::Working, i32::MAX);
                } else if unit && !deploy_pending && guarding {
                    // 0x006AFFD6..0x006B003B: a Slave Miner whose deploy
                    // fell through (Unit+0x68C clear, back on Guard) drives
                    // on and tries again.
                    self.set_manager_state(master, ManagerState::Travelling, i32::MAX);
                }
            }
            ManagerState::Working => {
                if unit {
                    // 0x006B003C: an undeployed Slave Miner stops working.
                    self.set_manager_state(master, ManagerState::Ready, now);
                    return;
                }
                // 0x006B0062..0x006B009A: no ore within SlaveMinerShortScan
                // of the refinery puts its relocation to the test.
                let short =
                    crate::sim::miner::ore_scan::scan_cells(rules.general.slave_miner_short_scan);
                if crate::sim::miner::ore_scan::scan_for_tiberium(
                    self, rules, registry, master, short,
                )
                .is_none()
                    && self.refinery_should_relocate(master, rules, registry)
                {
                    self.relocate_refinery(master, rules);
                    return;
                }
                self.deploy_slaves(master, rules, registry);
            }
            ManagerState::PackingUp => {
                // 0x006B020F..0x006B022A: a Slave Miner recalls its idle
                // slaves and hunts.
                if unit {
                    self.reset_live_slaves(master, rules);
                    self.set_manager_state(master, ManagerState::Scanning, i32::MAX);
                }
            }
        }
    }

    /// `UnitClass::Deploy` (`deploy_mcv`) from states 2 and 3, which write
    /// state 4 (frame MAX) to the manager when it answers yes
    /// (`0x006AFF02`, `0x006AFF7A`). A deploy that converts moves the manager
    /// to the refinery during the call (0x00739956), and native writes the
    /// same manager afterwards; VERA's manager travels with the conversion,
    /// so state 4 is written first and a refusal overwrites it. The
    /// hand-off inside acts only on state 0, so it sees no difference.
    fn slave_master_deploys(&mut self, master: u64, rules: &RuleSet) -> bool {
        self.set_manager_state(master, ManagerState::Deployed, i32::MAX);
        self.deploy_mcv(master, rules, &Default::default())
    }

    /// State 5's relocation test (`0x006B00B2..0x006B01BF`), once no ore
    /// lies within `SlaveMinerShortScan`: the nearest field `S` within
    /// `SlaveMinerLongScan` (the Building's Scan_For_Tiberium; a miss is
    /// `(0,0)`, which the test uses as it is), a deploy cell `D` beside it
    /// (FindDeployCell, also run on a miss; its own miss is `(0,0)` too), and
    /// the refinery moves when `D` is `SlaveMinerScanCorrection` cells closer
    /// to `S` than the refinery's own cell (`vt+0x1B8`, the north-west
    /// one): `(ScanCorrection >> 8) + |D - S| < |O - S|`, each distance
    /// Sqrt_Approx's truncated root read back as a signed short
    /// ([`cell_distance`]). With no field within the long scan and no cell
    /// by `(0,0)`, both of `S` and `D` are `(0,0)`, so a refinery farther
    /// than the correction from the map's corner moves.
    fn refinery_should_relocate(
        &mut self,
        master: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        let long = crate::sim::miner::ore_scan::scan_cells(rules.general.slave_miner_long_scan);
        let field =
            crate::sim::miner::ore_scan::scan_for_tiberium(self, rules, registry, master, long)
                .unwrap_or((0, 0));
        let spot = self
            .find_deploy_cell(master, field, rules)
            .unwrap_or((0, 0));
        let Some(owner) = self.substrate.entities.get(master) else {
            return false;
        };
        let own = owner_cell(owner);
        let (fx, fy) = (field.0 as i16, field.1 as i16);
        // 0x00588C60: the CellStruct difference, 16-bit.
        let to_spot = cell_distance(
            (spot.0 as i16).wrapping_sub(fx),
            (spot.1 as i16).wrapping_sub(fy),
        );
        let to_own = cell_distance(own.0.wrapping_sub(fx), own.1.wrapping_sub(fy));
        let correction =
            crate::sim::miner::ore_scan::scan_cells(rules.general.slave_miner_scan_correction);
        correction.wrapping_add(to_spot) < to_own
    }

    /// The relocation (`0x006B01C1..0x006B01F5`): the refinery archives its
    /// own cell (`vt+0x1BC`, the CellClass at its Location) and queues
    /// Selling, whose UndeploysInto arm (`BuildingClass::Sell @
    /// 0x00449C30`) packs it up into the Slave Miner that its manager, now in
    /// state 6, sends hunting. VERA's undeploy owner is
    /// [`Simulation::undeploy_building`]; the conversion at its end hands
    /// the manager over and sends the Slave Miner to the archived cell
    /// (`tick_building_down`). The byte `+0x4F8` written first silences the
    /// undeploy voice Selling plays (`0x0044A9DF`: `0x00459C20` ->
    /// `0x00708E00`), which VERA does not play (`VoiceDeploy=` is unparsed),
    /// so it has no counterpart.
    fn relocate_refinery(&mut self, master: u64, rules: &RuleSet) {
        let Some(owner) = self.substrate.entities.get_mut(master) else {
            return;
        };
        let (x, y) = owner_cell(owner);
        owner.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(
            x as u16, y as u16,
        )));
        if !self.undeploy_building(master, rules) {
            log::debug!("slave refinery {master} could not start its undeploy");
        }
        self.set_manager_state(master, ManagerState::PackingUp, i32::MAX);
    }

    /// The Slave Miner's class setter (`vt+0x480(cell, 1)`, the Unit setter
    /// `0x00741970`) then `Queue_Mission(Move, 0)`.
    fn send_slave_master(&mut self, master: u64, cell: (u16, u16), rules: &RuleSet) {
        if !self.set_unit_cell_destination(master, cell, rules) {
            log::debug!("slave master {master} has no Unit setter for {cell:?}");
        }
        self.queue_slave_mission(master, MissionType::Move);
    }

    /// Every live node's slave (not state 6, not empty), last first, reset
    /// to Guard (`vt+0x3D0`, `TechnoClass::ResetOrdersToGuard`): the loop
    /// 0x006B0490, 0x006B0C80, 0x006B0CC0, 0x006B0D10 and
    /// HandleReturnedSlaves each run.
    fn reset_live_slaves(&mut self, master: u64, rules: &RuleSet) {
        let slaves: Vec<u64> = self.slave_manager(master).map_or_else(Vec::new, |manager| {
            manager
                .nodes
                .iter()
                .rev()
                .filter(|node| node.state != SlaveState::Dead)
                .filter_map(|node| node.slave)
                .collect()
        });
        for slave in slaves {
            self.reset_orders_to_guard(slave, rules);
        }
    }

    /// `SlaveManagerClass::ShouldRecallSlaves @ 0x006B1020`: an idle manager
    /// (state 0) sends its owner out when the owner's house is not human
    /// (`House+0x1EC`), when the owner stands on Tiberium land (its cell's
    /// LandType 5), or once `SlaveMinerKickFrameDelay` has passed since the
    /// manager's frame stamp (`+0x60`; a signed add, strictly before now)
    /// with ore within `SlaveMinerShortScan` of it.
    pub(crate) fn should_recall_slaves(
        &mut self,
        master: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        let Some(manager) = self.slave_manager(master) else {
            return false;
        };
        if manager.state != ManagerState::Ready {
            return false;
        }
        let stamp = manager.frame;
        let Some(owner) = self.substrate.entities.get(master) else {
            return false;
        };
        let cell = (owner.position.rx, owner.position.ry);
        if !self
            .houses
            .get(&owner.owner())
            .is_some_and(|house| house.is_human)
        {
            return true;
        }
        if crate::sim::miner::ore_scan::cell_is_tiberium_land(self, registry, cell) {
            return true;
        }
        if rules
            .general
            .slave_miner_kick_frame_delay
            .wrapping_add(stamp)
            < self.now_frame()
        {
            let range =
                crate::sim::miner::ore_scan::scan_cells(rules.general.slave_miner_short_scan);
            return crate::sim::miner::ore_scan::scan_for_tiberium(
                self, rules, registry, master, range,
            )
            .is_some();
        }
        false
    }

    /// `UnitClass::PerCellProcess @ 0x0073A98C..0x0073A9D9`: a unit leaving
    /// its war factory (radio 8 answered 0x17) without a destination of its
    /// own, neither a Harvester= nor a Weeder= type, that holds a slave
    /// manager starts the hunt (`0x006B0CC0`) where another unit takes the
    /// rally point or its computer house's base defence (`0x0073A9DE..`).
    /// VERA hands a produced unit its rally point at production rather than
    /// at the factory exit, so production asks this there. Answers whether
    /// the rally move is skipped.
    pub(crate) fn slave_master_leaves_factory(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.slave_manager.is_none() {
            return false;
        }
        if self
            .object_type(entity.type_ref(), rules)
            .is_none_or(|object| object.harvester || object.weeder)
        {
            return false;
        }
        self.begin_slave_hunt(id, rules);
        true
    }

    /// `0x006B0CC0`: an idle manager (state 0) starts its owner's hunt for a
    /// field (state 1, frame MAX) and resets its live slaves.
    pub(crate) fn begin_slave_hunt(&mut self, master: u64, rules: &RuleSet) {
        if !self
            .slave_manager(master)
            .is_some_and(|manager| manager.state == ManagerState::Ready)
        {
            return;
        }
        self.set_manager_state(master, ManagerState::Scanning, i32::MAX);
        self.reset_live_slaves(master, rules);
    }

    /// `0x006B0C80`: back to state 0 at now, with the live slaves reset. The
    /// order that takes a Slave Miner off its hunt runs it: a MEGAMISSION
    /// other than Attack (`0x004C73E1..0x004C73EA`), the IDLE event
    /// (`0x004C769C..0x004C76AC`) and FootClass::Mission_Hunt's new
    /// destination (`0x004D553D..0x004D5547`).
    pub(crate) fn reset_slave_manager(&mut self, master: u64, rules: &RuleSet) {
        if self.slave_manager(master).is_none() {
            return;
        }
        let now = self.now_frame();
        self.set_manager_state(master, ManagerState::Ready, now);
        self.reset_live_slaves(master, rules);
    }

    /// `SlaveManagerClass::HandleReturnedSlaves @ 0x006B0DB0`, the Enslaves
    /// prologue of `UnitClass::Mission_Harvest` (`0x0073E5E9..0x0073E612`):
    /// a Slave Miner sent to a field (its NavCom) drives to a cell there it
    /// can deploy on and hunts (state 2, its live slaves reset); with no
    /// such cell, or no NavCom, it guards (state 0 at now).
    ///
    /// The building arm (an archived field) belongs to the refinery's
    /// relocation (module residual).
    pub(crate) fn handle_returned_slaves(&mut self, master: u64, rules: &RuleSet) {
        let now = self.now_frame();
        let Some(owner) = self.substrate.entities.get(master) else {
            return;
        };
        let field = (owner.category == EntityCategory::Unit)
            .then_some(owner.navigation.nav_com)
            .flatten()
            .and_then(|target| {
                crate::sim::movement::nav_target_coordinate(
                    target,
                    Some(master),
                    &self.substrate.entities,
                    self.resolved_terrain.as_ref(),
                    Some((rules, &self.interner)),
                )
                .ok()
            })
            .map(|coord| {
                // `CDQ; AND EDX,0xFF; ADD; SAR 8` per axis (0x006B0E3E..).
                let cell =
                    |leptons: i32| (leptons.wrapping_add((leptons >> 31) & 0xFF) >> 8) as u16;
                (cell(coord.x), cell(coord.y))
            });
        let Some(field) = field else {
            self.queue_slave_mission(master, MissionType::Guard);
            self.set_manager_state(master, ManagerState::Ready, now);
            return;
        };
        match self.find_deploy_cell(master, field, rules) {
            None => {
                self.set_manager_state(master, ManagerState::Ready, now);
                self.queue_slave_mission(master, MissionType::Guard);
            }
            Some(cell) => {
                self.send_slave_master(master, cell, rules);
                self.set_manager_state(master, ManagerState::Travelling, i32::MAX);
                self.reset_live_slaves(master, rules);
            }
        }
    }

    /// `SlaveManagerClass::FindDeployCell @ 0x006B0300`: a cell near `seed`
    /// the owner can deploy on ([`DeployCellSearch`]). The foundation is a
    /// building owner's own, else the unit's `DeploysInto=` type's, else 1x1.
    /// A foundation wider (taller) than 2 moves the answer one cell east
    /// (south) (`0x006B0449..0x006B0479`).
    fn find_deploy_cell(
        &self,
        master: u64,
        seed: (u16, u16),
        rules: &RuleSet,
    ) -> Option<(u16, u16)> {
        use crate::sim::find_nearby_cell::{
            NearbyAnchorGate, NearbyFootprint, NearbyQuery, NearbySearchOptions, PassabilityArgs,
            find_nearby_passable_cell_with_options, map_owned_radius_cap,
        };
        let owner = self.substrate.entities.get(master)?;
        let object = self.object_type(owner.type_ref(), rules)?;
        let foundation = if owner.category == EntityCategory::Structure {
            crate::rules::foundation::foundation_dimensions(&object.foundation)
        } else if let Some(into) = object.deploys_into.as_deref().and_then(|n| rules.object(n)) {
            crate::rules::foundation::foundation_dimensions(&into.foundation)
        } else {
            (1, 1)
        };
        let search = DeployCellSearch::new(owner_cell(owner), seed, foundation);
        let terrain = self.resolved_terrain.as_ref()?;
        let zone = self.zone_grid.as_ref().and_then(|zones| {
            zones.get_path_zone_id_native(
                terrain,
                (search.zone_cell.0 as u16, search.zone_cell.1 as u16),
                search.zone_movement_zone,
                search.zone_check_bridge,
            )
        })?;
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())?;
        let grid = self.path_grid_snapshot();
        let found = find_nearby_passable_cell_with_options(
            search.seed,
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type: search.speed_type,
                    // A DWORD -1 disables the comparison; FNPC turns a raw
                    // 0xFFFF into -1 as well (`find_nearby_cell`).
                    required_zone_id: u16::try_from(zone).ok(),
                    movement_zone: search.movement_zone,
                    bridge_aware_zone: search.bridge_aware,
                },
                footprint: NearbyFootprint::new(search.footprint.0, search.footprint.1),
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: search.allow_bridge_cells,
                check_height: search.check_height,
                check_occupancy: search.check_occupancy,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                target_cell: Some(search.target),
                path_grid: grid.as_deref(),
                resolved_terrain: Some(terrain),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            NearbySearchOptions {
                reject_any_overlay: search.reject_any_overlay,
            },
            self.session.binary_frame,
        )?;
        Some((
            found.0 + u16::from(foundation.0 > 2),
            found.1 + u16::from(foundation.1 > 2),
        ))
    }

    /// The Slave Miner's kick out of Guard or Area Guard
    /// (`UnitClass::Mission_Guard @ 0x00740815..0x0074084F`,
    /// `UnitClass::Mission_AreaGuard @ 0x00744103..0x0074416B`): once
    /// `SlaveMinerKickFrameDelay` has passed since the mission began
    /// (MissionClass `+0xC0`, strictly before now) and ShouldRecallSlaves
    /// answers yes, it starts its hunt (`0x006B0CC0`) and the mission
    /// returns its Rate epilogue. `None` lets the ordinary mission run.
    pub(crate) fn slave_master_mission_kick(
        &mut self,
        id: u64,
        mission: MissionType,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<i32> {
        let entity = self.substrate.entities.get(id)?;
        entity.slave_manager.as_ref()?;
        let started = entity.mission.mission_start_frame() as i32;
        if rules
            .general
            .slave_miner_kick_frame_delay
            .wrapping_add(started)
            >= self.now_frame()
        {
            return None;
        }
        if !self.should_recall_slaves(id, rules, registry) {
            return None;
        }
        self.begin_slave_hunt(id, rules);
        Some(self.mission_rate_epilogue_for(rules, id, mission))
    }

    fn set_manager_state(&mut self, master: u64, state: ManagerState, frame: i32) {
        if let Some(manager) = self.slave_manager_mut(master) {
            manager.state = state;
            manager.frame = frame;
        }
    }

    /// The master's drop cell ([`deploy_center`]).
    pub(crate) fn slave_drop_cell(&self, master: u64, rules: &RuleSet) -> Option<(u16, u16)> {
        let owner = self.substrate.entities.get(master)?;
        let foundation = self
            .object_type(owner.type_ref(), rules)
            .map(|object| crate::rules::foundation::foundation_dimensions(&object.foundation));
        let (x, y) = deploy_center(owner, foundation)?;
        Some((x as u16, y as u16))
    }

    /// The slave's class setter `vt+0x480(cell, 1)` then
    /// `Queue_Mission(Move, 0)`, the order every AI_Update send uses.
    fn send_slave(
        &mut self,
        slave: u64,
        cell: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        match self.set_infantry_cell_destination(slave, cell, rules, registry) {
            Ok(true) => {}
            Ok(false) => log::debug!("slave {slave} has no Walk setter for {cell:?}"),
            Err(cause) => log::debug!("slave {slave} could not be sent to {cell:?}: {cause}"),
        }
        self.queue_slave_mission(slave, MissionType::Move);
    }

    fn queue_slave_mission(&mut self, slave: u64, mission: MissionType) {
        if let Some(entity) = self.substrate.entities.get_mut(slave) {
            queue_entity_mission_deferred(entity, MissionId::from_known(mission));
        }
    }

    /// `0x00522D00`: queue Harvest unless it is already the mission.
    fn slave_start_harvest(&mut self, slave: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(slave)
            && entity.mission.current().known() != Some(MissionType::Harvest)
        {
            queue_entity_mission_deferred(entity, MissionId::from_known(MissionType::Harvest));
        }
    }

    /// `0x00522D30`: Tiberium_Load (`0x00708BC0`, the stored amount over
    /// `Storage`) equals 1.0. Every cut is a whole level, so that is a full
    /// store.
    fn slave_is_full(&self, slave: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(slave) else {
            return false;
        };
        let storage = self
            .object_type(entity.type_ref(), rules)
            .map_or(0, |object| object.storage);
        storage > 0 && entity.slave.cargo.len() as i64 == i64::from(storage)
    }

    /// DeploySlaves (`0x006B04C0`): every node in state 0, last first. The
    /// drop cell's centre asks for an infantry spot (`0x004ACA10`, the
    /// CellClass::PlaceInfantryInCell `RandomRanged(0, 3)` row draw); a spot
    /// becomes the Unlimbo coordinate at facing 0, and a slave that lands
    /// scatters away from the owner's centre (`Scatter(coord, 1, 1)`) and
    /// goes looking for ore (state 1). A full cell, or a refused Unlimbo,
    /// keeps the node for the next visit.
    pub(crate) fn deploy_slaves(
        &mut self,
        master: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let count = self.slave_manager(master).map_or(0, |m| m.nodes.len());
        for index in (0..count).rev() {
            let Some(node) = self
                .slave_manager(master)
                .and_then(|manager| manager.nodes.get(index).copied())
            else {
                return;
            };
            if node.state != SlaveState::Ready {
                continue;
            }
            let Some(slave) = node.slave else { continue };
            let Some(drop) = self.slave_drop_cell(master, rules) else {
                continue;
            };
            // 0x006B0590: the null cell deploys nothing.
            if drop == (0, 0) {
                continue;
            }
            // A drop cell inside a foundation is never a bridge deck, so
            // 0x004ACA10's bridge flag is clear.
            let centre = crate::util::fixed_math::SimFixed::from_num(128);
            let Some(spot) = bump_crush::place_infantry_in_cell(
                &self.substrate.raw_cell_occupation,
                drop.0,
                drop.1,
                MovementLayer::Ground,
                centre,
                centre,
                &mut self.scenario_rng,
            ) else {
                continue;
            };
            let (sub_x, sub_y) = crate::util::lepton::subcell_lepton_offset(Some(spot));
            if !self.unlimbo_slave(slave, drop, (sub_x, sub_y), rules) {
                continue;
            }
            if let Some(source) = self.slave_owner_centre(master, rules)
                && let Err(cause) =
                    self.scatter_infantry_forced_from(slave, source, rules, registry)
            {
                log::debug!("slave {slave} did not scatter: {cause}");
            }
            if let Some(slot) = self
                .slave_manager_mut(master)
                .and_then(|manager| manager.nodes.get_mut(index))
            {
                slot.state = SlaveState::Scanning;
            }
        }
    }

    /// The owner's Center_Coord (`vt+0x48`), the point its slaves scatter
    /// away from.
    fn slave_owner_centre(&self, master: u64, rules: &RuleSet) -> Option<(i32, i32)> {
        let owner = self.substrate.entities.get(master)?;
        let object = self.object_type(owner.type_ref(), rules)?;
        let centre = crate::sim::movement::ground_pose::object_center_coord(owner, object);
        Some((centre.x, centre.y))
    }

    /// `InfantryClass::Unlimbo @ 0x0051DFF0` at `(cell, request)` with facing
    /// 0: its own PlaceInfantryInCell for the requested coordinate (a
    /// quadrant request takes its spot without a draw), then the Foot and
    /// Techno Unlimbo through the reveal.
    fn unlimbo_slave(
        &mut self,
        slave: u64,
        cell: (u16, u16),
        request: (
            crate::util::fixed_math::SimFixed,
            crate::util::fixed_math::SimFixed,
        ),
        rules: &RuleSet,
    ) -> bool {
        let Some(spot) = bump_crush::place_infantry_in_cell(
            &self.substrate.raw_cell_occupation,
            cell.0,
            cell.1,
            MovementLayer::Ground,
            request.0,
            request.1,
            &mut self.scenario_rng,
        ) else {
            return false;
        };
        let (sub_x, sub_y) = crate::util::lepton::subcell_lepton_offset(Some(spot));
        let z = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(cell.0, cell.1))
            .map_or(0, |terrain_cell| terrain_cell.level);
        if let Some(entity) = self.substrate.entities.get_mut(slave) {
            entity.sub_cell = Some(spot);
            entity.on_bridge = false;
            entity.facing = 0;
        }
        let outcome = self.try_reveal_entity_with_context(
            slave,
            RevealRequest {
                position: RevealPosition {
                    rx: cell.0,
                    ry: cell.1,
                    z,
                    sub_x,
                    sub_y,
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::with_rules(rules),
        );
        matches!(outcome, RevealOutcome::Revealed { .. })
    }

    /// The slave's Limbo (`vt+0xD4`, `InfantryClass::Limbo @ 0x0051DF10`).
    fn limbo_slave(&mut self, slave: u64, rules: &RuleSet) {
        let _ = self.techno_limbo_with_rules(slave, rules);
    }

    /// `0x006AF650`: a new slave for the master's house, left in limbo with
    /// the master as its SlaveOwner, and the node back in state 0 with its
    /// timer at now for 0. A refused construction leaves the node lost; its
    /// expired timer retries on the next visit.
    fn regrow_slave(&mut self, master: u64, node: &mut SlaveNode, rules: &RuleSet) {
        let Some((slave_type, owner, cell, z)) =
            self.substrate.entities.get(master).and_then(|m| {
                let manager = m.slave_manager.as_ref()?;
                Some((
                    self.interner.resolve(manager.slave_type).to_string(),
                    self.interner.resolve(m.owner()).to_string(),
                    (m.position.rx, m.position.ry),
                    m.position.z,
                ))
            })
        else {
            return;
        };
        let slave =
            self.construct_object_limbo_at_height(&slave_type, &owner, cell.0, cell.1, 0, z, rules);
        node.slave = slave;
        let Some(slave) = slave else {
            return;
        };
        self.limbo_slave(slave, rules);
        if let Some(entity) = self.substrate.entities.get_mut(slave) {
            entity.slave.owner = Some(master);
        }
        node.state = SlaveState::Ready;
        node.timer.start(self.now_frame(), 0);
    }

    /// `InfantryClass::Mission_Harvest @ 0x00522E70`, the slave's Harvest
    /// handler (Infantry `vt+0x224`). No `Storage`: Do_Action(0) and
    /// Queue_Mission(Guard), one frame. On Tiberium land and not full: dig
    /// (Do_Action(0x26) unless already), take one level through
    /// Reduce_Tiberium into its Storage, and wait `HarvestRate` frames.
    /// Anywhere else: Do_Action(0) and Guard, one frame. Answers the delay
    /// and whether Guard is queued.
    pub(crate) fn infantry_mission_harvest(
        &mut self,
        slave: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> (i32, bool) {
        let Some(entity) = self.substrate.entities.get(slave) else {
            return (1, false);
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return (1, false);
        };
        let storage = object.storage;
        let harvest_rate = object.harvest_rate;
        let cell = (entity.position.rx, entity.position.ry);
        let doing = entity.mission_leaf.as_infantry().map(|leaf| leaf.doing());
        if storage != 0
            && crate::sim::miner::ore_scan::cell_is_tiberium_land(self, registry, cell)
            && !self.slave_is_full(slave, rules)
        {
            if doing != Some(DO_SHOVEL) {
                self.slave_do_action(slave, DO_SHOVEL, rules);
            }
            // ftol(min(1.0, Storage - total)): one level while not full.
            let config = MinerConfig::from_rules(rules);
            if let Some(bale) =
                crate::sim::miner::extract_bale(self, rules, registry, cell, &config)
                && let Some(entity) = self.substrate.entities.get_mut(slave)
            {
                entity.slave.cargo.push(bale);
            }
            return (harvest_rate, false);
        }
        self.slave_do_action(slave, DO_READY, rules);
        (1, true)
    }

    fn slave_do_action(&mut self, slave: u64, action: i32, rules: &RuleSet) {
        // Mission_Harvest (`0x00522E87`, `0x00522EEB`, `0x00522F90`) and the
        // cheer (`0x00522C19`) push force 0.
        if let Err(cause) = self.infantry_do_action(slave, action, false, rules) {
            log::debug!("slave {slave} Do_Action({action}): {cause}");
        }
    }

    /// The slave's deposit (`InfantryClass 0x00522D50` with the master as its
    /// argument): each non-empty Storage slot in order (ore, then gems) leaves
    /// whole and is paid to the master's house, the base amount and then the
    /// purifier bonus ([`crate::sim::miner::pay_refinery_owner`]); any payment
    /// ends with the master's `vt+0x468` (a building's refinery smoke).
    pub(crate) fn slave_deposit(&mut self, slave: u64, master: u64, rules: &RuleSet) {
        let Some(cargo) = self
            .substrate
            .entities
            .get_mut(slave)
            .map(|entity| std::mem::take(&mut entity.slave.cargo))
        else {
            return;
        };
        let mut paid = false;
        for kind in [ResourceType::Ore, ResourceType::Gem] {
            let slot: Vec<&CargoBale> = cargo
                .iter()
                .filter(|bale| bale.resource_type == kind)
                .collect();
            if slot.is_empty() {
                continue;
            }
            let value: i32 = slot.iter().map(|bale| i32::from(bale.value)).sum();
            crate::sim::miner::pay_refinery_owner(self, rules, master, value, slot.len() as i32);
            paid = true;
        }
        if paid
            && self
                .substrate
                .entities
                .get(master)
                .is_some_and(|owner| owner.category == EntityCategory::Structure)
        {
            crate::sim::world::building_anim::emit_refinery_smoke(self, rules, master);
        }
    }

    /// RemoveSlave (`0x006B0A20`), from the dying slave's receiver
    /// (`0x0051808E`) and destructor (`0x00517E22`): the last node holding it
    /// loses it and waits `SlaveRegenRate` frames (state 6).
    pub(crate) fn remove_slave(&mut self, slave: u64) {
        let Some(master) = self
            .substrate
            .entities
            .get(slave)
            .and_then(|entity| entity.slave.owner)
        else {
            return;
        };
        let now = self.now_frame();
        if let Some(manager) = self.slave_manager_mut(master)
            && let Some(node) = manager
                .nodes
                .iter_mut()
                .rev()
                .find(|node| node.slave == Some(slave))
        {
            node.slave = None;
            node.state = SlaveState::Dead;
            node.timer.start(now, manager.regen_rate);
        }
        if let Some(entity) = self.substrate.entities.get_mut(slave) {
            entity.slave.owner = None;
        }
    }

    /// The destructors' slave links, at the pending-delete drain: a slave's
    /// InfantryClass destructor leaves its node (RemoveSlave at
    /// `0x00517E22`), and a master still holding its manager frees its
    /// slaves with no killer and no house (TechnoClass destructor
    /// `0x006F4571`). A rules-less drain (test fixtures) has no house to hand
    /// the ones outside to: they only lose their link, and the ones inside
    /// are UnInit.
    pub(crate) fn release_slave_links_at_destruction(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        self.remove_slave(id);
        match rules {
            Some(rules) => self.free_slaves(id, None, None, rules, registry),
            None => {
                let Some(manager) = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|entity| entity.slave_manager.take())
                else {
                    return;
                };
                for slave in manager.nodes.iter().rev().filter_map(|node| node.slave) {
                    let Some(entity) = self.substrate.entities.get_mut(slave) else {
                        continue;
                    };
                    entity.slave.owner = None;
                    if entity.lifecycle.in_limbo {
                        self.uninit_with_context(slave, UninitContext::default());
                    }
                }
            }
        }
    }

    /// FreeSlaves (`0x006B0AE0`, `killer`, `house`), last node first. Each
    /// slave loses its SlaveOwner. One inside (limbo) dies with its master:
    /// the kill is recorded for `killer` (`vt+0xE0`) and it is UnInit. One
    /// outside passes to the killer's house, else `house`, else the Civilian
    /// house: the owner change (`vt+0x3D4`), ResetOrdersToGuard and a cheer
    /// (`vt+0x388(1)` = Do_Action(0x20)); with no house at all it takes its
    /// Strength in `C4Warhead` damage. `SlavesFreeSound` plays at the first
    /// one freed. The manager leaves the master (`+0x24 = 0`).
    pub(crate) fn free_slaves(
        &mut self,
        master: u64,
        killer: Option<u64>,
        house: Option<InternedId>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(manager) = self
            .substrate
            .entities
            .get_mut(master)
            .and_then(|entity| entity.slave_manager.take())
        else {
            return;
        };
        let civilian = self.civilian_side_house(rules);
        let killer_house = killer
            .and_then(|id| self.substrate.entities.get(id))
            .map(|entity| entity.owner());
        let new_house = killer_house.or(house).or(civilian);
        let mut first_freed = None;
        for slave in manager.nodes.iter().rev().filter_map(|node| node.slave) {
            let Some(entity) = self.substrate.entities.get_mut(slave) else {
                continue;
            };
            entity.slave.owner = None;
            if entity.lifecycle.in_limbo {
                self.slave_dies_inside(slave, killer, rules);
                continue;
            }
            let Some(new_house) = new_house else {
                self.slave_dies_unfreed(slave, rules, registry);
                continue;
            };
            self.change_owner_with_rules(slave, new_house, rules);
            self.reset_orders_to_guard(slave, rules);
            self.slave_cheers(slave, rules);
            first_freed.get_or_insert(slave);
        }
        if let (Some(slave), Some(sound)) = (first_freed, rules.general.slaves_free_sound.clone())
            && let Some(entity) = self.substrate.entities.get(slave)
        {
            self.sound_events
                .push(SimSoundEvent::voc_at(sound, &entity.position));
        }
    }

    /// `vt+0xE0` (TechnoClass::RecordKill) for `killer`, then `vt+0xF8`
    /// (FootClass::UnInit).
    fn slave_dies_inside(&mut self, slave: u64, killer: Option<u64>, rules: &RuleSet) {
        let killer_owner = killer
            .and_then(|id| self.substrate.entities.get(id))
            .map(|entity| entity.owner());
        if let Some(victim) = self.substrate.entities.get_mut(slave) {
            crate::sim::combat::record_kill_credit(victim, killer_owner, rules, &self.interner);
        }
        if let Some(killer) = killer {
            crate::sim::combat::award_kill_experience(
                &mut self.substrate.entities,
                rules,
                &self.interner,
                &self.house_alliances,
                killer,
                slave,
            );
        }
        self.uninit_with_context(slave, UninitContext::with_rules(rules));
    }

    /// `vt+0x16C(&Strength, 0, C4Warhead, 0, 0, 0, 0)` (`0x006B0BDF..`).
    fn slave_dies_unfreed(
        &mut self,
        slave: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(health) = self
            .substrate
            .entities
            .get(slave)
            .map(|entity| entity.health.current)
        else {
            return;
        };
        let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            slave,
            health,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            c4,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        self.commit_direct_damage_receiver(rules, registry, event);
    }

    /// `InfantryClass vt+0x388(1)` (`0x00522C00`): Do_Action(0x20), the cheer.
    fn slave_cheers(&mut self, slave: u64, rules: &RuleSet) {
        let before = self
            .substrate
            .entities
            .get(slave)
            .and_then(|entity| entity.mission_leaf.as_infantry().map(|leaf| leaf.doing()));
        self.slave_do_action(slave, DO_CHEER, rules);
        let after = self
            .substrate
            .entities
            .get(slave)
            .and_then(|entity| entity.mission_leaf.as_infantry().map(|leaf| leaf.doing()));
        // Do_Action restarts the sequence it installs.
        if before != after
            && after == Some(DO_CHEER)
            && let Some(animation) = self
                .substrate
                .entities
                .get_mut(slave)
                .and_then(|entity| entity.animation.as_mut())
        {
            animation.switch_to(crate::sim::animation::SequenceKind::Cheer);
        }
    }

    /// The hand-off (`0x006B0D10`, and `0x006B0D60`, the same body): an idle
    /// manager (state 0) moves to 4 (frame MAX) and resets every slave that
    /// is not lost to Guard, last first. `UnitClass::Deploy` runs it before
    /// SetOwner (`0x00739956`); a refinery placed from production runs it
    /// once its Unlimbo succeeds (`BuildingClass::ExitObject @ 0x004452FA`,
    /// `HouseClass::Place_Production @ 0x004FB252`), so it waits out its
    /// build-up in state 4 as a deployed one does.
    pub(crate) fn slave_manager_hand_off(&mut self, master: u64, rules: &RuleSet) {
        if self
            .slave_manager(master)
            .is_some_and(|manager| manager.state == ManagerState::Ready)
        {
            self.set_manager_state(master, ManagerState::Deployed, i32::MAX);
            self.reset_live_slaves(master, rules);
        }
    }

    /// SetOwner (`0x006AF580`): the manager moves from `from` to `to`. A
    /// manager `to` already holds (the one its constructor built) frees its
    /// slaves with no killer and no house, which UnInits them in limbo, and
    /// is deleted; every node's slave then names `to` as its SlaveOwner.
    /// `UnitClass::Deploy` first runs the hand-off
    /// ([`Self::slave_manager_hand_off`]).
    pub(crate) fn transfer_slave_manager(
        &mut self,
        from: u64,
        to: u64,
        deploying: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if deploying {
            self.slave_manager_hand_off(from, rules);
        }
        let Some(manager) = self
            .substrate
            .entities
            .get_mut(from)
            .and_then(|entity| entity.slave_manager.take())
        else {
            return;
        };
        self.free_slaves(to, None, None, rules, registry);
        for slave in manager.slaves() {
            if let Some(entity) = self.substrate.entities.get_mut(slave) {
                entity.slave.owner = Some(to);
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(to) {
            entity.slave_manager = Some(manager);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cell_distance;

    #[test]
    fn cell_distance_truncates_the_approximate_root() {
        assert_eq!(cell_distance(0, 0), 0);
        assert_eq!(cell_distance(1, 0), 1);
        assert_eq!(cell_distance(1, 1), 1);
        assert_eq!(cell_distance(7, 7), 9);
        assert_eq!(cell_distance(-3, 4), 5);
    }
}
