//! A building's build-up and pack-up timing: the construction animation
//! (BState 0) that `BuildingClass::Begin_Mode(0)` (`0x00447780`) starts from
//! the type's control (`rules::buildup_asset_catalog`: first frame, frame
//! count, rate) and `BuildingClass::UpdateAnimation` (`0x004509D0`) steps once
//! per frame, watched by the Construction mission (a placed or deployed
//! building's build-up) or by the Selling mission (a sale's or an undeploy's
//! pack-up).
//!
//! Native, per frame of a building (`BuildingClass::Update`), in this order:
//! - `UpdateAnimation` (`0x0043FE22`): once the stage timer runs out
//!   (`CDTimerClass::GetTimeRemaining` 0) at a nonzero rate, the stage steps by
//!   one and the timer restarts for the rate (`0x004509E6..0x00450A2A`). While
//!   `Get_Mission` (the current mission, else the queued one) is Construction
//!   or Selling, a step onto the control's last frame sets `+0x6DD`
//!   (`0x004511DF`; an UndeploysInto sale with no ArchiveTarget also at stage
//!   `0x17`), a frame with no step at rate 0 sets it too (`0x00451218`), and a
//!   step past the end wraps to the first frame at the control's rate through
//!   `SpeedNormalize` (`0x004512D8..0x004512FC`). BState 1's control is the
//!   BuildingType constructor's `{0, 1, 0}` for every retail type.
//! - A ready building (`+0x6DD`, building vt+0x200 = `0x00454250`) out of
//!   BState 0 commences its queued mission and clears the byte
//!   (`0x0043FE27..0x0043FE54`).
//! - `TechnoClass::AI`'s mission dispatch (`MissionClass::AI 0x005B3060`),
//!   every frame for these missions (each visit returns a delay of 1).
//!   `Mission_Construction` (`0x00449A50`) restarts the animation on its first
//!   visit (status 0: `Begin_Mode(0)`, the radio broadcast 0xB and
//!   `[AudioVisual] Construction=`, retail `Dummy`) and completes on a later
//!   visit that finds `+0x6DD` (radio 0xC and 3, `Begin_Mode(1)`,
//!   `Grand_Opening`, Guard queued). `BuildingClass::Sell` plays stage 0, then
//!   stage 1 (`Begin_Mode(0)`, `+0x6DD` cleared; a tethered building (`+0x418`)
//!   waits in stage 1), then converts or sells on a stage-2 visit that finds
//!   `+0x6DD`.
//! - A ready building commences its queued mission (`0x0043FF91`).
//! - A queued BState applies (`0x0043FFB4..0x00440042`).
//!
//! Every placed or deployed building starts with Unlimbo's `Begin_Mode(0)` and
//! the Construction mission queued (`vt+0x484` = `0x0044D6A0`); the route
//! decides the rest:
//! - A computer house's factory places it and commences the mission at once
//!   (`BuildingClass::ExitObject 0x00445329..0x0044533F`): first visited the
//!   frame after, complete at N+1+(count-1)*rate.
//! - A human player's PLACE event (`HouseClass::Place_Production 0x004FB0E0`,
//!   `ExitObject` places nothing for a house `IsControlledByHuman`) leaves it
//!   queued, and the factory's OVER_OUT queues `Begin_Mode(1)`
//!   (`0x004FB4A6` -> `BuildingClass::Receive_Radio 0x0043CD01`): at the end
//!   of N+1 the building shows its idle body, at N+2 the idle control sets
//!   `+0x6DD`, which commences the mission, and it completes at
//!   N+2+(count-1)*rate.
//! - `UnitClass::Deploy` sets `+0x6DD` on the building it places
//!   (`0x0073984E`), so its first Update, in the creation frame D, commences
//!   the mission: complete at D+1+(count-1)*rate.
//!
//! VERA runs these frames in the late region (`tick_building_up` /
//! `tick_building_down`), after the object pass, where native runs them inside
//! each building's own Update: the completion's effects land later in the
//! same frame.
//!
//! Evidence: `tools/spatial_oracle/building_construction.json` `stepping`,
//! `mission` and `route` rows and `tools/spatial_oracle/building_sale.json`
//! `route` rows, replayed below. The route rows run the routes' entry points
//! and Update's pieces natively in the order read above; the rest of
//! `TechnoClass::AI` is not run.
//!
//! ## Dependency rules
//! - Part of sim/; sim/ never depends on render/, ui/, sidebar/, audio/, net/.

