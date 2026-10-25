# CnCNet 单人启动适配

让已有的 **YR CnCNet 客户端副本**直接启动支持 `-SPAWN` 的 VERA20k。客户端负责选图和设置，VERA 负责实际游戏。此目录不含客户端程序、原版资源或游戏可执行文件。

支持范围：一个本地玩家与最多七个电脑的 YR Battle 遭遇战；盟军、苏军各七关的 YR 战役，任意关卡和简单／普通／困难；使用 VERA 现有战役机制的普通自定义地图。网上对战、LAN、观战、RA2 本体战役及原版存档／录像不在本适配范围内。客户端界面中隐藏这些主要入口，启动解析器也会拒绝不支持的游戏请求。

默认构建的遭遇战国家编号仍限于原版 0–9。使用 `--features ares` 编译时，`[Settings] Side` 和 `[HouseCountries]` 可携带更多国家的实际注册索引；这些值不是阵营编号或菜单显示顺序，也不会重新抽取随机国家。现有启动流程会按最终加载的 Rules 国家表校验，不存在的索引会被拒绝。战役的 `Side=0/1` 保留盟军／苏军语义。

本目录的配置脚本仍面向标准 YR 客户端，不安装 Mod 或新增其国家选项。扩展国家入场不代表已支持该 Mod 的加载图、侧栏、EVA、战役或所有 Ares／Phobos 功能，也不加载这些扩展的 DLL；资源与游戏机制仍须分别得到 VERA 支持。

## 配置

1. 准备一个**可独立启动的官方 YR CnCNet 客户端副本**，例如 `E:\Games\VERA-CnCNet`。不要直接在原版游戏安装目录或日常使用的联机客户端上修改。关闭这个副本中的所有程序。
2. 准备本次适配版本的 `vera20k.exe`，以及自己的正版《红色警戒 2／尤里的复仇》资源目录。
3. 在 PowerShell 中执行下列命令，先用 `-WhatIf` 查看将要修改的范围。确认路径后移除 `-WhatIf` 再执行一次：

```powershell
& .\Configure-VERA20k.ps1 `
  -ClientDirectory 'E:\Games\VERA-CnCNet' `
  -VeraExecutable 'E:\Builds\vera20k.exe' `
  -RetailDirectory 'D:\steam\steamapps\common\Command & Conquer Red Alert II' `
  -WhatIf
```

4. 从这个副本的 `CnCNetYRLauncher.exe` 打开客户端，选择 **Skirmish** 或 **Campaign**。遭遇战使用 **Battle**；不要将本地玩家设置为观战者。选定战役关卡后，用难度滑块选择难度。

脚本适用于 Windows PowerShell 5.1 及以上，不依赖 Python，也不安装运行库或修改注册表。它复制选定的 VERA 可执行文件到客户端根目录，为 VERA 写入指向正版目录的 `config.toml`；原版资源留在原处。必须使用包含本适配的 VERA 构建，普通旧版程序会拒绝 `-SPAWN`。

客户端启动方式改为 `vera20k.exe -SPAWN`，清空 Syringe／Ares／Spawner／Phobos 注入参数，并禁用 QRes 和单核绑定。`ModMode=true` 关闭这个副本的官方自动更新，避免更新覆盖适配。CnCNet 与 VERA 共用这个客户端副本中的 `RA2MD.INI`／`KeyboardMD.INI`；生成的 `config.toml` 使用 `settings_dir="."`，正版资源目录中的设置不受影响。仅有 VERA 已支持的设置键会生效，客户端中的原版兼容选项不会变成新的 VERA 功能。

## 会修改什么

脚本会在指定客户端内生成以下内容：

- `vera20k.exe`、`config.toml`。
- `Resources/ClientDefinitions.ini`、`Renderers.ini`、`GameOptions.ini`，以及主菜单和选项面板相关 INI。主题主菜单也会同步隐藏不支持的入口。
- `INI/VERA20kMaps.ini`：从现有地图目录筛选 Battle 地图，保留地图描述／预览和起始点信息，只展示 Battle 模式。带有额外强制设置引用的地图不会导入；不会悄悄移除其规则。
- `INI/VERA20kBattles.ini`：14 个官方 YR 关卡的独立目录。地图本体仍从正版 MIX 读取。
- `INI/Map Code/VERA20k.ini` 与 `GlobalCode.ini`：空的规则补丁入口，让普通 Battle 使用 VERA 的零售版规则加载路径。

遭遇战面板只保留初始资金、起始单位、速度、短游戏、MCV 重新部署、可毁桥梁、盟友旁建造、多工程师、箱子和超级武器。客户端生成的 `spawnmap.ini` 保留所选地图本身的规则；此适配不会把自定义地图中的所有扩展键变成 VERA 已支持的机制。

