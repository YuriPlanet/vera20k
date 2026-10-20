# CMIN Drive instance receipts

`cmin_dock.py` is the existing original-executable owner for the Chrono Miner
refinery chain. Its seven legacy arrays retain all 50 rows and the canonical
SHA-256 `0c09972becd0dbae19513665f9629a251bf443efe5cff0117aa9724541f84879`.
Generation refuses a changed legacy payload before adding `instance_controls`.

The three additive histories use the existing fixture, observer, original-call
helper and FootAI END helper. They cover ordinary fresh/reused/repeated Move;
moving, permission and Foot swap END refusals; successful END and final Release;
and the stopped/moving refinery-pad setter callers. Distinctive retained Drive
fields are declared fixture writes, not a claim that a paid Process produced them.
Only new selected allocations are filled with A5 before their original constructor.
No new execution runner, locomotor port or gameplay reply is introduced.

Run from a source-frozen checkout with the configured retail environment:

```sh
PYTHONPATH=. python -m tools.spatial_oracle.cmin_dock --write
PYTHONPATH=. python -m tools.spatial_oracle.cmin_dock --check
```

The corrected v2 implementation was executed with `--write` then replayed once
with `--check` on 2026-10-03; both returned success on their first attempt. The three
histories contain respectively 10/1/2 steps, 84/12/25 boundary receipts and
69/11/17 completed original-call receipts. The old 50 rows remained canonically
identical. These results establish the bounded coverage below; they do not imply
the excluded mechanisms were executed. Final Rust source binding is maintained
by the shared metadata owner separately from these native output bytes.

## Receipt format

Each history has `steps` with explicit supplied writes, before/after indices into
`boundaries`, native-call/event ranges, raw terminal EAX and actual stack cleanup.
Unit SetDestination is void; its raw EAX does not become a return contract.
`boundaries` retain the reached original PC, ESP, EAX, frame and FPCW. Native call
entry/return receipts match both return PC and ESP, including the helper's stop
PC before it executes. Every recorded call must have a measured return.

Each boundary includes all 108 bytes of the fixture Drive and four allocator
slots, raw Foot/Unit `[ACTOR,ACTOR+0x700)`, Teleport `[TELE,TELE+0x4C)` and all three
1012-byte RNG objects. Allocator slots have explicit construction/delete status;
zero unallocated bytes are not constructor defaults. Decoded Drive fields expose
the **dword** previous/current slopes (`+0x20/+0x1C`), three timer words, the
**dword** interpolation total, destination/head,
residual, binary64 target bits, selector/cursor, all eight flag/padding bytes and
stash. The unused timer word and untouched padding remain raw evidence.

Constructor instructions `0x4AF54D/0x4AF550` zero complete-object current
`+0x1C` and previous `+0x20` as full dwords. Process `0x4B0523` reads current;
`0x4B052A` stores that old current as previous; `0x4B0533` stores the new current.
The declared retained input writes current 7 and previous 9 as two full dwords.
The first receipt interpreted two bytes at `+0x20/+0x21` as slopes; that decode
and its supplied field widths were wrong. Its original recipe/data/logs remain
preserved externally; the corrected recipe is replayed as v2 over the same
three histories and guarded old 50 rows.

Foot decoding retains physical XYZ, applied binary64 speed bits, signed reference
cell, all 24 path dwords, movement/blocked timer words, retries, NavCom/aux,
skip/force/swap/path-blocked bytes, mission/queue, facing and complete NavQueue
header plus its bounded pointed allocation. Contact/tether state accompanies it.

`cell_baseline` stores the entire mapped `0x90000` bytes. Each boundary publishes
exact changed spans and its full-region SHA-256; generation reconstructs and
compares every byte. Independent packed occupation/list words use y-major then
x-major order, four little-endian dwords per cell: ground `+0x124`, deck `+0x128`,
ground object head `+0xE4`, deck head `+0xE8`. Actor/refinery/other/pad-unit `+0x30`
next pointers are recorded separately. All three RNGs are seeded with original
`0x65C6D0(31)` only in these new histories, and reached raw/ranged calls retain
their ECX stream, caller, arguments and actual EAX. Legacy seeds remain unchanged.

## Native boundaries and coverage limits

Fresh construction observes original Drive `0x4AF540..0x4AF5D9`, allocator return,
original AddRef, Unit Link return `0x7426CC`, BEGIN return `0x742772` and install
`0x74277E/0x742780`. Reuse calls the original Unit setter and Drive MoveTo again
without another CoCreate. The gate `0x4AF970` executes original IsMoving and its
stash/permission/Foot-swap checks; both true/false return instructions are saved.
END `0x4AF930` receipts bracket the output write at `0x4AF94B/0x4AF94D` and its
successful return at `0x4AF956`. END preserves retained bytes `+0x1C..+0x68` while
transferring the stash. The last live Drive is saved before final Release.

The existing FootAI helper executes `0x4DAE5F..0x4DAEC6` and its original
IPiggyback Release corresponding to `0x4DAEFA..0x4DAEFD`. It omits intervening
callbacks and remaining FootAI. Operator delete is recorded without freeing;
later readable bytes are seam memory, not a live object. The repeated constructor
uses another fixed allocation; it does not establish CRT address reuse.

Guards cover Drive `+0x6C`, Foot `+0x700` and Teleport `+0x4C`; Drive padding
`+0x12..+0x14`, `+0x28..+0x2C`, `+0x66..+0x68` must remain unchanged. Original
text span `0x401000+0x3E0000`, original class vtables, stack cleanup and FPCW
`0x0E7F` are checked. Inherited fixture Drive was constructed at frame 100;
Teleport/new allocations see frame 200, and the repeated allocation sees 201.
Foot speed and physical Z are independent nonzero inputs.

Existing supplied docking/readiness/nearby-cell/zone/ore and no-effect
Scatter/idle/animation/sound/crate/PerCell seams remain as documented in the
recipe metadata. Every supplied answer must be consumed. These histories do
not establish complete paid-track Process, native save/load, Ship permissions,
generic nested-BEGIN policy or whole-game allocation. They supply bounded native
lifecycle evidence for the current lazy instance-storage migration.
