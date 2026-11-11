# Ares hooks skipped on purpose

Build with `--features ares`. These Ares 3.0 hooks have no Rust port on
purpose. Each one is
either equivalent to what VERA20K already does or has no counterpart in the
engine. Hooks that a
chain skips for its own reasons are listed in that chain's document, not
here.

## ExtData lifecycle (equivalent)

Allocate, free, save and load the Ares ExtData object beside its gamemd
owner. VERA20K keeps the same state in Rust structs owned by the type or
object, saved through serde and folded into the world hash, so nothing needs
hooking.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x42784B` | `1000A740` | AnimTypeClass_CTOR | `Ext/AnimType/Body.cpp` |
| `0x428800` | `1000A920` | AnimTypeClass_SaveLoad_Prefix | `Ext/AnimType/Body.cpp` |
| `0x42892C` | `1000A790` | AnimTypeClass_Load_Suffix | `Ext/AnimType/Body.cpp` |
| `0x428958` | `1000A790` | AnimTypeClass_Load_Suffix | `Ext/AnimType/Body.cpp` |
| `0x428970` | `1000A920` | AnimTypeClass_SaveLoad_Prefix | `Ext/AnimType/Body.cpp` |
| `0x42898A` | `1000A960` | AnimTypeClass_Save_Suffix | `Ext/AnimType/Body.cpp` |
| `0x428EA8` | `1000A8C0` | AnimTypeClass_SDDTOR | `Ext/AnimType/Body.cpp` |
| `0x43C022` | `10013AC0` | BuildingClass_DTOR | `Ext/Building/Body.cpp` |
| `0x453E20` | `10013BA0` | BuildingClass_SaveLoad_Prefix | `Ext/Building/Body.cpp` |
| `0x45417E` | `10013B90` | BuildingClass_Load_Suffix | `Ext/Building/Body.cpp` |
| `0x454190` | `10013BA0` | BuildingClass_SaveLoad_Prefix | `Ext/Building/Body.cpp` |
| `0x454244` | `10013BE0` | BuildingClass_Save_Suffix | `Ext/Building/Body.cpp` |
| `0x45E50C` | `1000FD10` | BuildingTypeClass_CTOR | `Ext/BuildingType/Body.cpp` |
| `0x45E707` | `1000FD70` | BuildingTypeClass_DTOR | `Ext/BuildingType/Body.cpp` |
| `0x465010` | `1000FF10` | BuildingTypeClass_SaveLoad_Prefix | `Ext/BuildingType/Body.cpp` |
| `0x4652ED` | `1000FDE0` | BuildingTypeClass_Load_Suffix | `Ext/BuildingType/Body.cpp` |
| `0x465300` | `1000FF10` | BuildingTypeClass_SaveLoad_Prefix | `Ext/BuildingType/Body.cpp` |
| `0x46536A` | `1000FF50` | BuildingTypeClass_Save_Suffix | `Ext/BuildingType/Body.cpp` |
| `0x4664BA` | `10019D20` | BulletClass_CTOR | `Ext/Bullet/Body.cpp` |
| `0x4665E9` | `10019D80` | BulletClass_DTOR | `Ext/Bullet/Body.cpp` |
| `0x46AF97` | `10019DB0` | BulletClass_Load_Suffix | `Ext/Bullet/Body.cpp` |
| `0x46AF9E` | `10019DB0` | BulletClass_Load_Suffix | `Ext/Bullet/Body.cpp` |
| `0x46AFB0` | `10019EB0` | BulletClass_SaveLoad_Prefix | `Ext/Bullet/Body.cpp` |
| `0x46AFC4` | `10019EF0` | BulletClass_Save_Suffix | `Ext/Bullet/Body.cpp` |
| `0x46BDD9` | `10019820` | BulletTypeClass_CTOR | `Ext/BulletType/Body.cpp` |
| `0x46C722` | `10019870` | BulletTypeClass_Load_Suffix | `Ext/BulletType/Body.cpp` |
| `0x46C730` | `100198E0` | BulletTypeClass_SaveLoad_Prefix | `Ext/BulletType/Body.cpp` |
| `0x46C74A` | `10019920` | BulletTypeClass_Save_Suffix | `Ext/BulletType/Body.cpp` |
| `0x46C8B6` | `100198B0` | BulletTypeClass_SDDTOR | `Ext/BulletType/Body.cpp` |
| `0x46CC03` | `1001AFB0` | CampaignClass_CTOR | `Ext/Campaign/Body.cpp` |
| `0x46CC36` | `1001B020` | CampaignClass_DTOR | `Ext/Campaign/Body.cpp` |
| `0x46CF3D` | `1001AFB0` | CampaignClass_CTOR | `Ext/Campaign/Body.cpp` |
| `0x46D0B6` | `1001B020` | CampaignClass_DTOR | `Ext/Campaign/Body.cpp` |
| `0x511635` | `1001D0A0` | HouseTypeClass_CTOR_1 | `Ext/HouseType/Body.cpp` |
| `0x511643` | `1001D0A0` | HouseTypeClass_CTOR_2 | `Ext/HouseType/Body.cpp` |
| `0x512290` | `1001D210` | HouseTypeClass_SaveLoad_Prefix | `Ext/HouseType/Body.cpp` |
| `0x51246D` | `1001D130` | HouseTypeClass_Load_Suffix | `Ext/HouseType/Body.cpp` |
| `0x512480` | `1001D210` | HouseTypeClass_SaveLoad_Prefix | `Ext/HouseType/Body.cpp` |
| `0x51255C` | `1001D250` | HouseTypeClass_Save_Suffix | `Ext/HouseType/Body.cpp` |
| `0x5127CF` | `1001D0D0` | HouseTypeClass_DTOR | `Ext/HouseType/Body.cpp` |
| `0x62FF20` | `10028E80` | ParticleSystemClass_SaveLoad_Prefix | `Ext/ParticleSystem/Body.cpp` |
| `0x630088` | `10028D70` | ParticleSystemClass_Load_Suffix | `Ext/ParticleSystem/Body.cpp` |
| `0x6300F3` | `10028EC0` | ParticleSystemClass_Save_Suffix | `Ext/ParticleSystem/Body.cpp` |
| `0x644DBB` | `10029210` | ParticleTypeClass_CTOR | `Ext/ParticleType/Body.cpp` |
| `0x645660` | `100293A0` | ParticleTypeClass_SaveLoad_Prefix | `Ext/ParticleType/Body.cpp` |
| `0x64578C` | `10029260` | ParticleTypeClass_Load_Suffix | `Ext/ParticleType/Body.cpp` |
| `0x6457A0` | `100293A0` | ParticleTypeClass_SaveLoad_Prefix | `Ext/ParticleType/Body.cpp` |
| `0x64580A` | `100293E0` | ParticleTypeClass_Save_Suffix | `Ext/ParticleType/Body.cpp` |
| `0x645A3B` | `10029340` | ParticleTypeClass_SDDTOR | `Ext/ParticleType/Body.cpp` |
| `0x667A30` | `1002B6E0` | RulesClass_DTOR | `Ext/Rules/Body.cpp` |
| `0x674730` | `1002B780` | RulesClass_SaveLoad_Prefix | `Ext/Rules/Body.cpp` |
| `0x675205` | `1002B7A0` | RulesClass_Save_Suffix | `Ext/Rules/Body.cpp` |
| `0x678841` | `1002B700` | RulesClass_Load_Suffix | `Ext/Rules/Body.cpp` |
| `0x6CE6F6` | `10034550` | SuperWeaponTypeClass_CTOR | `Ext/SWType/Body.cpp` |
| `0x6CE800` | `10034720` | SuperWeaponTypeClass_SaveLoad_Prefix | `Ext/SWType/Body.cpp` |
| `0x6CE8BE` | `100345E0` | SuperWeaponTypeClass_Load_Suffix | `Ext/SWType/Body.cpp` |
| `0x6CE8D0` | `10034720` | SuperWeaponTypeClass_SaveLoad_Prefix | `Ext/SWType/Body.cpp` |
| `0x6CE8EA` | `10034760` | SuperWeaponTypeClass_Save_Suffix | `Ext/SWType/Body.cpp` |
| `0x6CEFE0` | `100346E0` | SuperWeaponTypeClass_SDDTOR | `Ext/SWType/Body.cpp` |
| `0x6A4780` | `1002F150` | SideClass_SaveLoad_Prefix | `Ext/Side/Body.cpp` |
| `0x6A488B` | `1002F010` | SideClass_Load_Suffix | `Ext/Side/Body.cpp` |
| `0x6A48A0` | `1002F150` | SideClass_SaveLoad_Prefix | `Ext/Side/Body.cpp` |
| `0x6A48FC` | `1002F190` | SideClass_Save_Suffix | `Ext/Side/Body.cpp` |
| `0x6A499F` | `1002F110` | SideClass_SDDTOR | `Ext/Side/Body.cpp` |
| `0x71E7F8` | `100500F0` | TEventClass_CTOR | `Ext/TEvent/Body.cpp` |
| `0x71F8C0` | `10050280` | TEventClass_SaveLoad_Prefix | `Ext/TEvent/Body.cpp` |
| `0x71F92B` | `10050150` | TEventClass_Load_Suffix | `Ext/TEvent/Body.cpp` |
| `0x71F930` | `10050280` | TEventClass_SaveLoad_Prefix | `Ext/TEvent/Body.cpp` |
| `0x71FAA6` | `10050250` | TEventClass_SDDTOR | `Ext/TEvent/Body.cpp` |
| `0x70C249` | `10047030` | TechnoClass_Load_Suffix | `Ext/Techno/Body.cpp` |
| `0x70C250` | `10047110` | TechnoClass_SaveLoad_Prefix | `Ext/Techno/Body.cpp` |
| `0x70C264` | `10047150` | TechnoClass_Save_Suffix | `Ext/Techno/Body.cpp` |
| `0x711835` | `10040D50` | TechnoTypeClass_CTOR | `Ext/TechnoType/Body.cpp` |
| `0x711AE0` | `10040DB0` | TechnoTypeClass_DTOR | `Ext/TechnoType/Body.cpp` |
| `0x716DAC` | `10040E20` | TechnoTypeClass_Load_Suffix | `Ext/TechnoType/Body.cpp` |
| `0x716DC0` | `10040F20` | TechnoTypeClass_SaveLoad_Prefix | `Ext/TechnoType/Body.cpp` |
| `0x717094` | `10040F60` | TechnoTypeClass_Save_Suffix | `Ext/TechnoType/Body.cpp` |
| `0x721876` | `100509C0` | TiberiumClass_CTOR | `Ext/Tiberium/Body.cpp` |
| `0x72193A` | `100509E0` | TiberiumClass_DTOR | `Ext/Tiberium/Body.cpp` |
| `0x72208C` | `10050A30` | TiberiumClass_Load_Suffix | `Ext/Tiberium/Body.cpp` |
| `0x7220D0` | `10050B70` | TiberiumClass_SaveLoad_Prefix | `Ext/Tiberium/Body.cpp` |
| `0x75D1A9` | `10052E60` | WarheadTypeClass_CTOR | `Ext/WarheadType/Body.cpp` |
| `0x75E0C0` | `100530D0` | WarheadTypeClass_SaveLoad_Prefix | `Ext/WarheadType/Body.cpp` |
| `0x75E2AE` | `10052EF0` | WarheadTypeClass_Load_Suffix | `Ext/WarheadType/Body.cpp` |
| `0x75E2C0` | `100530D0` | WarheadTypeClass_SaveLoad_Prefix | `Ext/WarheadType/Body.cpp` |
| `0x75E39C` | `10053110` | WarheadTypeClass_Save_Suffix | `Ext/WarheadType/Body.cpp` |
| `0x75E5C8` | `10052FF0` | WarheadTypeClass_SDDTOR | `Ext/WarheadType/Body.cpp` |
| `0x771EE9` | `10055D30` | WeaponTypeClass_CTOR | `Ext/WeaponType/Body.cpp` |
| `0x772CD0` | `10055FD0` | WeaponTypeClass_SaveLoad_Prefix | `Ext/WeaponType/Body.cpp` |
| `0x772EA6` | `10055DC0` | WeaponTypeClass_Load_Suffix | `Ext/WeaponType/Body.cpp` |
| `0x772EB0` | `10055FD0` | WeaponTypeClass_SaveLoad_Prefix | `Ext/WeaponType/Body.cpp` |
| `0x772F8C` | `10055EF0` | WeaponTypeClass_Save | `Ext/WeaponType/Body.cpp` |
| `0x77311D` | `10055EC0` | WeaponTypeClass_SDDTOR | `Ext/WeaponType/Body.cpp` |

## ExtData reader entry points (equivalent)

Run the ExtData `LoadFromINI` after the native read. VERA20K runs the
matching `src/rules/ares_*.rs` reader at the same point of its type pass;
every key they read is covered.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x46C41C` | `10019840` | BulletTypeClass_LoadFromINI | `Ext/BulletType/Body.cpp` |
| `0x46C429` | `10019840` | BulletTypeClass_LoadFromINI | `Ext/BulletType/Body.cpp` |
| `0x46CD56` | `1001B050` | CampaignClass_LoadFromINI | `Ext/Campaign/Body.cpp` |
| `0x6CEE50` | `100345B0` | SuperWeaponTypeClass_LoadFromINI | `Ext/SWType/Body.cpp` |
| `0x75DEAF` | `10052EC0` | WarheadTypeClass_LoadFromINI | `Ext/WarheadType/Body.cpp` |