每个原有受管文件先复制到 `VERA20k-client-backup/`，并记录原始及安装后的 SHA-256。`ddraw.dll` 若存在也备份，因为客户端切换到原生渲染配置时可能删除它。脚本拒绝重复覆盖既有备份；它不会清理游戏资源或其他程序。备份仅覆盖本工具管理的配置，不是整个客户端的备份。

## 自定义战役

把普通 YR 战役地图复制到客户端副本中的 `Maps/VERA20kMissions/MyMission.map`，再编辑 `INI/VERA20kBattles.ini`：

```ini
[Battles]
; 保留原有条目，另加一项
14=MY_MISSION

[MY_MISSION]
Scenario=Maps/VERA20kMissions/MyMission.map
Description=My custom mission
Side=0
SideName=Allied
RequiredAddon=true
```

`Side=0` 为盟军，`Side=1` 为苏军；相应地选择 `SideName=Allied` 或 `Soviet`。保存后重启客户端。不要把示例的第二个 `[Battles]` 复制为重复章节，直接向原有章节加入条目即可。

该路径仅提供**地图选择与启动**。地图的 `[Basic]`、触发和资源须已受 VERA 支持。不支持依赖 Spawner `[GameMissionConfig]`／`ReadMissionSection`、Ares／Phobos，或额外自定义美术／声音安装的战役包。多关自定义战役的推进仍遵循 VERA 本身的关卡规则，本工具不承诺任意第三方战役包兼容。

## 还原

关闭客户端和 VERA 后执行：

```powershell
& .\Configure-VERA20k.ps1 -Action Restore -ClientDirectory 'E:\Games\VERA-CnCNet'
```

还原会先核对所有受管文件。若文件在安装后被修改（例如自行加入了战役目录），脚本停止并保留现场，不覆盖用户编辑；请保存改动，再从备份中手动取回相应原文件。自动还原成功后仍保留备份和还原收据，也保留游戏产生的用户设置、日志和存档。再次安装前，需要自行将整个备份目录移到别处，以免覆盖证据。

## 验证边界

已通过脚本语法检查；用官方配置文件构成的独立夹具检查了安装、配置字段及原始备份，初版工具也通过逐字节还原检查。最终安装夹具使用正式版 yr-9.3.5 配置，覆盖中文与空格路径、307 个 Battle 地图、14 个战役关卡、设置目录及安装文件 SHA-256；未执行客户端删除 ddraw.dll 后的还原场景。这只验证安装工具和生成配置；**不能替代实际客户端启动、对局与战役过关检查**。客户端版本不同可能改变控件和配置语义；本适配依据以下源码版本，不保证对所有第三方客户端主题或修改版通用：

- [XNA CnCNet Client 65732ba3](https://github.com/CnCNet/xna-cncnet-client/tree/65732ba38b37c2fdd8639f9abe98f72048007b5e)：[直接进程启动](https://github.com/CnCNet/xna-cncnet-client/blob/65732ba38b37c2fdd8639f9abe98f72048007b5e/ClientGUI/GameProcessLogic.cs)、[战役参数和难度](https://github.com/CnCNet/xna-cncnet-client/blob/65732ba38b37c2fdd8639f9abe98f72048007b5e/DXMainClient/DXGUI/Campaign/CampaignSelector.cs)、[渲染兼容设置默认值](https://github.com/CnCNet/xna-cncnet-client/blob/65732ba38b37c2fdd8639f9abe98f72048007b5e/DXMainClient/Domain/DirectDrawWrapper.cs)。
- [官方客户端 yr-9.3.5](https://github.com/CnCNet/cncnet-yr-client-package/releases/tag/yr-9.3.5)：已核对正式版配置，相关 UI 配置与下列 develop 版本一致，ClientDefinitions 的新增录像字段不影响本适配。
- [官方 YR 客户端包 01fecbdd](https://github.com/CnCNet/cncnet-yr-client-package/tree/01fecbdd1c62cbce49a9aaea1aafefd0a97571ae)：现有客户端布局、国家／颜色 ID、地图目录和关卡标识。

VERA 不输出 CnCNet 原版格式的胜负战绩。配置将战绩读取位置指向专用的空缺日志路径，避免把旧版日志当成 VERA 的结果；不会生成虚假的统计文件。客户端退出回调允许日志不存在，但超过一分钟的遭遇战仍可能在客户端本地留下不完整历史，这不代表胜负或战役进度。

客户端及其配置体系来自 Rampastring、The Dawn of the Tiberium Age 与 CnCNet 项目；本目录提供 VERA20k 适配，不分发或改变这些项目的程序许可。
