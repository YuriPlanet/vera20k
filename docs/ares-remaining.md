# Ares 3.0: remaining work

This page lists what the Ares 3.0 port still lacks, and for each gap the gamemd logic it
depends on. The supported chains are listed in [ares.md](ares.md); each chain's addresses,
dispositions and smaller residuals are in its own `docs/ares-*.md`, and the hooks skipped
on purpose are in [ares-skipped-hooks.md](ares-skipped-hooks.md).

## Where it stands

- Everything is behind the `ares` cargo feature; default builds keep their behaviour. The
  one deliberate default-build change: buildings and aircraft no longer take stealth from
  `Cloakable=` (native vanilla never did; see [ares-bugfixes.md](ares-bugfixes.md)).
- INI keys: the restored Ares source reads 684 distinct keys. The engine reads every
  gameplay key among them. The keys still unread are UI and front-end keys (uimd.ini
  `[UISettings]`, campaign `HoverSound`, the movie list,
  `DebugKeysEnabled`, `DirectX.Force`) and the
  presentation keys `DamageSmokeParticleSystems` / `DamageSparksParticleSystems` and
  `AlternateTheaterArt` (read by `pType->ID`, which the key scan misses). Ares
  3.0 itself never loads `[RadiationTypes]` or `AttachEffect.Damage`/`.DamageDelay`
  (no reader in the DLL; see [ares-radiation.md](ares-radiation.md) and
  [ares-attach-effect.md](ares-attach-effect.md)). `ares-src/report/ini_coverage.tsv` is
  not exhaustive; use the key scan under Tools.
