//! Presentation-only radiation glow inputs: derive one green point light per live radiation
//! site from the sim-side `RadiationState` + `[Radiation]` rules, mirroring
//! gamemd's per-site light source. Pure read of sim state — never mutates it,
//! never feeds the deterministic hash.
//!
//! Depends on `sim/` (read-only `RadiationState`/`RadSite`), `rules/`
//! (`RadiationRules`), and `map/lighting` (`PointLight`). This projection never
//! feeds radiation damage, gameplay state hashes or RNG; it can be recorded at
//! native source-event boundaries before presentation samples Cell lighting.
//!
//! ## Float exception
//! Light intensity/tint use render-side `f32`, matching the render math exception
//! (the original computes this in `double`; these values are never hashed).

use crate::map::lighting::{self, LIGHT_CLAMP_MAX, LIGHT_UNIT, PointLight};
use crate::rules::ruleset::{RadiationRules, RuleSet};
use crate::sim::radiation::RadSite;
use crate::sim::world::Simulation;
use crate::util::fixed_math::sim_to_f32;

/// Derive the stepwise green point light for one radiation site, or `None` when
/// the site is degenerate or fully faded.
///
/// Stepwise (per-step cadence): every value is quantized to the integer step
/// index `k = (duration - remaining) / RadLightDelay`, so the light is
/// piecewise-constant between steps and dims in discrete steps like the original.
///
/// - intensity = `min(level * RadLightFactor, 2000)` minus a fixed per-step
///   decrement `intensity_spawn / (duration / RadLightDelay)`. With `k` clamped
///   to `[0, steps_total]` and an integer (floor) decrement this never goes
///   negative, so the `.max(0)` is a no-op safety net, not a divergence.
/// - tint_c = `min(c * 1000 / 255 * RadTintFactor, 2000) * remaining_at_step / duration`.
pub fn radiation_site_light(site: &RadSite, rules: &RadiationRules) -> Option<PointLight> {
    if site.duration < 1 {
        return None;
    }
    let light_delay = rules.light_delay.max(1);
    let steps_total = (site.duration / light_delay).max(1);
    let elapsed = site.duration - site.remaining; // remaining <= duration always
    let k = (elapsed / light_delay).clamp(0, steps_total);
    let remaining_at_step = site.duration - k * light_delay;

    // Intensity: min(level * RadLightFactor, 2000), faded by the fixed per-step decrement.
    let light_factor = sim_to_f32(rules.light_factor);
    let intensity_spawn = ((site.level as f32 * light_factor) as i32).min(LIGHT_CLAMP_MAX);
    let decrement = intensity_spawn / steps_total;
    let intensity = (intensity_spawn - k * decrement).max(0);

    // Tint: per channel min(c * 1000/255 * RadTintFactor, 2000), faded by the
    // remaining/duration ratio (computed at the step boundary).
    let tint_factor = sim_to_f32(rules.tint_factor);
    let channel_base = |c: u8| -> i32 {
        let rescaled = (i32::from(c) * LIGHT_UNIT) / 255; // x1000/255 byte rescale (verified)
        ((rescaled as f32 * tint_factor) as i32).min(LIGHT_CLAMP_MAX)
    };
    let faded = |base: i32| -> i32 {
        // i64 intermediate avoids overflow at heavy stacking.
        (i64::from(base) * i64::from(remaining_at_step) / i64::from(site.duration)) as i32
    };
    let (cr, cg, cb) = rules.color;
    let tint = [
        faded(channel_base(cr)),
        faded(channel_base(cg)),
        faded(channel_base(cb)),
    ];

    if intensity == 0 && tint == [0, 0, 0] {
        return None;
    }
    Some(lighting::radiation_point_light(
        site.center.0,
        site.center.1,
        site.radius_leptons,
        intensity,
        tint,
    ))
}

