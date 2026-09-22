//! Fly's retained destination, integer height target and vertical-motion range.
//!
//! Native4CDD0D..4CDFBC/4CE145 reads Object coordinates; it has no elapsed-time
//! climb rate or four-phase altitude machine. The surrounding horizontal,
//! landing-effect and Display transactions are separate callers, still being
//! migrated. Native comparisons: tools/spatial_oracle/fly_height.{py,json}.

use super::locomotor::AirMovePhase;
use crate::sim::components::DriveCoord;

/// Constructor4CC9EE..4CC9FA clears target+38 and takeoff/landing+50/+51.
/// Only this owner mutates those fields. Object coordinates own current Z.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FlyRuntime {
    target_height: i32,
    taking_off: bool,
    landing: bool,
    /// Full Fly+1C/+20/+24, initialized to CoordStruct::Empty by4CC9A0.
    /// This is independent of Foot's NavCom identity and its changing position.
    destination: [i32; 3],
    /// Fly+5C, independent of the takeoff/landing callback flags. MoveTo
    /// chooses it from retained destination height, armed Target, Aircraft
    /// readiness and Landable. Enter resets it; the non-Landable phase branch
    /// forces it true. Constructor4CCA09 initializes false. Null MoveTo and
    /// mode-driven speed/navigation decisions still require their native port.
    #[serde(default)]
    cruise_mode: bool,
}

pub(crate) enum TakeoffFacing {
    Unchanged,
    PrimaryToDestination,
    SecondaryToPrimaryDestination,
}

impl FlyRuntime {
    pub(crate) fn has_phase_callback(&self) -> bool {
        self.taking_off || self.landing
    }

    /// Landing runs first in4CD2A0 when both flags are set; its callback must
    /// complete before that case can select the takeoff arm.
    pub(crate) fn has_only_takeoff_callback(&self) -> bool {
        self.taking_off && !self.landing
    }

    /// Takeoff4CE746..4CE821. Height is already bridge-normalized by the caller.
    /// Both flags clear even below either threshold; this is a callback latch,
    /// not a persistent ascending phase. No coordinate write occurs here.
    pub(crate) fn complete_takeoff(&mut self, height: i32, landing_base: i32) -> TakeoffFacing {
        let height = height.wrapping_sub(landing_base);
        let target = self.target_height.wrapping_sub(landing_base);
        self.taking_off = false;
        self.landing = false;
        if height > target.wrapping_sub(target / 3) {
            TakeoffFacing::SecondaryToPrimaryDestination
        } else if height > target / 2 {
            TakeoffFacing::PrimaryToDestination
        } else {
            TakeoffFacing::Unchanged
        }
    }

    pub(crate) fn destination(&self) -> DriveCoord {
        let [x, y, z] = self.destination;
        DriveCoord { x, y, z }
    }

    pub(crate) fn cruise_mode(&self) -> bool {
        self.cruise_mode
    }

