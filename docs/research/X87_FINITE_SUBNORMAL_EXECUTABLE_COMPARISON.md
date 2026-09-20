# Finite x87 subnormal values

## Acceptance and ownership

The existing `src/util/native_x87.rs::X87Chop53` integer arithmetic owner must
accept finite binary32/binary64 subnormal loads and produce chopped subnormal
stores, including correctly signed underflow zero. Preserve its existing normal
arithmetic, explicit memory-store boundaries, public Result signatures and
cross-platform integer implementation. Remove the obsolete subnormal error
variants after checking their consumers. No second numeric representation or
host floating-point fallback is introduced.

This is a prerequisite for native movement/rocking numeric migrations. It does
not complete those production receivers or the whole-engine authority audit.
The same load/store owner serves existing callers. The independent review found
one alternate finite arithmetic implementation in `house_base::multiply_factor`,
retained specifically because the shared owner rejected subnormals. This
increment removes its decoder, multiplication and underflow/store logic in favor
of `X87Chop53`. Only its existing masked-overflow policy remains as an adapter:
`StoreOverflow` becomes signed maximum finite binary32. Other callers keep their
checked-store policy. `scaled_cost` also drops its separate subnormal shortcut.

`refresh_factors` is the sole fold caller. The production lifecycle reaches it
through `World::remove_house_base_membership` and `HouseBaseState::remove_membership`.
The independent ordered membership, cached factors and registration data remain
unchanged and continue to serialize/hash through `HouseState::base_projection`;
no save-layout change is needed. Factory construction integration and weighted
base-centre consumers remain deferred in the current source. This arithmetic
consolidation does not claim to finish those receivers.

## Original executable evidence

Pinned `gamemd.exe` SHA256:
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The established normal-session x87 control word is `0E7F` (53-bit precision,
truncate toward zero, masked exceptions). Load/store comparisons preserve that
ambient policy. The representation normalizes a memory fraction into the
existing sign/exponent/53-bit significand; memory stores discard fraction bits
below the destination's least subnormal bit and preserve the sign of zero.

`tools/spatial_oracle/x87_subnormal_primitives.py` executes unchanged original
instruction fragments: FLD binary32 at70B743, FLD binary64 at4B1150, FADDP at
4B1064, FCHS at70B5D3, FMUL at4B105E, FSTP binary32 at70B812 and FSTP binary64
at4B10F6. Fixture sequencing chooses operands/operations between those fragments;
it does not claim original whole-function control flow. Expected output bits
come from execution, not a Python/Rust arithmetic model. Metadata records
instruction identity, sequencing, control state, integrity and excluded inputs.

The 1,780 rows include 180 load/store, 512 add, 512 subtract and 576 multiply
cases. They cover signed zeros, signed subnormal/normal boundaries, cancellation,
large exponent gaps, PC53 rounding and signed underflow zero. These are bounded
executable comparisons, not exhaustive enumeration of all finite operands.

`unit_per_cell_overlay.py` separately executes original73AFD4..73B074 with
explicit Map, ability, coordinate, sound and damage receiver stubs. Its 296 rows
include144 admitted original velocity additions, including subnormal input bits.
The Rust comparison uses its observed admission result solely to choose whether
to compare the arithmetic; it does not certify the Rust overlay predicate,
callbacks, wall removal or scheduler. All native gates/stores remain original.

`factory_plant_factors.py` runs the complete original House50BF60 fold. Its ten
existing stock-factor rows are preserved; 18 added raw finite-factor rows cover
signed subnormals, normal boundaries, signed zero and masked overflow. The Rust
comparison builds membership, removes one plant through the same receiver used
by production removal, and compares the recomputed factors with all 28 original
outputs. This exercises the migrated consumer rather than only its math helper.
`building_weight_cost.py` retains its twelve original rows and adds 24 original
BuildingType45EDD0 cost-receiver comparisons using signed tiny factors, extreme
integer costs and optional FreeUnit costs. These cover removal of the separate
subnormal shortcut. Raw boundary inputs are arithmetic fixtures, not assertions
that stock rule files provide every value.

## Regression checks and limits

Named Rust checks in `util::native_x87::tests`:

- `finite_subnormal_primitives_match_original_x87_instructions`
- `finite_overlay_velocity_addition_matches_original_common_tail`

Consumer checks in `sim::world::house_base::tests`:

- `original_factory_plant_f32_fold_including_gradual_underflow`
- `original_building_weight_cost_uses_free_unit_and_live_house_factors`

Existing normal arithmetic tests remain intact. The obsolete exceptional-domain
test now retains binary64 nonfinite rejection instead of subnormal rejection.
No external consumers matched the removed subnormal error variants.

Reproduce from the repository with the original executable configured:

```powershell
$env:VERA20K_GAMEMD_EXE='C:/path/to/gamemd.exe'
python -m tools.spatial_oracle.x87_subnormal_primitives --check
python -m tools.spatial_oracle.unit_per_cell_overlay --check
python -m tools.spatial_oracle.factory_plant_factors --check
python -m tools.spatial_oracle.building_weight_cost --check
cargo test -p vera20k --lib util::native_x87::tests::
```

NaN/infinity, division by zero, full80-bit exponent extremes,
exception/status flags and unmasked traps remain outside the existing finite
value model. Shared stores continue to reject overflow; the House consumer's
existing signed-saturation adapter is now explicitly compared with native stores.
The separate retail `sqrt_approx_f32` lookup is unchanged; these
checks do not prove its complete input domain. Production migration of the
rocking update, its impulse producers, scheduling, damage and rendering remains
separate work. Clean-candidate full library, Clippy and independent review are
required before publication; current execution results are recorded in the PR.
