# Ares 3.0 alignment: handoff (finish_ares, phase 1)

This page hands the Ares 3.0 alignment over to whoever continues it. It records where the
`finish_ares` branch stands, how the work was done, what is still missing, and how to resume on
another machine. The supported-extension list itself stays in [ares.md](ares.md); each chain's
details, addresses and residuals are in its own `docs/ares-*.md`.

## Where it stands

- Branch `finish_ares`, cut from `origin/main` f331b9856 on 2026-11-07. At the commit that adds this page:
  101 commits, 474 files changed, about 50,000 insertions and 2,500 deletions.
- Everything is behind the `ares` cargo feature; default builds keep their behaviour. The one
  deliberate default-build change: buildings and aircraft no longer take stealth from
  `Cloakable=` (native vanilla never did; see [ares-bugfixes.md](ares-bugfixes.md)).
- Ares ExtData INI keys read: **651 of 651** coverage rows (636 distinct keys; exact count, see
  Tools below).
- Ares 3.0 hooks: 1448 (`report/hooks.tsv` also lists 234 0.A hooks that 3.0 removed; they do
  not count). The engine source cites the gamemd or Ares.dll address of **874** (60%); the chain
  docs give a disposition (presentation, equivalent, not applicable or residual) to **171** more;
  **403** (28%) are untouched. All 157 Misc/Bugfixes*.cpp hooks are settled (90 cited, 67 with a
  disposition in [ares-bugfixes.md](ares-bugfixes.md)).
- Untouched hooks by area: gameplay ExtData hooks about 260 (Rules 44, Techno 32, Building 22,
  Side 17, WarheadType 16, WeaponType 15, TechnoType 11, SWType 10, and smaller); UI and
  presentation about 75 (Interface.UIColors 19, Interface 15, Interface.PlayerColors 13,
  Interface.Buttons 11, CSFLoader 6, Surfaces 4, and smaller); random maps (RMG) 21; DLL-only
  plumbing about 45 (Checksum 9, Savegame 8, Ares.cpp 7, CopyProtection 5, Debug 5, Network 5,
  Swizzle 3), most of it not applicable to the engine.
- Tests at HEAD (RA2_DIR set to a retail install):
  - core `--features ares`: 8408 passed, 1 failed, 246 ignored
  - core default: 7981 passed, 1 failed, 248 ignored
  - client `--features ares`: 3834 passed, 6 failed, 280 ignored (the full run had a seventh failure, a repair-target
    oracle that the Ares heal hook changes; it was fixed in 04051293a and rerun on its own)
  - client default: 3785 passed, 6 failed, 280 ignored
  - `check --features ares,phobos`: passes
  The remaining failures are environment-only: the local retail install is Traditional
  Chinese, so `rules::campaign_loading::tests::campaign_mission_names_use_physical_mission_and_csf_inputs`
  and six client CSF tests (selection_navigation x4,
  `messages::trigger_message_csf_labels_match_original_lookup_through_retail_archive_reader`,
  `tooltips::retail_ifv_tip_names_its_turret_or_passenger`) compare against English text. They fail
  the same way on main.
- The branch exists only in the original checkout until it is pushed.

## Chains ported in this phase

