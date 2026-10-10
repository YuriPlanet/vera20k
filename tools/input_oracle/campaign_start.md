# Original campaign start inputs

`campaign_start.py` composes the existing `BulletReader` → `Reader` → `Lists`
fixture, original PE loader and checked runner. It does not introduce another
INI parser, VM, decoder, allocator or simulation. Native scalar readers,
constructors, RNG and numeric instructions execute unchanged. Each case compares
every executable PE section with the original file after execution.

The accepted `gamemd.exe` SHA256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
No executable, complete retail INI/map or rendered capture is included.

## Reproduction and source seals

Run from the checkout with its retail `ini/` files. Set `RA2_DIR` or
`VERA20K_GAMEMD_EXE` as described by [the shared runner](../native_oracle.md).
`VERA20K_CAMPAIGN_START_ASSETS` must contain the original extracted
`battlemd.ini`, `missionmd.ini`, `all01umd.map`, `sov01umd.map` and `SPLDBR.SHP`.
Their identities, plus RULESMD, ARTMD and decoded RA2MD.CSF, are in the sidecars.

```sh
python -m tools.input_oracle.campaign_start --check
python -m tools.input_oracle.campaign_start --houses --check
python -m tools.spatial_oracle.house_difficulty --check
python -m tools.spatial_oracle.house_difficulty --readers --check
```

`--write` deliberately regenerates a corpus and its `.meta.json`; default and
`--check` only compare. The shared runner seals the producer and transitive
fixture sources before generation and rejects source drift before writing or
comparison. Metadata retains executable and Unicorn identity, input hashes,
reproduction command, substitutions, coverage and canonical payload hash.
Transient execution logs, failed controls and static disassembly packets belong
in scratch, not this directory.

## Corpus schema and executed bounds

| Corpus / collection | Original execution |
| --- | --- |
| `campaign_start.json` / `metadata`, `metadata_controls` | Campaign list46CE10, ctor46CB60, ReadINI46CCD0 and name lookup46CC90; physical BATTLEMD, initialized selected CSF lookup, and eight retained list/read layers |
| `movies` | Full ARTMD Movies674550, lookup48DF30 and ReadMovie4757D0; physical catalog, caller defaults, sentinels, case, duplicates and32/128-byte buffers |
| `loading_geometry` | Full552B10 and552BE0; mode0/5 at eight dimensions including odd and negative-centered sizes |
| `mission_loading.rows` | Full_Init687000..6873AB; both first-map MISSIONMD sections and absence/case/truncation/coordinate controls |
| `progress_rows.rows` | 505 cases: every integer0..100 at five meter points, physical456×25 SPLDBR, original642E80/642EF0,643C50,643720 and643400 up to terminal CC_DrawShape4AED70 |
| `rng_cases` | Full52FC20 and original seed/RandomStraw bodies; ten entropy, override, saved/replay/network/mode and MapGen-disabled controls with full raw RNG states |
| `difficulty_cases` | Full_Init686B6A..686C48 for Options0..4; only0..2 are ordinary campaign-selector inputs |
| `basic_start` | Selected Basic.Action/Home/Alt readers, SpecialFlags6B8CA0, selected Waypoint ReadInt/divide/remainder and opening-view cell-center kernel before world ground-height/Radar calls |
| `special_controls`, `reset_control` | Native SpecialFlags read and SetDefaults683610; current-bit defaults, exact case and repeated selection's Home/Alt reset |
| `campaign_start_houses.json` / `stock_rows` | ALL/SOV×Options0..2: selected native Rules prerequisites, Country/Side/SuperWeaponType registration, full5009B0/500B40 House creation/read/SetDifficulty, then current-house68ACAA..68AD2F |
| `controls`, `edge_controls`, `allies_reader_controls` | Eleven credits/TechLevel/no-Houses/Allies histories,20 original ReadEdge475980 controls and27 original ReadHousesList475260 controls |
| `color_controls` | 22 full ReadColor474A90 calls and24 full Country511850 reads; current0/3, whole-section admission, missing/stored-empty/physical-empty, case, unknown/numeric and31/32-byte boundaries |
| `side_controls` | Six retained histories/19 passes through Country-registration668C8B..668CDB, full Side672440 and Country ReadTypeData679A10..679A3A;22 lookup,22 list,16 Side-reader and11 Scenario-reader boundary controls |
| `side_controls.trigger_type_reader_rows` | 25 original TriggerType727240 owner-prefix controls after actual retained Country setup; valid bindings execute7272CB and stop before second-token processing7272D1 |
| `house_difficulty.json` | Existing72 full4F6EC0 controls, extended to all nine stored scalars with explicit row/Country/GameSpeedBias inputs; previous ROF/index/team-timer outputs retained |
| `house_difficulty_readers.json` / `layers` | Eight full66D270 layers retaining prior rows: physical rules, absent/partial/wrong-case/stored-empty sections and defined numeric-prefix scans |