/// Collect one green point light per live radiation site.
pub fn collect_radiation_lights(sim: &Simulation, rules: &RuleSet) -> Vec<PointLight> {
    sim.radiation
        .sites()
        .filter_map(|site| radiation_site_light(site, &rules.radiation))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_rules() -> RadiationRules {
        RadiationRules {
            duration_multiple: 1,
            application_delay: 16,
            level_max: 500,
            level_delay: 90,
            light_delay: 90,
            level_factor: 0.2,
            light_factor: crate::util::fixed_math::sim_from_f32(0.1),
            tint_factor: crate::util::fixed_math::sim_from_f32(1.0),
            color: (0, 255, 0),
            site_warhead: "RadSite".to_string(),
        }
    }

    /// Build a stock Desolator site at a given `remaining` (level 500, spread 10,
    /// duration 500).
    fn site_at_remaining(remaining: i32) -> RadSite {
        RadSite {
            center: (100, 100),
            spread: 10,
            radius_leptons: 10 * 256 + 128,
            level: 500,
            level_steps: 500 / 90,
            duration: 500,
            remaining,
            level_timer_start: 0,
            level_timer_duration: 90,
        }
    }

    #[test]
    fn stock_desolator_spawn_intensity_and_pure_green_tint() {
        // k=0 (remaining == duration): intensity 50, tint [0,1000,0].
        let light = radiation_site_light(&site_at_remaining(500), &stock_rules()).unwrap();
        assert_eq!(light.intensity, 50, "500 * 0.1 = 50, under the 2000 clamp");
        assert_eq!(
            light.tint,
            [0, 1000, 0],
            "green 255*1000/255*1.0 = 1000; R/B = 0"
        );
        assert_eq!(light.radius_leptons, 2688);
    }

    #[test]
    fn stock_desolator_dual_decay_curves_are_stepwise() {
        let rules = stock_rules();
        // steps_total = 500/90 = 5; decrement = 50/5 = 10.
        // k=1 (remaining 410): intensity 40, green 1000*410/500 = 820.
        let l1 = radiation_site_light(&site_at_remaining(410), &rules).unwrap();
        assert_eq!((l1.intensity, l1.tint[1]), (40, 820));
        // k=2 (remaining 320): intensity 30, green 640.
        let l2 = radiation_site_light(&site_at_remaining(320), &rules).unwrap();
        assert_eq!((l2.intensity, l2.tint[1]), (30, 640));
        // k=5 (remaining 50): intensity 0 (faded out first), green 100 (still lit).
        let l5 = radiation_site_light(&site_at_remaining(50), &rules).unwrap();
        assert_eq!((l5.intensity, l5.tint[1]), (0, 100));
    }

    #[test]
    fn stepwise_holds_constant_between_step_boundaries() {
        let rules = stock_rules();
        // remaining 410 (k=1) and 330 (still k=1: elapsed 170 / 90 = 1) must match —
        // the value steps only at boundaries, not continuously.
        let a = radiation_site_light(&site_at_remaining(410), &rules).unwrap();
        let b = radiation_site_light(&site_at_remaining(330), &rules).unwrap();
        assert_eq!((a.intensity, a.tint), (b.intensity, b.tint));
    }

    #[test]
    fn intensity_clamps_at_2000_when_stacked() {
        // level 25000 * 0.1 = 2500 -> clamped to 2000.
        let mut site = site_at_remaining(1);
        site.level = 25000;
        site.duration = 25000;
        site.remaining = 25000;
        let light = radiation_site_light(&site, &stock_rules()).unwrap();
        assert_eq!(light.intensity, LIGHT_CLAMP_MAX);
    }

    #[test]
    fn tint_channel_clamps_at_2000_with_high_tint_factor() {
        let mut rules = stock_rules();
        rules.tint_factor = crate::util::fixed_math::sim_from_f32(3.0); // 1000*3 = 3000 -> clamp 2000
        let light = radiation_site_light(&site_at_remaining(500), &rules).unwrap();
        assert_eq!(light.tint[1], LIGHT_CLAMP_MAX);
    }

    #[test]
    fn degenerate_duration_yields_no_light() {
        let mut site = site_at_remaining(0);
        site.duration = 0;
        assert!(radiation_site_light(&site, &stock_rules()).is_none());
    }
}
