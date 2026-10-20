# Ares 3.0 generic prerequisite controls

The small JSON corpus contains 57 controls executed from Ares 3.0 instructions.
It uses synthetic type names and supplied component inputs, not a mod's rules,
maps or assets. The adjacent metadata pins both input binary identities, entry
points and coverage limits. No executable is distributed.

From the checkout, with the dependencies in `tools/requirements-test.txt`:

```powershell
python -m tools.rules_oracle.ares_prerequisites --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check
python -m tools.rules_oracle.ares_prerequisites --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --output <new-corpus.json>
python -m tools.rules_oracle.ares_prerequisites --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --disassemble
```

Both paths are explicit. The harness rejects identities other than the recorded
inputs before mapping instructions. It reuses `tools.native_oracle` for PE
section validation, mapping and bounded execution. The explicit PE32 mapper
preserves stock executable selection and validation. No DLL patches or Windows startup run.
The DLL is mapped at its preferred base, without relocations or import binding.

The controls cover retained empty/missing reads, nonempty replacement,
comma tokenization, case-insensitive group lookup, building-name precedence,
General-to-generic read order, stock-group override, alternate techno types,
owned all/any checks, planned building membership, and clearing groups before
rebuilding the six defaults. Planned checks intentionally ignore alternates.
One read uses a group beginning with a digit, proving that the first character
is uppercased only when it is an ASCII letter. Alternate checks execute Ares
`CountOwnedNow` at `0x100032F0`: a tracked unit outside the active/on-map set
still qualifies; an on-map count does not replace tracking, and a negative
tracked count is nonzero.

Specific building requirements share `HouseOwnsSpecific` (`0x100082B0..0x10008362`,
exclusive end) through both direct and group members of All and Any. A positive
active building counter (`House+0x5550`, pointer `+0x5554`) succeeds immediately.
Otherwise, the requirement's `PowersUpBuilding` string (`Type+0xE88`) resolves
its declared host through original `0x10007E50`. That host's active counter must
be positive. The body scans every House Buildings entry (`+0x6C`, count `+0x78`),
matches `Building+0x520` against the host type and checks all three type slots
`+0x5EC/+0x5F0/+0x5F4` for the requirement type. It reads no health, limbo,
online, Selling or upgrade-count field. The controls execute direct/group and
All/Any upgrades, an older matching host followed by a newer empty host,
wrong-host and missing-slot failures, zero/negative host counters, case-insensitive
host lookup, slot 2 with count 0, and negative direct/member counters.
`native_specific_visits` records selected original PCs visited in those rows;
visits establish path coverage, not object initialization.

The supporting original Building Unlimbo arm branches on `Type+0xE88` at
`0x440871`, writes the installed type at `0x4409E2`, calls the temporary
building's virtual UnInit at `0x440A20`, and returns at `0x440A2F`. It bypasses
normal Techno Unlimbo and its `House::Added_To_Game` call at `0x6F6D8F`.
An installed addon therefore does not acquire its own active counter through
that admission. These lifecycle and caller statements are instruction reads;
the component controls supply counters and installed slots. They do not execute
installation or destructor tracking. Existing [building-upgrade controls](../spatial_oracle/building_upgrades.md)
own bounded install execution.

Original CanBuild `0x10021B00` calls qualification `0x10021FF0` at `0x10021B40`
before the human gate. Qualification calls Any at `0x10022041` and `0x100220C0`
for negative and override lists. The subsequent normal-list All call at
`0x10021B81` is gated by effective IsHuman (`House+0x1EC/+0x1ED`) and qualification
result 3. These are caller reads, not complete CanBuild execution. The bounded
`--disassemble` command reproduces these original spans without writing files.
No RNG draw, timer write or detach call occurs inside the inspected prerequisite
component bodies; supporting install construction and UnInit remain separate.

Eleven controls execute the exported alternate-membership refresh hooks. The
DLL's `.syhks00` records attach them to original Foot Put `0x4D7221`, Aircraft,
Infantry and Unit destructors `0x4140EB/0x517DF2/0x7357F6`, owner change
`0x7015EB`, and successful discovery tails `0x6F4A1D/0x6F4A37`. The shared
`IsGenericPrerequisite` body (`0x1003DD00`) checks whether the type pointer occurs
in any retained group's alternate list. A matching Foot Put or destructor
sets the current owner's `RecheckTechTree` byte (`House+0x1FC`). Owner change
sets both old and new houses; discovery sets the object's current owner.
The latter two hooks exclude Building objects. Unrelated types set neither
house. The controls use actual original object vtables for type/category
queries and supplied register frames, type extensions, group lists and houses.
They do not execute complete Put, destructor, discovery or ownership changes.

At the hooked stock sites, Put precedes its owner discovery callback. The
destructor hooks precede LeaveTeam and RemoveTracking inside the native game-state
and nonnull-type arm. Owner change follows old RemoveTracking/new AddTracking,
while the object still holds the old owner. The discovery `0x6F4A1D` tail follows
synchronous Tag actions, so the hook reloads the owner after those actions;
early discovery refusals/repeats do not reach either registered tail.
These ordering statements are instruction reads reproduced by `--disassemble`.

The membership helper caches a value and initialized flag at TypeExt `+0x68/+0x69`.
Original constructor `0x1003C717..0x1003C7AB` clears both, and the load reader
`0x1003A3D6..0x1003A3F3` restores the flag and optional value from the stream.
For this fresh-match chain, membership comes from the retained catalog after
rules/type loading and stays fixed during play. Full saved-game behavior and
live INI rereads after a cache has already been filled remain outside coverage;
the bounded ReadINI offset scan is not a proof of every cache invalidation path.
The [older hook source](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Ext/Techno/Hooks.Prerequisites.cpp)
is a matching interpretation lead; DLL instructions and execution establish
the recorded controls.

The reader boundary supplies `ReadString` return payloads, rather than parsing
physical INI bytes. Existing type registries and owned counts are supplied.
ASCII CRT leaves, allocation/deallocation, logging and the type's `WhatAmI`
category query are explicit fixture services. `CountOwnedNow` itself executes
the original category dispatch and reads House tracked counters `+0x5500`,
`+0x5514`, `+0x5528` and `+0x553C`. Original Ares lookup loops, tokenization, list writes,
owned/planned combination loops and group clear/default code execute unchanged.
The corpus does not certify custom-key registration, complete `CanBuild`,
scenario startup, persistence, nested generic definitions or whole-mod parity.

The public `Enum/Prerequisites.cpp` source is an interpretation lead. That
published revision declares an older Ares version; the recorded Ares 3.0
instructions establish these controls and take precedence where they differ.
