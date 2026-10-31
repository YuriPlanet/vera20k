//! VERA-only `[PostProcess]` presentation configuration.
//!
//! These keys are not read by gamemd: retail has no post-processing. They
//! choose whether the match frame is handed to the optional full-frame shader
//! chain ([`crate::render::post_process`]) instead of the 1:1 texture copy.
//! Every effect is off by default, so an unedited rules set keeps the retail
//! presentation path and its cost.
//!
//! ## INI format
//! ```ini
//! [PostProcess]
//! ColorGrade=no
//! ColorGrade.Brightness=0
//! ColorGrade.Contrast=1
//! ColorGrade.Saturation=1
//! ```

use crate::rules::ini_parser::IniFile;

/// `[PostProcess]` retained for presentation. Empty by default.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PostProcessRules {
    /// The colour-grade pass; `None` keeps the frame on the copy path.
    pub color_grade: Option<ColorGradeRules>,
}

/// Linear-space colour grading applied to the composed match frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorGradeRules {
    /// Added to each linear channel; 0 is neutral.
    pub brightness: f32,
    /// Scales each channel around mid-grey; 1 is neutral.
    pub contrast: f32,
    /// Mixes toward luma; 1 is neutral, 0 is greyscale.
    pub saturation: f32,
}

impl PostProcessRules {
    /// Read `[PostProcess]`. `ColorGrade=` is that effect's own switch; when it
    /// is `no` (the default) the section's other keys are not read.
    pub fn from_ini(ini: &IniFile) -> Self {
        let section = ini.section_or_empty("PostProcess");
        // The shader consumes binary32, so read each key through the native
        // ReadDouble-into-float control word instead of narrowing an f64.
        Self {
            color_grade: section.read_bool("ColorGrade", false).then(|| ColorGradeRules {
                brightness: section.read_float("ColorGrade.Brightness", 0.0),
                contrast: section.read_float("ColorGrade.Contrast", 1.0),
                saturation: section.read_float("ColorGrade.Saturation", 1.0),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_grade_is_off_without_the_section_or_its_switch() {
        assert_eq!(
            PostProcessRules::from_ini(&IniFile::from_str("")),
            PostProcessRules::default()
        );
        let inert = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nColorGrade=no\nColorGrade.Saturation=0\n",
        ));
        assert_eq!(
            inert,
            PostProcessRules::default(),
            "other keys do not enable it"
        );
    }

    #[test]
    fn color_grade_reads_the_neutral_defaults_and_authored_values() {
        let neutral =
            PostProcessRules::from_ini(&IniFile::from_str("[PostProcess]\nColorGrade=yes\n"));
        assert_eq!(
            neutral.color_grade,
            Some(ColorGradeRules {
                brightness: 0.0,
                contrast: 1.0,
                saturation: 1.0,
            })
        );
        let authored = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nColorGrade=yes\n\
             ColorGrade.Brightness=0.1\nColorGrade.Contrast=1.2\nColorGrade.Saturation=0.8\n",
        ));
        let grade = authored.color_grade.expect("authored colour grade");
        assert!((grade.brightness - 0.1).abs() < 1e-6);
        assert!((grade.contrast - 1.2).abs() < 1e-6);
        assert!((grade.saturation - 0.8).abs() < 1e-6);
    }

    /// The production `RuleSet` publishes the section; an empty INI leaves the
    /// presentation on the copy path.
    #[test]
    fn ruleset_publishes_the_post_process_section() {
        let empty = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str("")).unwrap();
        assert_eq!(empty.post_process, PostProcessRules::default());

        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[PostProcess]\nColorGrade=yes\nColorGrade.Saturation=0.5\n",
        ))
        .unwrap();
        assert_eq!(
            rules.post_process.color_grade,
            Some(ColorGradeRules {
                brightness: 0.0,
                contrast: 1.0,
                saturation: 0.5,
            })
        );
    }
}
