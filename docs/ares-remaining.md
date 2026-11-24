# Ares 3.0: remaining work

This page lists what the Ares 3.0 port still lacks, and for each gap the gamemd logic it
depends on. The supported chains are listed in [ares.md](ares.md); each chain's addresses,
dispositions and smaller residuals are in its own `docs/ares-*.md`, and the hooks skipped
on purpose are in [ares-skipped-hooks.md](ares-skipped-hooks.md).

## Where it stands

- Everything is in every build and plays when the game plays Ares ([ares.md](ares.md));
  other games keep their behaviour. The one deliberate change for every game: buildings
  and aircraft no longer take stealth from `Cloakable=` (native vanilla never did; see
  [ares-bugfixes.md](ares-bugfixes.md)).
- INI keys: the restored Ares source reads 684 distinct keys. The engine reads every
  gameplay key among them. The keys still unread are UI and front-end keys (uimd.ini
  `[UISettings]`, campaign `HoverSound`, the movie list,
  `DebugKeysEnabled`, `DirectX.Force`) and the
  presentation keys `DamageSmokeParticleSystems` / `DamageSparksParticleSystems`. Ares
  3.0 itself never loads `[RadiationTypes]` or `AttachEffect.Damage`/`.DamageDelay`
  (no reader in the DLL; see [ares-radiation.md](ares-radiation.md) and
  [ares-attach-effect.md](ares-attach-effect.md)). `ares-src/report/ini_coverage.tsv` is
  not exhaustive; use the key scan under Tools.
