# Shared animation coordinates and Building slot admission

PR415 moves Building animation slots into the simulation. Their positions and
lifecycle therefore belong to the same saved state as the animation timer; they
cannot depend on a local window or renderer-only predicates.

## Shared pixel conversion

The match descriptor supplies `PixelConversionBounds`; ScenarioSession retains
and serializes it, and snapshot/hash schema 171 includes it. The default profile
is tactical 472x448, corresponding to the original 640x480 visible rectangle
minus the 168x32 margins. This is an explicit compatibility choice, not a value
read from a player's display. A custom descriptor can select another shared
profile. Rust diagnostic replay headers record it and playback rejects mismatched
initialization. Original native recording headers have no profile extension and
use this engine's default profile during normal scenario initialization.

Building slot creation, damaged replacement, retained-slot reposition on Reveal,
and both live/eager Tile construction use one converter. Signed X >= width or
Y >= height returns a zero offset, matching Tactical6D2360. Negative offsets
remain admitted. The matrix is the fixed rational value of native f32 4.2667,
with truncation toward zero and explicit low32 wrapping. Per AGENTS.md, intermediate
native float32 rounding is intentionally omitted; extreme offsets can differ by
leptons but remain deterministic. Arbitrary native saved Tactical matrices are
not imported or claimed compatible. No local resize writes this match input.

Native evidence: `tools/spatial_oracle/building_pixel_coordinates.py` executes
unchanged matrix initialization, Building coordinate calculation, tactical bounds
and Anim damage-coordinate forwarding. Its three constructor, three bounds and
two damage-argument cases have saved results/metadata. DamageArea is a stopping
boundary, so this is not a damage-result or whole-lifecycle parity claim. Run:

```powershell
python -m tools.spatial_oracle.building_pixel_coordinates --check
```

Configure the pinned retail executable as described in `tools/native_oracle.md`.

## Building slot admission

Two policies formerly approximated in the renderer now use the retained slots:

- InfantryAbsorb with signed ExtraPower > 0 selects ActiveAnim while empty and
  ActiveAnimTwo while occupied. The Building Logic visit clears the other slot
  first and creates the selected slot only when absent, before the Silo update.
  Later active slots are unchanged. Construction remains gated by the existing
  BuildingUp owner; this does not implement the general native body-state machine.
- Authored NeedsEngineer Buildings start with HasEngineer=false. Their load
  finalization disables StuffEnabled and pauses existing slot-Powered animations,
  independent of Type.Powered. A changed-owner transfer sets HasEngineer and
  enables those slots before the delegated owner swap. Same-owner calls do nothing.
  HasEngineer is retained in save/hash state and gates operational queries.

Native identities: map finalization44FD35..44FD4B ->452480; changed owner
4484AF..448522 before448BE8 ->7014A0; absorb selection450B34..450CB7. Slot
construction451890 has a deliberately different pause gate (Type.Powered AND
slot.Powered), and damaged replacement copies the prior frame but not pause.

Native absorbed cargo and garrison occupants are separate containers. The existing
Rust shared-cargo model cannot establish both independently for custom types that
combine InfantryAbsorb and CanBeOccupied. Stock YAPOWR is covered; this change does
not certify hybrid-container parity or complete the passenger architecture.
## Rust validation

The final full library run passes 9,145 tests with 0 failures and 134 ignored.
Relevant production regressions cover custom bounds in create/replace/Reveal,
live and eager Tile construction, diagnostic replay and snapshot persistence,
authored NeedsEngineer pause then changed-owner resume, same-owner refusal,
absorbed cargo empty/occupied/damaged/save transitions, and the constructor's
distinct Type.Powered replacement gate. Independent read-only review and original
`building_pixel_coordinates --check` also passed within the boundaries above.