- Hooks: Ares 3.0 installs 1448 (`report/hooks.tsv` also lists 234 0.A hooks that 3.0
  removed). The engine source cites 1013; the chain docs give 369 more a disposition
  (ported elsewhere, equivalent, not applicable, skipped or residual); 66 are
  untouched, listed at the end with `0x5F3D65`, whose address the engine quotes only
  for the stock destructor. A residual disposition is still a gap: every
  residual in the chain docs is gathered on this page, the gameplay ones under
  [Smaller gameplay residuals](#smaller-gameplay-residuals).
- Tests at the commit that adds this page (RA2_DIR set to a retail install):
  - core `--features ares`: 8468 passed, 2 failed in the last full run; one of the two, the
    WithConst test, was a fixture expectation fixed afterwards and passes in a focused rerun
    of `sim::scenario_bootstrap::`
  - core default: 7985 passed, 1 failed
  - client `--features ares`: 3829 passed, 6 failed
  - client default: 3788 passed, 6 failed
  - `check -p vera20k --lib --features ares,phobos`: passes
  The remaining failures are environment-only: the local retail install is Traditional
  Chinese, so `rules::campaign_loading::tests::campaign_mission_names_use_physical_mission_and_csf_inputs`
  and six client CSF tests (selection_navigation x4,
  `messages::trigger_message_csf_labels_match_original_lookup_through_retail_archive_reader`,
  `tooltips::retail_ifv_tip_names_its_turret_or_passenger`) compare against English text.
  They fail the same way on main.

## Blocked on gamemd logic VERA does not port

These Ares features hook stock gamemd code that VERA has no port of, in either build.
Porting that stock code changes default builds, so it belongs in its own native chain
first; the Ares part is then small. None of them is reachable with retail YR data.

| Ares feature (keys, hooks) | Needs this stock gamemd logic | Retail |
|---|---|---|
| `NoSelfGuardArea` (0x51E748) | The healer's self-GuardArea arm of the infantry input ladder (0x51E710..0x51E748) | Healers never self-guard |
| Gate placement selling walls (0x440709, 100156F0) | The hook's wall test in BuildingClass::Unlimbo's Gate arm; the stock arm is ported with the stock wall ids | A mod `Wall=` overlay stays under a gate |
| Tunnel entry by an absorber click (0x44351A, 100172F0) | The absorber click that sends radio 0x10 | Ares tunnels are mod-only |

Stock gaps found in passing, with no Ares dependency:
- BuildingType GetPipMax (0x45ECE0): retail refineries show 5 ore pips where native shows
  30 (GAREFN, NAREFN) or 15 (YAREFN).
- Generate_Starting_Units (0x5D6D80) stops for every later house when one house has no
  starting-unit candidates; VERA skips only that house.

## Ares mechanisms ported in part

| Mechanism | What is missing | Where |
|---|---|---|
| `CanMakeStuffUp` | The Rules draws and the 90-frame infantry panic are ported and the written values are saved in `AresMadeUpRules`, but nothing reads them yet: 28 Rules fields with about 110 read sites, many in free functions without a Simulation | [ares-warhead-residuals.md](ares-warhead-residuals.md) |
| EMP | Tunnel movers; a landed Aircraft's Fly Power_Off spin (0x4CFD20); a LaserFencePost's fence recalc (0x4533A0, VERA builds no fence sections) | [ares-emp.md](ares-emp.md) |
| Refinery storage | `[Basic] FillSilos=`, the Weeder store, Removed_From_Game's keepTiberium arm, a fifth tiberium slot, the storage pips (0x44D755, 0x709B4E) and the `Message.SilosNeeded` text (needs a sim-to-client message event) | [ares-tiberium.md](ares-tiberium.md) |
| Trench traversal | Only a buildings-only selection gets the cursor and click; Ctrl+Alt over a friendly trench keeps its stock answer; `GarrisonedIn` is not kept | [ares-building-hooks.md](ares-building-hooks.md) |
| Deactivated objects | The deactivated What_Action over an object gates only the hover cursor of the object that owns it; the click paths keep their stock rungs below the ClickedAction refusal | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Firestorm zones | Recalc writers other than the toggle (an overlay change, trigger action 40) and placing a section during a firestorm keep the old zone class | [ares-firestorm.md](ares-firestorm.md) |
| `StartInMultiplayer.WithConst` | By the ported chain a human's starting yard is never Grand_Opened (no lights or active anims), and its House's primary centre falls back to the start cell (4FD150 republishes none, 1001ED60 restores the saved cell); compare with the game in a mod that sets it | [ares-combat-sim-hooks.md](ares-combat-sim-hooks.md) |
| DropPod | A negative count (native loops about 2^32 tries), SetDestination's prologue and Teleporter arm, the Foot's own locomotor answering while the pod flies, a landing past the end of `DropPod=` | [ares-superweapon-types.md](ares-superweapon-types.md) |
| HunterSeeker | The tube coordinate of a target in a tunnel; the facing conversion may differ by one binary-angle unit | [ares-hunter-seeker.md](ares-hunter-seeker.md) |
| Terrain fire | Finish_Fire_Death's crumble of a zero-health tree (a lethal hit removes the tree first), the spread runs after the animation step | [ares-combat-sim-hooks.md](ares-combat-sim-hooks.md) |
| Ares ParticleSystemExt | Ares' own Smoke/Spark/Railgun particle simulation, drawing from the synchronised Scenario stream (a simulation port) | [ares-anim-particles.md](ares-anim-particles.md) |

## Smaller gameplay residuals

The chain docs' other gameplay residuals. They need a mod key or are dormant on retail
data.

| Residual | Hooks | Where |
|---|---|---|
| Serial AI queues (`AllowParallelAIQueues=no`); the default changes nothing | `0x4502F4`, `0x4CA07A`, the four KickOutUnit hooks (`0x443CCA`, `0x444119`, `0x444131`, `0x44531F`) | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| An upgrade's ProduceCash timer is not started at Put, so the Update hook starts it one frame longer | `0x4409F4` | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| Flying carriers: spawned aircraft keep their launch Z, and the spawn manager aims them at its cell | `0x414338`, `0x6B783B` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Paradrop canopy: the dropped type's own `Parachute.Anim` before the Country's and Side's | `0x5F5ADD` (`10047C80`) | [ares-side-defaults.md](ares-side-defaults.md) |
| Capture manager and parasite on any weapon slot (TechnoClass Init_1), with the parasite slot hooks | `0x6F407D`, `0x629804`, `0x62A020`, `0x62A7B1` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Gunner temporal swap | `0x74642C`, `0x74653C` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Hover `DeployToLand` landing | `0x513EAA`, `0x514A21`, `0x514DFE` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Occupier + VehicleThief destination expiry | `0x4D9A83` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Infantry `DamageSparks` parse order | `0x5240BD` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| The PermaDisguise arm of `InfantryClass::ClearDisguise` has no Rust caller | `0x5227A3` | [ares-combat-sim-hooks.md](ares-combat-sim-hooks.md) |
| `GarrisonedIn` is not kept: a garrisoned infantryman's skipped death anim, its release on Limbo and Unlimbo | `0x518434`, `0x51DF38`, `0x51DFFD` | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| A map house section's `Degrades=` | `0x50114D` | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| A repacked MCV that is still unplaced skips the conversion tail | `0x449FF8` | [ares-bugfixes.md](ares-bugfixes.md) |
| UnInit announces expiry only when Limbo fails; VERA keeps both broadcasts and their order | `0x5F6612` | [ares-bugfixes.md](ares-bugfixes.md) |
| Expired Flamer/Scorch anims make no SmallFire/LargeFire anims (needs `MapClass::GetRandomCoordsNear`); retail sets neither key | `0x4250C9`, `0x42511B`, `0x42513F` | [ares-anim-particles.md](ares-anim-particles.md) |
| Dormant on retail data: Doggie, the `DeathAnims=` owner, Mission_Guard's superweapon gate, the ICBM crate's Ares Nuke type, a bullet damage anim's MakeInfantryOwner outside Phobos | `0x51F628`, `0x518CB3`, `0x51ABD7`, `0x5200C1`, `0x51849A`, `0x449716`, `0x6CEEB0`, `0x469C4E` | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| Superweapon money: charge-drain activation with `Money.Amount` | none | [ares-superweapon-money.md](ares-superweapon-money.md) |
| UnitDelivery: `Deliver.Buildups`; the placement order and RNG are VERA's (Ares asks Pathfinding_Find `0x56DC20` once, with a `buildable` argument VERA's finder port does not take) | none | [ares-unit-delivery.md](ares-unit-delivery.md) |
| AttachEffect: `TemporalHidesAnim` and the airborne half-distance collection | none | [ares-attach-effect.md](ares-attach-effect.md) |
| Country power plants: a house with no candidate is a fatal error natively (`0x1001DEF6`); VERA splices nothing | none | [ares-country-powerplants.md](ares-country-powerplants.md) |
| With Phobos too: Phobos `SuperWeaponSidebar.*Houses` keep the 0x80 buffer | `0x4750EC`, `0x475107` | [ares-bugfixes.md](ares-bugfixes.md) |

