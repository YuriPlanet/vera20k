//! A building's build-up and pack-up timing: the construction animation
//! (BState 0) that `BuildingClass::Begin_Mode(0)` (`0x00447780`) starts from
//! the type's control (`rules::buildup_asset_catalog`: first frame, frame
//! count, rate) and `BuildingClass::UpdateAnimation` (`0x004509D0`) steps once
//! per frame, watched by the Construction mission (a placed or deployed
//! building's build-up) or by the Selling mission's UndeploysInto arm (an
//! undeploy's pack-up).
//!
//! Native, per frame of a building (`BuildingClass::Update`):
//! - `UpdateAnimation` (`0x0043FE22`): once the stage timer runs out
//!   (`CDTimerClass::GetTimeRemaining` 0) at a nonzero rate, the stage steps by
//!   one and the timer restarts for the rate (`0x004509E6..0x00450A2A`). Under
//!   the Construction or Selling mission, a step onto the control's last frame
//!   sets `+0x6DD` (`0x004511DF`; an UndeploysInto sale with no ArchiveTarget
//!   also at stage `0x17`), a frame with no step at rate 0 sets it too
//!   (`0x00451218`), and a step past the end wraps to the first frame at the
//!   control's rate through `SpeedNormalize` (`0x004512D8..0x004512FC`).
//! - The mission visit (`TechnoClass::AI`'s dispatch, `0x0043FE56`), every
//!   frame (each returns a delay of 1). `Mission_Construction` (`0x00449A50`)
//!   restarts the animation on its first visit (status 0: `Begin_Mode(0)`, the
//!   radio broadcast 0xB and `[AudioVisual] Construction=`, retail `Dummy`)
//!   and completes on a later visit that finds `+0x6DD` (radio 0xC and 3,
//!   `Begin_Mode(1)`, `Grand_Opening`, Guard queued). `BuildingClass::Sell`
//!   plays stage 0, then stage 1 (`Begin_Mode(0)`, `+0x6DD` cleared), then
//!   converts on a stage-2 visit that finds `+0x6DD`.
//! - A queued mission commences once `+0x6DD` is set (`0x0043FF91`, building
//!   vt+0x200 = `0x00454250`); UnitClass::Deploy sets it on the building it
//!   places (`0x0073984E`) so the Construction mission it queued commences in
//!   the building's first Update.
//!
//! So a building placed at frame N (the command tail; its Unlimbo's
//! `Begin_Mode(0)`, TechnoClass::Unlimbo vt+0x484 `0x0044D6A0`, and
//! ExitObject's commence) is first visited at N+1 and completes at
//! N+1+(count-1)*rate; a deployed one commences in its creation frame and
//! follows the same count. VERA runs these frames in the late region
//! (`tick_building_up`/`tick_building_down`), after the object pass where
//! native runs them inside the building's own Update: the completion's effects
//! land later in the same frame.
//!
//! Evidence: `tools/spatial_oracle/building_construction.json` `stepping`
//! and `mission` rows, replayed below.
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
    /// A building placed from production at frame `now`: its Unlimbo's
    /// `Begin_Mode(0)` and ExitObject's commence (`0x00445329..0x0044533F`).
    pub fn placed(control: [i32; 3], now: i32) -> Self {
        Self {
            anim: BuildupStage::begin(control, now),
            mission: ConstructionMission::Commenced { since: now },
            done: false,
        }
    }

    /// A building a unit deploys into at frame `now`: `Begin_Mode(0)` at its
    /// Unlimbo, the Construction mission queued (`0x007396D5`) and `+0x6DD`
    /// set so its first Update commences it (`0x0073984E`).
    pub fn deployed(control: [i32; 3], now: i32) -> Self {
        Self {
            anim: BuildupStage::begin(control, now),
            mission: ConstructionMission::Queued,
            done: true,
        }
    }

    /// One frame of the building's Update: `UpdateAnimation`, the mission
    /// visit, then the ready-to-commence check.
    pub(crate) fn frame(&mut self, now: i32, options: &GameOptions) -> ConstructionFrame {
        if let ConstructionMission::Commenced { since } = self.mission
            && now == since
        {
            // Placed after this frame's Logic pass: no Update this frame.
            return ConstructionFrame::Building;
        }
        if self.anim.update(now, false, options) {
            self.done = true;
        }
        match self.mission {
            ConstructionMission::Queued => {
                // No current mission to visit; 0x0043FF91 commences the
                // queued one on the ready byte and clears it.
                if self.done {
                    self.done = false;
                    self.mission = ConstructionMission::Commenced { since: now };
                }
                ConstructionFrame::Building
            }
            ConstructionMission::Commenced { .. } => {
                // Status 0: Begin_Mode(0) again (0x00449B36); +0x6DD stays.
                let done = self.done;
                self.anim = BuildupStage::begin(self.anim.control, now);
                self.done = done;
                self.mission = ConstructionMission::Watching;
                ConstructionFrame::Building
            }
            ConstructionMission::Watching if self.done => ConstructionFrame::Complete,
            ConstructionMission::Watching => ConstructionFrame::Building,
        }
    }

    /// The frames from a placement at frame 0 ([`BuildingUp::placed`]) to the
    /// frame whose Construction visit completes it, or `None` when none does
    /// (a one-frame Buildup at a nonzero rate never rests on its last frame
    /// after a step). The tactical capture ledger derives its construction
    /// milestones from it.
    pub(crate) fn placement_frames_to_complete(
        control: [i32; 3],
        options: &GameOptions,
    ) -> Option<i32> {
        // A build-up completes by frame 1 + (count - 1) * rate, or 2 at rate 0.
        let limit = control[1]
            .max(1)
            .saturating_mul(control[2].max(1))
            .saturating_add(2);
        let mut building = Self::placed(control, 0);
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
                anim: BuildupStage::begin(
                    crate::rules::buildup_asset_catalog::NO_BUILDUP,
                    current_frame,
                ),
                mission: ConstructionMission::Watching,
                done: true,
            };
        }
        Self::placed([0, ticks, 1], current_frame.wrapping_sub(1))
    }
}