Scenario/House raw double and RNG storage in `campaign_start*` is hexadecimal
**little-endian bytes**. `house_difficulty*` double values are canonical
16-digit **u64 bit patterns**. They are different encodings of observations,
not independent arithmetic implementations.

House rows retain `sections`, selected `rules`, pre/post RNG bytes, native call
order, `houses` before Basic.Player and `final_houses` after selection. Each
House stores its ctor attack timer, difficulty/team timer, credits, Country,
ratios, Edge, color index, directional current/initial alliance masks, human/control
bits, all nine doubles, Super IDs/type identities and Super ctor flags/timer.
`superweapon_types_section` and `superweapon_types` preserve source and observed
registration order. The original nesting-counter kernels record0→1→2 before
ordinary House initialization; `scenario_init_override=0` identifies the separate
direct live-admission control. Main/MapGen states must remain identical across this
segment. The full native list body is split at its real outer call/return
boundaries only to stay under the shared instruction cap; no register, local or
stack state is changed between the contiguous regions.

The fixture executes selected Country and SuperWeaponType constructors before
the House pass. Thus `id_before=1000034` for ALL and `1000033` for SOV is a
**bounded fixture prefix**, not complete Rules Process allocation. The observed
House/Super deltas are221 for17 ALL Houses and208 for16 SOV Houses. Consumers
must rebase those IDs or compare the segment's delta. Complete Rules registry
allocation counts, Resize, Fill and the saved-ID+10000 reservation are not
executed by these rows.

Color controls retain physical `[Colors]` order and two explicitly supplied
31/32-byte names in the same paired1/53-shade table used by House rows. `rows`
record the caller's `current`, original result and ReadString's default name,
capacity, returned count and buffer. `country_rows` record ctor color0, optional
native Gold setup, `before`, `admitted` and `after`. Whole absent or wrong-case
Country sections skip the reader; present missing Color with current0 resolves
its default name to the normal paired index1. A stored empty value instead
produces an empty buffer/count0 and retains0. Physical empty-key inputs use the
existing lexical cache owner, which omits that key; physical INI parsing is
still a supplied seam. Unknown names retain the caller index. These controls
exclude undefined out-of-range/default-pointer inputs and rendered color output.

Side controls use the same physical paired palette prior. `histories` retain
each supplied pass and snapshots `before`, `registered` after explicit Countries,
`membership` after Side672440, and `after` after the original Country ReadTypeData
loop. Snapshots contain the actual Country stored ID, reached Name alias, color,
side and native ID, and every Side's ordered signed member vector/native ID.
Each input mapping also has an `ordered_sections`, `ordered_setup_sections` or
`ordered_palette_sections` array of `{name, entries: [[key, value], ...]}`. These
arrays preserve the actual supplied section and entry order, including stored
empty values, for consumers whose JSON object maps do not preserve order.
Constructor call order, ReadString arguments/count/buffer, observed ID cursor and
unchanged RNG streams are recorded. The full Rules Process allocation prefix is
excluded; these IDs describe only this constructor segment.
Every history starts with empty Country/Side registries. Its `palette_sections`
setup supplies physical `[Colors]` through the existing palette transport;
all identities and memberships come from the retained input passes, never from
hydrating an observed output snapshot.

`lookup_rows` call original5117D0 with empty and populated registries.
`list_rows` call full4767C0 using a by-value copy of an actual Side prior;
missing/stored-empty retain it, while a nonempty list replaces it. Resolved tokens
and expanded Side members keep duplicates. Unknown Country names fall back to
existing Side lookup and otherwise are skipped; they allocate no Country.
Side672440 creates/reuses each source-key Side immediately before reading its
members, so self-reference uses its prior vector, and earlier Side entries can
already have updated vectors. A Country's later `Side=` read can allocate a new
Side; that new Side receives no membership reread in the same pass.
The `late_override_retains_actual_vectors` history also records the original
Country-index-based removal at5120D0: moving Country1 from a vector `[1]` leaves
that old vector intact. Subsequent Side expansion therefore uses that retained
vector rather than reconstructing membership from final Country Side bindings.

