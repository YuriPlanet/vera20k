//! Post-load `OreTwinkle` pass and the value-only `Get_Tiberium_Value` helper.
//!
//! Depends on `map::authored_overlay` (native cell-iterator shape),
//! `map::overlay_types`, `rules`, `sim::anim_class`, and `util::lepton`;
//! never on render/, ui/, app/, sidebar/, audio/, or net/.

use crate::map::authored_overlay::NativeOverlayMapShape;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::tiberium_type::TiberiumTypeRegistry;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{
    GROUND_LEVEL_HEIGHT_LEPTONS, LEPTONS_PER_CELL_I32, ground_height_leptons,
};

/// `AnimClass` constructor draw-flags argument of the twinkle spawn:
/// `FUN_00684C30` calls `AnimClass(OreTwinkle, coord, 0, 1, 0x600, 0, 0)`.
const ORE_TWINKLE_DRAW_FLAGS: u32 = 0x600;
/// Constructor `loop` argument of the twinkle spawn.
const ORE_TWINKLE_LOOP_ARG: i32 = 1;
const CELL_CENTRE_LEPTONS: i32 = LEPTONS_PER_CELL_I32 / 2;

/// `CellClass::Get_Tiberium_Value @ 0x00485020`: zero unless
/// `CellClass::OverlayToTiberiumIndex` resolves the overlay to a
/// TiberiumClass, otherwise `TiberiumClass+0xB8 (Value) * (OverlayData + 1)`
/// in native signed 32-bit arithmetic.
///
/// A resolved class index whose TiberiumClass slot is absent dereferences null
/// natively; VERA returns zero for that malformed registry state.
pub(crate) fn tiberium_value(
    overlay_id: Option<u8>,
    overlay_data: u8,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
) -> i32 {
    let Some(overlay_id) = overlay_id else {
        return 0;
    };
    let Some(type_id) = overlay_registry.tiberium_type_for_overlay(tiberium_types, overlay_id)
    else {
        return 0;
    };
    let Some(tiberium) = tiberium_types.get(type_id) else {
        return 0;
    };
    tiberium
        .value
        .wrapping_mul(i32::from(overlay_data).wrapping_add(1))
}

/// Logging/test receipt of one post-load twinkle pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OreTwinkleReceipt {
    /// Real cells whose `Get_Tiberium_Value` was nonzero (one Scenario ranged
    /// draw each).
    pub(crate) resource_cells_rolled: u32,
    /// Twinkle animations constructed on zero rolls.
    pub(crate) spawned: u32,
    /// Zero rolls whose Anim construction failed (missing art); native has no
    /// such failure, so production logs each one.
    pub(crate) spawn_failures: u32,
    /// This pass consumed the GasCloudSys `ParticleSystemClass` native ID
    /// (false when a generated launch's synthetic setup already did, or when
    /// no fresh-load cursor exists).
    pub(crate) particle_system_id_consumed: bool,
}

impl Simulation {
    /// Spend the GasCloudSys `ParticleSystemClass @ 0x0062DC50` native ID the
    /// first time a post-load setup runs after `Clear_Scene` nulled
    /// `DAT_00A8ED78`. Returns whether this call spent it. The object itself
    /// (leptons `(0xA80, 0xA80, 0)`, no RNG in its constructor) is not modeled.
    pub(crate) fn construct_post_load_particle_system_id(&mut self) -> bool {
        if self.post_load_particle_system_constructed {
            return false;
        }
        let Some(cursor) = self.native_unique_ids.as_mut() else {
            return false;
        };
        let _ = cursor.next_id();
        self.post_load_particle_system_constructed = true;
        true
    }

