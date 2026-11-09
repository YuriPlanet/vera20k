# INI file directives

VERA expands includes in its existing INI source-selection path. Ares
`[#include]` and Ares `[CHILD]:[PARENT]` section inheritance are compiled in
with `--features ares`; Phobos `[$Include]` is compiled in with
`--features phobos`. The default build treats all of them as ordinary, unused
INI content. These are extension directives, not native YR syntax. Files
without directives keep their native parsed representation, duplicate-section
behavior, content hash and saved encoding. The raw `IniFile` byte parser and
deserializer never load other files; the `[CHILD]:[PARENT]` suffix is retained
only as skipped parse metadata so the stock lexer and `SaveToPipe` bytes stay
unchanged.

The `$Inherits` section-copy policy below is the ra2-mono adaptation credited in
[Source mapping](#source-mapping), not an Ares or Phobos claim; Phobos's own
`$Inherits` reads through a different hook.

```ini
[#include]
2=units.ini
10=weapons.ini

[ExampleUnit]
$Inherits=CommonUnit
Strength=250
```

## Precedence and boundaries

- With `--features phobos`, a document's `[$Include]` runs and takes over from
  its `[#include]`, as Phobos `-Include` does by restoring the vanilla bytes at
  the Ares hooks (gamemd `0x474200`/`0x474314`). With `--features ares`, a
  document without `[$Include]` expands its `[#include]`, in a combined build
  too: VERA has no `-Include` launch switch, while natively one launch runs one
  dialect.
- Include lists are read in stored entry order, as Ares walks them with
  `INIClass::GetKeyName` (`0x00526CC0`). Phobos iterates the section's
  `EntryIndex`, gamemd's CRC lookup index; VERA keeps stored order for it too.
  Numeric-looking keys only provide unique names; they do not reorder reads.
  MO 3.3.6 writes its `[#include]` entries as `+=name`. All of them share the
  `+` key, so the name lookup would return one winner for the whole list;
  VERA instead reads each appended occurrence's own value in file order, still
  cut and trimmed to the native `0x80` buffer. Other keys keep the name lookup.
  This is a VERA adaptation of the observed MO packaging, not a DLL trace.
  The same `+=` append syntax is now applied to the explicit type registries
  too (`RulesClass::Process`'s `EXPLICIT_RULE_TYPE_FAMILIES`, such as
  `[VehicleTypes]`, `[InfantryTypes]`, `[Animations]`, `[Warheads]` and
  `[SuperWeaponTypes]`). `allocate_explicit_family` walks the physical entries
  and reads each appended occurrence's own stored value with the native
  32-byte `ReadString` cut and trim, so a mod's custom rules file registers
  its custom vehicles, infantry and art. Only Ares/Phobos builds take
  this path; stock YR still walks unique key names. RESIDUAL: `[WeaponTypes]`
  and `[Projectiles]` are not explicit registries in VERA (they are allocated
  by reference), so their `+=` entries are not read as a registry list; game
  value lists appended with `+=` are still not applied.
- The current document is read before its included files in both dialects.
  Descendants override their including document, and later list entries
  override earlier ones. Ares documents the same result: root
  `[E1] Strength=200` plus included `Strength=300` gives `300`
  ([official example](https://ares-developers.github.io/Ares-docs/new/misc/include.html));
  Phobos says the later-read value wins. Ares's tail hook at `0x474314` and
  Phobos's `CCINIClass_Load_Inheritance` at `0x474230` both merge after load.
- Both dialects read each file once per root load. Ares records every file it
  reads (`CCINIClass_ReadCCFile1`), skips a listed name already recorded
  (case-insensitive `_stricmp`) and clears the record only when the outermost
  read returns; Phobos keeps `SavedIncludes`. A missing listed file is skipped
  by Ares and fatal under Phobos (`Debug::FatalErrorAndExit`).
- Residual: nested lists are expanded depth-first per document. Natively each
  included file merges into the same INI, so its list joins one shared include
  section: Ares walks it with one index shared by every nesting level, and
  Phobos re-walks it skipping saved names. A parent's remaining entries are
  therefore read before a child's own list, and a child entry that reuses a
  parent's key name resolves through the shared section. Matching that needs
  the native same-INI `ReadCCFile` merge, which `IniFile::merge` does not model.
- Ares `[CHILD]:[PARENT]` runs after Include expansion and before `$Inherits`.
  Ares resolves such a parent while the section is parsed, so the parent must
  appear earlier in the composed document; a missing or later parent produces a
  diagnostic and no copy. Child-authored values win, directive keys are not
  copied, and copies share the `$Inherits` expanded-text budget. RESIDUAL:
  repeated colon headers for one child and a parent extended by a later include
  are not modelled. Native copies everything last read for the parent, so
  duplicate `[PARENT]` sections accumulate; VERA takes the first earlier
  occurrence as a single physical section instead. Both need the native
  same-INI parse hook. This is a VERA adaptation of the community-documented
  Ares behavior, not an Ares DLL trace.
- `$Inherits=A,B` then copies only missing keys: child-authored values win,
  then the first supplying parent. Parent section names remain case-sensitive.
  Directive keys are not inherited.
- Missing files, callback failures, missing parents, cycles and depth limits
  produce structured diagnostics with the source and reference chain. The
  default policy warns and skips the failed reference; independent references
  continue, except that a missing Phobos `$Include` reference is always fatal.
  Strict callers reject the owned candidate on the first diagnostic.
- The default depth limit is 64 reference edges below the active root, a VERA
  safety bound because native has none. It is checked before the next Include
  read. Completed inheritance parents may be reused without descending.
- Inheritance copies are also charged against an aggregate expanded-text budget,
  checked before each copy and only for entries the child does not author: 64
  times the composed document's own key and value text, at least 1 MiB and at
  most 32 MiB. The depth and cycle limits cannot bound one parent inherited by
  many children; the multiple leaves room for a legitimately wide fan-out while
  a small document cannot license a large store. Exceeding the budget always
  fails the load instead of publishing a partially inherited store, as the 1024
  include-reference limit does. Retail content is far below it.
- Logical Include identities normalize slash direction, dot segments and ASCII
  casing independently of host OS/cwd. File lookup stays with the existing VFS.
  All Include references use that same resource root, not the including file's
  host directory. This adds neither arbitrary host-path access nor new MIX mounts.

Native lexical rules still apply, including exact section/key casing, omitted
empty values and entry-less sections, byte widening and the physical line limit.
This deliberately does not transplant mono's UTF-8 parser or change vanilla YR
lexing to match a different extension dialect. In particular an empty directive
section discarded by the native parser cannot override another parsed section.
The depth limit bounds active recursion, not total graph size or repeated-DAG
expansion work; the include-reference limit, the 32 MiB included-source budget
and the expanded-text budget bound that work instead.
Opaque resource aliases are not a filesystem/symlink sandbox.
Optional layers retain their existing error policy: an unavailable or failed
`langrule.ini`, `soundmd.ini` or `evamd.ini` is skipped with a warning, so a
fatal Phobos include error inside one of those optional files does not abort
startup. Required Rules, ART, AI and map selections propagate it.

## Production integration

With `ares` or `phobos` enabled, `rules::retail_sources::select_ini` expands
directives before transferring the selected physical snapshots to the existing
ordered Rules owner. `DirectiveOptions::production` enables the compiled
dialects, and a document's `[$Include]` takes over from its `[#include]` when the
Phobos capability is present. That shared entry serves Rules, fixed Art, AI,
mode/campaign definitions, sound and EVA. The theme catalog uses the same entry
while retaining its separate YR/RA2 playlist handling. Asset-backed map gameplay
and metadata expand the decoded map INI before reading its fields; maps retain
the existing physical root source identity, including MMX wrapper bytes.

The root `IniSourceIdentity` still hashes the exact root bytes. `SelectedIni`
additionally exposes the included physical sources, in traversal order, and
structured diagnostics. Included values enter the existing effective INI/rules
hash and stored processed INI. Loading a snapshot reads that stored content; it
does not open Include files again. The saved Rules owner and snapshot header retain
the build capability; a build with a different capability rejects the save before
admitting its simulation.

The change does not select `rulesmo.ini`/`artmo.ini`/`aimo.ini` as game roots
(that is [INI root profiles](ini-roots.md)), configure an arbitrary archive
stack, port other Ares/Phobos gameplay tags, or establish full MO compatibility.
Raw metadata helpers without an asset provider remain physical readers. It does
not load Ares or Phobos DLLs.

## Read-only probe

```text
asset ini-expand rulesmd.ini --ra2-dir <YR directory> --strict
asset ini-expand rulesmo.ini --ra2-dir <MO directory> --source-only --all-mixes --strict
asset ini-expand root.ini --ra2-dir <game directory> --max-depth 16
```

The JSON reports root and dependency identities, diagnostics, physical and unique
section counts, newly supplied sections and the effective INI hash. `--strict`
rejects reference errors; normal mode reports them without hiding the valid
branches. `--all-mixes` is the pre-existing tooling-only wider search: a successful
probe with it does not mean the game's normal archive stack finds those files.
The explicit read-only `asset ini-expand` probe remains available without either
feature and asks for both dialects (`DirectiveOptions::default`), preferring
`$Include` when both sections exist. It uses the same parser, provider and
traversal as enabled gameplay. Neither probe success nor a passing regression is
an in-game MO acceptance test.

`--source-only` creates an explicit INI-tool resource root without the normal
game's required archive bootstrap (for example a MOD directory without
`multimd.mix`). By itself it reads loose root files; adding `--all-mixes` uses
the existing disk-MIX tool scan. The JSON marks this as `tooling_ini_root`, not
the game archive stack. The game constructor and its required-media checks are
unchanged. This mode does not promise the native archive priority for a MOD.

## Source mapping

The include and inheritance traversal was adapted from **baixiong443**'s
ra2-mono `IniFile`, including the bounded traversal fix. `$Inherits` remains
that adaptation; its uncommitted files are not claimed at a published donor
commit. This contribution is under VERA's GPLv3; no Ares/Phobos C++ is imported.

| native source | VERA adaptation |
| --- | --- |
| Ares.dll `CCINIClass_ReadCCFile1`/`2` hooks (gamemd `0x474200`/`0x474314`, `ReadCCFile` `0x4741F0`) | Ares dialect in `rules/ini_directives.rs`: stored-order list read, once-per-root-load skip, missing file skipped, merge after load |
| Ares `INIClass::GetKeyCount` `0x526960`, `GetKeyName` `0x526CC0`, `ReadString` `0x528A10` | `IniSection::physical_entries` walk plus `read_name` with the native `0x80` buffer capacity |
| Phobos `src/Misc/Hooks.INIInheritance.cpp` (`CCINIClass_Load_Inheritance` `0x474230`) | Phobos dialect: `$Include`, `SavedIncludes` once-per-file, fatal missing file, merge after load |
| Phobos `src/Phobos.cpp` `-Include` (`0x474200`/`0x474314` restored) | A document's `$Include` takes over from its `[#include]`; selected by `DirectiveOptions::production` |
| Ares DLL `[CHILD]:[PARENT]` (undocumented; parse-order-sensitive, see [PPM](https://ppmforums.com/topic-61295/how-inheritance-works-in-ares-dll/)) | VERA adaptation in `rules/ini_directives.rs`: suffix kept as skipped parse metadata, earlier-parent lookup, missing-parent diagnostic, shared expanded-text budget |
| ra2-mono `IniFile.cs` (`InheritFromParent`, `DirectiveTraversal`), `IniDirectiveProcessingOptions.cs` | `$Inherits` policy, structured diagnostics, strict mode and the VERA depth/work limits |
| `Ra2.Tests/IniDirectiveTests.cs` | `rules/ini_directives_tests.rs`, plus VERA VFS, typed processing, map, hash and restore regressions |

Intentional adaptations are the host's native parser/merge and immutable source
snapshots, root-relative VFS naming instead of host-cwd identity, VERA's bounded
depth/work limits (native has neither), and strict loads discarding their owned
candidate. Phobos's skipped-first-line quirk is not reproduced: VERA parses the
whole included file. The documented Ares example and the include policies read
from Ares.dll and the Phobos source are covered by Rust regressions, not an Ares
DLL run.
