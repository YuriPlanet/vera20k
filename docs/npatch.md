# Optional NPatch compatibility

NPatch is a long-standing binary patch for Yuri's Revenge that predates the DLL-based Ares and
Phobos extensions. VERA20K implements the supported behavior in Rust. Enable it explicitly:

```sh
cargo run --release --bin vera20k --features npatch
```

The feature compiles to `rules::NPATCH_ENABLED`. A build without it ignores the extension keys
and keeps stock behavior, like a build without `ares` or `phobos`.

`[General] VehicleParachute=` chooses the canopy for airdropped non-infantry, falling back to
`Parachute=` when unset. Infantry keeps `Parachute=`. The name uses the retained animation
reference and asset-loading owners, including empty-layer retention and `none` clearing.
This follows NPSE 2.0 #359's ReadGeneral (`0xC31005..0xC31017`) and ObjectClass::Paradrop
(`0x5F5ADD`, `0xC2B0E4`, `0xC2FA75`, `0xC2F939`) from the
[author-linked release drive](https://zhytat.ysepan.com/). Parachuted bombs and their `IsBomb=`
branch remain outside the current descent implementation.

Per-building `AIPlanningCount=` repeats eligible base-plan candidates before seed selection
(`0x5056C1 → 0xC2C331`). The reader stores one byte; the native signed loop comparison repeats
counts 1–127, while zero and high-bit counts retain one entry. The existing planner and RNG
owners handle the resulting candidate vector.

Per-type `AIBuildLimit=` limits AI production using the existing tracked and produced-count
owners (`Factory_AI`, `0xC335A0..0xC33726`). Positive limits include limbo factory objects;
non-positive limits compare the magnitude against both counters, and zero blocks the type.
Reaching a cap clears the standing choice, without advancing the base plan. A capped building
can therefore stall construction.

NPSE also splits the vehicle choice by `Naval=`: `Suggest_New_Object`'s hook
(`0x4FBDAF -> 0xC2F130`) reads `House+0x16130` for a naval producer and `House+0x5650`
otherwise, so a shipyard and a war factory no longer share one pick. Without the split the
shipyard fetched the war factory's land unit, failed the `Naval=` mismatch check at `0x450343`
and idled, which stock does too.

`[General] DisableParallelAIQueues=` (default no) suppresses the other half of the cloning
behavior: a computer house's factories normally all fetch the same standing choice and produce
it in parallel. NPSE 2.0 reads the switch into `Rules+0x2D` from its `RulesClass::ReadGeneral`
hook (`0xC302E9..0xC3031A`) and gates `Factory_AI`'s fetched type on it (`0x45033C` hook, cave
`0xC335A0` -> `0xC2F07C`): the start is skipped while the house already counts a producing object
of the same class. The cave keeps five counters at `House+0x160D4..0x160E4` — aircraft, unit with
`UnitType+0xCCE` clear, unit with it set, infantry, building — and `0xCCE` is the `Naval=` byte
(`0x00714A77`), so ground and naval units count apart: NPSE #333 fixed the switch having stopped
the AI's naval yards behind its war factories. A per-type `DisableAIParallelProduction=` existed
in older revisions only; NPSE #331 removed it in favor of the per-type `AIBuildLimit=` above, so
this port reads no per-type switch. The start gate scans live factory state instead of adding a
second set of counters, which also releases a production the native counters would leak.

## Cross-extension compatibility

The three capabilities — `ares`, `phobos`, `npatch` — are orthogonal and mixable:

```sh
cargo run --release --bin vera20k --features ares,phobos,npatch
```

Every implemented feature must keep working when the other capabilities are compiled in. A chain
may depend on another **capability's own** state (for example a Phobos key that only a Phobos type
reads), but it must not assume the other features are absent, and it must not re-own state another
capability already owns. When two extensions touch the same native site, one of them wins by native
precedence and the other keeps its own owner — the Phobos radiation activation replacing the retail
one, with the retail substitution kept for builds that lack the capability, is the pattern to
follow.

The build capability is recorded per combination: a save from one combination cannot load in
another, so a mixed build never reads a save written by a single-extension build.
