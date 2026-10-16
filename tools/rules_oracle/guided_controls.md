# Retained General projectile controls

`guided_controls.py` executes original gamemd.exe SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The JSON preserves original binary64 bits and signed altitude values. Its
metadata pins the payload and fixture boundaries.

Full RulesClass constructor `665650` initializes `+598` to binary64 `0.25`
and `+5A0` to signed `500`. The original reader slice `66EC1B..66EC62`
(end exclusive) reads exact-case `[General] MissileROTVar` through
`ReadDouble5283D0`, stores it at `66EC3C`, then reads exact-case
`MissileSafetyAltitude` through `ReadInt5276D0` and stores it at `66EC5C`.
The altitude key is **MissileSafetyAltitude**, not SafetyAltitude. Both
reader calls use the retained field as their default. The same native Rules
object survives all 30 sequential controls; no result is supplied by Python.

The controls include missing sections/keys, physical-empty-value omission,
case distinctions, precise decimal input, numeric suffixes, percent, signed
zero, decimal/hexadecimal overflow, malformed values and float range limits.
ReadDouble's original `%f` scanner first stores binary32, then widens it to
binary64. For example, text `0.1` stores binary64 bits `3fb99999a0000000`;
`0.25000000000000006` stores `3fd0000000000000`. A percent character anywhere
in the string applies the original percent multiplication. ReadInt preserves
native signed wrapping and its hexadecimal-versus-decimal parser branches.
No RNG draw or native runtime identity allocation occurs in these reads.

The fixture supplies lexical cached INI entries after the production loader's
ASCII trimming and omission of empty values. It does not execute physical INI
loading. A separate two-row raw-cache sequence intentionally retains empty
strings; it must not be interpreted as the physical `Key=` behavior. There is
no verified retail-value claim for the authored controls.

## Comparison boundaries

The constructor executes in full, but the full General reader and full
Process chronology do not execute here. The inherited fixture supplies
allocator/CRT/TLS/archive boundaries and constructs an unrelated Weapon before
the Rules object. That Weapon is not read by the selected slice. Original
scalar instructions/readers remain unpatched, with x87 precision/control
`0E7F` (53-bit, chop).

A failed float scan leaves its destination word unchanged. In this isolated
caller/cache fixture it contains `24ba5dd9` (the General cache CRC), which
widens to binary64 `3c974bbb20000000`. This is a fixture observation of stale
ABI data, not a portable malformed-input default: the unexecuted preceding
General body may leave different scratch data. Production Rust uses its
existing deterministic malformed-input policy; exact native equivalence for
failed float scans is not claimed.

The decimal minimum-subnormal probe produces native binary32 `00000002`,
where Rust parsing produces `00000001`. The saved native result is retained
without fitting a replacement parser. Production comparison excludes that
row and the malformed row plus its immediately retained successor. Overflow
and underflow observations remain explicit parser results; they do not certify
non-finite or extreme controls as valid gameplay inputs.

`src/rules/guided_controls_tests.rs` applies the saved sequential lexical inputs
through the production Rules process and transfers each retained registry
receipt into the next pass. It checks the effective RuleSet fields as well as
the retained owner. Native arithmetic goldens come from this corpus. Cargo
validation is coordinated by the chain owner; a passing parser comparison
alone does not establish complete IFV flight or whole-world parity.

## Reproduce

Set `VERA20K_GAMEMD_EXE` or `RA2_DIR` and
`VERA20K_PROJECTILE_RENDER_ASSETS` as described in
[the physical-input guide](../projectile_oracle/bridge_render_inputs.md),
install the existing Unicorn 2.1.4 fixture dependencies, and run from the repo:

```sh
PYTHONPATH=. python -m tools.rules_oracle.guided_controls --check
VERA20K_REQUIRE_RETAIL_INI=1 cargo test -p vera20k-core --lib rules::native_processing::guided_controls_tests::
```
