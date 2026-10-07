# Independent packed/query/shadow review

One fresh read-only critic, `/root/packed_reveal_critic`, reviewed the dirty candidate after native, CPU, GPU, strict-retail library/focused/Clippy and release-map validation. No edits, Cargo or publication were performed by the critic. This is the single packed mechanism critic pass; the earlier ordinary admission mechanism had its own prior pass.

## Confirmed finding

P2: `terrain_packed.rs` dispatched ceil(union rectangle area /64) workgroups on X alone. Production requests `Limits::default()` (wgpu-types27.0.1 allows65535 per dimension); supported high-resolution launch choices include4096x4096. A disjoint same-policy wave spanning3840x2160 requires129600 groups. Two distant objects can therefore produce a reachable wgpu validation error even without one large sprite. The previous1280x720 workload did not cover it.

Recommendation: use device-limited two-dimensional dispatch and correctly flatten indices, retaining negative-offset residue ordering; add an actual4K distant-parent GPU regression checking native output and validation errors.

Owner reproduced this before changing production: `packed-4k-red.txt` failed on DX12 with actual dispatch `[129600,1,1]` above65535 (test exit101). The new regression draws two distant parents in a3840x2160 viewport, compares both endpoint pixels to an executed native leaf, and checks the untouched middle in both sRGB formats. The owner then bounded X by the actual requested device limit, added Y for the remainder, and flattened the WGSL builtin grid. Negative-offset residue traversal stays with the same pixel owner; no extra state/uniform field is added.

Owner validation completed: the full packed GPU module passed all three tests on Vulkan and DX12 (native corpus, 4K endpoints and synthetic workload); DrawState passed 10 focused checks; strict-retail Clippy exited 0 with 721 warnings; the final field ratchet was 2501 versus 2505. The corrected release built successfully. Fourteen serial production replays against the first packed candidate were strict MATCH with no differences/errors, including ordinary scenery/mobile frontiers and SUB guard/attack/decloak endpoints. The [final receipt](after-critic.receipt.json) records actual logs, binary identity and comparison hashes. The full library suite was not repeated. This is owner adjudication, not a second critic pass.

## Other findings and coverage

No other confirmed new defect was found in examined canonical query/live Rules migration and affected combat/AI/transition/snapshot/hash consumers; negative-offset residue feedback; overlap scheduling and zero-offset batching; per-frame unique uniform slots and continuation submissions; or shared raw-cloak VXL shadow/NoShadow reader consumers. The critic checked original corpus/control-flow/source-dependency evidence and actual DX12/focused/Clippy logs. The full library run's two fixture failures followed by focused repairs must not be described as a repeated all-green full run.

The review is bounded to packed/query/shadow migration. Existing SHP companion shadow, turret/body source depth, Building nonzero override and FogOfWar Wave gaps do not block the selected single-VXL SUB material validation. They prevent any claim that all objects, mechanisms or whole frames equal native. No repeat critic is required after owner fixes under AGENTS.md unless the user asks.