## Extension teardown (equivalent)

Free per-object Ares state when its ion blast, bolt, bomb or wave is
deleted. Rust ownership drops that state with its owner.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x53CB91` | `10053EF0` | IonBlastClass_DTOR | `Ext/WarheadType/Hooks.Ion.cpp` |
| `0x53CC0D` | `10054190` | IonBlastClass_Update_DTOR | `Ext/WarheadType/Hooks.Ion.cpp` |
| `0x4C2951` | `10056130` | EBolt_DTOR | `Ext/WeaponType/Hooks.Bolt.cpp` |
| `0x4393F2` | `100569F0` | BombClass_SDDTOR | `Ext/WeaponType/Hooks.IvanBomb.cpp` |
| `0x763226` | `10056F40` | WaveClass_DTOR | `Ext/WeaponType/Hooks.Wave.cpp` |

## Side save and load (not applicable)

Write and skip Ares' SideExt vector around gamemd's save stream. VERA20K's
save format is its own.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x67D315` | `1002F8B0` | SaveGame_EarlySaveSides | `Ext/Side/Hooks.cpp` |
| `0x67E09A` | `1002F8D0` | SaveGame_LateSkipSides | `Ext/Side/Hooks.cpp` |
| `0x67E74A` | `1002F7A0` | LoadGame_EarlyLoadSides | `Ext/Side/Hooks.cpp` |
| `0x67F281` | `1002F800` | LoadGame_LateSkipSides | `Ext/Side/Hooks.cpp` |

