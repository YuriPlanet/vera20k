# Ares Country loading presentation

`ares_country_loading.py` reuses `ares_country_veterancy.CountryFixture` and
`ares_prerequisites.Probe`; there is no second mapper, VM, heap or INI parser.
Supply enrolled binaries through `VERA20K_ARES_DLL` and `VERA20K_GAMEMD_EXE`, then
run `python -m tools.rules_oracle.ares_country_loading --check`.

## Native component execution

The reference contains twelve retained histories (sixteen dispatches), eleven
constructor/ID-default controls, and four original `sprintf` filename controls.
The sidecar records the scope, substitutions and payload identity. The original
instructions stay unchanged, and monitored RNG/detach entries reject execution.

`1001C530` admits phases 0/1/2. Phase 0 resolves the current Side's default load
color, without resetting GUI fields. Side0/1 store -1 if their named scheme is
missing; Side2 tries YuriLoad then Purple and stores the resulting index/-1.
Other Side values leave the prior color unchanged. Its primary-Rules pointer check at
`1001C5A6` admits `1001C770` only for the actual selected root. That GUI reader
reads `File.LoadScreen`, `File.LoadScreenPAL`, `LoadScreenText.Name`,
`.SpecialName` and `.Brief` with ReadString2048 and 31-byte destination payloads.
Missing/empty retains; none clears file fields but remains a literal CSF key.

Common reader `1001C5D0` then runs the existing one-shot parent owner `1001BDC0`.
It copies GUI fields and color alongside VeteranBuildings, without copying the
extension phase or parent flag. Thus first-primary child GUI may be overwritten
by the parent's current values; a later primary read can override them. Color
read `1001C686` follows that copy on every admitted pass. Find `1001BB30`
requires a current kind1 scheme, unknown retains, and none yields signed -1.
The fixture includes paired kind1/kind53 schemes, as does production. Kind1 is
the even slot; base Country Color uses kind53/odd slots, a distinct reader.

Constructor prefix `1001B767..1001B911` and initializer `1001C0F0` establish the
ten literal Country-ID defaults and unknown-ID observer/GUI:Unknown defaults.
The comparison is case-insensitive by stored ID, never registry ordinal or Side.
Named PCX/SHP resource initialization calls are explicit null-return sinks.
Actual CSF734E60 executes against an empty catalog; localized pixels are outside
these controls. ReadString supplies lexical ASCII inputs, not physical INI data.

Original `5534DF` calls `sprintf7C8EF4` with one width-string argument. Four
executed controls cover 640/800 substitution, a fixed filename and escaped `%`.
The Rust filename owner accepts fixed names, `%s` and `%%`. Other format
directives are rejected explicitly; arbitrary printf formatting is a residual.

## Consumers and lifecycle (instruction evidence)

The installed `553412 -> 1001DFD0` hook selects the actual first participant's
Country extension. Width640 alone selects the 640 token; all other widths800.
`72B690 -> 1001E0E0 -> 1001DCB0 -> 72ADE0` reads the Country palette through the
ordinary file/Convert path, not the palette CRC first-winner cache.
Name/Special/Brief hooks `1001E010/1001E070/1001DF70` feed the existing draw
layers. The shared Special uppercase and 640/800/cooperative layout stay intact.
Color binding `6847B7 -> 1002F8F0` captures the Country's signed index for
`642B36/642B91/643BB9`; negative resumes the stock Side lookup.

The process Rules owner supplies two observations from the same retained
Country identity. Color and the resolved PAL bind before Full_Init's E_multi;
its second primary Country body (687451) then supplies the filename and CSF
labels for first Draw687588, before TypeReset6876AC. Direct CnCNet and tactical
capture adapters share PrepareSession's Country master/body admission without
cached shell settings or a PortPool RNG draw. The owner retains E_multi's
constructor chronology behind one move-only attempt ticket until the real
reset boundary; cancellation discards that chronology while retaining the
actually mutated registry. The loading attempt owns its early decoded palette
and the atlas owns its uploaded pixels until success or handled failure.
This corresponds to native success `684C00 -> 554370 -> 72B870` and handled
failure `684A8B -> 552AA0 -> 552AAA -> 554370`. Native cleanup clears palette
buffers/Convert while retaining mode; the next `72B530` detects null Convert,
resets mode and resolves the next Country palette. No per-frame reload is claimed.

Country save/load suffixes `51255C/51246D -> 1001D250/1001D130` reach
`1001B240/1001B080`. Files and CSF labels are 32-byte records; helper10076570
saves labels, not cached wide pointers. Load100764D0 rebinds nonempty labels
through734E60. Color is a raw signed index. Rust persists retained labels/color,
keeps phase/parent state, and resolves text through the fixed process CSF owner;
GUI values do not enter the existing simulation veterancy hash.

## Boundaries

These are component goldens and instruction evidence, not whole loading-screen
or mod parity. Whole native save framing, live localized/font/Convert pixels,
allocation failures, forced aborts and all observer/network/campaign routes are
not certified. Existing stock progress insignia and the other Country GUI keys
are separate work. Earlier map-metadata art overrides and external client PCX
input are separate mechanisms; no external PCX is used to simulate this SHP/PAL
extension. No mod resources, private resource readers or client configuration
belong in this change.