The5117D0 special literal is `<random>`, which returns signed-2 before scanning
the registry. `<none>` and `none` are ordinary alias/ID tokens in this function.
4767C0 tests only-1 as lookup failure, so it preserves-2 in its output vector.
Full672440 would dereference that negative Country index; this undefined pointer
binding is excluded from the execution controls, not replaced with a safe index.
`scenario_reader_rows` stop at full475540 results, including exact-case
`<Player @ A>` routing and unknown-Country construction. They do not establish
later House admission for these special or newly constructed identities.

`trigger_type_reader_rows` preserve the separate TriggerType caller rule.
Original727240 executes its prologue, INI lookup-cache clear526B00 and exact
`[Triggers]`/stored Type-ID ReadString512 with an empty default. The first
comma-delimited token is compared case-insensitively with the executable's
`<none>` literal at727292. That caller branch takes Country vector entry0
without calling5117D0; ordinary IDs and reached aliases use the full shared
scan. Valid controls execute the actual owner-pointer write at7272CB and stop
before7272D1 parses the next token. The cases include case variants, whole-value
versus per-token spaces, first-token selection, the512-byte copy boundary,
alias order and aliases changed by retained native Country-read passes.

Each row records ordered `setup_passes`, `sections`/`ordered_sections`, its
`trigger_id`, valid `prior_owner_index`, registry snapshots, ReadString arguments,
count and buffer, native lookup calls and unchanged RNG. `first_token` is null
for missing/count-zero entries. Those rows stop at7275B4 with `outcome` equal
to `missing_entry` and the prior pointer unchanged. `bound_country` rows have
`binding_executed=true`, the observed `owner_index_after` and `resolved_index`.
Unknown-1 and random-2 rows have `outcome=excluded_negative_index` and stop
**before**7272BB would dereference that signed array index. Their signed
`lookup_result` is observed, while `resolved_index` remains null. This boundary
does not establish a safe full native reader result for invalid tokens.
The supplied Type storage, stored ID and valid prior pointer are fixture inputs;
the Type constructor, remaining Trigger parsing and later action execution are
excluded. An observed pointer preserved at a pre-dereference stop is not native
failure recovery.

## Instruction-established order outside the executed kernels

These conclusions are caller/body evidence, not full engine replay:

* Startup loads campaigns at52C605 before ARTMD Movies52D121. ClearScene reloads
  campaigns at685609. LoadCampaigns52CB90 constructs a fresh CCINI source per
  call and appends/reuses the existing process registry; it does not clear it.
* Normal New Campaign PrepareSession52DF05 calls carryover reset4C6140 before
  selection. Selected campaign Start_Scenario52E718 returning false routes
  through52E732 to main-shell retry52D9C9.
* The selected Campaign+9C supplies the first map directly at52E718.
  `MISSIONS.YRO` has its native reference at699CB8 in the multiplayer
  wildcard enumeration's exclusion, not the ordinary New Campaign lookup.
  Start_Scenario reads Campaign.CD at683B8F, sets the media requirement through
  4790B0 at683BC9 and tests4790E0 at683BEE. The setter normalizes nonnegative
  values to2; both physical first-campaign rows specify2. The original current
  media query4A80D0 returns2, but the ensure body4A8270 also handles drive,
  backup and prompt/remount state. VERA's installed retail archive owner
  supplies both first-map sources; Windows drive prompting/remounting is an
  OS boundary outside these native fixtures and the stock-digital launch
  comparison. Missing map bytes follow the shared loader's failure route.
* Campaign Full_Init686B20 skips the nonzero-mode early Country/House/Gather/
  Resize generation. Its second ClearScene resets native IDs before the optional
  basename `.INI` Process; common Rules reset and RULESMD/LANGRULE/map Process
  follow. Campaign then creates one House/Super generation at6877BD, one Resize,
  Fill, and the map reader's saved-ID+10000 reservation. Complete type counts
  still require their original registry producer evidence.
* Optional basename `.INI` uses file existence admission. After an existing
  file is opened, CCINI.Read4741F0's return is ignored and Process668BF0 consumes
  its resulting cache, including a partial/failed parse.