- Hooks: Ares 3.0 installs 1448 (`report/hooks.tsv` also lists 234 0.A hooks that 3.0
  removed). The engine source cites 1055; the chain docs give 389 more a disposition
  (ported elsewhere, equivalent, not applicable, skipped or residual); 4 are
  untouched, listed at the end. A residual disposition is still a gap: every
  residual in the chain docs is gathered on this page, the gameplay ones under
  [Smaller gameplay residuals](#smaller-gameplay-residuals).
- Tests at the commit that adds this page, when Ares was still a build feature
  (RA2_DIR set to a retail install): besides one fixture expectation fixed afterwards
  (the WithConst test), the failures were environment-only: the local retail install is
  Traditional Chinese, so `rules::campaign_loading::tests::campaign_mission_names_use_physical_mission_and_csf_inputs`
  and six client CSF tests (selection_navigation x4,
  `messages::trigger_message_csf_labels_match_original_lookup_through_retail_archive_reader`,
  `tooltips::retail_ifv_tip_names_its_turret_or_passenger`) compare against English text.
  They fail the same way on main.

## Blocked on gamemd logic VERA does not port

These Ares features hook stock gamemd code that VERA has no port of.
Porting that stock code changes games without Ares too, so it belongs in its own native chain
first; the Ares part is then small. None of them is reachable with retail YR data.

| Ares feature (keys, hooks) | Needs this stock gamemd logic | Retail |
|---|---|---|
| Gate placement selling walls (0x440709, 100156F0) | The hook's wall test in BuildingClass::Unlimbo's Gate arm; the stock arm is ported with the stock wall ids | A mod `Wall=` overlay stays under a gate |

Stock gap found in passing, with no Ares dependency: Generate_Starting_Units (0x5D6D80)
stops for every later house when one house has no starting-unit candidates; VERA skips
only that house.

## Ares mechanisms ported in part

| Mechanism | What is missing | Where |
|---|---|---|
| `CanMakeStuffUp` | The Rules draws and the 90-frame infantry panic are ported and the written values are saved in `AresMadeUpRules`, but nothing reads them yet: 28 Rules fields with about 110 read sites, many in free functions without a Simulation | [ares-warhead-residuals.md](ares-warhead-residuals.md) |
| EMP | A LaserFencePost's fence recalc (0x4533A0, VERA builds no fence sections) | [ares-emp.md](ares-emp.md) |
| Refinery storage | The Weeder store, Removed_From_Game's keepTiberium arm, a fifth tiberium slot, the storage pips (0x44D755, 0x709B4E) and the `Message.SilosNeeded` text (needs a sim-to-client message event) | [ares-tiberium.md](ares-tiberium.md) |
| Trench traversal | Only a buildings-only selection gets the cursor and click; Ctrl+Alt over a friendly trench keeps its stock answer; `GarrisonedIn` is not kept | [ares-building-hooks.md](ares-building-hooks.md) |
| Firestorm zones | Recalc writers other than the toggle (an overlay change, trigger action 40) and placing a section during a firestorm keep the old zone class | [ares-firestorm.md](ares-firestorm.md) |
| DropPod | A negative count (native loops about 2^32 tries), SetDestination's prologue and Teleporter arm, the Foot's own locomotor answering while the pod flies, a landing past the end of `DropPod=` | [ares-superweapon-types.md](ares-superweapon-types.md) |
| HunterSeeker | The tube coordinate of a target in a tunnel; the facing conversion may differ by one binary-angle unit | [ares-hunter-seeker.md](ares-hunter-seeker.md) |
| Terrain fire | Finish_Fire_Death's crumble of a zero-health tree (a lethal hit removes the tree first), the spread runs after the animation step | [ares-combat-sim-hooks.md](ares-combat-sim-hooks.md) |

## Smaller gameplay residuals

The chain docs' other gameplay residuals. They need a mod key or are dormant on retail
data.

| Residual | Hooks | Where |
|---|---|---|
| Flying carriers: spawned aircraft keep their launch Z, and the spawn manager aims them at its cell | `0x414338`, `0x6B783B` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Capture manager and parasite on any weapon slot (TechnoClass Init_1), with the parasite slot hooks | `0x6F407D`, `0x629804`, `0x62A020`, `0x62A7B1` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Gunner temporal swap | `0x74642C`, `0x74653C` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Hover `DeployToLand` landing | `0x513EAA`, `0x514A21`, `0x514DFE` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Occupier + VehicleThief destination expiry | `0x4D9A83` | [ares-techno-hooks.md](ares-techno-hooks.md) |
| The PermaDisguise arm of `InfantryClass::ClearDisguise` has no Rust caller | `0x5227A3` | [ares-combat-sim-hooks.md](ares-combat-sim-hooks.md) |
| `GarrisonedIn` is not kept: a garrisoned infantryman's skipped death anim, its release on Limbo and Unlimbo | `0x518434`, `0x51DF38`, `0x51DFFD` | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| A repacked MCV that is still unplaced skips the conversion tail | `0x449FF8` | [ares-bugfixes.md](ares-bugfixes.md) |
| UnInit announces expiry only when Limbo fails; VERA keeps both broadcasts and their order | `0x5F6612` | [ares-bugfixes.md](ares-bugfixes.md) |
| Expired Flamer/Scorch anims make no SmallFire/LargeFire anims (needs `MapClass::GetRandomCoordsNear`); retail sets neither key | `0x4250C9`, `0x42511B`, `0x42513F` | [ares-anim-particles.md](ares-anim-particles.md) |
| Dormant on retail data: Doggie, the `DeathAnims=` owner, Mission_Guard's superweapon gate, the ICBM crate's Ares Nuke type, a bullet damage anim's MakeInfantryOwner outside Phobos | `0x51F628`, `0x518CB3`, `0x51ABD7`, `0x5200C1`, `0x51849A`, `0x449716`, `0x6CEEB0`, `0x469C4E` | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| Superweapon money: charge-drain activation with `Money.Amount` | none | [ares-superweapon-money.md](ares-superweapon-money.md) |
| AttachEffect: `TemporalHidesAnim` and the airborne half-distance collection | none | [ares-attach-effect.md](ares-attach-effect.md) |
| Country power plants: a house with no candidate is a fatal error natively (`0x1001DEF6`); VERA splices nothing | none | [ares-country-powerplants.md](ares-country-powerplants.md) |
| With Phobos too: Phobos `SuperWeaponSidebar.*Houses` keep the 0x80 buffer | `0x4750EC`, `0x475107` | [ares-bugfixes.md](ares-bugfixes.md) |

Equivalence notes that need no port stay in their chain docs: INI `+` numbering and
section caching ([ares-ini-syntax.md](ares-ini-syntax.md)), the random map's urban
receiver and Main draws ([ares-rmg.md](ares-rmg.md)), the country picks and flags
([ares-country-presentation.md](ares-country-presentation.md)), and the side theme and
dialog caching ([ares-side-defaults.md](ares-side-defaults.md)).

## WaterImage indexed guns

`WaterImage=` is ported: the read (`715320`, body `10041DC0`) in
`rules::native_processing::pass_techno`, the VXL draw (`73B4A0`, body `100481E0`) in
`app::presentation::instances::units`, and the SHP draw (`73C5FC`, `73C655`, `73C69D`,
`73C702`) in `app::presentation::instances::shp`.

Open: a pair whose two types differ in TurretCount or IsGattling. Native uses the
actual type's TurretCount/IsGattling to choose the drawn model's base or indexed gun
slots, while the atlas keys derive them from the drawn model, so such a pair can show
a different gun or none. Fixing it needs separate physical slot identities through
the voxel catalog and atlas. The mods scanned so far (Red
Resurrection's nine pairs) have no such pair.

[Ares WaterImage specification](https://ares-developers.github.io/Ares-docs/restored/amphibiousimagechanges.html)

## Presentation and UI not ported

- Unit art still needs the mixed indexed-gun draws above. A `Turret=yes` NoSpawnAlt unit's turret over its WO body (`0x73B6E3`) is a
  residual in [ares-techno-hooks.md](ares-techno-hooks.md).
- Technos: the damage spark and smoke
  particle-system lists (`DamageSparksParticleSystems`, `DamageSmokeParticleSystems`;
  `0x6FAD49`, `0x702894`). See [ares-techno-hooks.md](ares-techno-hooks.md).
- Sidebar and screens: the observer strip art (`0x6AA0CA`, `0x6AA164`;
  [ares-country-presentation.md](ares-country-presentation.md)), the Money superweapon
  tooltip (`0x6A932B`).
- Palettes: a ParticleType's `Palette` (`0x62D015`). See
  [ares-anim-particles.md](ares-anim-particles.md) and [ares-bugfixes.md](ares-bugfixes.md).
- Audio: the move sound under a Magnetron (`0x4DAA68`, `0x7101CF`), the amphibious
  water sound (`0x51D799`), and loose WAV samples and extra audio bags (`0x4016F0`).
- The Ares campaign menu (uimd.ini `[UISettings]` `CampaignList`, `Campaign1`..`Campaign4`,
  the campaign keys `DebugOnly`/`HoverSound`/`Summary`) and unlimited player colours
  (`[Colors] Count` above 8). Ares' defaults match the engine. See
  [ares-sidebar-ui-hooks.md](ares-sidebar-ui-hooks.md).
- Ares' MapSnapshot keyboard command (hook `0x533058`); FPSCounter, TogglePower
  and the debug commands are in [ares-commands.md](ares-commands.md).
- The front-end hooks among the untouched ones below: the skirmish MultiEngineer
  option and the uimd.ini settings. The DLL plumbing
  (checksums, save stream, copy protection, log, swizzling, start and command
  line) is in [ares-skipped-hooks.md](ares-skipped-hooks.md).
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
  the game's switch, `RuleSet::ares` (`RuleSet::ares_form` where it needs data only Rules
  read the Ares way hold), citing the Ares.dll function and the gamemd hook address.
- New simulation state is saved (serde default) and world-hashed in an owner struct with
  rule-named operations; the architecture guards check this.
- One to three focused tests per mechanism. A stock oracle test that an installed Ares
  hook changes keeps its expectation in a game without Ares, and an Ares case gets the
  Ares expectation, citing the hook.
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

These 4 Ares 3.0 hooks have no citation in the engine or in another chain doc (the
hook script above skips this page).

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x69801A` | `1006AF40` | Game_GetGameTypePrefs_MultiEngineer | `Misc/Skirmish.cpp` |
| `0x699043` | `1006AF00` | GameMode_SaveGameSettings_MultiEngineer | `Misc/Skirmish.cpp` |
| `0x6AEE6A` | `1006B070` | Skirmish_InitializeDialog_MultiEngineer | `Misc/Skirmish.cpp` |
| `0x5FACDF` | `10006030` | Options_LoadFromINI | `Ares.UISettings.cpp` |
