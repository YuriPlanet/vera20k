//! Phobos `[Side] ToolTip.Background.*` overrides
//! (`Ext/Side/Body.cpp:48-50`): each side may tint, fade and blur the tooltip
//! backing; unset fields fall back to the `[AudioVisual]` globals
//! (`Ext/Rules/Body.cpp:169-171`).

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF32Bits;

/// One side's three nullable tooltip-background overrides. Every field is
/// `Nullable`, so nothing is stored when the side leaves it unset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TooltipBackgroundSideSettings {
    /// `ToolTip.Background.Color=` (`Nullable<ColorStruct>` unset at
    /// `Ext/Side/Body.h:44,75`).
    pub color: Option<[u8; 3]>,
    /// `ToolTip.Background.Opacity=` (`Nullable<int>` unset at `:45,76`),
    /// `FillRectTrans`'s 0..100 percent.
    pub opacity: Option<i32>,
    /// `ToolTip.Background.BlurSize=` (`Nullable<float>` unset at `:46,77`):
    /// the radius the client's `ToolTipBlur` switch blurs the backing by.
    pub blur_size: Option<NativeF32Bits>,
}

impl TooltipBackgroundSideSettings {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            color: section.read_phobos_nullable_color("ToolTip.Background.Color", None),
            opacity: section.read_phobos_nullable_int("ToolTip.Background.Opacity", None),
            blur_size: section
                .read_phobos_nullable_double("ToolTip.Background.BlurSize", None)
                .map(|value| NativeF32Bits::from_bits((value as f32).to_bits())),
        }
    }
}
