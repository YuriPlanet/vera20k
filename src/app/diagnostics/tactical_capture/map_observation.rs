//! Chosen-map controller within the existing hidden tactical capture lifecycle.
//! The launch is the production session DTO; this diagnostic owns no gameplay.

use super::super::integrity::{SealedJsonFile, parse_strict_json, read_stable_regular_bytes};
use super::super::manifest::{PublishFault, encode_manifest, publish_transaction};
use super::*;
use crate::app::diagnostics::state::{MAP_PRESENTATION_CLOCK_POLICY, MAP_PRESENTATION_INTERVAL_MS};
use crate::app::presentation::render::GameRenderTimes;
use crate::audio::sfx::{PcmObservationConfig, PcmObservationContext, PcmObservationReport};
use crate::skirmish_launch::{LaunchStartPosition, SkirmishLaunchSession};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[path = "map_observation/command_bindings.rs"]
mod command_bindings;
use command_bindings::{MapScheduledCommand, production_type_names};

const PROFILE_V1: &str = "vera20k.map-observation-profile.v1";
const PROFILE_V2: &str = "vera20k.map-observation-profile.v2";
const PROFILE_V3: &str = "vera20k.map-observation-profile.v3";
const CHILD_SCHEMA: &str = "vera20k.map-observation.v8";
const OBSERVATION_POLICY: &str = "map-ordinary-command-observation-v5";
const MAX_COMMANDS: usize = 1024;
const MAX_GESTURES: usize = 1024;
const GESTURE_POLICY: &str = "map-tactical-left-gesture-v1";
const SIDEBAR_GESTURE_POLICY: &str = "map-local-left-gesture-v2";
const KEYBOARD_GESTURE_POLICY: &str = "map-local-gesture-v3";
const COMMAND_BAR_GESTURE_POLICY: &str = "map-local-gesture-v4";
const RIGHT_BUTTON_GESTURE_POLICY: &str = "map-local-right-gesture-v5";
const SIDEBAR_POLICY: &str = "map-retained-sidebar-observation-v1";
const MAX_OBSERVED_OWNERS: usize = 30;
const MAX_OBSERVED_TYPES: usize = 256;
const MAX_TERRAIN_CELLS: usize = 256;
const MAX_OBSERVATION_SAMPLES: usize = 100_000;
const MAX_RECEIPT_BYTES: usize = 128 * 1024 * 1024;
const AUDIO_POLICY: &str = "map-device-pulled-player-pcm-v2";
const LOAD_SEGMENT_POLICY: &str = "map-literal-quickload-clock-segments-v1";

#[path = "map_observation/drive.rs"]
mod drive;
#[path = "map_observation/input.rs"]
mod input;
#[path = "map_observation/observation.rs"]
mod observation;
#[path = "map_observation/profile.rs"]
mod profile;
use profile::deserialize_present;
#[path = "map_observation/record.rs"]
mod record;
#[path = "map_observation/world.rs"]
mod world;
#[cfg(test)]
use input::MapBuildingPlacementObservation;
#[cfg(test)]
use input::MapLocalInputObservation;
#[cfg(test)]
use input::MapSelectionScopeObservation;
#[cfg(test)]
use input::MapSelectionVoiceRequest;
#[cfg(test)]
use input::MapSidebarControl;
#[cfg(test)]
use input::MapSidebarItem;
#[cfg(test)]
use input::MapSidebarTab;
#[cfg(test)]
use input::MapSidebarToggle;
use input::{MapCommandBarGestureReceipt, MapCommandBarThumbEdge};
use input::{MapCommandBarThumbState, MapCommandReceipt, MapGestureCommandReceipt};
use input::{MapGestureReceipt, MapHeldCursorReceipt, MapInputObservation, MapKeyboardBinding};
use input::{MapKeyboardGestureReceipt, MapMinimapGestureReceipt};
use input::{MapSidebarFrame, MapSidebarGestureReceipt, MapSidebarObservation};
use input::{MapSidebarToggleReceipt, command_bar_index};
pub(super) use observation::MapObservation;
#[cfg(test)]
use observation::sound_state_sample_count;
use observation::{MapClock, MapLoadSegment, capture_tooltip, pcm_report, sound_state};
pub(crate) use profile::MapCaptureProfile;
#[cfg(test)]
use profile::MapCoordinateRequestScope;
#[cfg(test)]
use profile::MapPresentationProfile;
use profile::{MapAudioProfile, MapGesture, MapKeyEdge};
use profile::{MapScheduledGesture, MapSidebarTarget, literal_key};
use profile::{literal_modifier_edges, literal_modifiers, sidebar_tab_name};
#[cfg(test)]
use record::unit_atlas_statistics;
use world::terrain_render_observation;
use world::{MapCrateSlotObservation, MapEffectObservation, MapElectricBoltDrawObservation};
use world::{MapElectricBoltObservation, MapFrameObservation, MapLaserObservation};
use world::{MapRadiationBeamDrawObservation, MapRadiationBeamObservation, bomb_sample_count};
use world::{cloak_runtime_observation, diplomacy_row, disguise_inputs, fire_observation};
use world::{prism_observation, retask_observation, spawn_observation, spawn_sample_count};
use world::{super_weapon_rows, temporal_observation, temporal_sample_count};

#[cfg(all(test, tests_app))]
#[path = "map_observation/tests.rs"]
mod tests;