    /// Tail of the native post-load setup `FUN_00684C30` (called from
    /// `ScenarioClass::Read_Scenario @ 0x00684620` after `Full_Init`).
    ///
    /// gamemd-derived: `FUN_00684C30 @ 0x00684FF0..0x006850F3`. After the
    /// zone rebuilds it constructs the global GasCloudSys
    /// `ParticleSystemClass @ 0x0062DC50` whenever `DAT_00A8ED78` is null,
    /// which spends one native ID through `AbstractClass::AssignUniqueID @
    /// 0x00410230`; `Clear_Scene @ 0x006851F0` deletes that object and nulls
    /// the pointer (`0x0068562E`) inside every `Full_Init`, so every fresh
    /// load reconstructs it. Retail `[ParticleSystems]` already registers
    /// `GasCloudSys`, so no Type is allocated. Then, when
    /// `Rules+0x1870` (`[General] OreTwinkle`) is non-null, it walks every
    /// real cell through `MapClass::CellIterator_Next @ 0x00578290` and for
    /// each cell whose `Get_Tiberium_Value @ 0x00485020` is nonzero draws
    /// `Random::RandomRanged @ 0x0065C7E0` on the Scenario RNG
    /// (`ECX = Scenario+0x218`, `0x00685095`) over
    /// `(0, Rules+0x186C OreTwinkleChance - 1)`; a zero roll constructs
    /// `AnimClass @ 0x00421EA0` at the `CellClass` vtable `+0x48` centre
    /// coordinate (`0x00486840`: `(x<<8)+0x80`, `(y<<8)+0x80`, ground height)
    /// with delay 0, loop 1, draw flags 0x600, ZAdjust 0, no reverse. The
    /// constructor assigns the native ID and registers the Anim before any
    /// RandomRate draw and calls `Middle` immediately for delay 0.
    ///
    /// The ParticleSystem object itself is not modeled; only its counter
    /// effect is, through `construct_post_load_particle_system_id` (a generated
    /// launch spends it earlier, from the synthetic `Full_Init`'s own setup
    /// call at `0x00599A5B`). `FUN_0055AF40/50` after the pass write two globals
    /// with no simulation consumer.
    pub(crate) fn run_post_load_ore_twinkle_pass(
        &mut self,
        rules: &RuleSet,
        overlay_registry: &OverlayTypeRegistry,
        map_width: u16,
        map_height: u16,
    ) -> OreTwinkleReceipt {
        let mut receipt = OreTwinkleReceipt::default();
        // `DAT_00A8ED78 == 0` gate at `0x00684FF0`: a generated launch already
        // constructed the object inside the synthetic `Full_Init`'s setup.
        receipt.particle_system_id_consumed = self.construct_post_load_particle_system_id();
        let Some(anim_name) = rules.general.ore_twinkle.as_deref() else {
            return receipt;
        };
        let chance = rules.general.ore_twinkle_chance;
        let type_id = self.interner.intern(anim_name);
        let shape = NativeOverlayMapShape::new(i32::from(map_width), i32::from(map_height));
        for (x, y) in shape.recalc_cells() {
            let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let Some((level, slope_type)) = self
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(rx, ry))
                .map(|cell| (cell.level, cell.slope_type))
            else {
                continue;
            };
            let Some(grid) = self.overlay_grid.as_ref() else {
                break;
            };
            let cell = grid.cell(rx, ry);
            if tiberium_value(
                cell.overlay_id,
                cell.overlay_data,
                overlay_registry,
                &rules.tiberium_types,
            ) == 0
            {
                continue;
            }
            receipt.resource_cells_rolled += 1;
            // Native compares the int bounds signed and swaps reversed ones:
            // chance 0 draws once over {-1, 0}; chance 1 has equal bounds,
            // draws nothing, and spawns on every resource cell (retail is 30).
            let roll = self
                .scenario_rng
                .next_range_i32_inclusive(0, chance.wrapping_sub(1));
            if roll != 0 {
                continue;
            }

            let world_x = i32::from(rx)
                .wrapping_mul(LEPTONS_PER_CELL_I32)
                .wrapping_add(CELL_CENTRE_LEPTONS);
            let world_y = i32::from(ry)
                .wrapping_mul(LEPTONS_PER_CELL_I32)
                .wrapping_add(CELL_CENTRE_LEPTONS);
            // VERA-internal: a slope record outside the 0..20 retail domain has
            // no safe native evaluation; flat level height stands in
            // (gamemd equivalent UNCHECKED).
            let world_z = ground_height_leptons(level, slope_type, world_x, world_y)
                .unwrap_or_else(|_| i32::from(level).wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS));
            let mut descriptor = AnimClassSpawnDescriptor::new(
                type_id,
                rx,
                ry,
                SimFixed::from_num(CELL_CENTRE_LEPTONS),
                SimFixed::from_num(CELL_CENTRE_LEPTONS),
                level,
            );
            descriptor.draw_flags = ORE_TWINKLE_DRAW_FLAGS;
            descriptor.loop_count = ORE_TWINKLE_LOOP_ARG;
            let native_unique_id = match self.native_unique_ids.as_mut() {
                Some(cursor) => cursor.next_id(),
                // Compatibility fixtures without a fresh-load cursor mirror the
                // runtime `spawn_anim` identity scheme.
                None => self.substrate.next_stable_object_id as i32,
            };
            match self.spawn_load_anim_at_world(
                &rules.art_registry,
                rules,
                descriptor,
                AnimWorldCoord {
                    x: world_x,
                    y: world_y,
                    z: world_z,
                },
                native_unique_id,
            ) {
                Ok(_) => receipt.spawned += 1,
                Err(error) => {
                    receipt.spawn_failures += 1;
                    log::error!(
                        "OreTwinkle {anim_name} at ({rx},{ry}) failed to construct: {error}"
                    );
                }
            }
        }
        receipt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn get_tiberium_value_is_zero_for_non_resources_and_wraps_signed_products() {
        let ini = IniFile::from_str(
            "[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nValue=2147483647\n\
             [OverlayTypes]\n0=ORE\n1=ROCK\n[ORE]\nTiberium=yes\n[ROCK]\nIsARock=yes\n",
        );
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let tiberiums = TiberiumTypeRegistry::from_ini(&ini);

        assert_eq!(tiberium_value(None, 5, &overlays, &tiberiums), 0);
        assert_eq!(tiberium_value(Some(1), 5, &overlays, &tiberiums), 0);
        assert_eq!(tiberium_value(Some(0), 0, &overlays, &tiberiums), i32::MAX);
        assert_eq!(
            tiberium_value(Some(0), 1, &overlays, &tiberiums),
            i32::MAX.wrapping_mul(2),
            "native imul wraps the signed product"
        );
    }
}
