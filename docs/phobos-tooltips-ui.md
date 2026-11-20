# Phobos uimd.ini 提示框组（2026-11-16）

Phobos 在进程启动时一次读完 `uimd.ini` 的 `[ToolTips]` 与 `[LoadingScreen]` 块
（`src/Phobos.INI.cpp:135-170`）。VERA 的 `uimd.ini` 入口早已存在（超级武器侧栏、
暂停菜单、侧栏指示器、简报对话框），本次把剩下的 7 键并入同一条链，扫描器 GAP 归零。

## 读取

`src/rules/phobos_tooltips_ui.rs` 的 `ToolTipsUi` 拥有这 7 个键，语义与既有 uimd
模块一致：

- `ExtendedToolTips` / `AnchoredToolTips`：bool，默认 no。
- `MaxWidth`：int，默认 0。
- `CostLabel` / `TimeLabel` / `SWShotsFormat`：`ReadString(..., "<none>")`，空白或
  `<none>` 保留字面默认（`$`、`⌚`、`Shots: %d`），其余值按
  `GeneralUtils::LoadStringOrDefault` 走 CSF 标签。
- `DisableEmptySpawnPositions`：`[LoadingScreen]` bool，默认 no。

进程级解析在 `ProcessAssets::load_phobos_tooltips_ui`（启动时 CSF 已装载、档案已挂载），
因为载入界面在匹配呈现层建立之前就要读 `[LoadingScreen]`。

## 消费

- **扩展提示文本**（`app/input/tooltips.rs`）：
  - 技术类或超级武器 cameo 在 `ExtendedToolTips=yes` 时改用
    `PhobosToolTip::HelpText_Techno/Super` 的文本：名称、`CostLabel`+绝对值、
    `TimeLabel`+`MM:SS`、超武的 `SWShotsFormat`，以及（`ToolTipDescriptions` 打开时）
    `UIDescription` 末行。关闭时保留 gamemd 原有文本，且不再附加描述——Phobos 的
    钩子在 `IsEnabled()` 为假时根本不进 `HelpText_*`。
  - 建造时间走 `PhobosToolTip::GetBuildTime`：`max(54, TechnoClass::Time_To_Build)`
    对本地玩家运行完整公式（`sim::production::tooltip_build_time`），再由
    `TickTimeToSeconds` 换算（15 fps 基准；`RealTimeTimers` 时按实测帧率或
    `60/GameSpeed`）。
  - 超武充能来自 `SuperClass::GetRechargeTime`；`SW.Shots` 余量取自 Ares 侧同一
    状态（见残渣）。
- **绘制**（`tooltip_quads`/`build_tooltip_instances`）：
  - cameo 提示的换行宽度改为 `MaxWidth>0 ? MaxWidth : 战术视口宽`（钩子 `0x478EF8`）。
  - `AnchoredToolTips` 且扩展文本与描述均开启时，右侧栏 cameo 固定在侧栏左缘
    `left - 宽 - 2` 并向上一 40 px；左超武栏按钮固定在按钮列右缘 `+2`、按钮顶部
    （`Misc/PhobosToolTip.cpp:391-451`）。
- **载入界面**（`app/loading/composition.rs`）：`DisableEmptySpawnPositions=yes` 时不再
  烧录 gamemd 对每个起点画出的 4x4 黑点（钩子 `0x640B8D` 跳过 `0x640B93` 的黑块循环）；
  带编号的彩色标记保留。

## 残渣

- 已无。此前列在这里的三项都已接线（2026-11-18 核）：
  - **电力读数**：`Simulation::cameo_power_delta`（`src/sim/power_system.rs:692-735`）
    两个臂都在——非建筑臂在 `[General] UnitPowerDrain=yes` 时回类型 `Power=`
    （`:706-713`），建筑臂走 `GetEnhancedPower` + enhancer 金额减去原生 drain
    （`:731-735`）；`PowerLabel` 由侧栏指示器 owner 读取，文本侧
    （`src/app/input/tooltips.rs:669-675`）在成本/时间行后追加
    “`PowerLabel`±N”（`extended_techno_power` 在 `:683-698` 取非零值）。
  - **`SWShotsFormat`**：计数由 `super_tooltip_facts`
    （`src/sim/superweapon/mod.rs:1801-1814`）供给，phobos-only 构建走
    `phobos_super_weapon_metadata` 的 `SW.Shots`（默认 `-1`）与实例的
    发射计数（`ares` 构建走 Ares 元数据）；格式串读取在
    `src/rules/phobos_tooltips_ui.rs`，拼装/追加在 `tooltips.rs:738-751`。
  - **`MaxWidth` 的 5 px 内边距**（钩子 `0x479029`）：`tooltips.rs:1222-1241`
    在 `MaxWidth>0` 时把换行宽度取 `MaxWidth-5`，回归测试在 `:2702`。
- 完整 `CCToolTip::Draw2` 重写（Phobos TODO）不在本条链内。
