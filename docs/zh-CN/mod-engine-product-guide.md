---
layout: default
title: VERA Mod Engine 产品说明书
lang: zh-CN
---

# VERA Mod Engine 产品说明书

VERA Mod Engine 是 YuriPlanet 面向《红色警戒 2》MOD 和任务地图制作的 VERA20K 开发分支。它的设计目标是让 RA2、尤里的复仇（YR）和 Ares 三套预设从同一套引擎派生。项目随附的预设提供基础规则，玩家的游戏安装只读提供素材和选用的原版地图。制作组可以用 INI 在所选预设上配置单位运动、弹道、附加效果（AE）和地图触发。

**当前状态为设计草案。** 仓库已收录 VERA 源码、玩法规格、实施路线图和三份 INI 来源索引。当前客户端沿用 VERA20K 的零售 INI 加载路径。随附预设、下文的具名节与导入报告均属计划交付范围。引擎代码沿用上游 GPLv3 许可证及附加条款。[现有规则加载入口](../../src/rules/retail_sources.rs#L234) · [源码起点](https://github.com/YuriPlanet/vera20k/commit/4b9af402654fd61ca6338aca9502071e6e2d4063) · [许可证](../../LICENSE-GPL) · [附加条款](../../LEGAL.md#additional-terms)

## 选择基础预设

| 制作目标 | 项目随附的基础预设 | 验收基准 |
|---|---|---|
| 延续 RA2 的国家、科技树和任务 | `ra2` 提供 RA2 的单位、武器、AI、美术资源绑定和任务指令表。自定义 MOD 可在其上选用已接入的扩展玩法 | 用 RA2 原版程序和相同地图核对。给自定义坦克加减速 AE，另开纯 RA2 局检查原有单位速度 |
| 制作 YR 遭遇战或战役 | `yr` 提供 YR 的基础定义、模式规则和任务指令表。所选 MOD、模式和地图的显式覆盖依该版本的次序生效 | 优先验证零售 YR 的遭遇战和战役，包括任务胜负、援军、存读档 |
| 迁移既有 Ares MOD | 相应版本的 Ares 预设复用随附的 YR 定义，加上该版新增的标签、事件、动作及默认生效的行为修订 | 以 YR 1.001 加对应版本的 Ares 为参照。3.0 与 3.0p1 分别记录差异 |

Phobos 弹道和 AE、KratosPP 的单位与弹体挂载提供额外的旧写法和行为参考。每项兼容都需注明目标版本和已验证场景。第三方地图与战役采用逐项导入报告。[INI 兼容性手册](ini-preset-wrapping.md)

## 一局游戏的数据来源

| 来源 | 加载内容 |
|---|---|
| 项目随附的版本预设 | 项目维护的原型、默认值、旧编号、AI 和素材绑定取代原版基础 INI。以 `Base=yr` 为例，灰熊主炮 `yr.Weapon.105mm` 的基础定义来自 YR 预设。正常开局无需从安装目录或 MIX 查找、解包或转换 `rulesmd.ini`、`artmd.ini`、`aimd.ini`，RA2 同理 |
| 玩家指定的原版安装 | 以只读方式加载所需的 SHP 图像、VXL 模型、地块、音频及选用的原版地图。例如，动画使用哪个素材名由预设指定，实际画面从 MIX 中相应的图像条目取得 |
| 选中的 MOD、模式和地图 | 按来源和所选预设的方言解释作者提供的覆盖。旧 MOD 自带的 `rulesmd.ini` 仍可使用。FA2 地图中的 `[Triggers]`、`[Events]`、`[Actions]` 及嵌入规则随该图加载。例如地图把 `[105mm]` 的 `Range` 改为 `6` 时，覆盖仅作用于这张图 |

预设缺失或版本不符时，加载器应指出需要的预设和版本。素材缺失时应指出资源名及来源。开发者可在离线工具里显式打开零售 INI，对照标签和行为。验收时让基础 INI 对加载器不可见，保留相同的预设、素材和地图，以同一输入核对开火、命中和任务触发。[实施路线图](composable-gameplay-roadmap.md)

## 配置入口

一个新包声明自己的名称、格式和基础预设。以下为拟议语法，示例包名为 `demo`。

```ini
[Package]
Id=demo
Format=1
Base=yr
```

| 配置对象 | 主要用途 | 制作时需要决定的内容 |
|---|---|---|
| 运动方案 `Motion` | 描述一段位移，单位和在途弹体可引用同一方案 | 目标何时取样、路径、速度或耗时、高度、朝向。使用方另定碰撞、落地或引爆 |
| 对象标签 `Flag` | 供过滤器筛选当前对象，例如 `Infantry`、`Hero`、`Tesla` | 类型可带默认标签。每个效果记录自己授予或屏蔽的标签，到期时撤销该来源的贡献 |
| 规则 `Rule` | 将一次事件、过滤条件和效果器接在一起 | `On` 选触发时机，`Where` 筛对象，`Do` 指定命中条件后执行的动作 |
| 持续行为 `Behavior` | 表达有时长、来源、刷新或叠层规则的效果，包括 AE | 写明持续帧数、同一来源重复施加的处理，以及对象离场时如何清理 |
| FA2 触发与 Tag | 保留老地图的事件、动作、触发实例和绑定 | 使用该预设的数字编号。FA2 的 `[Tags]` 绑定触发，`Flag` 描述对象当前可用于筛选的属性 |

新定义按包名和节名发现，可以先引用后定义。手写 INI 和织网者模板共用字段字典。导入旧包时按旧读法解释 `[InfantryTypes]` 等注册表，保留原编号、注册顺序以及地图中的引用。[玩法系统规格](composable-gameplay-design.md)

## 配置示例 · 分别设定飞行时间与弧高

以 YR 灰熊坦克的 `105mm` 为基准。在测试图保留使用原炮的单位，给第二个单位绑定示例武器 `demo.ArcGun`。新武器继承原弹头，使用新的弹体。运动方案在开火时记录目标坐标。

```ini
[Motion demo.HighArc]
Goal=SnapshotAtStart
Path=Line
Pace=FixedTicks(60)
Height=PeakAboveHigherEnd(192lepton)
Facing=AlongPath

[Actor demo.SlowShell]
From=yr.Projectile.Cannon
Flag.Add=Interceptable
Motion=demo.HighArc
Motion.Contact=Sweep(Terrain,Bridge)
Motion.OnContact=DetonatePayload
Motion.OnArrive=DetonatePayload

[Weapon demo.ArcGun]
From=yr.Weapon.105mm
Projectile=demo.SlowShell
```

| 参数或工况 | 规格要求 |
|---|---|
| `FixedTicks(60)` | 无遮挡时从起点飞至采样落点共 60 个逻辑帧。目标中途离开，落点仍取开火时的坐标 |
| `PeakAboveHigherEnd(192lepton)` | 每格 256 lepton。起点高度 0、终点高度 128 时，第 30 帧顶高为 320。将参数改为 64 后，顶高为 192，抵达仍为第 60 帧 |
| 途中与桥面接触 | 提前在接触点引爆。接触与到达落在同一帧只引爆一次 |
| 飞行中存档 | 读档后保留起点、采样落点和进度，首次接触、引爆帧和伤害与连续运行一致 |

地图作者可以把同一 `demo.HighArc` 用于步兵跳跃。能力负责预留落点，步兵落地后交回占格和原任务，炮弹沿用自己的引信、接触和弹头。完整示例包含两个独立电场、绝缘区、减速弹头和 FA2 援军触发。[实施路线图](composable-gameplay-roadmap.md)

## 导入旧包与核对差异

1. **确定参照版本。** 选择基础预设及版本，记录 MOD 自带的规则文件、扩展、模式和地图。相同键的含义取决于节和读取位置，例如武器节的 `ROF` 是开火间隔，难度节的 `ROF` 是倍率。
2. **核对本局生效值。** 导入报告应定位到文件、节、键或地图触发编号，给出覆盖它的规则层、最终值和使用它的子系统。遇到未知地图动作时，报告应指出所属触发和动作序号。
3. **在目标场景验收。** 武器需检查开火、引爆和掉血。任务需检查触发、援军和胜负。兼容清单分别记录已在场景核对、仅可读取或保留、已知差异、待实现或待验证的条目。
4. **按目标格式导出。** 原样导回同版本时，应按字节保留未改的原文及地图压缩块。导回旧弹道时，应列出原格式无法表达的独立弧高和飞行时长，供作者决定是否采用近似配置。

三份可查询的索引分别收录零售档案的字段与记录、两款原版程序的字面键读点，以及 Ares 手册的字段与数字指令。具体标记为支持之前，还要核对其上下文、默认值和实际效果。[零售来源](ini-preset-wrapping/retail-index.json) · [RA2/YR 读点](ini-preset-wrapping/native-reader-index.json) · [Ares 条目](ini-preset-wrapping/ares-index.json)

## 开发范围与反馈

第一阶段从温带战区开始。地形定义随预设提供，地块素材来自玩家安装。地图预置和对局中新建的地面对象从同一份定义取得运动、通行和占格能力。灰熊慢弧线炮、步兵跳跃及附加效果是后续玩法用例。模组自编 Wasm 安排在 INI 玩法验收之后。VERA20K 的 #4653 讨论继续记录扩展接口及参与者意见。 · [玩法用例与验收](composable-gameplay-roadmap.md) · [#4653 参与者建议](rfc-4653-feedback.md)

报告兼容问题时，请附目标产品及版本、最小 INI 或 FA2 触发片段、原版表现和实际表现。涉及地图覆盖或存读档时，请注明模式、触发顺序或存档时点。[项目 Issues](https://github.com/YuriPlanet/vera-mod-engine/issues)
