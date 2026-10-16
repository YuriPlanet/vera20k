//! BuildingType rubble art: which SHP frames a destroyed `LeaveRubble=yes`
//! building leaves on its foundation, bound GPU-free from the active
//! theater's assets so graphical and headless matches share it.
//!
//! Native: `BuildingTypeClass` `0x0045F160` (body) and `0x0045F1D0` (shadow)
//! answer for a type whose `LeaveRubble` byte (`+0x1768`) is set. A separate
//! art `Rubble=` shape (`+0x14F4`, loaded by `BuildingTypeClass::
//! LoadVisualAssets @ 0x0045F230`) draws its frame 0 and shadow `count/2`.
//! Otherwise the type's own image (`vt+0x9C`) draws frame 3 and shadow
//! `count/2 + 3`, where `count` is the SHP header's signed frame word (`+6`)
//! and `count/2` is halved toward zero; an image with `count/2 <= 3` leaves no
//! rubble. The answer gates both the
//! destruction write (`BuildingClass::AfterDestruction @ 0x00441F60`) and the
//! cell draws (`CellClass::DrawOverlay_Body @ 0x0047F6A0`,
//! `DrawOverlay_Shadow @ 0x0047F510`).
//!
//! RESIDUALS:
//! - The shadow (`0x0045F1D0`, frame `count/2 + 3`) is not bound: VERA has no
//!   ordinary-overlay shadow pass (`DrawOverlay_Shadow`), so rubble draws
//!   without its shadow. Trigger: every rubble cell. Effect: no darkened
//!   footprint under the rubble.
//! - The art `Rubble=` arm is not bound. No retail art section sets
//!   `Rubble=`, so it is dormant; a mod that sets it gets no rubble here.
//!
//! ## Dependency rules
//! - Part of rules/; depends on assets/ only.

use std::collections::BTreeMap;

use crate::assets::asset_manager::AssetManager;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;

/// The main-image body frame `0x0045F160` selects.
pub const RUBBLE_BODY_FRAME: u16 = 3;

/// One rubble-leaving building type's bound art.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RubbleArt {
    /// The SHP file the type's image resolved to (the building's own draw
    /// resolves the same name).
    pub shp: String,
    pub body_frame: u16,
}

/// The main-image arm's body frame from the SHP's raw frame word, or `None`
/// when the image is too short to carry rubble (`0x0045F160`).
pub fn rubble_body_frame(raw_frames: i16) -> Option<u16> {
    // Signed division: halved toward zero.
    let half = i32::from(raw_frames) / 2;
    (half > 3).then_some(RUBBLE_BODY_FRAME)
}

/// Rubble art of every `LeaveRubble=yes` building type whose image the active
/// theater resolves to a long enough SHP. Keys are uppercase type IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RubbleAssetCatalog {
    art: BTreeMap<String, RubbleArt>,
}

impl RubbleAssetCatalog {
    pub fn bind(
        rules: &RuleSet,
        asset_manager: &AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Self {
        let mut catalog = Self::default();
        for object in rules
            .all_objects()
            .filter(|object| object.category == ObjectCategory::Building && object.leave_rubble)
        {
            let image = rules
                .art()
                .resolve_effective_image_id(&object.id, &object.image);
            let Some((shp, data)) = crate::rules::art_data::object_shp_candidates(
                Some(rules.art()),
                &image,
                theater_ext,
                theater_name,
            )
            .into_iter()
            .find_map(|candidate| {
                let data = asset_manager.get_ref(&candidate)?;
                Some((candidate, data))
            }) else {
                continue;
            };
            let Some(raw) = data
                .get(6..8)
                .map(|word| i16::from_le_bytes([word[0], word[1]]))
            else {
                continue;
            };
            let Some(body_frame) = rubble_body_frame(raw) else {
                continue;
            };
            catalog.art.insert(
                object.id.to_ascii_uppercase(),
                RubbleArt { shp, body_frame },
            );
        }
        catalog
    }

    /// `type_id`'s rubble art, or `None` when it leaves no rubble.
    pub fn art(&self, type_id: &str) -> Option<&RubbleArt> {
        self.art.get(&type_id.to_ascii_uppercase())
    }

    /// Every bound rubble art, in type-ID order.
    pub fn iter(&self) -> impl Iterator<Item = &RubbleArt> {
        self.art.values()
    }

    /// Install one type's art directly (test fixtures without assets).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn insert_for_test(&mut self, type_id: &str, art: RubbleArt) {
        self.art.insert(type_id.to_ascii_uppercase(), art);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rubble_needs_more_than_three_body_frames() {
        assert_eq!(rubble_body_frame(7), None, "7/2 = 3");
        assert_eq!(rubble_body_frame(8), Some(3));
        assert_eq!(rubble_body_frame(9), Some(3), "odd counts halve down");
        assert_eq!(
            rubble_body_frame(-9),
            None,
            "a negative word halves toward zero"
        );
    }

    /// The production readers on retail `rulesmd.ini`/`artmd.ini`: the
    /// building flag, and the overlay AfterDestruction stamps by index.
    #[test]
    fn retail_leave_rubble_and_rubble_overlay_index() {
        let Some((ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
        for (id, leave) in [
            ("CAOUTP", true),
            ("CAWASH01", true),
            ("YAPPPT", false), // LeaveRubble=false
            ("GAPOWR", false), // unset: the constructor's false
        ] {
            assert_eq!(rules.object(id).unwrap().leave_rubble, leave, "{id}");
        }
        let registry =
            crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art_ini));
        // AfterDestruction's literal `MOV [ESI+0x44],0xEF` (`0x0044202C`).
        let rubble = 0xEF;
        assert_eq!(registry.name(rubble), Some("RUBBLE_OVERLAY"));
        assert!(registry.flags(rubble).unwrap().is_rubble);
    }

    /// Bound from the retail archives: a civilian building's own image
    /// carries the rubble frame, and a non-LeaveRubble type binds nothing.
    #[test]
    fn retail_rubble_art_binds_from_the_theater_image() {
        let Some((ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
        let catalog = RubbleAssetCatalog::bind(&rules, &assets, "tem", "TEMPERATE");
        // The fixture registers no theater archive, so CAWASH01 resolves to
        // its generic-letter image (8 raw frames).
        assert_eq!(
            catalog.art("CAWASH01"),
            Some(&RubbleArt {
                shp: "CGWASH01.SHP".into(),
                body_frame: RUBBLE_BODY_FRAME,
            })
        );
        assert!(catalog.art("GAPOWR").is_none());
    }
}
