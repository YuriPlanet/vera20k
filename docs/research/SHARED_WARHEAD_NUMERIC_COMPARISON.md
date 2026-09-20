# Shared warhead numeric receiver

The production owner is `sim/combat/damage/kernel.rs::apply_warhead_damage`.
Ordinary Object damage, the accepted Psychedelic path, and Terrain damage use
this same function. It now uses the shared deterministic x87 value operations;
there is no estimator-specific second damage kernel. Native identity is
`gamemd.exe` `00489180..0048926C`, SHA256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.

## Acceptance and behavior

This increment replaces the existing host-f64 arithmetic and saturating casts
with native PC53/chop evaluation, binary32 spills and signed64 conversion's low
32 bits. It preserves the existing production API and migrates all six existing
consumers that independently implemented the same masked low32 conversion policy.
Save layout and state ownership do not change in this numeric increment.

Original instruction order is consequential:

1. Zero damage, the Scenario no-damage flag and null warhead return zero. The
   decoded Rust API leaves null-warhead admission with its caller.
2. Negative damage returns unchanged only at signed distance less than eight;
   it bypasses Verses and MaxDamage. This is a distance gate, not an armor gate.
3. FILD loads exact signed damage. FST writes its binary32 spill without popping;
   PercentAtMax multiplication still uses the exact live register, then spills
   the product separately to binary32.
4. Binary32 CellSpread times 256 goes through original `_ftol` semantics. Valid
   signed64 results retain low EAX; invalid masked conversion has low EAX zero.
5. Unequal ordered spills and nonzero converted spread admit interpolation.
   Native FCOMP's C3 test bypasses it for both equal and unordered operands.
   Distance subtraction wraps signed32; subtraction, multiply, divide, add and
   conversion retain their original order even when distance is zero.
6. Negative falloff is floored to zero, then multiplied by binary64 Verses and
   converted again. Signed MaxDamage caps the result without a minimum floor.

For example, damage 16,777,217 with spread one, PercentAtMax zero and distance
zero yields 16,777,216 because of the spill. With spread zero it retains the
original damage. Regrouping the expression loses that distinction.

`WarheadType` keeps widened parsed binary32 fields for compatibility. The kernel
recovers those memory encodings; exceptional recovery uses raw bits rather than
a host NaN cast. The complete parser admits PercentAtMax=+/-1e40 as infinity.
For damage 100 and unit Verses, native output is 100 with spread zero and zero
with spread one. Parser-to-production tests protect this path from a finite-only
load panic. Raw NaN and infinite-spread fixtures do not claim parser admission:
literal NaN currently parses as zero, and spread has a separate fixed projection.

## Arithmetic authority and migrated consumers

`util/native_x87.rs` retains its fallible finite APIs. Its shared
`ftol_i32_low_masked` maps only signed64 integer-conversion failure to the
masked indefinite low dword; its masked binary32 store uses signed maximum
finite on overflow under chop rounding.

The opaque `MaskedX87Value` wrapper adds signed infinity, quieted loaded NaNs,
invalid-operation indefinite, unordered comparisons and masked stores/conversion.
Finite operations delegate to the established finite owner. Callers cannot
construct invalid payload encodings. NaN propagation remains within that owner.

The crate placement timer, postmortem delay, base-defense response timer, scenario
AI opening grant, ore growth reload and Jumpjet flight now call the same low32
conversion policy. Their preceding arithmetic and output signedness are preserved.
Jumpjet's redundant local conversion function is removed. Consumers that
intentionally reject invalid conversion or assert bounded inputs keep the
fallible API; these are different contracts, not missed duplicate policies.

## Reproducible evidence

Set `VERA20K_GAMEMD_EXE` to the pinned original retail executable, then run:

```text
python -m tools.spatial_oracle.estimated_damage --check
python -m tools.spatial_oracle.x87_masked_values --check
python -m tools.spatial_oracle.x87_masked_hardware --check
cargo test -p vera20k --lib
cargo clippy -p vera20k --lib
```

