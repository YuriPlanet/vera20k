# INI auto-numbering and section inheritance

VERA supports `+=` and `[Child]:[Base]` in gameplay INIs. Both forms work with
Ares Off. The Ares reading mode also applies its own repeated-key rule.

```ini
[VehicleTypes]
+=TANK2

[MTNK]
Strength=400
Sight=6

[TANK2]:[MTNK]
Sight=9
```

`TANK2` keeps `Strength=400` from `MTNK` and uses its own `Sight=9`.

## Behaviour

- **Auto-numbering.** A key exactly equal to `+` becomes `var_<n>`, using the
  current signed 32-bit count and incrementing it. Startup source selection
  shares one count across `rulesmd.ini`, its includes, the selected
  `langrule.ini` when present, and `artmd.ini` with its includes. A standalone
  file or map read starts at zero and continues through its include tree.
  An explicitly authored `var_<n>` still participates in the normal key
  collision rules. The resolved key is visible to INI readers and digests.
- **Section inheritance.** The parent must already exist in the document
  being loaded. The child incorporates the parent's keys, then applies its
  authored keys. The first header in an included file can inherit from the
  including document. Standalone INIs such as a scenario map and a rules
  layer still have independent section lookups. The copy budget bounds
  expansion and logs a skipped copy.
- **Repeated keys.** Ares-mode text uses the later value and lists it last.
  The stock text reader retains physical duplicates and uses its existing CRC
  index. In either mode the child's own value takes priority over an inherited
  default.

Independent and diagnostic reads start at a fixed counter, so browsing an
INI or previewing a map cannot affect a subsequent gameplay load. Fresh
text and stock overlays discard unterminated final lines. The Ares merge
reader puts a final key before the EOF test.

## Ares 3.0p1 hooks

| gamemd | Ares.dll | Activity |
|---|---|---|
| `0x5260A2` | `0x10066200` | `INIClass_Parse_IteratorChar1`, fresh auto-numbering |
| `0x525D23` | `0x10066270` | `INIClass_Parse_IteratorChar2`, merge auto-numbering |
| `0x525DDB` | `0x10066560` | `INIClass_Parse_IniSectionIncludes_PreProcess2`, fresh parent lookup |
| `0x525E44` | `0x10066500` | `INIClass_Parse_IniSectionIncludes_CopySection2`, fresh copy before linking the child |
| `0x525CA5` | `0x10066530` | `INIClass_Parse_IniSectionIncludes_PreProcess1`, merge parent lookup |
| `0x525C28` | `0x100664D0` | `INIClass_Parse_IniSectionIncludes_CopySection1`, merge copy |
| `0x5260D9` | `0x10066590` | `INIClass_Parse_Override`, later-wins duplicate in a fresh load |

The indexed Rust reader is [`ini_ares_syntax.rs`](../src/rules/ini_ares_syntax.rs).
The source selection order is in [`retail_sources.rs`](../src/rules/retail_sources.rs).

## Residuals

- P1 shares one counter across all INI objects and clears it in
  `Scenario_ClearClasses` (`0x685659` → `0x100057C0`, reset at `0x10004FF7`).
  VERA's scoped sequences keep previews and multiplayer map agreement
  deterministic. Interleaving separate reads, or starting the next scenario
  in an existing P1 process, can produce other numbers.
- P1 copies fresh defaults through `PutString` before linking the child's own
  section. The duplicate section name can hide either set of keys. VERA
  combines both sets in the child section. P1 ordinarily skips parsing the
  first included header's own parent suffix. A parent pointer left by the
  prior file's unterminated final header can be consumed by that first header.
  VERA resolves each explicit suffix within its own document.
- The copied-text budget and include traversal protections still apply to
  deliberately oversized inputs. The [INI directives](ini-directives.md)
  describe how VERA handles included files and their lookup roots.

The resulting entries enter the existing map and rules agreements. The load
counter is transient. Saves store the resulting entries. No RNG, timer, or
detach call is involved.
