# Ares INI syntax

An Ares launch hooks gamemd's INI text reader, `INIClass::LoadFromStraw` (`0x525A60`), so
every INI it loads gets three extensions: `+=` keys, `[Child]:[Base]` sections and
later-wins repeated keys. With `--features ares` VERA adds them where the reader is
ported: `IniFile::from_str` / `from_bytes` ([`ini_ares_syntax.rs`](../src/rules/ini_ares_syntax.rs)).
Rules, art, AI, mode, map, sound, EVA, theme, UI and settings INIs all load through it.
Default builds keep the native reader.

```ini
[VehicleTypes]
+=TANK2          ; becomes var_0, var_1, ...

[TANK2]:[MTNK]   ; starts with MTNK's entries
Sight=9
Sight=10         ; the later Sight wins
```

## Behaviour

`LoadFromStraw` has two arms, chosen by whether the INI already holds a section. A fresh
load builds each section and links it once it ends. Loading into an INI that has sections
(`0x525BFA`) writes every entry with `INIClass::PutString` (`0x528660`), which replaces a
key and lists it last. `ReadCCFile` loads every Ares `[#include]` and Phobos `$Include`
file this way into the INI that lists it, so an Ares build now loads included files into
the root INI (`IniFile::merge_load`) instead of composing them with `IniFile::merge`.
That holds in `ares,phobos` too. Phobos-only builds keep `merge`. The load also returns
the entries the file put under its include section, which is the list VERA follows
next. Those entries include an unterminated last line, which this arm loads.

- **`+` keys.** A key that is exactly `+` becomes `var_<n>`. One counter serves both arms.
  VERA numbers per INI, continuing through its included files, so a registry such as
  `[BuildingTypes]` allocates every entry, including entries from included files.
  Layers (rules, mode, map) are separate INIs. Each is walked on its own pass, so equal
  `var_` names in two layers do not collide.
- **Inheritance.** The text after a header's `]` may name a base: blanks, `:`, blanks,
  `[name]`, with no `;` before its `]`. The base's entries are written, in order, under the
  child's name. They come from the INI being loaded, so the base must already be loaded:
  earlier in the same file, or in the including file or an earlier included one. A
  missing or later base logs a warning and copies nothing, and a map cannot inherit a
  rules section. In an included file the child's own keys then replace the copied ones.
  In a fresh load, as in a root `rulesmd.ini`, the copy happens before the child section
  is linked. A child with keys of its own therefore becomes two sections of one name, and
  readers see whichever the native CRC index answers. That is Ares's own behaviour (Ares
  bug 895552: inheritance in the main rules file loses keys). The first header of an
  included file never inherits: the reader handles it before the hooked loop.
- **Repeated keys.** A key whose CRC an earlier entry of the section has deletes that
  entry, so the later value wins and is listed last. Without Ares the first one wins.
  This also applies to retail files that repeat keys.

The parse is deterministic. Map and INI digests, multiplayer content hashes and
`GetCRC` all hash the resulting entries. A saved game stores the processed INIs and does
not parse them again. The `+` counter is not saved: nothing loads into a saved INI.

## Hooks

| gamemd | Ares.dll | Name | VERA |
|---|---|---|---|
| `0x5260A2` | `10065280` | INIClass_Parse_IteratorChar1 | `ares_syntax::load_fresh` (`iterator_key`) |
| `0x525D23` | `100652F0` | INIClass_Parse_IteratorChar2 | `ares_syntax::load_into` (`iterator_key`, value cut to the buffer) |
| `0x525DDB` | `100655E0` | INIClass_Parse_IniSectionIncludes_PreProcess2 | `load_fresh` → `pre_process` (`10065470`) |
| `0x525E44` | `10065580` | INIClass_Parse_IniSectionIncludes_CopySection2 | `load_fresh` → `copy_section` (`10065380`) |
| `0x525CA5` | `100655B0` | INIClass_Parse_IniSectionIncludes_PreProcess1 | `load_into` → `pre_process` |
| `0x525C28` | `10065550` | INIClass_Parse_IniSectionIncludes_CopySection1 | `load_into` → `copy_section` |
| `0x5260D9` | `10065610` | INIClass_Parse_Override | `load_fresh` → `replace_entry` |

`put_string` ports `INIClass::PutString`. Lookups by name follow `INIClass::GetSection`
(`0x526810`), including the cached pair that answers before the index is sorted. Those
lookups decide which same-named section a later base lookup finds.

## Residuals

- Native `+` numbering continues across every INI loaded since the last scenario clear,
  because `Ares::Clear` (`10004D90`) resets it. VERA's names are unique within each INI
  but can differ in number. A mod key literally named `var_<n>`, and `GetCRC` compared
  with a native peer, can tell the difference. Two layers can share a name natively too,
  since each layer is its own INI.
- The cached section pair is followed within one load only. VERA's readers search the
  sorted index, which differs only when a reader first asks for the last name a load
  looked up, and that name has two sections.
- A header on an included file's unterminated last line leaves its base set for the next
  load's first header; VERA drops it.
- Ares walks one include list that every loaded file adds to (`CCINIClass_ReadCCFile2`
  `10064660`). VERA still follows each file's own list depth-first, so with nested lists
  a later root entry can be read before a child's own entry. A reused key can also hide
  a root entry. See [INI directives](ini-directives.md).

No RNG draw, timer or detach is involved.