## Win32 front end (not applicable)

Main menu, single player and movie dialog buttons, owner-draw control
colours from uimd.ini, dialog resources and subtext, the movies list and the
version text. VERA20K draws its own front end and has none of these gamemd
dialogs.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x52D713` | `10065DB0` | SinglePlayer_hDlg_SkirmishButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x52D724` | `10065D80` | SinglePlayer_hDlg_CampaignButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x52D7ED` | `10065D20` | MoviesAndCredits_hDlg_SneakPeeksButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x52D7FB` | `10065CF0` | MoviesAndCredits_hDlg_PlayMoviesButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x52D809` | `10065D50` | MoviesAndCredits_hDlg_ViewCreditsButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x52DDBA` | `10065BD0` | Frontend_WndProc_MessageBox | `Misc/Interface.Buttons.cpp` |
| `0x52E446` | `10065BA0` | Frontend_WndProc_JustAfterAction | `Misc/Interface.Buttons.cpp` |
| `0x532051` | `10065C60` | Main_hDlg_NetworkButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x53208D` | `10065C90` | Main_hDlg_SinglePlayerButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x5320AE` | `10065C30` | Main_hDlg_MoviesAndCreditsButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x5320C2` | `10065CC0` | Main_hDlg_WWOnlineButtonClick | `Misc/Interface.Buttons.cpp` |
| `0x60DDA6` | `10067480` | Handle_Combobox_Dropdown_Messages_SelectionA | `Misc/Interface.UIColors.cpp` |
| `0x60DDB6` | `100674A0` | Handle_Combobox_Dropdown_Messages_SelectionB | `Misc/Interface.UIColors.cpp` |
| `0x60FAD7` | `10067680` | Ownerdraw_PostProcessColors | `Misc/Interface.UIColors.cpp` |
| `0x612DA9` | `10067400` | Handle_Button_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x614FF2` | `100675C0` | Handle_NewEdit_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x615AB7` | `10067660` | Handle_Static_Messages_Disabled | `Misc/Interface.UIColors.cpp` |
| `0x615DF7` | `10067640` | Handle_Static_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x61664C` | `10067440` | Handle_Checkbox_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x616655` | `10067460` | Handle_Checkbox_Messages_Disabled | `Misc/Interface.UIColors.cpp` |
| `0x616AF0` | `100675E0` | Handle_RadioButton_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x617A2B` | `100674C0` | Handle_Combobox_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x617A57` | `100674E0` | Handle_Combobox_Messages_Disabled | `Misc/Interface.UIColors.cpp` |
| `0x619270` | `10067580` | Handle_Listbox_Messages_SelectionA | `Misc/Interface.UIColors.cpp` |
| `0x619288` | `100675A0` | Handle_Listbox_Messages_SelectionB | `Misc/Interface.UIColors.cpp` |
| `0x6198D3` | `10067540` | Handle_Listbox_Messages_DisabledA | `Misc/Interface.UIColors.cpp` |
| `0x619A4F` | `10067520` | Handle_Listbox_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x619A5F` | `10067560` | Handle_Listbox_Messages_DisabledB | `Misc/Interface.UIColors.cpp` |
| `0x61E2A5` | `10067600` | Handle_Slider_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x61E8A0` | `10067500` | Handle_GroupBox_Messages_Color | `Misc/Interface.UIColors.cpp` |
| `0x52C939` | `1006A0B0` | InitGame_MoviesList | `Misc/MoviesList.cpp` |
| `0x5FAFFB` | `1006A0C0` | Options_SaveToINI_MoviesList | `Misc/MoviesList.cpp` |
| `0x52CA37` | `1006B310` | InitGame_Delay | `Misc/Status.cpp` |
| `0x531413` | `1006B140` | Game_Start | `Misc/Status.cpp` |
| `0x532017` | `1006B0D0` | DlgProc_MainMenu_Version | `Misc/Status.cpp` |
| `0x74FAE0` | `1006B2D0` | GetModuleInternalVersion | `Misc/Status.cpp` |
| `0x74FDC0` | `1006B2F0` | GetModuleVersion | `Misc/Status.cpp` |
| `0x4A3B4B` | `10075E70` | FetchResource | `UI/Dialogs.cpp` |
| `0x60411B` | `10075EC0` | Game_DialogFunc_Subtext_Load | `UI/Dialogs.cpp` |
| `0x604136` | `10075F30` | Game_DialogFunc_Subtext_Propagate | `UI/Dialogs.cpp` |

## DirectDraw surfaces (not applicable)

Surface creation, the VRAM and WOL resolution checks, and a blitter buffer.
VERA20K renders through its own GPU path.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x437CCC` | `1006BB70` | BSurface_DrawSHPFrame1_Buffer | `Misc/Surfaces.cpp` |
| `0x4BA61B` | `1006BBD0` | DSurface_CTOR_SkipVRAM | `Misc/Surfaces.cpp` |
| `0x78997B` | `1006C000` | sub_789960_RemoveWOLResolutionCheck | `Misc/Surfaces.cpp` |
| `0x7C89D4` | `1006BBE0` | DirectDrawCreate | `Misc/Surfaces.cpp` |

## Dropship loadout (not applicable)

Tiberian Sun's dropship loadout screen; no YR scenario opens it.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x4B9A52` | `10006EF0` | DropshipLoadout_PrintArmor | `Enum/ArmorTypes.cpp` |
| `0x4B769B` | `1002CCA0` | ScenarioClass_GenerateDropshipLoadout | `Ext/Rules/Hooks.Dropships.cpp` |
| `0x4B93BD` | `1002CCC0` | ScenarioClass_GenerateDropshipLoadout_FreeAnims | `Ext/Rules/Hooks.Dropships.cpp` |
| `0x4B99A2` | `1002CA30` | DropshipLoadout_WriteUnit | `Ext/Rules/Hooks.Dropships.cpp` |

## EVA voice table teardown (equivalent)

Free Ares' extra EVA voice table at exit; VERA20K's EVA tables are owned
Rust values.

| Hook | Ares.dll | Name | Source |
|---|---|---|---|
| `0x7531CF` | `100638D0` | VoxClass_DeleteAll | `Misc/EVAVoices.cpp` |
