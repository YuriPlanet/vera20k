//! `Translucency=` / `Translucency.Cloaked=` as Phobos `Animatable<TranslucencyLevel>`
//! (`Ext/AnimType/Body.cpp:113-114`, `Utilities/Template.h:560-594`).
//!
//! Native reader: `Utilities/TemplateDef.h:2275-2314` (the vector walk and
//! reset) and `:2330-2368` / `:2446-2524` (entry and property); evaluator:
//! `:2390-2441` (`Get`), `Utilities/Interpolation.h:57-64` and
//! `Utilities/Constructs.h:464-501`.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_translucency_track::{TranslucencyKeyframe, TranslucencyTrack};

fn track(ini: &str, section: &str, base: &str, end: i32, require_plain: bool) -> TranslucencyTrack {
    let file = IniFile::from_str(ini);
    let section = file.section(section).expect("the fixture section");
    TranslucencyTrack::read(section, base, end, require_plain)
}

/// `Get` (`Utilities/TemplateDef.h:2390-2441`) interpolates only in `Linear`
/// mode, and `interpolate<TranslucencyLevel>` (`Utilities/Interpolation.h:57-64`)
/// truncates the `double` result toward zero before
/// `TranslucencyLevel(value, true)` buckets it (`Utilities/Constructs.h:464-478`):
/// 25 is `TransLucent25`, 12 buckets back to `None`.
#[test]
fn keyframe_tracks_interpolate_at_the_native_truncation() {
    let ini = "[TRACK]\n\
               Translucency.Keyframe0.Percentage=0\n\
               Translucency.Keyframe0.Value=0\n\
               Translucency.Keyframe1.Percentage=1\n\
               Translucency.Keyframe1.Value=50\n\
               Translucency.Interpolation=linear\n";
    let parsed = track(ini, "TRACK", "Translucency", 0, false);
    assert_eq!(
        parsed.keyframes(),
        &[
            TranslucencyKeyframe {
                percentage: 0.0,
                level: 0,
            },
            TranslucencyKeyframe {
                percentage: 1.0,
                level: 50,
            },
        ]
    );
    assert_eq!(
        parsed.get(0.5),
        Some(25),
        "0 -> 50 halfway is TransLucent25"
    );
    // 0 + (50 - 0) * 0.25 = 12.5; (int)12 is below 25, so the level is None
    // and the draw keeps the stock instance ramp.
    assert_eq!(parsed.get(0.25), None, "12 truncated is not a level");
}

/// `KeyframeN.Absolute` is read only when the animation type's `End` is
/// positive, and its value becomes `Absolute / End`
/// (`Utilities/TemplateDef.h:2345-2355`). `End=0` leaves the entry without a
/// percentage, so the vector walk stops and the plain key's fallback entry
/// remains.
#[test]
fn absolute_keyframes_divide_by_the_animation_end() {
    let ini = "[ABS]\n\
               Translucency.Keyframe0.Absolute=20\n\
               Translucency.Keyframe0.Value=50\n";
    let parsed = track(ini, "ABS", "Translucency", 40, false);
    let keyframes = parsed.keyframes();
    assert_eq!(keyframes.len(), 1);
    assert_eq!(keyframes[0].percentage, 0.5, "20 of End=40");
    assert_eq!(keyframes[0].level, 50);
    // `Get` steps back from `lower_bound`, so a query equal to the first
    // entry's percentage sits at `begin()` and answers None; just above it
    // reads that entry.
    assert_eq!(parsed.get(0.5), None);
    assert_eq!(parsed.get(0.6), Some(50));

    let parsed = track(ini, "ABS", "Translucency", 0, false);
    assert_eq!(
        parsed.keyframes(),
        &[TranslucencyKeyframe {
            percentage: 0.0,
            level: 0,
        }],
        "End=0 ignores .Absolute and falls back to the absent plain key"
    );
}

/// `requireParsedFallback` (`Utilities/TemplateDef.h:2444-2477`,
/// `Ext/AnimType/Body.cpp:114`) keeps the cloaked track empty unless its own
/// plain key parsed; an entry without a percentage never enters the vector
/// (`:2357-2363`, `:2292-2293`).
#[test]
fn the_cloaked_track_requires_its_own_plain_key() {
    let plain = track("[A]\nTranslucency=50\n", "A", "Translucency", 0, false);
    assert_eq!(
        plain.get(0.9),
        Some(50),
        "the plain fallback answers everywhere"
    );

    let absent = track(
        "[A]\nTranslucency=50\n",
        "A",
        "Translucency.Cloaked",
        0,
        true,
    );
    assert!(!absent.has_values(), "an absent cloaked key stays empty");
    assert_eq!(absent.get(0.9), None);

    let no_percentage = track(
        "[A]\nTranslucency.Cloaked.Keyframe0.Value=50\n",
        "A",
        "Translucency.Cloaked",
        0,
        true,
    );
    assert!(
        !no_percentage.has_values(),
        "an entry without a percentage is rejected"
    );

    let authored = track(
        "[A]\nTranslucency.Cloaked=50\n",
        "A",
        "Translucency.Cloaked",
        0,
        true,
    );
    assert_eq!(
        authored.keyframes(),
        &[TranslucencyKeyframe {
            percentage: 0.0,
            level: 50,
        }]
    );
    assert_eq!(authored.get(0.0), Some(50));
    assert_eq!(authored.get(0.75), Some(50));
}

/// Native ends the process on a duplicated percentage
/// (`Utilities/TemplateDef.h:2480-2505`). VERA keeps the first entry, drops
/// the duplicate and continues.
#[test]
fn a_duplicate_percentage_is_dropped_not_fatal() {
    let ini = "[DUP]\n\
               Translucency.Keyframe0.Percentage=0\n\
               Translucency.Keyframe0.Value=25\n\
               Translucency.Keyframe1.Percentage=0\n\
               Translucency.Keyframe1.Value=75\n";
    let parsed = track(ini, "DUP", "Translucency", 0, false);
    assert_eq!(
        parsed.keyframes(),
        &[TranslucencyKeyframe {
            percentage: 0.0,
            level: 25,
        }],
        "the first duplicate wins"
    );
    assert_eq!(parsed.get(0.5), Some(25));
}

/// The `Interpolation` token is `none`/`linear`, case-insensitively; an absent
/// key and any other token leave the default `None`
/// (`Utilities/TemplateDef.h:1354-1376`). Without interpolation the answered
/// level is the start keyframe's own, so this track draws `None` at 0.5.
#[test]
fn interpolation_reads_the_two_tokens_and_keeps_none_otherwise() {
    let base = "[I]\n\
                Translucency.Keyframe0.Percentage=0\n\
                Translucency.Keyframe0.Value=0\n\
                Translucency.Keyframe1.Percentage=1\n\
                Translucency.Keyframe1.Value=50\n";
    for (token, halfway) in [("Linear", Some(25)), ("none", None), ("bogus", None)] {
        let ini = format!("{base}Translucency.Interpolation={token}\n");
        let parsed = track(&ini, "I", "Translucency", 0, false);
        assert_eq!(parsed.get(0.5), halfway, "Interpolation={token}");
    }
}