    /// Process4CD664..4CD67F, before its power/health branches. The effective
    /// Mission getter5B3040 falls back to queued only when current is -1.
    pub(crate) fn prepare_process(&mut self, mission: crate::sim::mission::MissionId) {
        if mission
            == crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter)
        {
            self.cruise_mode = false;
        }
    }

    /// MoveTo4CCED9..4CCFA7, after the destination and takeoff transaction.
    /// Ground is queried from the retained coordinate even when the armed or
    /// non-Landable arm will force true. Signed addition wraps like native.
    pub(crate) fn select_destination_mode(
        &mut self,
        ground: i32,
        armed: bool,
        aircraft_ready: bool,
        non_landable_aircraft: bool,
    ) {
        self.cruise_mode = self.destination[2] > ground.wrapping_add(120)
            || armed
            || aircraft_ready
            || non_landable_aircraft;
    }

    /// Phase4CD2CA..4CD2F5 precedes the ordinary callback/Display transaction.
    /// It clears both requests even when neither was set and rereads the live
    /// type FlightLevel. It does not move the owner or reset either speed.
    pub(crate) fn force_non_landable_flight(&mut self, flight_level: i32) {
        self.cruise_mode = true;
        self.landing = false;
        self.taking_off = false;
        self.target_height = flight_level;
    }

    /// MoveTo4CCC80..4CCCE0: signed truncation, then 16-bit CellStruct equality.
    /// This refusal precedes owner disable/power gates and all ground queries.
    pub(crate) fn ignores_destination(&self, request: DriveCoord) -> bool {
        self.landing
            && (self.destination[0] / 256) as i16 == (request.x / 256) as i16
            && (self.destination[1] / 256) as i16 == (request.y / 256) as i16
    }

    /// Admitted non-null MoveTo4CCE1C..4CCE6E. A live Target and signed Ammo!=0
    /// replace Z with ground+FlightLevel; ground is read only on that arm.
    /// The caller selects mode+5C after takeoff admission. Moving+34 and
    /// null/Stop still require the pending native Process/landing migration.
    pub(crate) fn retain_destination(
        &mut self,
        request: DriveCoord,
        armed_flight_level: Option<i32>,
        ground: impl FnOnce() -> i32,
    ) {
        self.destination = [request.x, request.y, request.z];
        if let Some(flight_level) = armed_flight_level {
            self.destination[2] = ground().wrapping_add(flight_level);
        }
    }

    /// Historical hash projection before the destination was retained (188).
    pub(crate) fn height_hash_fields(&self) -> (i32, bool, bool) {
        (self.target_height, self.taking_off, self.landing)
    }

    pub(crate) fn target_height(&self) -> i32 {
        self.target_height
    }

    /// MoveTo4CCEAE..4CCED4. The caller owns admission and the Aircraft
    /// landing-base query (zero for ordinary, non-Carryall aircraft).
    pub(crate) fn should_begin_takeoff(
        &self,
        health: i32,
        height: impl FnOnce() -> i32,
        landing_base: i32,
    ) -> bool {
        // GetHeight may stamp the shared map Dummy. Native calls it only on
        // the final arm, after health and both phase gates.
        health > 0 && !self.taking_off && (self.landing || height() <= landing_base)
    }

    /// Admitted BeginTakeoff stores4CF9A5/4CF9A8/4CF9F8. Owner refusal,
    /// facing, spatial membership and sound belong to the calling transaction.
    pub(crate) fn begin_takeoff(&mut self, flight_level: i32) {
        self.landing = false;
        self.taking_off = true;
        self.target_height = flight_level;
    }

    /// Admitted BeginLanding stores4CFADF..4CFAEB. The full native landing
    /// callback/effect transaction remains required; these are not phase names.
    pub(crate) fn begin_landing(&mut self) {
        self.taking_off = false;
        self.landing = true;
        self.target_height = 0;
    }

    pub(crate) fn set_target_height(&mut self, height: i32) {
        self.target_height = height;
    }

    /// Read projection for the remaining legacy mission adapters. This is
    /// deliberately not the native+50/+51 flags or an independently saved FSM.
    pub(crate) fn mission_phase(&self, height: i32) -> AirMovePhase {
        if height == 0 && self.target_height == 0 {
            AirMovePhase::Landed
        } else if height < self.target_height {
            AirMovePhase::Ascending
        } else if height > self.target_height {
            AirMovePhase::Descending
        } else {
            AirMovePhase::Cruising
        }
    }

    pub(crate) fn step_height(&self, input: HeightInput) -> HeightOutput {
        let HeightInput {
            world_z,
            ground_z,
            mut on_bridge,
            structural_bridge,
            health,
            has_passenger,
            is_dropship,
            flight_level,
        } = input;
        // Object GetHeight5F5F40 subtracts ground and its current OnBridge
        // adjustment. Fly then independently normalizes an unattached deck.
        let mut height =
            world_z
                .wrapping_sub(ground_z)
                .wrapping_sub(if on_bridge { 416 } else { 0 });
        let mut bridge_bonus = 0;
        if !on_bridge && height >= 416 && structural_bridge {
            height = height.wrapping_sub(416);
            bridge_bonus = 416;
        }
        let mut output_z = world_z;
        if height < self.target_height && health > 0 {
            let rate = if is_dropship {
                16
            } else if has_passenger {
                10
            } else {
                20
            };
            let step = self.target_height.wrapping_sub(height).min(rate);
            // SetHeight happens BEFORE OnBridge is cleared (4CDE9D/4CDEA6).
            output_z = ground_z
                .wrapping_add(height)
                .wrapping_add(step)
                .wrapping_add(bridge_bonus)
                .wrapping_add(if on_bridge { 416 } else { 0 });
            on_bridge = false;
        }
        // Native compares the original normalized height again. Health==0
        // forces descent, while a negative health is not interchangeable.
        if height > self.target_height || health == 0 {
            let delta = height.wrapping_sub(self.target_height);
            let step = if is_dropship {
                if self.landing {
                    delta.min((delta / 20 + 10).min(48))
                } else {
                    height.min(if self.target_height == flight_level {
                        16
                    } else {
                        6
                    })
                }
            } else {
                delta.min(delta / 20).clamp(20, 50)
            };
            height = height.wrapping_sub(step);
            if health == 0 && height <= 1 {
                height = 1;
            } else if height <= 0 {
                height = 0;
                if bridge_bonus > 0 {
                    on_bridge = true;
                    bridge_bonus = 0;
                }
            }
            output_z = ground_z
                .wrapping_add(height)
                .wrapping_add(bridge_bonus)
                .wrapping_add(if on_bridge { 416 } else { 0 });
        }
        HeightOutput {
            world_z: output_z,
            on_bridge,
            height: output_z
                .wrapping_sub(ground_z)
                .wrapping_sub(if on_bridge { 416 } else { 0 }),
        }
    }
}