Input rungs
([ares-techno-hooks.md](ares-techno-hooks.md),
[ares-building-house-hooks.md](ares-building-house-hooks.md)):
- the heal cursor and action over objects (`0x51E710`, `0x73FDBD`): a healer heals by its
  warhead's Verses, so a mod's mechanic infantry (Brute Force `CVEngineer`, `REPDRONE`,
  placed by maps or triggers) shows no heal action over a damaged vehicle;
- an Ivan entering an allied Grinder without Ctrl (`0x51EB48`; VERA answers IvanBomb);
- the structure and defense tab hotkeys, the cameo click and the ready-building strip
  asking for a usable factory (`0x535DB6`, `0x535E76`, `0x6AB312`, `0x6A9822`);
- `SuperGapRadiusInCells` multiples of 256: the stock What_Action (`0x004472AA`)
  reads the char, VERA the int.

Equivalence notes that need no port stay in their chain docs: INI `+` numbering and
section caching ([ares-ini-syntax.md](ares-ini-syntax.md)), the random map's urban
receiver and Main draws ([ares-rmg.md](ares-rmg.md)), the country picks and flags
([ares-country-presentation.md](ares-country-presentation.md)), and the side theme and
dialog caching ([ares-side-defaults.md](ares-side-defaults.md)).

