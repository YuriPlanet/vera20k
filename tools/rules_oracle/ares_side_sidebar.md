# Bounded Ares Side sidebar opening controls

Set `VERA20K_ARES_DLL` and `VERA20K_GAMEMD_EXE` to the locally enrolled binaries, then run from the checkout:

```text
python -B -m tools.rules_oracle.ares_side_sidebar --check
```

`--write` deliberately regenerates the JSON and metadata. The producer reuses
`ares_prerequisites.Probe`, `native_oracle`, the existing sink-return helper and
`ini_token_readers.Crt` thread storage transport. It includes no mod assets,
mod INI, executable files, machine paths or copied image mapper/numeric parser.
Ares enrollment uses the identity already owned by `ares_prerequisites`.

The JSON separates these evidence levels:

- `constructor`: actual literal-store prefix1002D9F0..1002DA89 on 0xCC-filled
  storage yields phase0, MIX0 and false. It excludes the remaining constructor.
- `initial.rows`: four actual1002E190 calls with reordered stored IDs. Nod
  defaults to MIX2/false, ThirdSide to MIX2/true, other IDs to MIX1/false.
  These direct initializer calls do not themselves advance the phase.
- `retained.rows`: ten selected read-prefix executions, starting from supplied
  phase2. Omitted/empty/invalid values retain; integer0/-1 remain valid retained
  fields; `$10` reads10 and `10h` reads16. Each stops before1002E57B, excluding
  unrelated fields/score assets and the final dispatcher phase store.
- `phase_dispatch`: the actual1002B450 controller and initializer. Phase0 runs
  initialization, stores1, calls the reader, then stores2; phase1/2 call the
  reader; negative/>2 skip it. The complete reader is an explicit returning
  sink in these controller-only controls.
- `read_all`: actual1002EFC0 traverses every existing nonnull Side in registry
  order and passes its caller INI. The whole field reader is again a sink.
- `selector` and `filename_sets`: independent actual-side/MIX/Yuri decisions,
  original numeric filename formatting and file-backed string slots. Side2/MIX3
  and Side3/MIX4 both work independently of the actual side identity. Three
  filename hooks select the SHP and palette branches. No pixels are generated.
- `archive_requests`: actual positive controller order and optional/required
  branches, with archive/file/heap services declared as sinks. MD is optional,
  base SIDEC is required, SIDENC is optional. Missing base returns false before
  UI reload; a prior successful MD request remains observable.
- `fallback`: the enrolled executable's selector leaf returns2, so the original
  nonpositive fallback returns false without retiring the prior archive slots.
  This is a binary-specific result, not a universal replacement for CD policy.
  INT_MIN is a distinct wrapping case: decrement produces7FFFFFFF and the
  positive branch formats `SIDEC-2147483648MD.MIX` / `SIDEC-2147483648.MIX`.
- `cache_retirement`: original730400/5B4270 execute against nonzero cache nodes.
  Five calls retire four unique normalized-CRC values in order: YRII.PAL,
  OBSERVER.PAL, SIDEBAR.PAL, SIDEBAR.PAL again, UIBKGD.PAL. A non-UI entry survives.
  Payload/surface pointers are zero, so this does not certify their destruction.

Every executed file-backed code section stays unchanged. Ares's linked numeric
scanner executes; it is not replaced with Python/Rust results. The scanner's CRT
thread accessor uses the existing thread-storage transport. Actual10085839
selects the file-backed C locale, supplied to thread+4C as in the original thread
initializer. This supplies a C-locale platform context, not complete DLL startup,
TLS locking, arbitrary locale or exotic CRT input coverage.

## Caller, lifecycle and persistence instruction evidence

Process668BF0 calls original ReadSides672440 at668CDE, General66D530 at668EE8,
and ReadTypeData679A10 at668EF0. Installed679A10→1002EFC0 visits existing Sides;
installed679A15→1002C9C0→1002AEF0 then runs the Ares before-type-data work;
Country bodies follow at679A22..679A38. Early Country-only startup reads do not
reach this Side hook. No Sides/Side section guard precedes the extension read.
These are instruction-established caller facts, not a whole Process execution.

SideExt is allocated by Side ctor hook6A4609→1002EF60→1002D9F0. Its destructor
hook6A499F→1002F110 removes that association. Field state belongs to this
retained extension, not a new UI-side/faction mapping. It is initialized lazily
at the first reached extension reader and survives later missing fields.

Full SideExt save1002D410 writes MIX+88 as4 bytes at1002D4F6..1002D505 and
Yuri+8C as1 byte at1002D506..1002D515. Full load1002D1D0 reads them at
1002D2CE..1002D2ED. Outer save/load suffixes write/read phase+4 first, then
call that same whole-extension owner (1002F1ED..1002F20B and1002F0A2..1002F0BC).
This corpus does not execute the full extension stream or import native saves.

InitSideMixFiles534FA0 retires slots884E68,884E74,884E70,884E78 before new
positive requests. It constructs MD→base→neutral and calls72F350 only after
base admission succeeds. Native failure is not atomic: old slots have already
been retired. Full_Init68781F tests false and exits via6878CE..687923;
saved Load67E761 tests false and returns immediately at67E76A..67E771.
Missing required resources therefore do not establish a successful empty UI.

The actual file sets are in `filename_sets`. Yuri uses RADARY.SHP and
BKGD{LG,MD,SM}Y.SHP, RADARYURI.PAL for radar and UIBKGDY.PAL for backgrounds.
Plain uses RADAR.SHP, BKGD{LG,MD,SM}.SHP, SIDEBAR.PAL for radar and UIBKGD.PAL
for backgrounds. SIDEBAR.PAL/OBSERVER.PAL/YRII.PAL are also loaded by common
callers. YRII.PAL is not the radar palette replacement. These names follow the
YuriFiles flag, while geometry and the actual house side remain separate.

Palette consumers72F379/72F3A0 call72ADE0, which constructs CCFile4739F0 and
reads the complete file through4A3890. CCFile availability/size/read methods
473C50/473C00/473B10 reach the current MIX lookup5B4430 (including Open473D10),
after the loose-file route. This does not pass through the SHP loader's
4A38D0/5B40B0 first-winner cache. RADARYURI.PAL and UIBKGDY.PAL therefore do
not become sticky merely because730400 retires differently named cache keys.
This is instruction-established lookup control flow, not complete I/O or
ConvertClass lifetime execution.

Physical INI/layer startup chronology, real archive parsing/list priority,
whole resource/ConvertClass lifetime, rendering, GPU output and general scenario
reload parity remain outside
this component corpus. It is not proof of full fourth-side or mod compatibility.
