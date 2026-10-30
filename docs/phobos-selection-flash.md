# Optional Phobos selection flash

Build with `--features phobos` to flash a unit briefly when the local player
selects it.

```ini
[AudioVisual]                  ; rulesmd.ini
SelectionFlashDuration=15      ; flash length in frames, 0 = off

[Phobos]                       ; RA2MD.INI, imported into the VERA profile
ShowFlashOnSelecting=yes       ; client switch, default no
```

A selection flashes when both settings allow it: the client switch must be
`yes` and the duration above 0, and the selected object must be controlled by
the current player (`GetOwningHouse()->IsControlledByCurrentPlayer()`).
Computer-owned units never flash.

## Where VERA20K owns it

- `rules/phobos_selection_flash.rs`: `[AudioVisual]
  SelectionFlashDuration` (`RulesExt::LoadBeforeTypeData`, constructor
  default 0).
- `app/persistence/phobos_settings.rs`: `[Phobos]
  ShowFlashOnSelecting`, read from the imported player profile (Phobos reads
  `RA2MD.INI [Phobos]` directly; VERA imports that file into `VERA.INI`).
- `app/presentation/state.rs`: the selection edge arms the flash on each
  local selection, matching Phobos's `ObjectClass_Select` hooks
  (`0x5F46AE`/`0x5F4718`).
