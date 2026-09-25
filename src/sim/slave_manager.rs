//! `SlaveManagerClass` (Techno+0x2D8): the slaves of a Yuri Slave Miner or its
//! deployed refinery, and their harvest cycle.
//!
//! Evidence: `tools/spatial_oracle/slave_manager.{py,json,meta.json}` runs the
//! per-slave machine, the manager machine for a building owner, DeploySlaves
//! (through the original Unlimbo and Scatter), the slave's Mission_Harvest and
//! its deposit; `world/slave_manager_oracle_tests.rs` replays them through
//! this owner.
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
//!   Scatter away from the building).
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
//!   `0x006AF580`, with the hand-off `0x006B0D10`); undeploying hands it back.
//!
//! ## Residuals
//! - The Slave Miner's own hunt for a field is chain 6: manager states 1 to 3
//!   and 6, a unit owner's state 4 (`0x006AFFD6..0x006B002F`),
//!   HandleReturnedSlaves (`0x006B0DB0`), the Mission_Guard kick
//!   (ShouldRecallSlaves `0x006B1020`), the MEGAMISSION recall (`0x006B0C80`)
//!   and state 5's relocation (`0x006B0062..0x006B01F5`): when no ore lies
//!   within `SlaveMinerShortScan` of a refinery and a field is
//!   `SlaveMinerScanCorrection` closer elsewhere, retail sells the refinery
//!   back into a Slave Miner and drives it there; VERA keeps deploying slaves
//!   where it stands. Trigger: the ore around a Yuri refinery runs out.
//!   Effect: the refinery stays put and its slaves walk farther. Frequency:
//!   every Yuri game past the early field. Downstream: the Selling/undeploy
//!   chain is not entered.
//! - State 4 reads the building's BState (`+0x534`, zero while it builds up);
//!   VERA has no BState owner and reads "its mission is not Construction".
//! - The slave's Doing is not advanced by an InfantryClass::AI sequencer: a
//!   digging slave keeps Doing 38 while it walks home (retail: Walk), and a
//!   freed slave keeps 32 after its cheer. The shown sequence follows the
//!   animation cascade (`animation.rs`). A loaded slave's Carry walk (Doing
//!   39) is not represented.
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
    /// The constructor (`0x006AF1A0`) at `now`: a node per created slave, in
    /// state 0 with its timer at `now` for 0, and the AI timer at `now` for 10.
    pub(crate) fn new(
        slave_type: InternedId,
        slaves: impl IntoIterator<Item = u64>,
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
                    slave: Some(slave),
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
            let Some(slave) = self.construct_object_limbo_at_height(
                &slave_type,
                &owner,
                cell.0,
                cell.1,
                facing,
                z,
                rules,
            ) else {
                continue;
            };
            if let Some(entity) = self.substrate.entities.get_mut(slave) {
                entity.slave_owner = Some(master);
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
                let harvesting = entity.mission.current().known() == Some(MissionType::Harvest);
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
                // 0x006AFCE6: back to the type's Strength, then ready.
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

    /// The manager's own machine (`0x006AFD60`) for the states a deployed
    /// refinery reaches, and a unit owner's state 5.
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
        let mission = owner.mission.current().known();
        let Some(state) = self.slave_manager(master).map(SlaveManager::state) else {
            return;
        };
        match state {
            ManagerState::Ready => {
                // 0x006AFD7A: a building that is neither building up nor sold.
                if building
                    && !matches!(
                        mission,
                        Some(MissionType::Selling) | Some(MissionType::Construction)
                    )
                {
                    self.set_manager_state(master, ManagerState::Working, i32::MAX);
                }
            }
            ManagerState::Deployed => {
                // 0x006AFF8E: the refinery has built up (BState, module residual).
                if building && mission != Some(MissionType::Construction) {
                    self.set_manager_state(master, ManagerState::Working, i32::MAX);
                }
            }
            ManagerState::Working => {
                if unit {
                    // 0x006B003C: an undeployed Slave Miner stops working.
                    self.set_manager_state(master, ManagerState::Ready, now);
                    return;
                }
                // Module residual: the relocation check when no ore lies
                // within SlaveMinerShortScan.
                self.deploy_slaves(master, rules, registry);
            }
            ManagerState::Scanning
            | ManagerState::Travelling
            | ManagerState::Deploying
            | ManagerState::PackingUp => {}
        }
    }

    fn set_manager_state(&mut self, master: u64, state: ManagerState, frame: i32) {
        if let Some(manager) = self.slave_manager_mut(master) {
            manager.state = state;
            manager.frame = frame;
        }
    }

    /// `SlaveManagerClass::GetDeployCenter @ 0x006B0690`: a building's cell
    /// plus `(FoundationWidth - 1, FoundationHeight / 2)`, else the owner's
    /// own cell.
    pub(crate) fn slave_drop_cell(&self, master: u64, rules: &RuleSet) -> Option<(u16, u16)> {
        let owner = self.substrate.entities.get(master)?;
        let cell = (owner.position.rx, owner.position.ry);
        if owner.category != EntityCategory::Structure {
            return Some(cell);
        }
        let object = self.object_type(owner.type_ref(), rules)?;
        let (width, height) = crate::rules::foundation::foundation_dimensions(&object.foundation);
        // CellStruct shorts; the height halves with CDQ; SUB EAX,EDX; SAR 1.
        let dx = (i32::from(width) - 1) as i16;
        let dy = (i32::from(height) / 2) as i16;
        Some((
            (cell.0 as i16).wrapping_add(dx) as u16,
            (cell.1 as i16).wrapping_add(dy) as u16,
        ))
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
        storage > 0 && entity.slave_cargo.len() as i64 == i64::from(storage)
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
            entity.slave_owner = Some(master);
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
                entity.slave_cargo.push(bale);
            }
            return (harvest_rate, false);
        }
        self.slave_do_action(slave, DO_READY, rules);
        (1, true)
    }

    fn slave_do_action(&mut self, slave: u64, action: i32, rules: &RuleSet) {
        if let Err(cause) = self.infantry_do_action(slave, action, rules) {
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
            .map(|entity| std::mem::take(&mut entity.slave_cargo))
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
            .and_then(|entity| entity.slave_owner)
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
            entity.slave_owner = None;
        }
    }

    /// The destructors' slave links, at the pending-delete drain: a slave's
    /// InfantryClass destructor leaves its node (RemoveSlave at
    /// `0x00517E22`), and a master still holding its manager frees its
    /// slaves with no killer and no house (TechnoClass destructor
    /// `0x006F4571`). A rules-less drain (test fixtures) only drops the links.
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
                for slave in manager.slaves() {
                    if let Some(entity) = self.substrate.entities.get_mut(slave) {
                        entity.slave_owner = None;
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
            entity.slave_owner = None;
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

    /// SetOwner (`0x006AF580`): the manager moves from `from` to `to`. A
    /// manager `to` already holds (the one its constructor built) frees its
    /// slaves with no killer and no house, which UnInits them in limbo, and
    /// is deleted; every node's slave then names `to` as its SlaveOwner.
    /// `UnitClass::Deploy` first runs the hand-off `0x006B0D10`: an idle
    /// manager (state 0) moves to 4 and resets every slave that is not lost
    /// to Guard.
    pub(crate) fn transfer_slave_manager(
        &mut self,
        from: u64,
        to: u64,
        deploying: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if deploying {
            let reset = self
                .slave_manager_mut(from)
                .map_or_else(Vec::new, |manager| {
                    if manager.state != ManagerState::Ready {
                        return Vec::new();
                    }
                    manager.state = ManagerState::Deployed;
                    manager.frame = i32::MAX;
                    manager
                        .nodes
                        .iter()
                        .rev()
                        .filter(|node| node.state != SlaveState::Dead)
                        .filter_map(|node| node.slave)
                        .collect()
                });
            for slave in reset {
                self.reset_orders_to_guard(slave, rules);
            }
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
                entity.slave_owner = Some(to);
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