## Presentation and UI not ported

- Unit art: WaterImage and the SHP facings and change-type draws (Ext/Techno/Hooks.Art.cpp),
  AlphaImage lights (Ext/Techno/Hooks.Alpha.cpp), the ore-gather anim under EMP. Listed at
  the end. A `Turret=yes` NoSpawnAlt unit's turret over its WO body (`0x73B6E3`) and
  `AlternateTheaterArt` (`0x5F9634`, read for Load2DArt `0x5F9070`) are residuals in
  [ares-techno-hooks.md](ares-techno-hooks.md). PCX cameos are ported
  ([ares-pcx-cameos.md](ares-pcx-cameos.md)), but that doc cites neither of their hooks
  (`0x6A9A43`, `0x712045`), so both stay in the list.
- Technos: the radial indicators of non-buildings (`0x41BE80`), the damage spark and smoke
  particle-system lists (`DamageSparksParticleSystems`, `DamageSmokeParticleSystems`;
  `0x6FAD49`, `0x702894`) and the `Pip=`/`OccupyPip=` colours (`0x4748A0`). See
  [ares-techno-hooks.md](ares-techno-hooks.md) and [ares-bugfixes.md](ares-bugfixes.md).
- Sidebar and screens: the observer strip art (`0x6AA0CA`, `0x6AA164`;
  [ares-country-presentation.md](ares-country-presentation.md)), the Money superweapon
  tooltip (`0x6A932B`).
- Palettes and draw order: an anim's `CustomPalette` (`0x4232CE`), a ParticleType's
  `Palette` (`0x62D015`) and the Ground layer's per-frame sort (`0x551A30`). See
  [ares-anim-particles.md](ares-anim-particles.md) and [ares-bugfixes.md](ares-bugfixes.md).
- Audio: the move sound under a Magnetron (`0x4DAA68`, `0x7101CF`), the amphibious
  water sound (`0x51D799`), and loose WAV samples and extra audio bags (`0x4016F0`).
- The Ares campaign menu (uimd.ini `[UISettings]` `CampaignList`, `Campaign1`..`Campaign4`,
  the campaign keys `DebugOnly`/`HoverSound`/`Summary`) and unlimited player colours
  (`[Colors] Count` above 8). Ares' defaults match the engine. See
  [ares-sidebar-ui-hooks.md](ares-sidebar-ui-hooks.md).
- The CSF loader hooks (Misc/CSFLoader.cpp) are deferred: CSF loading is handled separately.
- DLL plumbing (Checksum, Savegame, CopyProtection, Debug, Swizzle, Ares.cpp), most
  of which does not apply to the engine.
- Overlay radar colours (OverlayTypeClass_GetRadarColor, `1006ACC0`, hook `0x5FED00`):
  Ares returns the OverlayType's `RadarColor` field where VERA reads the SHP frame
  header colour for the map preview. The rest of `Misc/RMG.cpp` is in
  [ares-rmg.md](ares-rmg.md).

## How to continue

Evidence, in this order of authority: `ares-src/decompiled/functions/<addr>.c` (Ghidra C of
the real 3.0 DLL), `ares-src/reference/gamemd_hooks/<addr>.c` (the gamemd function around
each hook, hooked line marked), the annotated gamemd Ghidra database, and `ares-src/src`
(restored source, for orientation; where it and the DLL disagree, the DLL wins). These
are not in the repository.

For each item:
- Port the INI read exactly (section, key, constructor default, Valueable/Nullable,
  parser, Find vs FindOrAllocate, read order) into the extension's `src/rules/ares_*.rs`
  reader with the `read_ares_*` helpers.
