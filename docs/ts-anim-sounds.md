# Tiberian Sun debris sounds

Tiberian Sun let a bouncing chunk of debris make a noise when it hit the ground. Yuri's
Revenge dropped the two keys: `gamemd.exe` holds neither name, though `RULESMD.INI` still
describes them in its `[VoxelAnims]` comments. VERA reads and plays them as Firestorm 2.0
does. No retail INI sets them, so stock games sound as before.

```ini
; ARTMD.INI: an animation with Bouncer=yes
[DBRIS1LG]
BounceSound=SomeSound   ; default none: plays at the chunk each time it bounces
ExpireSound=SomeSound   ; default none: plays where it comes down on dry ground

; RULESMD.INI: a [VoxelAnims] type
[TIRE]
BounceSound=SomeSound
ExpireSound=SomeSound
```

Each value names a `[SoundList]` entry of `SOUNDMD.INI`. On an animation, a name that list
does not hold leaves the key as an earlier INI set it, as `StopSound=` does. On a
`[VoxelAnims]` type the last INI that sets the key wins, as VERA reads that type's
`StartSound=`, so a map naming a missing sound silences one the rules named; Firestorm
keeps the rules' sound there.

- **BounceSound=** plays each time the piece bounces, right after its `BounceAnim=` and
  before the bounce damages what it hits, whether or not the type names a `BounceAnim=`.
  A voxel piece that touches water stops there without bouncing, so it plays no sound.
- **ExpireSound=** plays once when the piece comes down on dry ground, or on a bridge
  deck, after its `ExpireAnim=` and the damage that follows, whether or not the type
  names an `ExpireAnim=`. A piece that lands in water makes its wake and splash instead
  and plays no sound.

Only bouncing pieces play them: an animation without `Bouncer=yes` never reads its own.

Firestorm 2.0 evidence: `AnimTypeClass::Read_INI` `0x00419181..0x00419216` (AnimType
+180/+184), the bounce step `0x00414CE8..0x00414D0C` and the landing in `AnimClass::AI`
`0x004150D3..0x004150F6`; `VoxelAnimTypeClass::Read_INI` `0x0065F6AA..0x0065F734`
(+170/+174) and `VoxelAnimClass::AI` `0x0065EC59..0x0065EC86` and
`0x0065E482..0x0065E4AF`. Code: `sim::anim_class::bounce` and
`sim::world::voxel_anim_effects`.

Residual: Firestorm skips a sound whose `[SoundList]` index ends in the byte `0xFF`
(index 255, 511, …): it compares only the low byte with -1 (`CMP BYTE PTR [..], 0xFF`).
VERA plays every listed sound.
