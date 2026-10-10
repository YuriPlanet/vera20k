# Laser reader prerequisites

`weapon_laser.py/json/meta.json` execute the pinned active-retail executable
using the existing Weapon/BulletReader and Rules reset fixtures. The corpus
owns the selected laser and detail reader prerequisites; `building_prism`
owns laser construction, update, detail admission and drawing.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.rules_oracle.weapon_laser --check
```

Use the configured executable and retail directory described in
[`native_oracle.md`](../native_oracle.md). The producer reads RULESMD from
expandmd01.mix, optional loose LANGRULE, Battle1's MPBattleMD from
ra2md.mix/localmd.mix, and XMP03T4.MAP from multimd.mix. It retains physical
hashes and extracts only the selected keys into native caches. Physical source
selection and complete production processing are also exercised by the Rust
retail fixture; the native harness does not execute the archive/INI loader.

## Weapon fields

Whole `WeaponType771C70` constructor initializes `LaserDuration` to10 in
byte+14E, the three RGB triples at+120/+123/+126 to zero, and the
IsLaser/IsBigLaser/IsHouseColor flags at+149/+14C/+14D to false. Whole reader
772080 gates on the exact type section and reads current values as defaults.
The duration read uses signed `ReadInt5276D0` at7727DE then stores AL at7727E9;
the active laser consumer sign-extends byte+14E at6FD21D. VERA keeps the
public duration as i32 while reproducing the signed-byte narrowing.

The corpus contains24 fresh duration controls and eight calls on one original
weapon. They cover absent section/key, explicit cache-empty values, exact-case
keys, invalid boolean retention, complete RGB triples and integer overflow,
prefix and hexadecimal parsing. Partial malformed RGB triples are excluded:
that original reader can copy uninitialized stack bytes. No synthetic numeric
formula supplies a golden result.

VERA's `IniSection` typed projection already folds each key's read history,
including retained bool/RGB defaults. The final signed-byte projection is
sufficient for the duration reader: present ReadInt values do not depend on
the prior default. No competing Weapon laser-state cache is needed. Retained
weapon laser fields participate in `RuleSet::simulation_config_hash`.

## Detail fields and lifetime

`RulesClass665650` writes normal15, movie20 and buffer5 to+0/+4/+8 at
665665..665672. Exact-case `[AudioVisual]` keys are
`DetailMinFrameRateNormal`, `DetailMinFrameRateMovie`, and
`DetailBufferZoneWidth`, read in that order by66921E/669237/669250. They are
signed ReadInt values, using current defaults, without clamps. They do not
belong to `[General]`.

The constructor and complete AudioVisual6691E0 reader execute15 scalar
controls. A ten-step history covers cold AudioVisual reading, full Process
calls with absent/wrong-section/wrong-case/partial inputs, original destructive
type reset6686C0 through its first complete Process (stop668A2C), then another
Process. The reset retains all three Rules fields. The later reset file reload
tail is excluded. The Rust process-resident registry owner retains these
values through cold startup, process handoffs and destructive type resets;
`rules.general.detail` is its public projection. Its values participate in the
configuration identity.

Fill_In_Data6850FD passes normal+0 to55AF40 and68510B passes buffer+8 to55AF50.
Radar movie start/stop select the movie/normal threshold through55AF40.
Clock sampling, hysteresis and laser suppression belong to the detail/laser
owners and their separate native corpus, not to this reader fixture.
