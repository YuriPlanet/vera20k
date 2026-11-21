use super::*;

/// `0x0042319A..0x004231BC`: only an exact `Translucency=` 25, 50 or 75
/// selects a stage; any other value draws without one.
/// `0x0042306B`: the `+0x119` arm ORs 75% for a `DoubleThick=` type,
/// else 50%, and falls through to the type's own arms (`0x0042307D`).
#[test]
fn forced_translucency_reads_double_thick_and_falls_through() {
    let input = |double_thick, explicit_translucency, instance_ramp| AnimTranslucencyInput {
        base_flags: 0,
        forced_translucent: true,
        double_thick,
        translucency_detail_level: 0,
        game_detail_level: 2,
        translucent_ramp: false,
        current_frame: 0,
        end_frame: 10,
        explicit_translucency,
        instance_ramp,
    };
    for (double_thick, explicit, ramp, expected) in [
        (false, 0, 0, (true, 4)),
        (true, 0, 0, (true, 6)),
        (false, 25, 0, (true, 6)),
        (false, 0, 16, (false, 4)),
    ] {
        let result = anim_translucency_selection(input(double_thick, explicit, ramp));
        assert_eq!((result.draw, result.flags), expected);
    }
}

/// `0x004231C1..0x004231EE`: the `+0x178` bands a fading building
/// writes into its slot anims.
#[test]
fn instance_ramp_bands_draw_at_25_then_50_then_not_at_all() {
    for (instance_ramp, expected) in [
        (0, (true, 0)),
        (1, (true, 2)),
        (5, (true, 2)),
        (6, (true, 4)),
        (15, (true, 4)),
        (16, (false, 0)),
    ] {
        let result = anim_translucency_selection(AnimTranslucencyInput {
            base_flags: 0,
            forced_translucent: false,
            double_thick: false,
            translucency_detail_level: 0,
            game_detail_level: 2,
            translucent_ramp: false,
            current_frame: 0,
            end_frame: 10,
            explicit_translucency: 0,
            instance_ramp,
        });
        assert_eq!(
            (result.draw, result.flags),
            expected,
            "ramp {instance_ramp}"
        );
    }
}

/// `0x004230A8..0x004230FB`: End 15 (EXPLOMED, WAKE1, RING) reaches 75%
/// at stage 9, because 15 times the 0.6 double chops below 9, while the
/// 0.4 and 0.2 doubles sit above their fifths.
#[test]
fn translucent_ramp_takes_75_percent_at_three_fifths() {
    for (current_frame, flags) in [(3, 0), (4, 2), (6, 2), (7, 4), (9, 6)] {
        let result = anim_translucency_selection(AnimTranslucencyInput {
            base_flags: 0,
            forced_translucent: false,
            double_thick: false,
            translucency_detail_level: 0,
            game_detail_level: 2,
            translucent_ramp: true,
            current_frame,
            end_frame: 15,
            explicit_translucency: 0,
            instance_ramp: 0,
        });
        assert_eq!(
            (result.draw, result.flags),
            (true, flags),
            "stage {current_frame}"
        );
    }
}

#[test]
fn only_exact_fixed_translucency_values_select_a_stage() {
    for (explicit_translucency, flags) in [
        (25, 2),
        (50, 4),
        (75, 6),
        (1, 0),
        (26, 0),
        (100, 0),
        (0, 0),
        (-25, 0),
    ] {
        assert_eq!(
            anim_translucency_selection(AnimTranslucencyInput {
                base_flags: 0,
                forced_translucent: false,
                double_thick: false,
                translucency_detail_level: 0,
                game_detail_level: 2,
                translucent_ramp: false,
                current_frame: 0,
                end_frame: 10,
                explicit_translucency,
                instance_ramp: 0,
            }),
            AnimTranslucencyResult { draw: true, flags },
            "Translucency={explicit_translucency}"
        );
    }
}
