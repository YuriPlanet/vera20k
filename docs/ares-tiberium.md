# Optional Ares tiberium, ore and harvesters

Build with `--features ares` to let each `[Tiberiums]` type choose its
damage, healing and chain-reaction blast, to make objects take damage from,
heal in, spill and leave tiberium (and die into visceroids), to make
`ChainReaction=yes` ore explode, to tune harvester scans per type, and to
let refineries store ore in silos (`Refinery.UseStorage`).
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
SmallVisceroid=VISC_SML        ; stock key: what a tiberium death becomes
TiberiumTransmogrify=40        ; stock key: its percent chance
[CombatDamage]
TiberiumExplosive=yes          ; stock key, now a harvester blast
TiberiumExplosiveWarhead=HE    ; the blast's warhead (none)
[AudioVisual]
TiberiumExplosiveAnim=TWLT070  ; played at the blast (none)
[CrateRules]
RandomCrateMoney=900           ; Money crate top-up range
[TIB01]                        ; any tiberium [OverlayTypes] entry
ChainReaction=yes              ; stock key, now a chain-reacting cell
; map [Basic] TiberiumDeathToVisceroid=no turns the visceroid off (yes)

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
[GAREFN]                       ; any refinery (or slave miner)
Refinery.UseStorage=yes        ; raw ore goes into the House's silos (no)
TiberiumSpill=yes              ; its destruction spills the store too
[General]
Message.SilosNeeded=MSG:Silos  ; read; its print is a client residual
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
- `sim/ares_tiberium.rs`: the tiberium-death visceroid
  (`TechnoExt::SpawnVisceroid @ 100468B0`, from `10049280`): a set
  `[General] SmallVisceroid` (published by `RulesTypeReferences`) spawns
  for the Neutral country's House at the dead Foot's GetCoords after one
  `RandomRanged(0, 99) < TiberiumTransmogrify` draw, unless a unit holds
  the cell. It needs the map's `[Basic] TiberiumDeathToVisceroid`
  (Scenario+0x34B3, `Read_INI_Basic @ 0x00689E90`), which
  `Set_Defaults @ 0x00683610` resets to **true**; `ScenarioSession` keeps
  it (Ares builds only, saved and hashed when false). Retail's
  `TiberiumTransmogrify=40` and `SmallVisceroid=VISC_SML` make this live
  once a mod sets `TiberiumDamageEnabled=yes`.
- `sim/ares_tiberium.rs`: `CellChainReact` (`10050DD0`), Ares' body for
  the stock RET at `CellChainReaction__Request @ 0x00489270`. A
  `ChainReaction=yes` tiberium cell at density 2+ reacts with
  `RandomRanged(0, 99) < 5 * density`, loses half its density, plays the
  ExplosionWarhead's damage anim for `4 * Power * loss` and deals
  `Power * loss` as Apply_area_damage (tiberium untouched), seeds delayed
  `INVISO` anims on dense neighbours (80%, `RandomRanged(15, 120)`), and a
  full-grown cell re-registers for growth (`AddToGrowthQueue @
  0x007235A0`). Callers: Apply_area_damage's spread walk ahead of the
  cell's Reduce_Tiberium (`DamageArea_ChainReaction`, `10051000`, hook
  `48964F`, `combat/world_receiver.rs collect_area`), the SpawnsTiberium
  terrain death (`0x0071BACD`, same file) and the wave cell tail
  (`0x0075F44A..0x0075F45F`, `world/mod.rs`). Retail has no
  `ChainReaction=yes` tiberium overlay.

- `sim/ares_tiberium_storage.rs`: refinery storage. `AresTiberiumStorage`,
  kept in each House's wallet (`sim/economy.rs`), owns the House store
  (`+0x2FC`) and the store (`+0x33C`) of each of its in-game buildings;
  Limbo hands a building's store back to its `BuildingStorage`, whose art
  (refinery tiers, `SiloDamage=` frames) reads the ledger while the House
  holds it. `TechnoExt::DepositTiberium @ 10044230` replaces both refinery
  payments (`miner::pay_refinery_owner`: the Unit unload, hook `73E4A2`,
  and the slave's `RefineTiberium @ 100461B0`, hook `522D75`): the purifier
  bonus and, without `Refinery.UseStorage`, the raw amount pay
  `ftol(amount * Value * IncomeMult)` cash through Add_Credits, with **no
  score** for any refinery; a `UseStorage` refinery hands the raw amount to
  `GiveTiberium`, which Ares' `10024290` (hook `4F9610`) makes: score
  `ftol(amount * 5)`; a campaign or human House fills its on-map `Storage=`
  buildings in House+68 order up to the free capacity (`+0x310` minus the
  stored total; the rest is lost), any other House takes cash. Ares builds
  also port the stock arms reading the stores, absent from default builds
  whose stores stay empty: Spend_Money's drain (`0x004F97DD..0x004F9925`,
  single units at `ftol(Value * IncomeMult)` each, the excess back as the
  balance), Available_Money's `GetTotalAsInt(+0x2FC) * IncomeMult`
  (`0x004F6990`, `0x006C9600`), the Added_To_Game merge (`0x00502B83`), the
  Removed_From_Game withdrawal (`0x00502801..0x00502821`, called by Limbo
  and ChangeOwner), and the sale's store payout through GiveTiberium
  (`0x0044A232..0x0044A285`, `production/production_sell.rs`). A destroyed
  building loses `ceil` of each slot from its store and the House's
  (`10023DA0`, hook `441B30`, `combat/destruction_effects.rs`); with
  `TiberiumSpill` each unit draws `RandomRanged(256, 768)` and one
  direction draw (`0x0049F420`, cell centred) and grows one tiberium of
  its slot there. `UpdateAllSilos @ 0x004F9970` only marks `SiloDamage=`
  buildings for redraw. The silo nag (`10024AE0`, hook `4F8C23`,
  `sim/house_eva.rs`): with the speak timer expired, a capacity above 50
  within 30 of its stored total speaks `EVA_SilosNeeded` and re-arms the
  timer. No retail refinery sets `Refinery.UseStorage`, but every Ares
  build's refinery deposit now scores nothing.