use crate::sim::components::{BuildingDown, BuildingUp, BuildupStage, ConstructionMission};
use crate::sim::game_options::GameOptions;
use crate::sim::timer::CdTimer;

/// UnitClass stage 0x17: an UndeploysInto sale with no ArchiveTarget completes
/// here (`0x00451186..0x004511DF`).
const ARCHIVE_LESS_UNDEPLOY_STAGE: i32 = 0x17;

impl BuildupStage {
    /// `Begin_Mode(0)`: the stage at the control's first frame, and the rate
    /// and its timer from `now` (`0x00447780`; BState 0 takes the control's
    /// rate unscaled).
    pub fn begin(control: [i32; 3], now: i32) -> Self {
        Self {
            control,
            stage: control[0],
            rate: control[2],
            timer: CdTimer::started(now, control[2]),
        }
    }

    /// One `UpdateAnimation` frame under the Construction or Selling mission:
    /// the step, then whether `+0x6DD` is set this frame.
    fn update(&mut self, now: i32, archive_less_sale: bool, options: &GameOptions) -> bool {
        let [start, count, control_rate] = self.control;
        let stepped = self.timer.expired(now) && self.rate != 0;
        if !stepped {
            return self.rate == 0;
        }
        self.stage = self.stage.wrapping_add(1);
        self.timer.start(now, self.rate);
        let end = start.wrapping_add(count);
        let done = self.stage == end.wrapping_sub(1)
            || (archive_less_sale && self.stage == ARCHIVE_LESS_UNDEPLOY_STAGE);
        if end <= self.stage {
            let rate = options.speed_normalize(control_rate);
            self.timer.start(now, rate);
            self.rate = rate;
            self.stage = start;
        }
        done
    }
}

/// What one frame of a build-up asks of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConstructionFrame {
    /// Nothing beyond the animation.
    Building,
    /// The Construction mission's visit found `+0x6DD`: the building is
    /// complete (`Grand_Opening`).
    Complete,
}

impl BuildingUp {
    /// The state every placement or deploy starts from at frame `now`:
    /// Unlimbo's `Begin_Mode(0)` and the Construction mission queued
    /// (`0x0044D6A0`).
    fn unlimboed(control: [i32; 3], now: i32) -> Self {
        Self {
            anim: BuildupStage::begin(control, now),
            idle: false,
            idle_queued: false,
            mission: ConstructionMission::Queued,
            done: false,
            first_frame: now.wrapping_add(1),
        }
    }

    /// A building a human player places at frame `now` (the command tail):
    /// queued, with the factory's OVER_OUT queuing `Begin_Mode(1)`.
    pub fn placed_by_player(control: [i32; 3], now: i32) -> Self {
        Self {
            idle_queued: true,
            ..Self::unlimboed(control, now)
        }
    }

    /// A building a computer house's factory places at frame `now`, after
    /// that frame's Logic pass: ExitObject commences the mission at once.
    pub fn placed_by_computer(control: [i32; 3], now: i32) -> Self {
        Self {
            mission: ConstructionMission::Due,
            ..Self::unlimboed(control, now)
        }
    }

    /// A building a unit deploys into at frame `now`, during the Logic pass:
    /// Deploy queues the mission (`0x007396D5`) and sets `+0x6DD`, and the
    /// building's first Update is in this frame.
    pub fn deployed(control: [i32; 3], now: i32) -> Self {
        Self {
            done: true,
            first_frame: now,
            ..Self::unlimboed(control, now)
        }
    }

    /// `Begin_Mode(0)`: BState 0 at once, the animation restarted, a queued
    /// BState dropped (`+0x538` back to -1). `+0x6DD` stays.
    fn begin_construction(&mut self, now: i32) {
        self.anim = BuildupStage::begin(self.anim.control, now);
        self.idle = false;
        self.idle_queued = false;
    }

    /// A ready building commences its queued mission (`Commence` succeeds
    /// only with one queued) and clears the byte.
    fn commence_if_ready(&mut self) {
        if self.done && self.mission == ConstructionMission::Queued {
            self.mission = ConstructionMission::Due;
            self.done = false;
        }
    }

