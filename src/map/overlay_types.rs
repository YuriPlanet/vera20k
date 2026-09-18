//! Map-side overlay ID helpers: bridge overlay-ID classification and
//! high-bridge stamp geometry over `map::bridge_facts`.
//!
//! The overlay type registry itself is rules data (`rules::overlay_types`,
//! re-exported below for existing map/sim call sites); render-only SHP name
//! helpers live in `render::overlay_assets`.
//!
//! ## Dependency rules
//! - Part of map/ — depends on rules/ and map siblings only.

pub use crate::rules::overlay_types::{
    OverlayTypeFlags, OverlayTypeRegistry, is_bridge_overlay_index, is_high_bridge_index,
};
pub(crate) use crate::rules::overlay_types::{
    clears_tiberium_on_slope, native_mark_overlay_data, retained_overlay_land,
    uses_early_recalc_land_branch,
};

/// Check if an overlay index is one of the four high-bridge map-load anchors
/// that dispatch through `SetBridgeDirection`.
pub fn is_high_bridge_anchor_overlay_index(id: u8) -> bool {
    crate::map::bridge_facts::high_bridge_stamp_for_overlay(id).is_some()
}

/// Get the binary `SetBridgeDirection` direction for a high-bridge anchor.
pub fn high_bridge_stamp_direction(id: u8) -> Option<u8> {
    crate::map::bridge_facts::high_bridge_stamp_for_overlay(id).map(|(_, dir)| dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_bridge_anchor_overlay_helpers_are_narrow() {
        for id in [0x18, 0x19, 0xED, 0xEE] {
            assert!(is_high_bridge_anchor_overlay_index(id));
            assert!(high_bridge_stamp_direction(id).is_some());
            assert!(is_bridge_overlay_index(id));
        }
        for id in [0x4A, 0x7A, 0xCD, 0xE9] {
            assert!(!is_high_bridge_anchor_overlay_index(id));
            assert_eq!(high_bridge_stamp_direction(id), None);
        }
        assert!(is_bridge_overlay_index(0x4A));
        assert!(is_bridge_overlay_index(0xCD));
    }
}