| Chain | Doc |
|---|---|
| Weapon beam, bolt and wave visuals, detached railguns | [ares-weapon-visuals.md](ares-weapon-visuals.md) |
| Warhead damage modifiers and effects (relative damage, culling, curtain, permanent control, weapon lock, ion cannon, InfDeathAnim) | reader `src/rules/ares_warhead.rs` |
| Building infiltration and sabotage | [ares-infiltration.md](ares-infiltration.md) |
| Cloak, PoweredBy, radar jammers, sensor arrays, deactivation | [ares-cloak-power.md](ares-cloak-power.md) |
| Superweapon common keys, Battery, Protect, SonarPulse | [ares-superweapon-common.md](ares-superweapon-common.md) |
| Superweapon retail types: ChronoSphere/ChronoWarp (incl. building chronoshift), Nuke, Dominator, Genetic Mutator, SpyPlane, Lightning Storm, EMPulse (with the stock EMP cannon mission), DropPod readers | [ares-superweapon-types.md](ares-superweapon-types.md) |
| Firestorm walls (Firewall superweapon) | [ares-firestorm.md](ares-firestorm.md) |
| Veterancy, promotion, academies, build time, cloning | [ares-veterancy.md](ares-veterancy.md) |
| Ivan bombs, abductors, ApplyDamage, ProjectileRange | [ares-ivan-bomb-abductor.md](ares-ivan-bomb-abductor.md) |
| Building type keys (rubble, garrisons/UC, engineers, IsPassable, sell/return, degrade, toggle power, docks) | [ares-building-types.md](ares-building-types.md) |
| Techno type keys (custom missiles, reload/ammo, gattling cycle, crush, berserk, smoke/sparks, self-heal, fall rates, transport and role keys) | [ares-techno-types.md](ares-techno-types.md) |
| Prism forwarding | [ares-prism-forwarding.md](ares-prism-forwarding.md) |
| Trigger events and actions | [ares-triggers.md](ares-triggers.md) |
| Tiberium, ore and harvesters | [ares-tiberium.md](ares-tiberium.md) |
| Line of fire, solid buildings, BulletTypeExt | [ares-line-of-fire.md](ares-line-of-fire.md) |
| Mouse cursors (`[MouseCursors]` and every `Cursor.*` key) | [ares-cursors.md](ares-cursors.md) |
| Misc/Bugfixes*.cpp (gameplay fixes; Buffer list readers are Find-only) | [ares-bugfixes.md](ares-bugfixes.md) |
| AnimType, ParticleType extensions | [ares-anim-particles.md](ares-anim-particles.md) |
| Remaining TechnoClass hooks (healers, gaps, spotlights, voices, immunities) | [ares-techno-hooks.md](ares-techno-hooks.md) |
| Remaining Building/House/Infantry/SWType hooks | [ares-building-house-hooks.md](ares-building-house-hooks.md) |
| The last INI keys (readers to 651/651) | RulesExt keys in [ares-techno-hooks.md](ares-techno-hooks.md#rulesext-general-keys), presentation keys in [ares-techno-types.md](ares-techno-types.md#presentation-keys), the rest in the type docs |

Integration fixes along the way: pre-existing Ares-build test failures on main (fixtures and
oracles that did not account for installed Ares hooks), architecture-guard findings for new
state, and a guard bug on main (`strip_test_items` swallowed production code after a
`#[cfg(test)]` field).

Findings worth knowing:
- Under Ares, type-list names are looked up with Find only (DLL ParseList 1005B340): a list name
  that no registry holds is dropped, where vanilla allocated it. Retail effects: the Weather
  Controller has two bolts (WCLBOLT2 is not in `[Animations]`), and SmallGreySmokeSys is never
  allocated. Fixtures must register every name they list.
- A chronoshifted Unit drives back to its source after an Ares ChronoWarp. That is Ares itself
  (DLL `Force_Track(-1, source)`; Ares bug 896231), pinned by a test, not an engine bug.
- Ares builds pin their own replay baselines (slice6, bridge crossing); a phobos combination may
  fold more state and was not pinned.

## What remains

Grouped by size; each item is described with its addresses in the chain doc named.

Large mechanisms the engine lacks (the keys are read; nothing consumes them yet):
- DropPod launch: DropPodLocomotionClass, CreateWithDroppod, SW_DropPod::Activate 10070E40,
  hooks 0x4B5EB0/0x4B5F9E/0x4B619F ([ares-superweapon-types.md](ares-superweapon-types.md)).
- Ares ParticleSystemExt: its own Smoke/Spark/Railgun particle simulation with its own RNG draws
  ([ares-anim-particles.md](ares-anim-particles.md)).
- Reverse engineering: `CanBeReversed`, `ReversedAs`, `ReverseEngineersVictims`, grinder
  reverse-engineering, trigger events 78..80, `UndoReverseEngineer`.
- Carryall totes, the Tunnel locomotor (`DigIn`/`DigOut`), unit selling at a repair depot
  (`UnitSell`, sell cursor), the Secret Lab rework, `InitialPayload`, TS laser waves
  (`Wave.IsLaser`/`IsBigLaser`), refinery storage (`Refinery.UseStorage`), visceroid merge and
  spawn, CellChainReact, airburst/Splits/Retarget bullets, the player Detonate action
  (`IvanBomb.CanDetonate*`).
- Building/Foot `EVA.Lost` (FootClass_Destroyed rewrite 4D98C0).

Keys read but not consumed yet, each with its missing mechanism (from the last-keys pass):
- Hunter-seeker flight: `HunterSeeker{Detonate,Descend}Proximity`, `HunterSeeker{Ascent,Descent,Emerge}Speed`
  and the five TechnoType `HunterSeeker.*` keys (hooks 0x4CCB84..0x4D8D95).
- `TeamRetaliate`: needs `TechnoClass::IsCloseEnoughToAttackCoords` (vt+0x3B0).
- `CanMakeStuffUp`: writes the immutable Rules and needs the 90-frame infantry panic (0x687C16, 0x55AFB3).
- `Message.SilosNeeded`: needs Ares tiberium storage (0x4F9610).
- `UnitsUnsellable`, `UnitSell`: unit sale on a repair depot (0x4D9EBD).
- `Carryall.Allowed`, `Carryall.SizeLimit`; `DigIn`, `DigOut`, `DigInSound`, `DigOutSound`.
- `CanBeReversed`, `ReversedAs`, `ReverseEngineersVictims` (0x5198AD, 0x73A1BC).
- `InitialPayload.Nums` (its partner `InitialPayload.Types` is not in the coverage file and is not read).
- `FakeOf` (selection value readout, 0x731E08), `Convert.Deploy` (0x73DE90), `Assaulter.Level` (0x457DB7).
- `NoSelfGuardArea` (healer GuardArea over itself, 0x51E710..0x51E748), `AIInnerBase` (CloakGenerator site key, 0x506306).
- `SecretLab.GenerateOnCapture`, `ProduceCashDisplay` (Ares' replaced ProduceCash, 0x43FD2C, 0x4482BD).
- `StartInMultiplayer.WithConst` (0x5D7048); `StartInMultiplayer.Types` is not read.
- `Wave.IsLaser`, `Wave.IsBigLaser`; `IvanBomb.CanDetonateTimeBomb`, `IvanBomb.CanDetonateDeathBomb`.
- Partly wired: `RadialIndicatorRadius` (buildings only; 0x41BE80 for the rest), `CameoPalette`
  (not the `~~~` theater placeholder), `EnemyWrench` (not the power-off marker), AnimType
  `CustomPalette` (the anim atlas has no per-type palette, 0x4232CE).

Interfaces the last-keys pass changed: `SimSoundEvent::BuildingPlaced` carries `building_type`;
`projectile_shp_frame*` and `projectile_draw_instance_sampled` take a frame counter and return
u16. Under Ares, Mission_Open plays GateUp/GateDown, and a disk laser hit always flashes its warhead.

Smaller gaps:
- Airborne victims of Ares warhead effects (curtain/EMP/AttachEffect air pass 10076660).
- Hospital/Armory eviction on sell and capture; cloak/subterranean EVA lines; deactivated
  cursors and clicks; PoweredBy team/spawn/slave updates.
- Light.* on palettes built mid-effect (0x555E50); building chronoshift light/cloak; EMP cannon
  laser barrel arm; firestorm zone rebuild (RecalcZoneType 0x483D94), trenches, FirewallToggle.
- Presentation hooks listed as residual in each chain doc (UI colours, buttons, cameo list,
  pips, palettes, radial indicators).

Areas not started (hook files with most uncited hooks): Ext/Rules/Hooks.CameoList.cpp (sidebar
cameo list, client), Misc/Interface*.cpp (UI colours, player colours, buttons), Ext/Side/Hooks.cpp,
Misc/RMG.cpp (random maps), Ext/Rules/Hooks.Selects.cpp, Misc/Checksum.cpp and Misc/Savegame.cpp
(mostly not applicable). Use the hook coverage script below for the current list.

Possible engine bug noticed in passing (not Ares): in the superweapon test arena, FireAt of an
`Arcing=yes` weapon from a building at a cell produced no bullet; straight projectiles worked.

## How the work was done

Each chain followed the brief in the appendix. In short:
- Evidence: `ares-src/src` (restored Ares source) for orientation, `ares-src/decompiled/functions/<addr>.c`
  (Ghidra C of the real 3.0 DLL) as the authority, `ares-src/reference/gamemd_hooks/<addr>.c`
  for the gamemd function around each hook, and the annotated gamemd Ghidra database for
  anything else.
- Port each INI read exactly (section, key, constructor default, Valueable/Nullable, parser,
  Find vs FindOrAllocate, clamps, read order) into the extension's `src/rules/ares_*.rs` reader,
  using the `read_ares_*` helpers on `IniSection`.
- Port each hook at the engine's single owner of the hooked gamemd function, behind
  `cfg(feature = "ares")`, citing the Ares.dll function and the gamemd hook address.
- New simulation state is saved (serde default) and folded into the world hash; new state sits
  in owner structs with rule-named operations, which the architecture guards enforce.
- 1-3 focused regression tests per mechanism through production code. Stock oracle tests that an
  installed Ares hook changes get an Ares-build expectation or a `cfg(not(feature = "ares"))`
  gate citing the hook.
- Commits by explicit whole files, no partial hunk staging, no Co-Authored-By trailer.

Coordination lessons from running up to five agents in one checkout:
- Agents share one checkout and one build cache; Cargo runs through `tools.cargo_run`, which
  serializes builds and rejects results when the tree changes during a build ("Source changed
  during Cargo"). Rerun, or read the compiled test binary's result, but never commit on that alone.
- Whole-file commits carry other agents' uncommitted hunks, so HEAD can briefly fail to compile
  on its own. Ask the owning agent to commit the files those hunks need. Never hand-stage hunks:
  zero-context staging put hunks in the wrong place twice.
- Two or three agents is the practical limit; more mostly waits on the build lock.

## Tools

Coverage of keys (exact quoted-literal match, the figure used above):

```python
import os, collections
src = [open(os.path.join(r, f), encoding='utf-8', errors='replace').read()
       for r, _, fs in os.walk('vera20k/src') for f in fs if f.endswith('.rs')]
low = '\n'.join(src).lower()
rows = [l.rstrip('\n').split('\t') for l in open('ares-src/report/ini_coverage.tsv', encoding='latin-1')][1:]
hit = 0
for ext, _, key, *_ in rows:
    k = key.lower()
    found = ('"' + k.split('%')[0]) in low if '%' in k else ('"' + k + '"') in low
    hit += found
    if not found: print(ext, key)
print(hit, len(rows))
```

Coverage of hooks (a 3.0 hook counts when the engine cites its gamemd or its Ares.dll address;
run the same test over `docs/ares-*.md` for the hooks with a disposition):

```python
import os, re
src = '\n'.join(open(os.path.join(r, f), encoding='utf-8', errors='replace').read()
                for r, _, fs in os.walk('vera20k/src') for f in fs if f.endswith('.rs')).upper()
toks = {t.lstrip('0') for t in re.findall(r'[0-9A-F]{5,8}', src)}
dll = {p[0].upper().lstrip('0'): p[3].upper().lstrip('0')
       for p in (l.rstrip('\n').split('\t') for l in open('ares-src/symbols/hooks_30.tsv', encoding='latin-1')) if len(p) > 3}
rows = [r for r in (l.rstrip('\n').split('\t') for l in open('ares-src/report/hooks.tsv', encoding='latin-1'))][1:]
rows = [r for r in rows if r[2] != 'removed_in_3.0']  # 0.A hooks that 3.0 dropped
missing = [r for r in rows if r[0].upper().lstrip('0') not in toks and dll.get(r[0].upper().lstrip('0'), '') not in toks]
print(len(rows) - len(missing), len(rows))
```

gamemd decompiler: a headless Ghidra project with the annotated database, outside the repo.
Import once, then decompile by address:

```sh
# one-time import (Ghidra 12.2, JDK 25)
analyzeHeadless <dir>/ghidra-gamemd gamemd -import gamemd-annotated-2026-11-03.gzf
# decompile.sh <hexaddr> [...]
exec /usr/bin/lockf -k "$G/.decompile.lock" analyzeHeadless "$G" gamemd -process gamemd-annotated-2026-11-03 \
  -noanalysis -readOnly -scriptPath "$G/scripts" -postScript DecompileAt.java "$@" 2>/dev/null \
  | sed -n '/^\/\/ =====/,$p' | grep -v '^INFO \|^WARN '
```

`scripts/DecompileAt.java` decompiles the function containing each address argument with
`DecompInterface.decompileFunction(getFunctionContaining(addr), 120, monitor)` and prints a
`// ===== <name> @ <entry>` header before its C.

Retail INIs for retail tests: run the repo's `extract-ini` into `ini/` and set `RA2_DIR` to the
retail install.

Build cache: a full core test build with `--features ares` is about 4 minutes and the cache grows
to 20-35 GB with several feature combinations. On the original machine `target/` was a symlink
to an APFS sparse image on an external disk (`.git/info/exclude` ignores the `/target` link,
because `**/target/` only matches directories). Trim with care: superseded rustc incremental
sessions whose lock is free, `vera20k`/`vera20k_core` artifacts unused for over 90 minutes, and
`deps/*.rcgu.o` files with a single link older than 30 minutes are safe; third-party deps are not.

## Resuming on another machine

1. Push `finish_ares` from the original checkout, then fetch it into the new clone.
2. Copy `ares-src/` (restored source with `decompiled/`, `reference/`, `report/`, `symbols/`) and
   `gamemd-annotated-2026-11-03.gzf` next to the repo; they are not in it.
3. Install Ghidra and import the gzf as above if gamemd decompiles are needed.
4. Extract the retail INIs and set `RA2_DIR`.
5. Run the five checks above to confirm the starting point, then pick from "What remains". Rerun
   the two coverage scripts after each chain.

## Appendix: agent brief

The brief every chain agent received, verbatim. Its absolute paths are those of the
original machine; its disk notes reflect that machine before the cache moved.

### Shared brief: porting Ares 3.0 into VERA20k (branch finish_ares)

The user asked to align the VERA20k engine fully with the Ares 3.0 implementation, on the
`finish_ares` branch. You own one chain (named in your prompt). Several agents work at the same
time in the SAME checkout, each on a different chain. Read this whole brief first.

#### Where things are

- Engine checkout (shared, branch `finish_ares`):
  `/Users/guoxingchai/Documents/workspace/other/vera20k/vera20k`
  Read its `AGENTS.md` first: it is the project contract (evidence, retail-data readers,
  one owner per native function, small regression tests, Cargo through `tools.cargo_run`,
  rustfmt only edited files). You are a maintainer agent; the "outside contributors" paragraph
  does not apply. Where this brief and AGENTS.md differ for this task, this brief wins.
- Existing Ares support: `docs/ares.md` (list of supported chains) and `docs/ares-*.md`;
  code under `#[cfg(feature = "ares")]`, readers in `src/rules/ares_*.rs`, Rules processing in
  `src/rules/native_processing.rs` / `ruleset.rs` / `object_type.rs`. Study one or two existing
  chains close to yours and follow their structure and comment style.
- Ares 3.0 restored source (C++, the behaviour to port):
  `/Users/guoxingchai/Documents/workspace/other/vera20k/ares-src/src`
  - `Ext/<Class>/Body.cpp`: ExtData constructor defaults and `LoadFromINIFile` key reads;
    `Ext/<Class>/Hooks*.cpp`: hooks into gamemd (`DEFINE_HOOK(gamemd_addr, Name, size)`);
    `Misc/` global features, `Misc/SWTypes/*.cpp` superweapon types.
  - The source files are Latin-1 in places: search with `command grep -rna` (plain `grep` is
    ugrep here and silently skips non-UTF-8 files).
  - Ares.dll function addresses ("installed Ares" addresses such as `10051B70`, which the engine
    already cites): `ares-src/decompiled/index.tsv` (address, name) and
    `ares-src/decompiled/functions/<address>.c` (Ghidra C of the real 3.0 DLL). Hook table:
    `ares-src/symbols/hooks_30.tsv`; `ares-src/report/hooks.tsv` maps hooks to source files;
    `ares-src/report/ini_coverage.tsv` lists every ExtData INI key with its offset.
  - Where restored source and the decompiled DLL disagree, the DLL is the authority.
- gamemd context:
  - `ares-src/reference/gamemd_hooks/<gamemd function address>.c`: the gamemd function around
    each Ares hook, with the hooked line marked `// >>> HOOK`; `index.tsv` maps hook to function.
  - Any other gamemd function: `/Users/guoxingchai/Documents/workspace/other/vera20k/ghidra-gamemd/decompile.sh <hexaddr> [...]`
    (headless Ghidra on the annotated database, ~10 s per call, serialized by a lock; batch
    several addresses in one call). Annotations are leads, not proof.
- Retail game folder: `/Users/guoxingchai/Downloads/Command & Conquer Red Alert II`
  (set `RA2_DIR` to it for retail tests). Retail INIs are being extracted into the engine's
  `ini/`; if `ini/rulesmd.ini` is missing when you need it, run retail tests later.

#### What "aligned" means here

- Port Ares 3.0 behaviour for your chain as Rust-native code, behind `#[cfg(feature = "ares")]`.
  Default builds must keep their current behaviour exactly; INI keys cannot enable an absent
  capability. Keep `phobos`/`npatch` combos compiling (`--features ares,phobos` is a cheap
  `check` you can run once at the end if you touched shared code they also gate).
- For every key: port the Ares read exactly (section, exact key, default from the ExtData
  constructor, parser/INI_EX semantics, clamps, read order, Nullable vs Valueable, art vs rules
  file). Reuse the engine's existing `read_ares_*` helpers on `IniSection`; add one there only if
  the Ares parser genuinely differs.
- For every behaviour: port the hook's logic at the matching place in the engine's existing owner
  of that gamemd function (search the engine for the gamemd address and native name first; each
  native function has one Rust port; extend it, never fork it). Cite next to the Rust the Ares.dll
  function address and the gamemd hook address, e.g. `// Ares 3.0 10051B70 (hook 0x701900)`.
- Simulation determinism, save/load (snapshot/world hash) and ownership rules in AGENTS.md apply.
  If you add simulation state, follow how existing Ares state is persisted/hashed.
- A chain is done when every key and hook in its scope is ported, or explicitly listed as a
  residual (with trigger, effect and why) in your final report. Prefer finishing whole mechanisms
  over touching many keys shallowly. Big separate mechanisms can stay residuals.
- Tests: per mechanism 1-3 focused regression tests through production code that fail without
  the change. No sweeps, no golden dumps.

#### Working in the shared checkout (important)

- Never switch branches, rebase, stash, reset, or run `git checkout -- <file>` on files you did not
  create. Never revert other agents' edits. Don't push. Don't open PRs.
- Keep scratch files OUTSIDE the repo (use your own temp dir under the scratchpad
  `/private/tmp/claude-501/-Users-guoxingchai--claude-spares-spare-M3He11/cac3b7c4-021c-4adb-b385-11b7b34cf770/scratchpad/<your-chain>/`).
  An untracked file in the repo invalidates everyone's builds.
- Make each batch of edits compile-ready before you start a build: add a field, its reader and its
  uses together, so the tree you leave never breaks another agent's build. If a build fails in a
  file you did not touch, another agent is mid-edit: do other work (read, write the next batch) and
  retry later; do not "fix" their code.
- Run Cargo only through the runner from the checkout, e.g.
  `python3 -m tools.cargo_run --min-free-gib 4 -- test -p vera20k-core --lib --features ares <module_path>::`
  (use `-p vera20k` for client modules such as render/app/ui). It serializes builds across agents.
  Free disk is near the runner's 12 GiB default floor, so always pass `--min-free-gib 4` before `--`.
  Retail INIs are extracted in `ini/`; set `RA2_DIR` (below) for retail tests.
  If it ends with `Source changed during Cargo` / `during the tests`, another agent edited at the
  same time: just rerun. Never kill a compile. Free disk is limited; never use a separate
  `--target-dir`, `CARGO_TARGET_DIR`, or a new worktree.
- Format only files you edited: `rustfmt --edition 2024 <file>`.
- Commit your chain on `finish_ares` in validated increments with
  `git commit -m "<message>" -- <your paths>` (commit by explicit paths; do not `git add -A`).
  If `index.lock` exists, retry shortly. A shared file you edited may also carry another agent's
  hunks; committing it is fine as long as the tree compiles. Do not hand-stage partial hunks
  (`git apply --cached`, zero-context patches): that has put hunks in the wrong place and left
  commits that do not compile. Commit whole files by path. Commit messages: plain English
  subject + short body; NO `Co-Authored-By` trailer.
- Do not edit `docs/ares.md` (the coordinator updates it). You may add or update a short
  `docs/ares-<topic>.md` for your chain if the mechanism needs one (match existing ones).

#### Final report (your last message)

Under ~40 lines: keys/hooks ported (with Ares and gamemd addresses), engine files touched,
commits (hashes), tests run and results, residuals left in scope, and anything another chain
must know (shared state you added, owners you changed).
