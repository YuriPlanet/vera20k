# Native IFV impact and complete bounded object lifetime

Reproducible native addition; `ifv_launch` evidence is unchanged. Run from the
owned checkout, with retail gamemd and Unicorn2.1.4:

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
~/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_impact.py --check
```

`ifv_impact.json` contains the same five flag/flight cases,
125 Bullet AI visits and 70 subsequent Anim AI visits. Their launch-to-impact
XYZ and binary64 velocity payloads match all125 earlier launch rows exactly.
Each executes full Detonate4690B0 and AreaDamage489280, returns1, selects physical
XGRYSML2, constructs runtime Anim421EA0 (native ID30 after Bullet29), runs Start,
then consumes the cluster tail: seed31 gives ranged256..512 result422 after four
raw transitions, followed by raw-angle107402731. No AreaDamage or destructor
body is replaced. Structured events preserve actual return values and call order.

UnInit5F65F0 -> expiry7258D0 -> Conceal5F4D30 -> pending enqueue is followed by
original pending drain725C70 -> COMRelease46AFF0 -> Bullet466560 -> second expiry
-> Object5F3B80/free. Bullet registry1→0 and queue1→0. Full physical AnimType
reader427D00 gives XGRYSML2 thirteen frames, end13, rate1, Scorch=false,
Crater=false, no particles. Fourteen separately scheduled original AnimAI423AC0
visits then reach Middle424F00, Destroy4255B0, UnInit, drain and full Anim scalar
destructor426590. Final Anim and pending registries are empty. No nested Anim or
Smudge constructor is reached; the only runtime identity spent is impact Anim30.

Declared inputs and boundaries:
- GameActive `0xA8E9A0=1` is supplied. Full original Anim Unlimbo now admits
  placement, commits the constructor coordinate and submits to Display.
  `NativeUnlimbo`/`NativeUnlimbo_return` events record its requested and retained
  coordinates and admission result. This closes the earlier research fixture's
  zero-GameActive rejection; original Bullet launch admission remains supplied.
- `SelectAnim` events decode the retained Bullet coordinate passed by the caller,
  land argument and both height-unit globals. That coordinate can differ from
  the AreaDamage and Anim constructor coordinate.
- Same prepared stationary source, supplied FLH/target/heading, single projectile,
  original COM factory activation boundary, supplied Bullet Unlimbo/Display
  admission and inter-frame bridge-bit changes as the launch witness. No Burst2
  scheduler or real collapse/repair producer. DRAGON is now physically loaded;
  Bullet+A8 trail remains null because supplied admission skips its creation.
- Original selected readers execute on lexical physical INI caches. Eight retained
  HE AnimTypes are explicitly ART-read after the selected setup. Sound registry
  is empty (Report=-1). This is not whole Rules::Process chronology or full pool
  construction/load ordering. In particular the older setup reads Gravity before
  its supplied postpass; root's newer full Process witness establishes the actual
  type/postpass-before-AudioVisual chronology. No final-config parity is claimed.
- Flat level6 cells, map dimensions64×64, empty object/air receiver lists, no
  overlays/tiles, ScenarioDestroyableBridges=false. Live structural bits affect
  guided contact, but this baseline does NOT execute bridge damage admission RNG
  or a bridge driver. The required damageable topology extension is separate.
- Original Object/BombList/Anim/pending/global receiver-vector initializer prefixes
  execute up to their CRT atexit registration. No at-exit cleanup is scheduled.
  Runtime object registry insertion/removal executes; source itself is prepared.
- Original Scenario RNG is independently seeded31. Original ID cursor starts at
  the constructor-subset value28; this is not a whole-game identity phase.
- Host calls pending drain after the Bullet tick, then visits the one impact Anim
  once per subsequent binary frame and drains on enqueue. Original per-object
  timers execute, but whole Logic/Display scheduling and registration are not
  demonstrated. Object/Anim/abstract registry cleanup is observable; Display
  membership remains a supplied launch boundary, not an asserted result.
- Shared harness supplies allocation/free/CRT TLS and exact archive file bytes.
  Added verified PE import boundary InterlockedDecrement(7E11CC) changes only
  COM refcount; original Release/deleting destructor executes. Native Anim drain
  RTTI uses verified IsBadReadPtr(7E115C), supplied false after mapped-memory
  validation, with an empty supplied FS exception chain. No native code patches.

The companion metadata pins the result payload hash.
Native input layers and physical assets are hashed in the payload. `--check`
executes again without writing. `--write` explicitly regenerates the corpus.
The separate `ifv_bridge_impact` executes bounded bridge damage/topology and
fallout; `ifv_trail_impact` joins the admitted trail producer and deferred detach.
Those witnesses keep their own supplied boundaries. Source FLH/Burst scheduling,
whole startup identity, actual Display/Logic admission, rendering and save/restore
integration are not established by this baseline.