/// What one frame of a pack-up asks of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackUpFrame {
    /// Nothing beyond the animation.
    PackingUp,
    /// Stage 0's visit (`0x0044A8DF`): the undeploy voice and the type's
    /// `DeploySound=` at its Location.
    StageZero,
    /// Stage 2's visit found `+0x6DD`: the building converts.
    Convert,
}

impl BuildingDown {
    /// One frame of the building's Update under Selling's UndeploysInto arm:
    /// `UpdateAnimation` (the construction animation from stage 1's
    /// `Begin_Mode(0)`; before that the idle frames, whose `+0x6DD` stages 0
    /// and 1 clear), then the Sell visit.
    pub(crate) fn frame(
        &mut self,
        now: i32,
        archive_less_sale: bool,
        options: &GameOptions,
    ) -> PackUpFrame {
        if now == self.commenced_frame {
            return PackUpFrame::PackingUp;
        }
        if self.sell_stage >= 2 && self.anim.update(now, archive_less_sale, options) {
            self.done = true;
        }
        match self.sell_stage {
            0 => {
                // 0x0044AB61: +0x6DD cleared, stage 1 next.
                self.done = false;
                self.sell_stage = 1;
                PackUpFrame::StageZero
            }
            1 => {
                // 0x0044A2EE..: Begin_Mode(0), +0x6DD cleared, stage 2.
                self.anim = BuildupStage::begin(self.anim.control, now);
                self.done = false;
                self.sell_stage = 2;
                PackUpFrame::PackingUp
            }
            _ if self.done => PackUpFrame::Convert,
            _ => PackUpFrame::PackingUp,
        }
    }

    /// Put the pack-up at its last frame: its next frame converts (fixtures).
    #[cfg(test)]
    pub(crate) fn finish_for_test(&mut self) {
        self.sell_stage = 2;
        self.done = true;
        self.commenced_frame = i32::MIN;
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

    /// A placement's `Begin_Mode(0)` at frame 0, then per frame
    /// `UpdateAnimation` and a `Mission_Construction` visit, as
    /// `BuildingUp::frame` runs them: the stage, `+0x6DD` and the frame whose
    /// visit completes the build-up (`Grand_Opening`).
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
            let mut building = BuildingUp::placed(control(input), origin);
            let mut completed_at = None;
            for frame in frames {
                let now = origin + frame["frame"].as_i64().unwrap() as i32;
                let step = building.frame(now, &options(0));
                let completed = frame["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|call| call[0] == "grand_opening");
                if completed {
                    completed_at = Some(now - origin);
                }
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
            assert!(completed_at.is_some(), "{name}: no Grand_Opening");
            assert_eq!(
                BuildingUp::placement_frames_to_complete(control(input), &options(0)),
                completed_at,
                "{name}: frames to complete"
            );
            compared += 1;
        }
        assert_eq!(compared, 5);
    }

    /// A deployed building commences in its first Update (clearing the ready
    /// byte Deploy set) and is first visited the frame after, so it completes
    /// at its creation frame + 1 + (count - 1) * rate, like a placement.
    #[test]
    fn a_deployed_building_completes_one_count_after_its_creation_frame() {
        let options = options(0);
        let (created, control) = (100, [0, 25, 2]);
        let mut building = BuildingUp::deployed(control, created);
        let mut completed_at = None;
        for now in created..created + 60 {
            if building.frame(now, &options) == ConstructionFrame::Complete {
                completed_at = Some(now);
                break;
            }
        }
        assert_eq!(completed_at, Some(created + 1 + 24 * 2));
        let mut placed = BuildingUp::placed(control, created);
        let placed_at = (created..created + 60)
            .find(|&now| placed.frame(now, &options) == ConstructionFrame::Complete);
        assert_eq!(placed_at, completed_at);
    }
}
