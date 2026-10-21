//! Admission to the ordinary AnimClass DrawIt422CA0 shape path.

use crate::rules::art_data::AnimTypeRuntimeConfig;

pub(super) fn ordinary_anim_type(name: &str, config: Option<&AnimTypeRuntimeConfig>) -> bool {
    !name.eq_ignore_ascii_case("RING1")
        && config.is_none_or(|config| !config.tiled && !config.bouncer && !config.is_meteor)
}