    /// One frame of the building's Update (module doc, in its order).
    pub(crate) fn frame(&mut self, now: i32, options: &GameOptions) -> ConstructionFrame {
        if now < self.first_frame {
            return ConstructionFrame::Building;
        }
        // UpdateAnimation: the idle control never steps and sets +0x6DD
        // (rate 0); Get_Mission is Construction, queued or current.
        if self.idle || self.anim.update(now, false, options) {
            self.done = true;
        }
        if self.idle {
            self.commence_if_ready();
        }
        match self.mission {
            ConstructionMission::Due => {
                // Status 0: Begin_Mode(0) again (0x00449B36).
                self.begin_construction(now);
                self.mission = ConstructionMission::Watching;
            }
            ConstructionMission::Watching if self.done => return ConstructionFrame::Complete,
            ConstructionMission::Watching | ConstructionMission::Queued => {}
        }
        self.commence_if_ready();
        if std::mem::take(&mut self.idle_queued) {
            self.idle = true;
        }
        ConstructionFrame::Building
    }

    /// The frames from a human player's placement at frame 0
    /// ([`BuildingUp::placed_by_player`]) to the frame whose Construction
    /// visit completes it, or `None` when none does (a one-frame Buildup at a
    /// nonzero rate never rests on its last frame after a step). The tactical
    /// capture ledger derives its construction milestones from it.
    pub(crate) fn player_placement_frames_to_complete(
        control: [i32; 3],
        options: &GameOptions,
    ) -> Option<i32> {
        // A build-up completes by frame 2 + (count - 1) * rate, or 3 at rate 0.
        let limit = control[1]
            .max(1)
            .saturating_mul(control[2].max(1))
            .saturating_add(3);
        let mut building = Self::placed_by_player(control, 0);
        (1..=limit).find(|&now| building.frame(now, options) == ConstructionFrame::Complete)
    }

    /// A build-up whose Construction mission completes on the `ticks`-th
    /// frame from `current_frame` (fixtures): the old `{elapsed, total}`
    /// fixtures' `total - elapsed`.
    #[cfg(test)]
    pub(crate) fn completing_in_ticks(ticks: i32, current_frame: i32) -> Self {
        assert!(ticks >= 1, "a build-up completes on a frame to come");
        if ticks == 1 {
            return Self {
                mission: ConstructionMission::Watching,
                done: true,
                first_frame: current_frame,
                ..Self::unlimboed(
                    crate::rules::buildup_asset_catalog::NO_BUILDUP,
                    current_frame,
                )
            };
        }
        Self::placed_by_computer([0, ticks, 1], current_frame.wrapping_sub(1))
    }
}

/// What one frame of a pack-up asks of its owner. Every Sell visit first stops
/// the building's repair (`vt+0x19C(0)` = `BuildingClass::Repair 0x00446FF0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackUpFrame {
    /// The frame the sale started: no visit yet.
    NoVisit,
    /// Stage 0's visit (`0x0044A8DF`); stage 1 is next.
    StageZero,
    /// Stage 1's visit (`0x0044A2EE`): the owner broadcasts OVER_OUT and,
    /// unless the building is then tethered (`+0x418`), runs the stage and
    /// calls [`BuildingDown::begin_stage_two`]; a tethered building visits
    /// stage 1 again next frame.
    StageOne,
    /// A stage-2 visit still waiting for `+0x6DD`.
    Waiting,
    /// Stage 2's visit found `+0x6DD` (`0x00449CA7`): the building converts
    /// or is sold.
    Complete,
}

impl BuildingDown {
    /// The Selling mission commenced at frame `now`: its visits start the
    /// frame after.
    pub(crate) fn commenced(control: [i32; 3], now: i32, undeploy_order: bool) -> Self {
        Self {
            anim: BuildupStage::begin(control, now),
            commenced_frame: now,
            done: false,
            undeploy_order,
        }
    }

    /// One frame of the building's Update under the Selling mission:
    /// `UpdateAnimation` (the construction animation from stage 1's
    /// `Begin_Mode(0)`; before that the idle frames, whose `+0x6DD` stages 0
    /// and 1 clear), then the Sell visit. `status` is Sell's stage (`+0xBC`,
    /// the mission's handler state): 0, 1, or 2 (waiting).
    pub(crate) fn frame(
        &mut self,
        status: &mut u32,
        now: i32,
        archive_less_sale: bool,
        options: &GameOptions,
    ) -> PackUpFrame {
        if now == self.commenced_frame {
            return PackUpFrame::NoVisit;
        }
        // Before stage 1's Begin_Mode(0) the building shows BState 1, whose
        // idle control never steps and sets +0x6DD every frame (rate 0,
        // 0x00451218); stages 0 and 1 clear it.
        let anim_done = *status < 2 || self.anim.update(now, archive_less_sale, options);
        if anim_done {
            self.done = true;
        }
        match *status {
            0 => {
                // 0x0044AB61: +0x6DD cleared, stage 1 next (0x0044ABAC).
                self.done = false;
                *status = 1;
                PackUpFrame::StageZero
            }
            1 => PackUpFrame::StageOne,
            _ if self.done => PackUpFrame::Complete,
            _ => PackUpFrame::Waiting,
        }
    }

