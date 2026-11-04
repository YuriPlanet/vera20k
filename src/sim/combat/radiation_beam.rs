//! Copied straight radiation-beam construction inputs. SpawnRadBeam6FD620
//! supplies these to the non-Abstract registry659110; presentation owns its
//! Tactical-composite progress and drawing. Native evidence is retained in
//! tools/procedural_drawing_oracle/{rad_beam,chrono_beam}.md.

use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::{MaskedX87Chop53 as M, NativeF64Bits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadiationBeamBirth {
    pub frame: i32,
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub color: [u8; 3],
    /// Phobos `Beam.IsHouseColor`: presentation replaces `color` with this
    /// House's laser color before the beam is stored.
    pub house: Option<crate::sim::intern::InternedId>,
    pub house_color: bool,
    pub duration: i32,
    pub amplitude: NativeF64Bits,
    /// Registry kind2 (SpawnRadEruption), drawn by RadBeam659CA0; otherwise
    /// the straight kind1 copied modes.
    pub eruption: bool,
}

/// SpawnRadBeam's copied-endpoint modes0/1. Attached Magna mode2 has a
/// different lifecycle and does not belong to this birth transport.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CopiedBeamMode {
    Radiation,
    Chrono,
}

/// FireAt6FF594..6FF5C6, after the laser/bolt alternatives. WH.Temporal
/// selects copied mode1 at6FF5B7; other IsRadBeam weapons use mode0.
/// Magna mode2 has a separate native caller/lifecycle; eruption kind2 is
/// [`erupted`].
pub(crate) fn fired(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    source_id: u64,
    target: super::TargetKind,
    weapon: &crate::rules::weapon_type::WeaponType,
) {
    if super::world_fire::fire_at_effect(weapon) != super::world_fire::FireAtEffect::RadiationBeam {
        return;
    }
    let mode = if weapon
        .warhead
        .as_deref()
        .and_then(|name| rules.warhead(name))
        .is_some_and(|warhead| warhead.temporal)
    {
        CopiedBeamMode::Chrono
    } else {
        CopiedBeamMode::Radiation
    };
    create(
        world,
        rules,
        source_id,
        target.projectile_target(world),
        mode,
    );
}

/// SpawnRadBeam6FD620's modes0/1: recompute GetFLH(index0, NullCoord)
/// after burst advancement and copy both endpoints.6FD752 selects retained
/// Rules.ChronoBeamColor for mode1, otherwise RadColor for mode0. Duration15
/// and amplitude40 are shared. Neither endpoint attaches; no RNG, ID or timer.
pub(crate) fn create(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    source_id: u64,
    target: crate::sim::projectile::ProjectileTarget,
    mode: CopiedBeamMode,
) {
    let Some(source) = world.substrate.entities.get(source_id) else {
        return;
    };
    let Some(object) = world.interner.object_type(source.type_ref(), rules) else {
        return;
    };
    let from = super::fire_coord::fire_coordinate(
        world,
        rules,
        &super::fire_coord::FireSource::of_entity(source),
        object,
        0,
        (source.weapon_burst.index() & 1) as u8,
        Default::default(),
    )
    .coord;
    let z_adjust = super::fire_coord::effect_z_adjust(
        world,
        source,
        from,
        super::fire_coord::EffectDepthProjection::ChangedY,
    );
    let Some(to) = super::fire_coord::effect_target_coordinate(world, rules, target) else {
        return;
    };
    create_endpoints(world, rules, from, to, z_adjust, mode, None);
}

