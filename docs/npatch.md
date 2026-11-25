# Optional NPatch compatibility

NPatch is a long-standing binary patch for Yuri's Revenge that predates the DLL-based Ares and
Phobos extensions. VERA20K implements the supported behavior in Rust. NPatch is in every
build and switches on from a game's data, like Ares: adding its keys to the Rules layers,
the selected mode or the map is all it takes.

```ini
[General]
VehicleParachute=VEHCHUTE
DisableParallelAIQueues=no
```

`rules::npatch_switch` decides the per-game switch: any key only an NPatch reader found
(`VehicleParachute`, `DisableParallelAIQueues` or its ext173 name
`DisableAIParallelProduction`, `EnableAIBuildLimitation`, a TechnoType's `AIBuildLimit=`,
a BuildingType's `AIPlanningCount=`) turns NPatch on and logs the deciding key. Retail data
holds none of them, so a retail game plays as gamemd. There is no build feature: a plain
`cargo run` carries the code, and a mod that ships NPatch keys plays NPatch.

The keys the processing passes read travel with the processed rules
(`ProcessedRulesLayers::npatch_inputs`), so a save, a replay and every peer decide the same
answer from the same files. A save written before the switch existed converts with an empty
read list and keeps NPatch off, as those matches played ([save compatibility](save-compatibility.md)
lists each layout).

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

NPSE's naval vehicle choice uses the existing mobile-choice owner. House Update asks the
land and naval channels in order (`0xC33AB0`), sharing the harvester decision and team
chooser. Suggest_New_Object reads the producer's channel (`0xC2F130`); exit, abandonment
and build-limit rejection clear the corresponding choice (`0xC33940`, `0xC33B50`,
`0xC33570`). These paths run when the game's data turns NPatch on (`rules::npatch_switch`).

The legacy `[General] EnableAIBuildLimitation=yes` switch applies the same comparison to
`BuildLimit=` (NPExt ext173 reader `0xC2D132`, gate `0xC2CF67`). It defaults to no. This is
compatibility with the [older NPExt release](https://ppmforums.com/topic-29537/npatch-extended-finally-released/);
NPSE #359 replaced the global switch with `AIBuildLimit=`. If a VERA ruleset explicitly sets
both, both constraints apply before production starts. Clearing a choice does not skip an
unsatisfied building-plan node or guarantee the mobile chooser selects an uncapped type.

`[General] DisableParallelAIQueues=` defaults to no; its key turns NPatch on.
NPatch Extended (ext173) spells the same global switch
`DisableAIParallelProduction=` ("whether we should prevent AI factories from
'cloning' units"). ext173's `ReadGeneral` hook reaches the reader at
`0xC2BDDA..0xC2BDE7`, which writes `Rules+0x640`, immediately before
`EnableAIBuildLimitation` at `Rules+0x641`; `0xC2D482` gates the same five
counters. VERA reads the NPSE name first and falls through to the ext173 name
when it is absent or unparsable. The 2022-era revision that
[PR #2877](https://github.com/YuriPlanet/vera20k/pull/2877) discusses sets the
name per type; only `[General]` is consulted here, so a type-section key stays
unread instead of driving this global switch. The NPSE native reader preserves
the current byte (`0xC30303..0xC3031A`); the Rules constructor zeroes it
(`0x66569B`). With yes, Factory_AI reserves one of five House
production counters before compatibility, allocation and construction (`0xC2F07C`):
aircraft, land vehicles, naval vehicles, infantry and buildings. Only vehicles split
by `Naval=`. The existing House AI production owner saves and hashes those counters.

Mobile exits release at their native class admission prefix, before later placement
can fail; buildings release after successful Unlimbo (`0xC33940`, `0xC2F184`,
`0xC2F1A3`). Abandonment releases the held object's House before refund (`0xC2EFB4`).
Failed starts and PointerExpired retain their reservation. Releases run independently
of the switch and clamp signed negative counts to zero, matching the original hooks.

## Original project and authors

VERA20K implements the documented behavior in Rust. It neither loads nor patches
`gamemd.exe`, and it does not redistribute NPatch binaries.

NPatch (NewPatch) is a RockPatch-based YR platform created by **CnCVK (VK)**,
a former RockPatch developer. The project has several editions:

- **NPatch 1.00** - the original release, published by CnCVK.
- **NPatch Extended (NPExt)** - released by **LH_Mouse**; its manual is
  NPExt #169 (2011), and it thanks pd and CnCVK. VERA's legacy
  `EnableAIBuildLimitation` path cites the NPExt `ext173` reader
  (`0xC2D132`, gate `0xC2CF67`).
- **NPatch Special Edition (NPSE)** - installer #0020 on CnCVK's NPatch
  download page. The **NPSE 2.0 #359** manual and changelog are authored by
  **lydjjlyd**.
- **NPatch Autumn Edition (NPAE)** - installer #0011 on the same page.

The NPSE 2.0 manual credits its testers **我爱二次元**, **蓝天白云fd**,
**好人先昇**, **行少** and **凌敏SHOS**. That manual and its release are the
source for the `0xC3xxxx` addresses cited above.

简体中文：[扩展支持与原作者](zh-CN/credits.md)。

## Cross-extension compatibility

Ares, Phobos and NPatch are orthogonal and mixable. Ares and NPatch are in every build and a
game's data switches them on ([Ares](ares.md), `rules::npatch_switch`); Phobos is still a build
feature:

```sh
cargo run --release --bin vera20k --features phobos
```

Every implemented feature must keep working when Ares plays or the other capabilities are
compiled in. A chain
may depend on another **capability's own** state (for example a Phobos key that only a Phobos type
reads), but it must not assume the other features are absent, and it must not re-own state another
capability already owns. When two extensions touch the same native site, one of them wins by native
precedence and the other keeps its own owner — the Phobos radiation activation replacing the retail
one, with the retail substitution kept for builds that lack the capability, is the pattern to
follow.

The build capability is recorded per combination: a save from one combination cannot load in
another, so a mixed build never reads a save written by a single-extension build. NPatch left
that matrix when it became a per-game switch: every build carries it, and a game with NPatch
keys differs from a stock game through its rules digest, not through the build stamp. A save
written by a build that still had the `npatch` feature names that capability and is refused by
the current build, which names the mismatch.
