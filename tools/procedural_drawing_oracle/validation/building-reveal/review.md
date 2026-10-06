# Local building first-reveal review

One fresh read-only critic pass by `/root/building_reveal_critic`, after
implementation, final CPU checks and production captures. No confirmed
implementation defects or blocking findings were reported. The critic made no
source, ref or Ghidra database changes and did not run Cargo.

- Traced the shared admission owner and all SHP, voxel and screen-selection
  consumers; found no competing gate or missed migration. Mobile admission and
  screen selection retain their prior exploration policy.
- Independently regenerated the native building corpus from the pinned Steam
  executable and matched every saved row. Original instructions confirm
  `43CEA0` lifecycle checks and the absence of a cell-exploration draw gate;
  `706389..7063EB` clears packed tint separately from brightness.
- Inspected the 91/92 images and checked the five endpoint manifests, raw frames,
  receipts and four PNG hashes. Confirmed first anchor revelation at step 92
  and observer-on/off byte and simulation-fingerprint equality.
- Inspected saved full lib, Clippy and field-ratchet logs, and the test-only
  archive-owner release before fixture deletion on Windows.

Coverage remains bounded by [the evidence report](../../building_reveal.md):
native geometry is substituted and native execution stops before rasterization;
runtime captures cover one neutral retail building on DX12. They do not
demonstrate native whole-frame pixel parity, enemy ownership, arbitrary
foundations/heights, observers, enabled FogOfWar snapshot lifecycle, or special
colored-effect producers.

The owner's final offline validation initially rejected a capture bundle because
a derived `frame.png` had been added beside its strict seven-entry inventory.
Those task-owned previews were moved outside all seven immutable bundles into
`logs/building-reveal-fix/previews/`; no raw frame, input copy or run manifest was
changed. All seven bundles then passed offline validation again. The original
failure receipt and final validation receipts remain in the local log directory.
