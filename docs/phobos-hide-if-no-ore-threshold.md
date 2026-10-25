# Optional Phobos ore-stage animation hiding

Build with `--features phobos` to read Phobos `AnimTypeExt`'s
`HideIfNoOre.Threshold=` on an `Animations` entry. Default builds keep the
original test: the anim hides whenever its cell holds no resource value at all.

In `artmd.ini`:

```ini
[TWNK1]                            ; AnimType
HideIfNoOre=true                   ; boolean, the stock gate
HideIfNoOre.Threshold=0            ; integer
```

`AnimTypeExt::LoadFromINI` reads the key as a `Valueable<int>` with constructor
default 0, so an absent or unparseable value retains what an earlier Rules layer
set.

`AnimClass::AI @ 0x00423AC0` rewrites the anim's hidden byte
(`+0x19D`) on every visit from its stored `Location` (`+0x9C`): the original
sets it as soon as `CellClass::Get_Tiberium_Value @ 0x00485020` is zero, and
Phobos' `AnimClass_AI_Early @ 0x423B95` replaces that equality with
`value <= abs(HideIfNoOre.Threshold)`. The absolute value is taken at the use,
not at the read, so `-60` and `60` behave alike; the native `abs` of `INT_MIN`
overflows, and VERA keeps the same two's-complement result. Only drawing is
suppressed — the animation's own AI, damage and trailer work continues, exactly
as under the stock gate.

Because `Get_Tiberium_Value` is `TiberiumClass::Value * (OverlayData + 1)`, the
threshold is a credit value, not a density: with retail `Riparius Value=25` a
threshold of `60` hides the anim over density 0 and 1 (25 and 50) and keeps it
drawing from density 2 (75) up. That is the "hide until the ore is far enough
along" use the tag exists for. A custom type with a negative `Value=` hides at
every density, since its cell value is then always at or below the threshold.

This feature does not load Phobos DLLs or establish complete mod compatibility.
