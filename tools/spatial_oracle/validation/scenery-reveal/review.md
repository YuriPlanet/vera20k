# Local ordinary scenery review

One fresh read-only pass by `/root/scenery_reveal_critic`, after implementation,
native/Rust/GPU checks and production captures. No confirmed implementation
defect or blocker was found. The critic did not author this change and modified
no files, database entries, processes or remote state.

- Independently read native parent loops `6D6D10`, `6D97D0`, `6D3AC0`, original
  Steam instructions for `71CC50`, and the dynamic Terrain branch's stub.
- Traced production `build_world_instances`, the shared Terrain/cell-overlay
  instance builder and retained Display lowering. Lifecycle, camera admission,
  current overlay identity and simulation authority remain intact.
- Inspected actual DX12 tree/deck images and verified all endpoint PNG,
  profile/config/contract, run/capture and BGRA hashes against the receipt.
- Checked five complete before/after observation and fingerprint comparisons,
  observer-on/off pixels and state equality, actual native/Rust/GPU validation
  logs, and `git diff --check`.

Coverage is bounded by [the evidence report](README.md). Native admission and
prepared draw results do not establish native whole-scene pixels. Rust captures
cover ordinary trees and a low wooden bridge on one retail map with FogOfWar
disabled; animated/crumbling lifecycle, enabled FogOfWar/gap, arbitrary
overlays/heights and scalability are unproven. The bridge GPU regression is
explicitly run, rather than included in the default lib suite. Tree transition
protection currently rests on the retained reproducible runtime captures.

Rock identification was unresolved during this pass. Actual overlay rocks may
share the repaired gate, while TMP details follow another path. Additional
owner validation must be recorded separately without broadening the critic's
coverage or claiming every stone symptom is fixed.