* Scenario+1254 is the persistent mission counter: ctor1, no SetDefaults write,
  Victory increment685AA9..685AB0, shared battle termination reset6865B0.
  It is not parsed from filename or Basic. Theme uses this owner.
* Start_Scenario683AB0 stops theme withfalse at683CAD only after Intro or fallback
  Brief resolves to a valid movie index; both=-1 skip it at683CA5. After
  ReadScenario, Basic.Action+1444 is gated by entryDL and
  invokes5BF260 at683DB6 without another Theme Stop/Pause. The whole5BF260 and
  Play_Movie5BED40 bodies have no Theme calls; their existing audio suspend/
  resume calls surround playback. Final theme queue/Stop(true) is later.
* Opening view684C9A selects Home/Alt using byte0 of saved carryover8A38E0.
  Reset4C6140 zeroes the50 saved Scenario global flags, so normal New Campaign
  selects Home. SetDefaults resets both indices to699 on every selection.
  Static zero-cell initializer683210 and ctor68341B establish missing/zero
  Waypoints asCell(0,0), matching ReadWaypoints68BE00.
* HouseRead5010E2 calls MakeAlly4F9B70 with current House, selected target and
  announce=false. ReadScenario68467C and Full_Init686B35 establish initialization
  depth2 at the House pass. CanAlly501540 still rejects self/already-allied, then
  bypasses live defeated/count admission at5015A1. MakeAlly writes the sender's
  current mask at4F9BB3 and initial mask at4F9C19; no reciprocal mask is added.
  The counter-zero direct control is separate from normal campaign startup.
  ReadHousesList475260 uses ReadString128 with empty default, comma-only strtok
  literal817F70, exact-case House lookup50C170 and x86 masked shift; unknown names
  set bit31 in its returned mask. Missing/empty retains the caller's mask.
  House ctor initializes the initial mask with self. Its downstream direct
  read50131C is the scenario-INI writer, while live allegiance reads current5788.
* Ordinary Process668BF0 reads Colors66D3A0 first, finding/appending shade1 then53
  pairs at66D444/66D45B before Country allocation and ReadTypeData679A10.
  The palette-name registry persists across ordinary Process handoffs. Explicit
  TypeReset6686C0 destroys ColorScheme entries at6686E6, clears count at668702,
  clears the vector at66870C and palette cache at668711. This lifetime/order is
  instruction-established; complete palette construction/destruction is excluded.
* RMG598960 publishes all253 MapGen dwords at598996 before generation; the
  preview return, OK/cancel and teardown do not roll them back. Authored first
  `.MAP` campaign load, its post-load path and pre-generation failing exits do
  not call the RMG stream. External bridge repair draws require an admitted
  Engineer repair action, outside this first-load bound; live MapGen continuity
  must remain with its existing owner.

## Supplied seams and limits

Physical file/archive admission and parsing are supplied by the existing lexical
cache owners. Synthetic stored-empty controls intentionally use cache projection
and do not describe physical empty-value parsing. Bounded allocation/delete,
single-thread CRT TLS, original OS atomic import transport and a supplied clock
are inherited. Windows entropy and selected decimal Waypoint formatting are
explicit OS boundaries. No supplied success result bypasses a gameplay body.

The CSF cache is initialized from the existing original CSF decoder; native
lookup executes. Palette storage follows the original66D3A0 paired1/53-shade
scheme registration and physical Colors order, but pixels/converters are
supplied. Progress font storage uses the existing native-layout fixture with
W width8, spacing1 and height17; original font select/lookup/measurement execute.
Hidden Surface/converter storage is supplied, and execution stops before actual
shape submission. Font/palette/raster parity is not claimed.

Difficulty readers skip an absent whole section; a present section resets
missing keys to reader literals. RepairDelay and BuildDelay defaults are native
literal doubles, unlike an authored value parsed as a single then widened.
Malformed floating scans can read undefined stack storage and are excluded from
consumable goldens. Direct Options3/4 prefix observations do not certify the
later out-of-range difficulty-row heap access.

These corpora establish their original instruction outputs and reader histories.
Production Rust comparisons, retail release loading and rendered output need
their own validation. Full Rules Process, scenario/world initialization,
trigger execution, whole-campaign behavior and engine parity remain outside
this native fixture's coverage.
