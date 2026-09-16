# VXL Stale Normal Index 255 → Ambient VPL Page 16 — Ghidra Research Report

**Date:** 2026-07-19
**Status:** VERIFIED (live Ghidra session, gamemd.exe)
**Trigger:** `tests/retail_goldens` `certify_vxl_structural` proved 8 retail VXLs
reference voxel `normal_index` 255 — 1,931 voxels, exclusively in limbs named
`DUMMY01`/`DUMMY02`: cahead.vxl (952), caheaddm.vxl (556), dpod.vxl (36),
icbm.vxl (64), bike.vxl (205), sreftur.vxl (28), cop.vxl (82), orcab.vxl (8).
Indices 250–254 (mode 4) and 36–254 (mode 2) never occur in retail data.

## Verdict

**gamemd shades every voxel with `normal_index == 255` using VPL page 0x10
(16), deliberately.** Both lighting-precompute variants end by storing the
ambient constant `0x10` into the last three bytes of the normal→page LUT
(indices 253, 254, 255), and the rasterizer indexes that LUT with the raw
unclamped normal byte. This is not "reading stale memory" — the page value for
index 255 is a defined, constant ambient shade.

**Rust-side impact (corrected after checking the actual render path):** the
renderer consumes `blinn_phong_pages` (`src/render/vxl_raster.rs:359`), which
ALREADY forced pages 253–255 to 16 — but only when the table had ≥254 entries,
i.e. only for mode 4. So the mode-4 retail files (sreftur, cahead, caheaddm,
cop) already matched gamemd; the real pre-fix drift was **mode 2 only**
(dpod, icbm, bike, orcab — TS-legacy/Easter-egg models), where index-255
voxels got the zero-initialized page 0 (darkest) instead of 16. The
`get_normal` +Z fallback is not on the shading path (tests only).

**Trigger frequency:** the limbs named `DUMMY01/02` are real geometry (cahead's
DUMMY01 is the entire 42k-voxel model; sreftur's DUMMY01 is 2,831 voxels of the
YR slave-miner refinery turret), so the affected voxels draw whenever those
models draw. The four previously-drifting mode-2 models are TS-legacy /
Easter-egg art not reachable in a stock YR skirmish, so the fix is
correctness-by-construction rather than a visible in-match change.

## Verified chain (each step cites its Ghidra evidence)

1. **Native normals tables.** Dispatch pointer table at `0x008469E0` =
   `[NULL, 0x00846A08, 0x00846AC8, 0x00846C78, 0x00846F78]`; count table at
   `0x008469F4` = `[0, 16, 36, 64, 245]` (prior finding,
   [VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md) §6.2; both dereferenced live this
   session via the decompiles below). Mode 2 (TS) = 36 entries; mode 4 (RA2)
   = **245 entries** at `0x00846F78`, stride 12 (3 × f32).
   - Entry 0 read live: bytes `d1cd063f 3e20b8be 7f3345bf` =
     (0.526578, −0.359621, −0.770317) — bit-identical to our
     `RA2_NORMALS[0]` (verified via `read_memory 0x00846F78`).
   - Entry 244 (last valid) read live: (−0.328…, 0.140…, 0.934…)
     (`read_memory 0x00847AE8`).
   - Bytes past the table end (`read_memory 0x00847AF4`, 132 bytes) are RTTI
     type-descriptor strings (`.?AV?$VectorClass@PAVMovieHandle@@…`) and the
     string `"Movie is sleeping\n"` — i.e. **the binary table simply ends at
     245 entries; there are no duplicate entries 245–249 in gamemd**. (The
     "245–249 duplicate 244" rows in our Rust table come from the community
     dump, not the binary.)

2. **Lighting precompute fills the LUT only up to the mode's count, then
   hardcodes the ambient tail.** Two variants, both verified by decompile:
   - `VXL_SimpleLighting @ 0x00758670` (verified via
     `decompile_function 0x00758670`): loop `for i in 0..counts[mode]`
     computes N·L and stores `g_VXL_NormalLUT[i]`; then unconditionally
     `DAT_00b45a8f = 0x10; DAT_00b45a8e = 0x10; g_VXL_AmbientRGB = 0x10;`.
   - `VXL_BlinnPhongLighting @ 0x007586F0` (verified via
     `decompile_function 0x007586F0`): same loop bound
     (`iVar2 = *(int*)(0x008469F4 + mode*4)`), same three tail stores.
   - `g_VXL_NormalLUT @ 0x00B45990` (verified via `list_globals`);
     `g_VXL_AmbientRGB @ 0x00B45A8D` (verified via `list_globals`).
     `0x00B45990 + 253 = 0x00B45A8D` — **the "ambient RGB" triple IS LUT
     entries 253/254/255.** After any lighting init, `LUT[255] == 0x10`.

