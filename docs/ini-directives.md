# INI file directives

I adapted Include and inheritance processing from my ra2-mono implementation
into VERA's existing INI source-selection path. Gameplay expands these directives
only in builds with `--features ares`. The default build treats them as ordinary,
unused INI content. These file and section composition directives are extension
syntax, not native YR directives. Files without directives keep their native parsed
representation, duplicate-section behavior, content hash and saved encoding.
The raw `IniFile` byte parser and deserializer never load other files.

```ini
[#include]
2=units.ini
10=weapons.ini

[ExampleUnit]
$Inherits=CommonUnit
Strength=250
```

## Precedence and boundaries

- `[$Include]` takes priority over `[#include]` if both parsed sections exist;
  their lists are not concatenated.
- Include keys sort by signed decimal integer, then ordinal key for ties.
  Nonnumeric or out-of-range keys sort as `i32::MAX`.
- For Ares `[#include]`, the current document is applied before its included
  files. Descendants override their including document, and later includes
  override earlier ones. This follows the [official Ares example](https://ares-developers.github.io/Ares-docs/new/misc/include.html):
  root `[E1] Strength=200` plus included `Strength=300` results in `300`.
- `[$Include]` retains mono's separate current-document-wins policy: expand
  descendants first, then apply the current document. I have not established
  executable Phobos equivalence for that policy. Ares evidence does not define
  the semantics of this other directive or of `$Inherits`.
- Inheritance runs after the complete Include expansion. `$Inherits=A,B` copies
  only missing keys: child-authored values win, then the first supplying parent.
  Parent section names remain case-sensitive. Directive keys are not inherited.
- Missing files, zero-byte files, callback failures, missing parents, cycles and
  depth limits produce structured diagnostics with the source and reference chain.
  The default policy warns and skips the failed reference; independent references
  continue. Strict callers reject the owned candidate on the first diagnostic.
- The default depth limit is 64 reference edges below the active root. It is
  checked before the next Include read. Only active-path identities are cycles:
  a shared child on two independent branches is processed twice in its proper
  merge order. Completed inheritance parents may be reused without descending.
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
expansion work. Opaque resource aliases are not a filesystem/symlink sandbox.

## Production integration

With the `ares` feature, `rules::retail_sources::select_ini` expands directives
before transferring the selected physical snapshots to the existing ordered Rules owner. That shared
entry serves Rules, fixed Art, AI, mode/campaign definitions, sound and EVA.
The theme catalog uses the same entry while retaining its separate YR/RA2
playlist handling. Asset-backed map gameplay and metadata expand the decoded
map INI before reading its fields; maps retain the existing physical root source
identity, including MMX wrapper bytes.

The root `IniSourceIdentity` still hashes the exact root bytes. `SelectedIni`
additionally exposes the included physical sources, in traversal order, and
structured diagnostics. Included values enter the existing effective INI/rules
hash and stored processed INI. Loading a snapshot reads that stored content; it
does not open Include files again. The saved Rules owner and snapshot header retain
the build capability; a build with a different capability rejects the save before
admitting its simulation.

The change does not select `rulesmo.ini`/`artmo.ini`/`aimo.ini` as game roots,
configure an arbitrary archive stack, port other Ares/Phobos gameplay tags, or
establish full MO compatibility. Raw metadata helpers without an asset provider
remain physical readers. It does not load Ares or Phobos DLLs.

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
The explicit read-only `asset ini-expand` probe remains available without the
`ares` feature. It uses the same parser, provider and traversal as enabled
gameplay. Neither probe success nor a passing regression is an in-game MO
acceptance test.

`--source-only` creates an explicit INI-tool resource root without the normal
game's required archive bootstrap (for example a MOD directory without
`multimd.mix`). By itself it reads loose root files; adding `--all-mixes` uses
the existing disk-MIX tool scan. The JSON marks this as `tooling_ini_root`, not
the game archive stack. The game constructor and its required-media checks are
unchanged. This mode does not promise the native archive priority for a MOD.

## Source mapping

The reference is my local ra2-mono working tree, including the bounded traversal
fix, not a claim that its uncommitted files exist at a published donor commit.
I contribute this adaptation under VERA's GPLv3; no Ares/Phobos C++ is imported.

| mono source | VERA adaptation |
| --- | --- |
| `Ra2.Shared/Ini/IniFile.cs`: `ProcessIncludesCore`, `InheritFromParent`, `DirectiveTraversal` | `rules/ini_directives.rs`, a child of the existing INI store |
| `Ra2.Shared/Ini/IniDirectiveProcessingOptions.cs` | Structured diagnostics, strict policy and active-path depth limit |
| `Ra2.IO/IniMerger.cs`: per-file directive processing | Expansion at the physical source boundary before existing Rules processing |
| `Ra2.Tests/IniDirectiveTests.cs` | `rules/ini_directives_tests.rs`, plus VERA VFS, typed processing, map, hash and restore regressions |

Intentional adaptations are the host's native parser/merge and immutable source
snapshots, root-relative VFS naming instead of host-cwd identity, and strict loads
discarding their owned candidate. Mono's separate multi-root catalog selection
and whole-definition AI replacement policy are not copied over VERA's existing
native AI readers. The reference and authored Rust tests establish a bounded
implementation contract, not executable Ares/Phobos equivalence.

I corrected Ares `[#include]` conflict precedence against its official
documentation instead of copying mono's current-file-wins policy. The other
directive policies, bounded traversal, root-relative lookup and raw metadata
reader boundaries are unchanged. The documented example and nested-conflict
regressions are Rust checks against that specification, not an Ares DLL run.
