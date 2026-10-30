# Optional Ares per-type promotion sounds

Build with `--features ares` to let a TechnoType choose the sound its
promotion to veteran or elite plays.

```ini
[HERO]                         ; rulesmd.ini TechnoType section
Promote.VeteranSound=HeroVet   ; SoundDialogList name, or `none` to silence
Promote.EliteSound=HeroElite
```

An unset override keeps the global promotion cue; an explicit `none`
silences the cue for that type while keeping the rest of the promotion/EVA
dispatch. Default builds ignore both tags and always play the global cue.

## Where VERA20K owns it

- `rules/object_type.rs`: both keys read through the shared nullable
  fixed-catalog reader, per rules layer, so the effective binding keeps the
  layer history and joins the ruleset's configuration identity. Native:
  TypeExt `LoadFromINI 1003F0C5/1003F0DB` reusing `NullableIdx<Voc>
  1000E9D0` (2048-byte read, fixed-catalog lookup, unknown retains).
- `sim/world/techno_ai.rs`: the promotion event resolves the effective
  sound at dequeue through the existing promotion owner.
