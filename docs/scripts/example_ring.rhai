// Example file script: a ring of beams around the current target.
//
// Drop a copy into <game>/vera20k/scripts/ and give a TechnoType
// `Script=example_ring` in rules.ini (the binding is the file stem, or the
// `name` of a scripts.json entry). The file is re-read whenever it changes, so
// edits take effect on the next event without restarting the game.
//
// Tuning lives in `cfg()`: Rhai functions do not see the file's top-level
// scope, so a `const` at file scope would only fail at runtime.

fn cfg() {
    #{
        beams: 8,               // beams in the ring
        radius: 166,            // ring radius in leptons (256 per cell)
        beam_width: 4,          // ring thickness; house colour widens with it
        beam_duration: 2,       // frames the beam stays on screen
        warhead: "ExampleImpactWH",
        impact_damage: 20,
        flash_every: 30,        // frames between centre detonations
        flash_frames: 60,       // map-lighting duration after a detonation
        light_superweapon: "ExampleLightSW",
    }
}

fn default_state() {
    #{ frames: 0 }
}

fn on_update(ctx, state) {
    let cfg = cfg();
    if !ctx.has_target {
        return state;
    }

    let ground = ctx.anchor[2] - ctx.height;
    for step in 0..cfg.beams {
        let point = ring_point(ctx.anchor, cfg.radius, step * 45);
        laser(
            [point[0], point[1], ground + 50000],
            [point[0], point[1], ground],
            cfg.beam_width,
            cfg.beam_duration,
        );
    }

    state.frames += 1;
    if state.frames % cfg.flash_every == 0 {
        detonate(cfg.warhead, cfg.impact_damage, [ctx.anchor[0], ctx.anchor[1], ground]);
    }
    state
}

fn on_detonate(ctx, state) {
    flash(cfg().light_superweapon, cfg().flash_frames);
    state
}