- Port the hook at the engine's single owner of the hooked gamemd function, behind
  `cfg(feature = "ares")`, citing the Ares.dll function and the gamemd hook address.
- New simulation state is saved (serde default) and world-hashed in an owner struct with
  rule-named operations; the architecture guards check this.
- One to three focused tests per mechanism. A stock oracle test that an installed Ares
  hook changes gets an Ares expectation or a `cfg(not(feature = "ares"))` gate citing the
  hook.
- Move the item's row from this page into its chain doc, and add the chain to
  [ares.md](ares.md).

## Tools

Ares INI keys the engine does not quote (run beside `ares-src/` and `vera20k/`):

```python
import os, re
pat = re.compile(r'(?:Read\w*|Get\w*|\.Read)\s*\(\s*(?:exINI|exArtINI|pINI|pArtINI|INI)?\s*,?\s*'
                 r'(?:pSection|section|pID|pArtSection|sec\w*|pKey|"[^"]+")\s*,\s*"([A-Za-z][A-Za-z0-9_.%]*)"')
keys = {m.group(1) for r, _, fs in os.walk('ares-src/src') for f in fs if f.endswith('.cpp')
        for m in pat.finditer(open(os.path.join(r, f), encoding='latin-1').read())}
src = '\n'.join(open(os.path.join(r, f), encoding='utf-8', errors='replace').read()
                for r, _, fs in os.walk('vera20k/src') for f in fs if f.endswith('.rs')).lower()
quoted = lambda k: ('"' + k.lower().split('%')[0]) in src if '%' in k else ('"' + k.lower() + '"') in src
print(sorted(k for k in keys if not quoted(k)))
```

Ares 3.0 hooks that neither the engine nor a chain doc cites:

```python
import os, re
def tokens(paths):
    text = '\n'.join(open(p, encoding='utf-8', errors='replace').read() for p in paths).upper()
    return {t.lstrip('0') for t in re.findall(r'[0-9A-F]{5,8}', text)}
src = tokens(os.path.join(r, f) for r, _, fs in os.walk('vera20k/src') for f in fs if f.endswith('.rs'))
docs = tokens(os.path.join('vera20k/docs', f) for f in os.listdir('vera20k/docs')
              if f.startswith('ares') and f != 'ares-remaining.md')
dll = {p[0].upper().lstrip('0'): p[3].upper().lstrip('0')
       for p in (l.rstrip('\n').split('\t') for l in open('ares-src/symbols/hooks_30.tsv', encoding='latin-1')) if len(p) > 3}
rows = [r for r in (l.rstrip('\n').split('\t') for l in open('ares-src/report/hooks.tsv', encoding='latin-1'))][1:]
rows = [r for r in rows if r[2] != 'removed_in_3.0']
keys = lambda r: {r[0].upper().lstrip('0'), dll.get(r[0].upper().lstrip('0'), '')} - {''}
print(sum(bool(keys(r) & src) for r in rows), 'cited in src of', len(rows))
for r in rows:
    if not keys(r) & src and not keys(r) & docs:
        print(r[0], r[1], r[3])
```

gamemd decompiler: a headless Ghidra project over the annotated database, outside the
repository. Import once (`analyzeHeadless <dir>/ghidra-gamemd gamemd -import
gamemd-annotated-2026-11-03.gzf`), then decompile the function containing each address
with a post-script that calls `DecompInterface.decompileFunction(getFunctionContaining(addr),
120, monitor)`.

Retail INIs for retail tests: run the repo's `extract-ini` into `ini/` and set `RA2_DIR`.

## Untouched hooks

