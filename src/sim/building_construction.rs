//! BuildingClass body control and transient placement/sale descriptors.
//!
//! Native447780 Begin_Mode,4509D0 UpdateAnimation and43FFB4 queued body
//! application share the entity's sole StageClass. Retained BState/queued BState
//! live here; MissionCom owns Construction/Selling status and cadence, and
//! MissionLeaf owns the one ready byte+6DD. Building43FB20 calls the animation,
//! pre-ready promotion, mission, post-ready promotion and queued body in that
//! order inside each live Logic visit. Evidence: original execution in
//! tools/spatial_oracle/building_construction.json; integrated comparisons live
//! beside world/techno_ai/building_construction_tests.rs.

use crate::sim::game_options::GameOptions;
use crate::sim::stage::StageClass;

const ARCHIVE_LESS_UNDEPLOY_STAGE: i32 = 0x17;

/// Begin_Mode's body modes. Construction and sale use 0 and 1; a nuclear
/// silo's Mission_Missile also steps through Active (2), Aux1 (4) and Aux2 (5)
/// (`0x0044C9C7`, `0x0044CA88`, `0x0044CCC5`). Gate animation modes remain
/// with the separate gate mechanism.
///
/// Begin_Mode reads each mode's {start, count, rate} from BuildingType
/// `+0xF04 + 12 * mode` (art `AnimIdle=`, `AnimActive=`, `AnimAux1=`,
/// `AnimAux2=`; constructor {0,1,0}). Only Construction binds its control
/// here; the others take the constructor's {0,1,0}.
/// RESIDUAL: art `AnimActive=` (stock GAWEAP/YAWEAP/NAWEAP {0,1,0}, the
/// construction yards, NAINDP and YACOMD {0,26,3}, the repair depots and
/// CAOUTP {0,7,2}) is not bound. No ported caller begins Active on those
/// types; the silo (NAMISL) authors none of the four keys, so its modes run
/// the constructor control exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildingBodyMode {
    Construction,
    Idle,
    Active,
    Aux1,
    Aux2,
}

impl BuildingBodyMode {
    const fn raw(self) -> i32 {
        match self {
            Self::Construction => 0,
            Self::Idle => 1,
            Self::Active => 2,
            Self::Aux1 => 4,
            Self::Aux2 => 5,
        }
    }
}

/// Native Building+534/+538 and a derived immutable type-control cache. The
/// cache is bound by the shared type initializer (or explicit oracle fixture);
/// it is not an animation clock or a second mission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingBody {
    state: i32,
    queued: i32,
    construction_control: [i32; 3],
}

impl Default for BuildingBody {
    fn default() -> Self {
        Self {
            state: -1,
            queued: -1,
            construction_control: crate::rules::buildup_asset_catalog::NO_BUILDUP,
        }
    }
}

impl BuildingBody {
    pub(crate) fn bind_control(&mut self, control: [i32; 3]) {
        self.construction_control = control;
    }
    pub(crate) const fn construction_control(&self) -> [i32; 3] {
        self.construction_control
    }
    pub(crate) const fn state(&self) -> i32 {
        self.state
    }
    pub(crate) const fn queued(&self) -> i32 {
        self.queued
    }
    fn control(&self) -> [i32; 3] {
        if self.state == 0 {
            self.construction_control
        } else {
            crate::rules::buildup_asset_catalog::NO_BUILDUP
        }
    }

