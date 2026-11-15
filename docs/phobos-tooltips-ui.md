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

- 扩展文本的**电力读数**未实现：`GetPower` 的建筑臂需要实时
  `PowerPlantEnhancer` 结算，单位臂需要尚未移植的 `UnitPowerDrain`；
  `PowerLabel` 键本身由侧栏指示器 owner 读取。
- `SWShotsFormat` 的余量依赖 `SW.Shots` 状态；VERA 的该状态目前在 Ares 包
  （`AresSuperWeaponAvailability` + 每实例 `ares_launch_count`）。仅开 phobos
  构建没有该计数，射击行保持不显示。
- `0x479029` 在 `MaxWidth>0` 时把绘制右内边距减 5；VERA 的文字内边距是固定常量，
  这 5 px 目前不生效。
- 完整 `CCToolTip::Draw2` 重写（Phobos TODO）不在本条链内。
