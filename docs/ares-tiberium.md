# Optional Ares tiberium, ore and harvesters

Build with `--features ares` to let each `[Tiberiums]` type choose its
damage, healing and chain-reaction blast, to make objects take damage from,
heal in, spill and leave tiberium, and to tune harvester scans per type.
Default builds keep the stock mineral Anim.Start (`0x00424CE0`),
Apply_area_damage Explodes tail (`0x0048A2D9`), UnitClass::Death_Explosion
(`0x00738680`), Mission_Harvest and slave-miner scans; INI keys cannot
enable the extension there.

```ini
[General]
TiberiumDamageEnabled=yes      ; Foot objects take damage on tiberium (no)
TiberiumHealEnabled=yes        ; TiberiumHeal= types heal in tiberium (no)
OverlayExplodeThreshold=50     ; Explodes= overlays need this damage (0)
RepairStopOnInsufficientFunds=no ; a broke human repair stays on (yes)
[CombatDamage]
TiberiumExplosive=yes          ; stock key, now a harvester blast
TiberiumExplosiveWarhead=HE    ; the blast's warhead (none)
[AudioVisual]
TiberiumExplosiveAnim=TWLT070  ; played at the blast (none)
[CrateRules]
RandomCrateMoney=900           ; Money crate top-up range

[Cruentus]                     ; any [Tiberiums] type
Damage=7                       ; else max(Power / 10, 1)
Warhead=TIBWH                  ; else C4Warhead
Heal.Step=5                    ; also Heal.IStep (infantry), Heal.UStep
Heal.Delay=0.02                ;   (units); else repair step, TiberiumHeal
ExplosionDamage=40             ; mineral anim blast, else
ExplosionWarhead=HE            ;   TiberiumExplosionDamage / C4Warhead
Debris.Chance=33               ; percent chance of a Debris anim (33)

[HARV]                         ; any TechnoType
TiberiumProof=no               ; default: all but infantry are proof
TiberiumRemains=yes            ; default: TiberiumHeal= with HealEnabled
TiberiumSpill=yes              ; a dying non-building spills its load
TiberiumTransmogrify=40        ; else [General]TiberiumTransmogrify
Harvester.LongScan=20          ; cells; also ShortScan, ScanCorrection
Harvester.TooFarDistance=8     ; cells
Harvester.KickDelay=200        ; frames; negative never kicks
```

## Where VERA20K owns it

- `rules/ares_tiberium.rs`: TiberiumExt (`ExtMap.LoadFromINI @ 100508D0`,
  every reached `TiberiumClass::ReadINI` exit, retained per pass on the
  processed type), its getters (`100507A0..100508B0`), the RulesExt globals
  (`LoadBeforeTypeData @ 1002AB60`) and the TechnoTypeExt keys
  (`1003DFB0`); warheads and anims are Find-only reads. The stock
  `Power=`, `[General]TiberiumHeal`, `TiberiumTransmogrify` (zeroed by
  `1002C990`), `[CombatDamage]TiberiumExplosive` and TechnoType
  `TiberiumHeal=` are read in Ares builds only. `RandomCrateMoney` and
  `RepairStopOnInsufficientFunds` sit with the other RulesExt controls in
  `rules/native_processing.rs`.
- `sim/anim_class.rs`: the mineral Start's `Debris.Chance` draw
  (`10050D50`, hook `424DD3`) and its `ExplosionDamage`/`ExplosionWarhead`
  (`10050DA0`, hook `424EC5`).
- `sim/combat/world_receiver.rs`: `OverlayExplodeThreshold` (`1002C8E0`,
  hook `48A2D9`); `TiberiumSpill` (`1004AEC0`, hook `702200`) and
  `TiberiumRemains` (`1004B050`, hook `702216`) in the ReceiveDamage kill
  arm, through `sim/ares_tiberium.rs`.
- `sim/ares_tiberium.rs`: the Foot damage (`10049280`, hook `4D85E4`, from
  `FootClass::Per_Cell_Process`), the Foot heal (`10014BA0`, hook `4DA53E`,
  from the Foot AI after TechnoAI) and the TiberiumExplosive blast
  (`1004BD90`, hook `738749`, from `Death_Explosion`). Tiberium is placed
  through `Simulation::increase_tiberium_cells`, the live map's
  `CellClass::IncreaseTiberium @ 0x00487190` the Tiberium crate also uses.
- `sim/miner/`: `Harvester.*` replace the Rules scans in Mission_Harvest
  (`1004D640`, `1004D680`, `1004D6D0`, `1004D710`) and the skipped Dock=
  loop (`1004D6C0`, hook `73E66D`); `sim/slave_manager.rs` the slave-miner
  scans and kick delays (`1004D3F0..1004D5F0`).
- `sim/crates/effects.rs`: RandomCrateMoney (`1002C8A0`, hook `48248D`);
  `sim/production/production_repair.rs`: RepairStopOnInsufficientFunds
  (`10014630`, hook `4509B4`).

## Not covered

- `CellChainReact` (`10050DD0`, replacing the stock RET at `0x00489270`,
  called from Ares' hook `48964F` in Apply_area_damage and from `71BAD5`
  and `75F45F`): a `ChainReaction=yes` tiberium overlay blasts with its
  ExplosionWarhead and seeds delayed `INVISO` anims. No retail overlay sets
  ChainReaction; VERA keeps the stock RET.
- The visceroid a tiberium death spawns (`SpawnVisceroid @ 100468B0`):
  it needs the map's `[Basic]TiberiumDeathToVisceroid`, which VERA does not
  read, so the unset arm runs (no draw, no visceroid).
- `Refinery.UseStorage` with its storage hooks (`4F9610`, `73E4A2`,
  `522D75`, `44D755`, `709B4E`) and the refinery spill on destruction
  (`441B30`): VERA keeps no building or house tiberium storage, so a
  refinery pays cash as in stock and has nothing to spill.
- A heal delay below one frame divides by zero natively (a crash); VERA
  heals nothing.