    ///447794 always publishes the request;44779A..4477BC applies an initial
    /// mode or Construction immediately. Map initialization uses its explicit
    /// scenario-init path below.
    pub(crate) fn begin(&mut self, mode: BuildingBodyMode, stage: &mut StageClass, now: i32) {
        self.queued = mode.raw();
        if self.state == -1 || mode == BuildingBodyMode::Construction {
            self.apply(stage, now, false, &GameOptions::default());
        }
    }
    pub(crate) fn initialize_idle(&mut self, stage: &mut StageClass, now: i32) {
        self.queued = 1;
        self.apply(stage, now, false, &GameOptions::default());
    }
    ///43FFB4..440042: an identical queued mode only clears the request.
    /// Different stock modes restart through SpeedNormalize, even mode0.
    pub(crate) fn apply_queued(&mut self, stage: &mut StageClass, now: i32, options: &GameOptions) {
        if self.queued != -1 {
            if self.queued != self.state {
                self.apply(stage, now, true, options);
            } else {
                self.queued = -1;
            }
        }
    }
    fn apply(
        &mut self,
        stage: &mut StageClass,
        now: i32,
        queued_tail: bool,
        options: &GameOptions,
    ) {
        self.state = self.queued;
        self.queued = -1;
        let [start, _, mut rate] = self.control();
        if queued_tail {
            rate = options.speed_normalize(rate);
        }
        // Immediate Construction uses raw rate. Idle's stock rate is0,
        // including when Type+d23 requests normalization (447A63).
        stage.restart(start, now, rate);
    }
    ///4509DE StageClass step,451145..451218 ready admission and451296 wrap.
    /// `ready_allowed` is !HasTurret OR effective Construction/Selling. Ready
    /// stays set until its mission or successful Commence clears it.
    pub(crate) fn update(
        &self,
        stage: &mut StageClass,
        now: i32,
        archive_less_sale: bool,
        ready_allowed: bool,
        options: &GameOptions,
    ) -> bool {
        let stepped = stage.advance(now);
        if !ready_allowed {
            return false;
        }
        if !stepped {
            return self.state == -1 || stage.rate() == 0;
        }
        let [start, count, control_rate] = self.control();
        let end = start.wrapping_add(count);
        let done = stage.value() == end.wrapping_sub(1)
            || (archive_less_sale && stage.value() == ARCHIVE_LESS_UNDEPLOY_STAGE);
        if stage.value() >= end {
            stage.restart(start, now, options.speed_normalize(control_rate));
        }
        done
    }
}

#[derive(Debug, Clone, Copy)]
enum ConstructionEntry {
    Queued,
    Commenced,
    DeployReady,
    #[cfg(test)]
    WatchingReady,
}

/// Transient Unlimbo/ExitObject/Deploy packet, never stored, hashed or serialized.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BuildingUp {
    control: [i32; 3],
    entry: ConstructionEntry,
}

impl BuildingUp {
    pub(crate) const fn placed_by_player(control: [i32; 3], _now: i32) -> Self {
        Self {
            control,
            entry: ConstructionEntry::Queued,
        }
    }
    pub(crate) const fn placed_by_computer(control: [i32; 3], _now: i32) -> Self {
        Self {
            control,
            entry: ConstructionEntry::Commenced,
        }
    }
    pub(crate) const fn deployed(control: [i32; 3], _now: i32) -> Self {
        Self {
            control,
            entry: ConstructionEntry::DeployReady,
        }
    }
    pub(crate) fn install(self, entity: &mut crate::sim::game_entity::GameEntity, now: i32) {
        use crate::sim::mission::{MissionId, MissionType};
        entity.bind_building_construction_control(self.control);
        entity.begin_building_body(BuildingBodyMode::Construction, now);
        crate::sim::mission::authority::queue_entity_mission_deferred(
            entity,
            MissionId::from_known(MissionType::Construction),
        );
        match self.entry {
            ConstructionEntry::Queued => {}
            ConstructionEntry::Commenced => {
                crate::sim::mission::authority::commence_entity_mission(entity, now as u32);
            }
            ConstructionEntry::DeployReady => entity.mission_leaf.set_building_ready_latch(1),
            #[cfg(test)]
            ConstructionEntry::WatchingReady => {
                crate::sim::mission::authority::commence_entity_mission(entity, now as u32);
                entity.mission.set_handler_state(1);
                entity.mission_leaf.set_building_ready_latch(1);
            }
        }
    }
    /// Presentation ledger only. Original stock human route completes at
    /// N+2+(count-1)*rate, or N+3 for rate0. One-frame nonzero-rate controls never
    /// land on their last frame after a step. Native route rows validate this
    /// estimate; it never participates in simulation admission.
    pub(crate) fn player_placement_frames_to_complete(
        control: [i32; 3],
        _options: &GameOptions,
    ) -> Option<i32> {
        let [_, count, rate] = control;
        if rate == 0 {
            Some(3)
        } else if count > 1 && rate > 0 {
            Some(2i32.saturating_add(count.saturating_sub(1).saturating_mul(rate)))
        } else {
            None
        }
    }
    #[cfg(test)]
    pub(crate) fn completing_in_ticks(ticks: i32, current_frame: i32) -> Self {
        assert!(ticks >= 1);
        if ticks == 1 {
            Self {
                control: crate::rules::buildup_asset_catalog::NO_BUILDUP,
                entry: ConstructionEntry::WatchingReady,
            }
        } else {
            Self::placed_by_computer([0, ticks, 1], current_frame)
        }
    }
}