These 67 Ares 3.0 hooks have no citation in the engine or in another chain doc (the
hook script above skips this page, and counts `0x5F3D65` as cited). Most are DLL plumbing, unit art and the alpha
lights.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x420A71` | `100475A0` | AlphaShapeClass_CTOR_Anims | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x420F75` | `10047470` | AlphaLightClass_UpdateScreen_ShouldDraw | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x4210AC` | `10047440` | AlphaLightClass_UpdateScreen_Header | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x4211AC` | `10047400` | AlphaLightClass_UpdateScreen_Body | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x421371` | `100477D0` | TacticalClass_UpdateAlphasInRectangle_ShouldDraw | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x42146E` | `10047780` | TacticalClass_UpdateAlphasInRectangle_Header | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x42152C` | `10047740` | TacticalClass_UpdateAlphasInRectangle_Body | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x421730` | `10047610` | AlphaShapeClass_SDDTOR | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x421798` | `10047670` | AlphaShapeClass_SDDTOR_Anims | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x423B0B` | `100476C0` | AnimClass_Update_AlphaLight | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x5F3D65` | `100476E0` | ObjectClass_DTOR | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x71944E` | `10047810` | TeleportLocomotionClass_ILocomotion_Process | `Ext/Techno/Hooks.Alpha.cpp` |
| `0x4A1C10` | `1005F550` | Checksummer_Add_BYTE | `Misc/Checksum.cpp` |
| `0x4A1CA0` | `1005F600` | Checksummer_Add_bool | `Misc/Checksum.cpp` |
| `0x4A1D30` | `1005F5E0` | Checksummer_Add_WORD | `Misc/Checksum.cpp` |
| `0x4A1D50` | `1005F5C0` | Checksummer_Add_DWORD | `Misc/Checksum.cpp` |
| `0x4A1D70` | `1005F660` | Checksummer_Add_float | `Misc/Checksum.cpp` |
| `0x4A1D90` | `1005F640` | Checksummer_Add_double | `Misc/Checksum.cpp` |
| `0x52E9AA` | `1005F680` | Frontend_WndProc_Checksum | `Misc/Checksum.cpp` |
| `0x6516F0` | `1005F720` | Multiplay_LogToSync_MPDEBUG | `Misc/Checksum.cpp` |
| `0x73B4A0` | `100481E0` | UnitClass_DrawVXL_WaterType | `Ext/Techno/Hooks.Art.cpp` |
| `0x73C5FC` | `10048190` | UnitClass_DrawSHP_WaterType | `Ext/Techno/Hooks.Art.cpp` |
| `0x73C613` | `10047FE0` | UnitClass_DrawSHP_FacingsA | `Ext/Techno/Hooks.Art.cpp` |
| `0x73C655` | `10047E80` | UnitClass_DrawSHP_ChangeType1 | `Ext/Techno/Hooks.Art.cpp` |
| `0x73C69D` | `10047E80` | UnitClass_DrawSHP_ChangeType1 | `Ext/Techno/Hooks.Art.cpp` |
| `0x73C702` | `10047E80` | UnitClass_DrawSHP_ChangeType1 | `Ext/Techno/Hooks.Art.cpp` |
| `0x73C733` | `10048180` | UnitClass_DrawSHP_SkipTurretedShadow | `Ext/Techno/Hooks.Art.cpp` |
| `0x73CD01` | `10048150` | UnitClass_DrawSHP_FacingsB | `Ext/Techno/Hooks.Art.cpp` |
| `0x483BF1` | `1006ADE0` | CellClass_Load_Crates | `Misc/Savegame.cpp` |
| `0x559F31` | `1006AE70` | LoadOptionsClass_GetFileInfo | `Misc/Savegame.cpp` |
| `0x565215` | `1006AEC0` | MapClass_CTOR_NoInit_Crates | `Misc/Savegame.cpp` |
| `0x67CEFE` | `1006ADF0` | Game_Save_FixLog | `Misc/Savegame.cpp` |
| `0x67D04E` | `1006AE10` | Game_Save_SavegameInformation | `Misc/Savegame.cpp` |
| `0x67D300` | `1006AEF0` | SaveGame_Start | `Misc/Savegame.cpp` |
| `0x67E42E` | `1006AED0` | SaveGame | `Misc/Savegame.cpp` |
| `0x67F7C8` | `1006AE50` | LoadGame_End | `Misc/Savegame.cpp` |
| `0x47AE36` | `10003B00` | CDFileClass_SetFileName | `Ares.cpp` |
| `0x47B026` | `10003B90` | FileFindOpen | `Ares.cpp` |
| `0x52C5E0` | `10003AE0` | Ares_NoLogo | `Ares.cpp` |
| `0x52F639` | `10003C90` | YR_CmdLineParse | `Ares.cpp` |
| `0x6AD0ED` | `10003AD0` | Ares_AllowSinglePlay | `Ares.cpp` |
| `0x7CD810` | `10003B40` | ExeRun | `Ares.cpp` |
| `0x7CD8EF` | `10003B60` | ExeTerminate | `Ares.cpp` |
| `0x479110` | `1005F7F0` | CD_NeverAsk | `Misc/CopyProtection.cpp` |
| `0x49F5C0` | `1005F820` | CopyProtection_IsLauncherRunning | `Misc/CopyProtection.cpp` |
| `0x49F620` | `1005F830` | CopyProtection_NotifyLauncher | `Misc/CopyProtection.cpp` |
| `0x49F7A0` | `1005F810` | CopyProtection_CheckProtectedData | `Misc/CopyProtection.cpp` |
| `0x55CFDF` | `1005F7A0` | BlowMeUp | `Misc/CopyProtection.cpp` |
| `0x4068E0` | `100620D0` | Debug_Log | `Misc/Debug.cpp` |
| `0x4A4AC0` | `100620D0` | Debug_Log | `Misc/Debug.cpp` |
| `0x4C850B` | `10062120` | Exception_Dialog | `Misc/Debug.cpp` |
| `0x534A4D` | `10062140` | Theater_Init_ResetLogStatus | `Misc/Debug.cpp` |
| `0x687C56` | `10062130` | INIClass_ReadScenario_ResetLogStatus | `Misc/Debug.cpp` |
| `0x69801A` | `1006AF40` | Game_GetGameTypePrefs_MultiEngineer | `Misc/Skirmish.cpp` |
| `0x699043` | `1006AF00` | GameMode_SaveGameSettings_MultiEngineer | `Misc/Skirmish.cpp` |
| `0x6AEE6A` | `1006B070` | Skirmish_InitializeDialog_MultiEngineer | `Misc/Skirmish.cpp` |
| `0x6CF240` | `1006C5B0` | SwizzleManagerClass_Swizzle | `Misc/Swizzle.cpp` |
| `0x6CF2C0` | `1006C540` | SwizzleManagerClass_Here_I_Am | `Misc/Swizzle.cpp` |
| `0x6CF350` | `1006C440` | SwizzleManagerClass_ConvertNodes | `Misc/Swizzle.cpp` |
| `0x6A9A43` | `1005E300` | StripClass_Draw_DrawPCX | `Misc/Cameos.cpp` |
| `0x712045` | `1005E480` | TechnoTypeClass_GetCameo | `Misc/Cameos.cpp` |
| `0x533058` | `10002A80` | CommandClassCallback_Register | `Ares.Commands.cpp` |
| `0x6BC0CD` | `10005820` | LoadRA2MD | `Ares.GlobalControls.cpp` |
| `0x5FACDF` | `10006030` | Options_LoadFromINI | `Ares.UISettings.cpp` |
| `0x73D219` | `10053E50` | UnitClass_Draw_OreGatherAnim | `Ext/WarheadType/Hooks.EMP.cpp` |
| `0x4C8FE0` | `100641A0` | Exception_Handler | `Misc/Exception.cpp` |
| `0x64CCBF` | `1006A280` | DoList_ReplaceReconMessage | `Misc/Network.cpp` |
