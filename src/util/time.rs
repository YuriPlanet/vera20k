//! Host clocks. Simulation tick/RNG semantics do not depend on this boundary.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub use std::time::*;
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub use web_time::*;