/// Sale route bookkeeping; body, ready and mission state stay with their owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingDown {
    undeploy_order: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackUpFrame {
    StageZero,
    StageOne,
    Waiting,
    Complete,
}
impl BuildingDown {
    pub(crate) const fn commenced(_now: i32, undeploy_order: bool) -> Self {
        Self { undeploy_order }
    }
    pub(crate) const fn undeploy_order(self) -> bool {
        self.undeploy_order
    }
    pub(crate) fn visit(self, status: &mut u32, ready: bool) -> PackUpFrame {
        match *status {
            0 => {
                *status = 1;
                PackUpFrame::StageZero
            }
            1 => PackUpFrame::StageOne,
            _ if ready => PackUpFrame::Complete,
            _ => PackUpFrame::Waiting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_intern;
    use serde_json::Value;

    fn corpus() -> Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_construction.json",
        ))
        .unwrap()
    }

    fn options(speed: i64) -> GameOptions {
        GameOptions {
            game_speed: speed as i32,
            ..GameOptions::default()
        }
    }

    fn control(input: &Value) -> [i32; 3] {
        serde_json::from_value(input["control"].clone()).unwrap()
    }

    /// Only the constructor clock and Building control receiver are exercised
    /// here. Type controls and subsequent expected fields come from the saved
    /// original rows, independently of this minimal actor's unrelated fields.
    fn actor(origin: i32) -> GameEntity {
        GameEntity::new_at_frame_for_test(
            1,
            0,
            0,
            0,
            0,
            test_intern("OracleOwner"),
            Health { current: 100 },
            test_intern("OracleBuilding"),
            EntityCategory::Structure,
            0,
            0,
            false,
            origin as u32,
        )
    }

    /// `Begin_Mode(0)` then `UpdateAnimation` frame by frame: the stage, its
    /// timer and rate, and `+0x6DD`, under Construction and Selling (with the
    /// archive-less UndeploysInto sale's stage-0x17 completion) and past the
    /// last frame (the wrap through SpeedNormalize).
    #[test]
    fn stepping_matches_the_original_update_animation() {
        let corpus = corpus();
        let mut compared = 0;
        for row in corpus["stepping"].as_array().unwrap() {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let options = options(row["game_speed"].as_i64().unwrap());
            let frames = row["frames"].as_array().unwrap();
            let origin = frames[0]["timer"][0].as_i64().unwrap() as i32;
            let mission = input["mission"].as_str().unwrap_or("construction");
            let archive_less_sale =
                mission == "selling" && input["undeploys"] == true && input["archive"] != true;
            let mut body = BuildingBody::default();
            body.bind_control(control(input));
            let mut stage = StageClass::constructed(origin);
            body.begin(BuildingBodyMode::Construction, &mut stage, origin);
            let mut done = false;
            for frame in frames.iter().skip(1) {
                let now = origin + frame["frame"].as_i64().unwrap() as i32;
                // Only Construction and Selling gate the completion; the Guard
                // row's fixture building answers vt+0x3FC false, which opens
                // the same gate.
                if body.update(&mut stage, now, archive_less_sale, true, &options) {
                    done = true;
                }
                let context = format!("{name} frame {}", frame["frame"]);
                assert_eq!(
                    i64::from(stage.value()),
                    frame["stage"].as_i64().unwrap(),
                    "{context}: stage"
                );
                assert_eq!(
                    i64::from(stage.rate()),
                    frame["rate"].as_i64().unwrap(),
                    "{context}: rate"
                );
                assert_eq!(
                    [
                        i64::from(stage.timer().start_frame()),
                        i64::from(stage.timer().duration())
                    ],
                    [
                        frame["timer"][0].as_i64().unwrap(),
                        frame["timer"][1].as_i64().unwrap()
                    ],
                    "{context}: timer"
                );
                assert_eq!(
                    u64::from(done),
                    frame["done"].as_u64().unwrap(),
                    "{context}: +0x6DD"
                );
            }
            compared += 1;
        }
        assert_eq!(compared, 12);
    }

    fn int(frame: &Value, key: &str) -> i64 {
        frame[key].as_i64().unwrap()
    }

    /// A row's call of the broadcast (`0x0065ACE0`) with `message`: stage 0
    /// broadcasts RUN_AWAY (0x17), each stage-1 visit OVER_OUT (3).
    fn broadcast(call: &Value, message: i64) -> bool {
        call[0] == "radio" && call[1] == message
    }

    /// Replays one route row's Sell visits from the order at frame 0;
    /// `tethered` is the building's `+0x418` at each frame's stage-1 visit.
    fn replay_sell_visits(row: &Value, archive_less_sale: bool, tethered: impl Fn(i32) -> bool) {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut actor = actor(0);
        // The recorded sale begins on the live idle control, without a
        // Begin_Mode(0) restart until the actual stage-one tail.
        actor.bind_building_construction_control(control(input));
        actor.begin_building_body(BuildingBodyMode::Idle, 0);
        actor.install_building_down(BuildingDown::commenced(0, false));
        let mut status = 0;
        let mut completed = false;
        for frame in row["frames"].as_array().unwrap() {
            let now = int(frame, "frame") as i32;
            let context = format!("{name} frame {now}");
            // SELL was the command tail at0; no further object visit that
            //frame. The scheduler, not a second countdown, owns this fact.
            let step = if now == 0 {
                None
            } else {
                actor.advance_building_body(now, archive_less_sale, false, &options(0));
                Some(actor.advance_building_down(&mut status))
            };
            let calls = frame["calls"].as_array().unwrap();
            assert_eq!(
                step == Some(PackUpFrame::StageZero),
                calls.iter().any(|call| broadcast(call, 0x17)),
                "{context}: stage 0"
            );
            assert_eq!(
                step == Some(PackUpFrame::StageOne),
                calls.iter().any(|call| broadcast(call, 3)),
                "{context}: stage 1"
            );
            if step == Some(PackUpFrame::StageOne) && !tethered(now) {
                actor.begin_building_pack_up_stage_two(&mut status, now);
            }
            assert_eq!(
                step == Some(PackUpFrame::Complete),
                frame["converts"] == true,
                "{context}: completes"
            );
            if step == Some(PackUpFrame::Complete) {
                completed = true;
                break;
            }
            assert_eq!(
                i64::from(status),
                int(frame, "status"),
                "{context}: Sell stage"
            );
            let construction = status >= 2;
            assert_eq!(
                i64::from(!construction),
                int(frame, "bstate"),
                "{context}: BState"
            );
            let stage = actor.native_stage();
            assert_eq!(
                i64::from(stage.value()),
                int(frame, "stage"),
                "{context}: stage"
            );
            assert_eq!(
                u64::from(actor.building_ready_latch()),
                frame["done"].as_u64().unwrap(),
                "{context}: +0x6DD"
            );
        }
        assert!(completed, "{name}: the row completes");
    }

    /// An UndeploysInto sale of an idle building, with and without an
    /// ArchiveTarget, frame by frame through Update's pieces and
    /// `BuildingClass::Mission_Selling`: its stage, BState, the stage, `+0x6DD`
    /// and the frame whose stage-2 visit finds `+0x6DD` (the conversion).
    #[test]
    fn undeploy_sales_match_the_original_sell_visits() {
        let corpus = corpus();
        let mut compared = 0;
        for row in corpus["route"].as_array().unwrap() {
            let input = &row["input"];
            if input["route"] != "sale" {
                continue;
            }
            replay_sell_visits(row, input["archive"] != true, |_| false);
            compared += 1;
        }
        assert_eq!(compared, 8);
    }

    /// A sale of a building that does not undeploy, from the SELL event
    /// (`Sell_Back(-1)`) at frame 0: stage 0's RUN_AWAY (0x17), stage 1's
    /// OVER_OUT (3) on every visit while the building stays tethered
    /// (`+0x418`), then the stage-2 visit that finds `+0x6DD`
    /// (`tools/spatial_oracle/building_sale.json` `route` rows; the row whose
    /// type has no Buildup is Sell_Back's refusal, `production_sell`'s).
    #[test]
    fn sales_match_the_original_sell_visits() {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_sale.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["route"].as_array().unwrap() {
            let input = &row["input"];
            if input["buildup"] == false {
                continue;
            }
            let tether_until = input["tether_until"].as_i64();
            replay_sell_visits(row, false, |now| {
                tether_until.is_some_and(|until| i64::from(now) < until)
            });
            compared += 1;
        }
        assert_eq!(compared, 9);
    }
}