## Hook dispositions

| Hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| `489270` | `10050DD0` | CellChainReact | Ported (`ares_cell_chain_react`) |
| `48964F` | `10051000` | DamageArea_ChainReaction | Ported (`ares_area_damage_chain_reaction`) |
| `739F21` | `1004C530` | UnitClass_UpdatePosition_Visceroid | Residual: needs stock visceroid AI |
| `4F9610` | `10024290` | HouseClass_GiveTiberium_Storage | Ported (`ares_give_tiberium`) |
| `73E4A2` | `10024BC0` | UnitClass_Mi_Unload_Storage | Ported (`ares_deposit_tiberium`) |
| `522D75` | `10024B60` | InfantryClass_Slave_UnloadAt_Storage | Ported (`ares_deposit_tiberium`) |
| `441B30` | `10023DA0` | BuildingClass_Destroy_Refinery | Ported (`ares_destroy_refinery_storage`) |
| `4F8C23` | `10024AE0` | HouseClass_Update_SilosNeededEVA | Ported EVA line; message print residual (client) |
| `44D755` | `10013FD0` | BuildingClass_GetPipFillLevel_Tiberium | Residual: storage pips (client) |
| `709B4E` | `100148B0` | TechnoClass_DrawPipscale_SkipSkipTiberium | Residual: storage pips (client) |

## Not covered

- Visceroid merge (`UnitClass_UpdatePosition_Visceroid`, `1004C530`, hook
  `739F21` in `UnitClass::PerCellProcess @ 0x00739EC0`): a SmallVisceroid
  arriving in the cell of the small visceroid it is heading for (its
  Destination) turns that one into `[General] LargeVisceroid` at full
  Strength and UnInits itself. Only stock `UnitClass__UpdateVisceroid`
  (called from `UnitClass::AI @ 0x007360C0`) gives a visceroid such a
  Destination, and VERA has no port of it, so the merge could never run.
  Retail defines no `SmallVisceroid=yes` unit.
- Storage pips (client): `10013FD0` (hook `44D755`) fills a building's
  tiberium pips from its store over `Storage=`, or from the House's
  `GetStoragePercentage @ 0x004F6E70` for a type without Storage; `100148B0`
  (hook `709B4E`) draws the tiberium pip scale on a Refinery or
  ResourceDestination with `Storage=` only with `Refinery.UseStorage`
  (other buildings always). VERA's pip drawing (`app/presentation/
  ui_overlays.rs`, stock `0x00709B61`) skips building tiberium pips and
  the base fill reads cargo bales. Visible only with UseStorage.
- `Message.SilosNeeded` is read but not printed: the nag needs a sim
  event carrying a message line in the House's colour (MessageDelay) to
  the client's message list; only the EVA line plays.
- `[Basic] FillSilos=` (`Fill_In_Data 0x00684F02`) still pays cash in
  Ares builds; natively its Add_Tiberium_Credits is Ares' `10024290`,
  which stores each unit for a campaign or human House, and its loop's
  Available_Storage (`0x004F69D0`) then sees the fill. No stock map sets
  the key.
- Removed_From_Game's `keepTiberium` arm (cashes a store out without
  touching the House store) has no VERA caller; the `Weeder=` store
  (`+0x314`, `0x004F9700`) is not ported.
- A slot index past 4 (a fifth `[Tiberiums]` type) indexes past the
  four-slot store natively; VERA stores none of it (cash Houses are paid).
- A heal delay below one frame divides by zero natively (a crash); VERA
  heals nothing.
- SpawnVisceroid keeps an object whose Put fails (3.0 leaks it); VERA
  deletes it. An explicit `ExplosionWarhead=none` hands CellChainReact's
  anim selector and Apply_area_damage a null warhead natively; VERA plays
  and deals nothing.