    /// Stage 1's tail (`0x0044A8A2..0x0044A8B5`): stage 2, `Begin_Mode(0)`
    /// and `+0x6DD` cleared.
    pub(crate) fn begin_stage_two(&mut self, status: &mut u32, now: i32) {
        *status = 2;
        self.anim = BuildupStage::begin(self.anim.control, now);
        self.done = false;
    }
}

#[cfg(test)]
impl crate::sim::game_entity::GameEntity {
    /// Put the building's pack-up at its last frame: its next frame completes
    /// (fixtures).
    pub(crate) fn finish_pack_up_for_test(&mut self) {
        let down = self.building_down.as_mut().expect("a pack-up");
        down.done = true;
        down.commenced_frame = i32::MIN;
        self.mission.set_handler_state(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn corpus() -> Value {
        serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_construction.json"
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
            let mut anim = BuildupStage::begin(control(input), origin);
            let mut done = false;
            for frame in frames.iter().skip(1) {
                let now = origin + frame["frame"].as_i64().unwrap() as i32;
                // Only Construction and Selling gate the completion; the Guard
                // row's fixture building answers vt+0x3FC false, which opens
                // the same gate.
                if anim.update(now, archive_less_sale, &options) {
                    done = true;
                }
                let context = format!("{name} frame {}", frame["frame"]);
                assert_eq!(
                    i64::from(anim.stage),
                    frame["stage"].as_i64().unwrap(),
                    "{context}: stage"
                );
                assert_eq!(
                    i64::from(anim.rate),
                    frame["rate"].as_i64().unwrap(),
                    "{context}: rate"
                );
                assert_eq!(
                    [
                        i64::from(anim.timer.start_frame()),
                        i64::from(anim.timer.duration())
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

    /// A computer house's placement (`Begin_Mode(0)` and the commenced
    /// mission at frame 0), then per frame `UpdateAnimation` and a
    /// `Mission_Construction` visit, as `BuildingUp::frame` runs them: the
    /// stage, its timer, `+0x6DD` and the frame whose visit completes the
    /// build-up (`Grand_Opening`).
    #[test]
    fn construction_matches_the_original_mission_visits() {
        let corpus = corpus();
        let mut compared = 0;
        for row in corpus["mission"].as_array().unwrap() {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let frames = row["frames"].as_array().unwrap();
            // The first visit restarts the timer at its own frame.
            let origin = frames[0]["timer"][0].as_i64().unwrap() as i32 - 1;
            let mut building = BuildingUp::placed_by_computer(control(input), origin);
            for frame in frames {
                let now = origin + frame["frame"].as_i64().unwrap() as i32;
                let step = building.frame(now, &options(0));
                let completed = frame["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|call| call[0] == "grand_opening");
                let context = format!("{name} frame {}", frame["frame"]);
                assert_eq!(
                    step == ConstructionFrame::Complete,
                    completed,
                    "{context}: complete"
                );
                assert_eq!(
                    i64::from(building.anim.stage),
                    frame["stage"].as_i64().unwrap(),
                    "{context}: stage"
                );
                assert_eq!(
                    u64::from(building.done),
                    frame["done"].as_u64().unwrap(),
                    "{context}: +0x6DD"
                );
                assert_eq!(
                    [
                        i64::from(building.anim.timer.start_frame()),
                        i64::from(building.anim.timer.duration())
                    ],
                    [
                        frame["timer"][0].as_i64().unwrap(),
                        frame["timer"][1].as_i64().unwrap()
                    ],
                    "{context}: timer"
                );
            }
            compared += 1;
        }
        assert_eq!(compared, 5);
    }

    fn int(frame: &Value, key: &str) -> i64 {
        frame[key].as_i64().unwrap()
    }

    /// The native Construction mission state: queued behind none, current
    /// before its first visit, or visited.
    fn native_mission(frame: &Value) -> Option<ConstructionMission> {
        match (
            int(frame, "mission"),
            int(frame, "queue"),
            int(frame, "status"),
        ) {
            (-1, 0x12, _) => Some(ConstructionMission::Queued),
            (0x12, -1, 0) => Some(ConstructionMission::Due),
            (0x12, -1, 1) => Some(ConstructionMission::Watching),
            _ => None,
        }
    }

    /// A human player's placement, a computer house's and a deploy, frame by
    /// frame through Update's construction pieces: BState and the queued one,
    /// the stage, `+0x6DD`, the mission and the completion frame; and the
    /// player's completion as the tactical ledger derives it.
    #[test]
    fn placement_routes_match_the_original_update_order() {
        let corpus = corpus();
        let mut compared = 0;
        for row in corpus["route"].as_array().unwrap() {
            let input = &row["input"];
            let route = input["route"].as_str().unwrap();
            if route == "sale" {
                continue;
            }
            let name = input["name"].as_str().unwrap();
            let mut building = match route {
                "player" => BuildingUp::placed_by_player(control(input), 0),
                "computer" => BuildingUp::placed_by_computer(control(input), 0),
                "deploy" => BuildingUp::deployed(control(input), 0),
                other => panic!("route {other}"),
            };
            let mut completed_at = None;
            for frame in row["frames"].as_array().unwrap() {
                let now = int(frame, "frame") as i32;
                let complete = building.frame(now, &options(0)) == ConstructionFrame::Complete;
                let context = format!("{name} frame {now}");
                assert_eq!(
                    complete,
                    frame["grand_opening"] == true,
                    "{context}: complete"
                );
                if complete {
                    completed_at = Some(now);
                    break;
                }
                assert_eq!(
                    i64::from(building.idle),
                    int(frame, "bstate"),
                    "{context}: BState"
                );
                assert_eq!(
                    if building.idle_queued { 1 } else { -1 },
                    int(frame, "queued_bstate"),
                    "{context}: queued BState"
                );
                // The idle control's stage is its first frame.
                let stage = if building.idle {
                    0
                } else {
                    building.anim.stage
                };
                assert_eq!(i64::from(stage), int(frame, "stage"), "{context}: stage");
                assert_eq!(
                    u64::from(building.done),
                    frame["done"].as_u64().unwrap(),
                    "{context}: +0x6DD"
                );
                assert_eq!(
                    Some(building.mission),
                    native_mission(frame),
                    "{context}: mission"
                );
            }
            if route == "player" {
                assert_eq!(
                    BuildingUp::player_placement_frames_to_complete(control(input), &options(0)),
                    completed_at,
                    "{name}: frames to complete"
                );
            }
            compared += 1;
        }
        assert_eq!(compared, 18);
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
        let mut down = BuildingDown::commenced(control(input), 0, false);
        let mut status = 0;
        let mut completed = false;
        for frame in row["frames"].as_array().unwrap() {
            let now = int(frame, "frame") as i32;
            let context = format!("{name} frame {now}");
            let step = down.frame(&mut status, now, archive_less_sale, &options(0));
            let calls = frame["calls"].as_array().unwrap();
            assert_eq!(
                step == PackUpFrame::StageZero,
                calls.iter().any(|call| broadcast(call, 0x17)),
                "{context}: stage 0"
            );
            assert_eq!(
                step == PackUpFrame::StageOne,
                calls.iter().any(|call| broadcast(call, 3)),
                "{context}: stage 1"
            );
            if step == PackUpFrame::StageOne && !tethered(now) {
                down.begin_stage_two(&mut status, now);
            }
            assert_eq!(
                step == PackUpFrame::Complete,
                frame["converts"] == true,
                "{context}: completes"
            );
            if step == PackUpFrame::Complete {
                completed = true;
                break;
            }
            assert_eq!(i64::from(status), int(frame, "status"), "{context}: Sell stage");
            let construction = status >= 2;
            assert_eq!(
                i64::from(!construction),
                int(frame, "bstate"),
                "{context}: BState"
            );
            let stage = if construction { down.anim.stage } else { 0 };
            assert_eq!(i64::from(stage), int(frame, "stage"), "{context}: stage");
            assert_eq!(
                u64::from(down.done),
                frame["done"].as_u64().unwrap(),
                "{context}: +0x6DD"
            );
        }
        assert!(completed, "{name}: the row completes");
    }

    /// An UndeploysInto sale of an idle building, with and without an
    /// ArchiveTarget, frame by frame through Update's pieces and
    /// `BuildingClass::Sell`: Sell's stage, BState, the stage, `+0x6DD` and
    /// the frame whose stage-2 visit finds `+0x6DD` (the conversion).
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
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_sale.json"
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
