//! Public read-only telemetry facade for the standalone fixture.

pub use crate::app::match_runtime::telemetry::TickTimingRing;
pub use crate::app::telemetry_server::{
    DEFAULT_PORT, DEFAULT_WEB_ROOT, ENV_ADDR, ENV_WEB_ROOT, TelemetryServer, start_from_env,
};