The JSON files record native outputs; their companion metadata records binary
identity, original code, fixture assumptions and substitutions.

- `estimated_damage`: 1,066 cases, comprising 381 estimated-damage wrapper cases
  and 685 direct kernel cases. The original finite 862 cases remain unchanged.
  Rust compares all 971 represented nonnull kernel invocations, 476 at nonzero
  distance, and 881 binary32 spill pairs. The 31 wrapper-only returns and 64
  null-warhead calls are accounted for separately from the decoded-field API.
  The surrounding estimated-damage factor pipeline is not implemented by this
  comparison; only its observed nested kernel invocation is compared.
- `x87_masked_values`: 2,376 emulator cases execute isolated original loads,
  FADDP, FSUBP, FMUL, FDIVP, FCOMPP, FSTP and the original `_ftol` body. Same-format
  operand matrices cross signed zero, subnormal, one, maximum finite, infinity
  and quiet/signaling NaNs. Mixed-format cases examine NaN sign/payload selection.
  The harness sequences original fragments and supplies operands; it does not
  execute their parent functions or calculate expected arithmetic in Python.
- `x87_masked_hardware`: the same 2,376 inputs captured on actual AMD x87 hardware.
  A standalone assembler probe relocates authenticated original numerical opcode
  bytes into an x86_64 fixture wrapper, preserving operand formats, evaluation
  order and 0E7F. It saves/restores caller FPU state and checks stack balance.
  Python supplies bits and records outputs; no VERA20k arithmetic generates these
  references. Rust compares every hardware result through the shared API.

The additional hardware capture was necessary: Unicorn 2.1.4 does not quiet
signaling NaNs at FLD. Load/store and later two-NaN arithmetic therefore disagree
with hardware in 228 rows (four loads, 56 each add/subtract/multiply/divide).
The raw emulator observations are retained alongside hardware outputs, rather
than changing production arithmetic to reproduce the emulator defect. Intel
specifies masked SNaN-to-QNaN conversion for these loads; Unicorn's conversion
code omits the quiet bit when constructing the extended value. See
[Intel SDM Vol.1, Tables 4-7, 8-9 and 8-10](https://cdrdv2-public.intel.com/671436/253665-sdm-vol-1.pdf)
and [Unicorn 2.1.4 conversion source](https://raw.githubusercontent.com/unicorn-engine/unicorn/2.1.4/qemu/fpu/softfloat-specialize.inc.c).

Hardware regeneration needs an x86_64 host and rustc; the saved capture records
CPU, compiler and probe-source identity. Another host requires deliberately
reviewed new provenance. Production arithmetic and saved-fixture tests remain
portable, including non-x86 hosts. The probe executes original FISTP under the
supplied control word, not the full `_ftol` wrapper; the emulator corpus executes
that full original helper. Neither probe executes the original parent gameplay.

FPCW 0E7F is the supplied normal startup PC53/chop policy, supported by prior
startup captures; this is not a new live capture inside the warhead function.
These corpora establish bounded value comparisons. They do not implement a full
x87 stack/environment, unmasked traps, exception-status handling, arbitrary
operation chains or full extended-exponent overflow/underflow.

## Validation and remaining work

Final isolated candidate validation (2026-09-20): full library **9,024 passed,
0 failed, 134 ignored**; library Clippy completed with **1,190 warnings**, matching
the prior finite-arithmetic candidate's count. Kernel/native and hardware replay
checks passed independently. The hardware-referenced Rust matrix passed every
2,376 row. No replay baseline was changed.

Ordinary receiver defense transforms, complete estimated-damage factor producers,
scanner acquisition/firing debits, signed actual-health storage and the Foot/track
receiver migrations remain separate required work. None is completed by changing
this shared kernel. Broader worktree replay failures must be resolved in their
own production paths, not copied into new hash expectations.