pub(crate) struct HeightInput {
    pub world_z: i32,
    pub ground_z: i32,
    pub on_bridge: bool,
    pub structural_bridge: bool,
    pub health: i32,
    pub has_passenger: bool,
    pub is_dropship: bool,
    pub flight_level: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HeightOutput {
    pub world_z: i32,
    pub on_bridge: bool,
    pub height: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_saved_original_vertical_steps_match() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_height.json"
        ))
        .unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 144);
        for row in rows {
            let c = &row["input"];
            let integer =
                |name: &str, fallback: i32| c[name].as_i64().map_or(fallback, |v| v as i32);
            let flag = |name: &str| c[name].as_bool().unwrap_or(false);
            let target = integer("target", 0);
            let mut state = FlyRuntime::default();
            if flag("landing") {
                state.begin_landing();
            }
            state.set_target_height(target);
            let ground_z = crate::util::lepton::ground_height_leptons(
                integer("level", 0) as u8,
                integer("slope", 0) as u8,
                2688,
                2688,
            )
            .unwrap();
            let authored_level = integer("flight_level", -1);
            let actual = state.step_height(HeightInput {
                world_z: integer("z", 0),
                ground_z,
                on_bridge: flag("on_bridge"),
                structural_bridge: flag("bridge"),
                health: integer("health", 100),
                has_passenger: flag("loaded") && c["kind"].as_str() != Some("unit"),
                is_dropship: flag("dropship"),
                flight_level: if authored_level == -1 {
                    1500
                } else {
                    authored_level
                },
            });
            assert_eq!(
                actual,
                HeightOutput {
                    world_z: row["z"].as_i64().unwrap() as i32,
                    on_bridge: row["on_bridge"].as_bool().unwrap(),
                    height: row["height"].as_i64().unwrap() as i32,
                },
                "{}",
                c["name"]
            );
        }
    }
}