/// Phobos `BulletExt::SimulatedFiringRadBeam`: the same copied modes0/1 with
/// caller-supplied endpoints (the simulated child's source coordinate).
/// `house` is `Some` when `Beam.IsHouseColor` takes that House's laser color.
pub(crate) fn create_endpoints(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    from: ProjectileCoord,
    to: ProjectileCoord,
    z_adjust: i32,
    mode: CopiedBeamMode,
    house: Option<crate::sim::intern::InternedId>,
) {
    let color = match mode {
        CopiedBeamMode::Radiation => {
            let (r, g, b) = rules.radiation.color;
            [r, g, b]
        }
        CopiedBeamMode::Chrono => rules.general.chrono_beam_color,
    };
    world
        .lifecycle_outputs
        .emit(crate::sim::world::LifecycleOutput::RadiationBeamCreated(
            RadiationBeamBirth {
                frame: world.session.binary_frame() as i32,
                from,
                to,
                z_adjust,
                color,
                house,
                house_color: house.is_some(),
                duration: 15,
                amplitude: NativeF64Bits::from_bits(40.0_f64.to_bits()),
                eruption: false,
            },
        ));
}

/// `TechnoClass::SpawnRadEruption @ 0x006FD800`, FireAt's IsRadEruption arm
/// (`0x006FF5D0..0x006FF5EE`, after the rad-beam alternative). The spread is
/// ftol of the warhead's float CellSpread (`+0x124`). Each of the 8 cells
/// around the firer's GetCoords cell, `spread` cells away (X outer, Y inner,
/// -1..1, skipping the centre; 16-bit wrapping cell words), takes
/// Map5657A0/Cell480A30 centre coordinates, then five Main draws in order:
/// X and Y jitter -128..128, duration 5..20, amplitude 100..500 and travel
/// rate 0..20. The beam runs from the firer's GetCoords (z-adjust 0) in
/// RadColor (`Rules+0x1830`). Its only travel-scale write (`0x006FD9B1`)
/// stores 0.0, so RadBeam659EE0's travel term is always zero and the drawn
/// rate is inert.
pub(crate) fn erupted(
    world: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    source_id: u64,
    weapon: &crate::rules::weapon_type::WeaponType,
) {
    if super::world_fire::fire_at_effect(weapon)
        != super::world_fire::FireAtEffect::RadiationEruption
    {
        return;
    }
    // Native dereferences the warhead unguarded; a weapon without one
    // would crash there.
    let Some(warhead) = weapon
        .warhead
        .as_deref()
        .and_then(|name| rules.warhead(name))
    else {
        return;
    };
    let spread = M::ftol_i32_low_masked(M::load_f64(NativeF64Bits::from_bits(
        warhead.cell_spread_f64.to_bits(),
    )));
    let Some(from) = super::world_fire::object_get_coords(world, source_id) else {
        return;
    };
    let cell = [(from.x / 256) as i16, (from.y / 256) as i16];
    let (r, g, b) = rules.radiation.color;
    let frame = world.session.binary_frame() as i32;
    for dx in -1_i32..=1 {
        for dy in -1_i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let x = cell[0].wrapping_add(dx.wrapping_mul(spread) as i16);
            let y = cell[1].wrapping_add(dy.wrapping_mul(spread) as i16);
            let center = crate::sim::cell_rect::get_cellclass_fallback(
                world.resolved_terrain.as_ref(),
                i32::from(x),
                i32::from(y),
            )
            .get_coords();
            let main = &world.main_rng;
            let to = ProjectileCoord::new(
                center
                    .x
                    .wrapping_add(main.next_range_i32_inclusive(-128, 128)),
                center
                    .y
                    .wrapping_add(main.next_range_i32_inclusive(-128, 128)),
                center.z,
            );
            let duration = main.next_range_i32_inclusive(5, 20);
            let amplitude = main.next_range_i32_inclusive(100, 500);
            let _travel_rate = main.next_range_i32_inclusive(0, 20);
            world
                .lifecycle_outputs
                .emit(crate::sim::world::LifecycleOutput::RadiationBeamCreated(
                    RadiationBeamBirth {
                        frame,
                        from,
                        to,
                        z_adjust: 0,
                        color: [r, g, b],
                        house: None,
                        house_color: false,
                        duration,
                        amplitude: NativeF64Bits::from_bits(f64::from(amplitude).to_bits()),
                        eruption: true,
                    },
                ));
        }
    }
}
