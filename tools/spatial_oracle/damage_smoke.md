# Ordinary damage-smoke numeric and ordering controls

```sh
python -m tools.spatial_oracle.damage_smoke --check
```

`damage_smoke.py` inherits the existing `electric_bolt.SparkBirth(BulletReader)`
runtime and observes original Particle and ParticleSystem bodies. It introduces
no image mapper, INI parser, RNG algorithm or gameplay implementation.
The JSON contains original-produced results; its sidecar records the selected
executable hash and command. Default execution checks; `--write` deliberately
creates or replaces a reference.

The selected physical types are SmallGreySSys/BigGreySmokeSys and
SmallGreySmoke/LargeGreySmoke. Original constructors and readers determine
their scalar defaults and fields across the existing physical RULESMD,
optional LANGRULE, MPBattleMD and XMP03T4 input layers. Original Image25 and
SHP loading use physical ART/shape bytes supplied by the shared archive owner.
General WindDirection executes its actual ReadInt/store slice, retaining the
Rules constructor default -1. Full Rules::Process and physical file walking
are outside this component fixture.

The compact cases distinguish the extra constructor signed-mod2 draw before
EC initialization, odd/even drift draws, native-ID animation cadence, uint16
EC wrap and exact-zero death, reverse parent split IDs/RNG order, emission
independent of Spawns/ParticleCap, float speed/accumulator boundaries, every
visit's rise/wind and the bridge cutoff using the old coordinate's live cell.
The accumulator walk retains only integer-boundary/end checkpoints, with
HoldsWhat=-1 as supplied prior state; offset draws still precede that native
gate. No Python formula supplies any expected output.

The ordinary Building control uses the original Object constructor and real
Building vtable/flags/coordinates/alive fields, with the existing LiveSparkBirth
foundation index3 and native empty Techno/Building expiry-vector prior states.
It does not claim complete
Building construction, placement, House state or received damage. A supplied
done system reaches original Limbo/Detach_All(control1)/PointerExpired, so
owner-slot clearing is observed before deferred destruction. Structural bridge
flags, flat cells and independently initialized level104/deck416 globals are
supplied from existing collision evidence. Parent ReceiveDamage admission,
full live Logic ordering, draw admission/pixels, audio and persistence require
their own production checks and evidence.

Particle registration is separate from ParticleSystem drawing: original
ParticleGetLayer62D770 returns AIR3, ObjectUnlimbo submits it through4A9720,
and LayerInsert5519B0 appends in global stable order. Tactical6D8DB0 visits
that retained order; SystemDraw62E280 handles optional light and has no child
draw loop. Reuse the existing Display owner for ordering. ParticleDraw62CEC0
passes decimal1000 (0x3E8) brightness and depth adjustment-15-AdjustForZ;
neither value is a screen-Y nudge. This corpus executes no draw pixels.

ParticleDraw calls the mutating minimum-frame-rate query `55AF60` before its
Detail check, admitting unsigned sampled Logic count `ABCD44` at least equal
to the result or nonzero ParticleType Damage. The existing app `DetailState`
owns the process counter/hysteresis and its original-produced controls;
the live Options detail projection owns `A8EB78`. Reuse both owners for
each actual draw entry in retained AIR order. This corpus supplies no render
admission or whole-scene ordering golden.

Native Particle dispatch62CE40's lifetime tail is common to Smoke/Gas/Fire/
Spark/Railgun. The Rust lifetime owner is system_ai::finish_lifetime, shared by Spark
finish and the other dispatcher branches. Native Gas and
Fire have distinct animation end/fade policies despite sharing the ID/EC
cadence arithmetic; this Smoke corpus does not execute their whole mechanisms.