3. **The rasterizer indexes the LUT with the raw normal byte, unbounded.**
   `VXL_Rasterizer_RenderMode @ 0x007DF9C0` inner voxel loop (verified via
   `disassemble_function 0x007DF9C0`):
   ```
   007dfa9a: XOR EDX,EDX
   007dfa9c: MOV DL, [EDI + 1]          ; DL = voxel normal_index (0–255, no clamp)
   007dfaa1: MOV DH, [EDX + 0xb45990]   ; DH = g_VXL_NormalLUT[normal_index]
   007dfaa7: MOV DL, [EDI]              ; DL = voxel color_index
   007dfab0: MOV DL, [EDX + 0xb41178]   ; DL = VPL[page*256 + color]
   007dfab6: MOV [EAX + 0xb2ff78], DL   ; shaded palette index -> line buffer
   ```
   The LUT read uses the full byte; the LUT value becomes the VPL page
   (VPL lookup base `0x00B41178`, page-major, 256 colors/page — identical
   layout to our `VplFile::get_palette_index(page, color)`).
   Five sibling rasterizer variants also reference `g_VXL_NormalLUT`
   (xrefs: `FUN_007dfae0`, `FUN_007575a0`, `FUN_00757980`, `FUN_00757790`,
   `FUN_007dfbf0`-family — listed via `get_xrefs_to 0x00B45990`); only the
   RenderMode variant was verified instruction-level. **Confidence for the
   siblings: MEDIUM (same-pattern assumption, not read).**

4. **Both modes converge.** For mode 2 the count is 36, so indices 36–252 are
   stale-from-previous-fill in gamemd (the LUT is shared across models/modes),
   and 253–255 are always freshly `0x10`. Retail mode-2 data references only
   255 (corpus-proven), so the observable is the same: page 16.

## Rust fix (IMPLEMENTED 2026-07-19)

- `src/render/vxl_normals.rs::blinn_phong_pages` now stores `AMBIENT_PAGE`
  (16) at indices 253/254/255 **unconditionally** — previously the override
  was gated on table size and skipped mode 2. Mirrors the native tail, which
  is unconditional in both lighting variants.
- Regression test: `stale_normal_255_gets_ambient_page_in_both_modes`
  (both modes × three facings).
- gamemd's staleness for 245–252 (mode 4) / 36–252 (mode 2) is NOT emulated —
  retail data never references those indices (retail_goldens corpus proof).
- `tests/retail_goldens` ratchets are unaffected (parser output unchanged);
  a pixel-golden of sreftur would be the certifying instrument once the
  pixel-oracle exists.

## Confidence axes (per project RE standard)

| Claim | Content | Identity | Binding |
|---|---|---|---|
| Lighting fills count entries then writes 0x10 ×3 | HIGH (decompiled both) | HIGH (named fns, addresses above) | HIGH (LUT address arithmetic exact) |
| Rasterizer unbounded LUT read → VPL page | HIGH (disassembly) | HIGH (`VXL_Rasterizer_RenderMode @ 0x007DF9C0`) | HIGH for this variant; MEDIUM that all 6 variants share it |
| Binary mode-4 table = 245 entries, no dup tail | HIGH (read_memory) | HIGH | HIGH |
| DUMMY limbs are drawn | HIGH (they are the models' real geometry; no hide mechanism found) | — | MEDIUM (no explicit per-section skip path was searched for) |

## Corrections to earlier claims

- `src/render/vxl_normals.rs` comment (pre-2026-07-19) claimed retail data
  never references indices ≥ 250 — **refuted** by the retail_goldens corpus
  scan (8 files, index 255).
- This report resolves the "what does stale slot 255 shade as" question left
  open in that comment and in [VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md) §9.2
  row 4 ("Rare in retail"): the answer is a deliberate constant — VPL page 16.
