---
layout: default
title: 扩展支持与原作者
lang: zh-CN
---

# 扩展支持与原作者

[English](../) · **简体中文**

VERA20K 用 Rust 独立重写游戏行为，不加载 `Ares.dll`、`Phobos.dll` 或
NPatch 补丁，不导入它们的源码，也不重新分发其二进制。下面按扩展列出已支持
的实现及其原作者；技术细节和原生地址见对应的英文文档。

## Ares

技术细节：[Ares 扩展支持](../ares.md)

Ares 由 **pd** 于 2007 年发起，现由
[The Ares Contributors](https://ares-developers.github.io/Ares-docs/credits.html)
维护：

- 现任开发者：**AlexB**。
- 历任开发者：**pd**（创建者）、**DCoder**、**Renegade**、**Electro**、
  **Marshall**、**Graion Dilach**。
- 贡献者：**Gluk-v48**（高级残骸扩展）、**Marshall**（棱镜折射、Launch Base
  与 YR Unofficial 1.002 Mini-Patch）、**A Mystery Guest**（新式电力单位逻辑）。
- 协作者：**Nighthawk**（项目管理）、**Steel Mirage**（文档）、
  **Graion Dilach**（用户支持/项目大使）、**Speeder**（宣传素材）、
  **Starkku**（Final Alert 支持）、**Project z Zomboid**（中文翻译）、
  **Kik0u**（法文翻译）。

Ares 官方没有按 INI key 公布单个功能的作者，因此这里只署名项目与开发者，
不臆造具体功能的负责人。完整的测试者与社区鸣谢见官方 credits 页。

## Phobos

技术细节：[Phobos 扩展支持](../phobos.md)

Phobos 由 Phobos Developers 开发。VERA20K 已移植功能对应的原作者：

- 自定义辐射类型 — **Otamaa**（移植并修正 **Ares developers** 未完成的
  RadTypes 代码）。
- 护盾 — **Uranusian**、**secsome**、**Belonit**；护盾弹头修改 —
  **Starkku**；吸收/穿透 — **Morton**；重生动画与武器 — **Ollerus**。
- 自定义矿石生成（ore spawners）— **Kerbiter**。
- 自定义采矿动画 — **secsome**、**Uranusian**。
- 矿石阶段动画隐藏（`HideIfNoOre` 阈值）— **Otamaa**。
- 超级武器科技与单位门槛（`SW.AuxTechnos`、`SW.NegTechnos`、
  `SW.TechLevel`）— **NetsuNegi**；其中的建筑升级计数 — **NetsuNegi**、
  **Ollerus**。
- 数字显示（DigitalDisplay）— **ststl**、**FlyStar**、**NaotoYuuki**、
  **Saigyouji**、**JunJacobYoung**、**CrimRecya**。
- 建筑放置预览 — **Otamaa**、**Belonit**。
- 侧边栏 GDI/NOD 坐标 — **Belonit**。
- 步兵接敌自动部署（`InfantryAutoDeploy`）— **TaranDahl**。
- 匍匐移动速度（`ProneSpeed`）— **TaranDahl**。
- 建筑类型选择（`BuildingTypeSelectable`）— **TaranDahl**（代码）、**Ollerus**（文档）。
- 选中单位闪烁（`ShowFlashOnSelecting`）— **Fryone**。
- 弹头自伤开关（`AllowDamageOnSelf`）— **Starkku**；它与 `DamageSelf` 对空中单位的修复 — **NetsuNegi**。
- 抛射体自定义重力（`Gravity`）— **secsome**。
- 弹头解除隐形开关（`DecloakDamagedTargets`）— **Starkku**。
- 空袭标记视觉自定义与修复（`LaserTargetColor`、`AirstrikeLineColor`）— **Starkku**。
- `Locomotor=Fly` 浮动开关（`FlyNoWobbles`）— **Noble_Fish**。
- 暂停界面居中 — **Starkku**。
- 去硬编码计时器闪烁配色方案（`TimerBlinkColorScheme`）— **Starkku**。
- `Locomotor=` 语义别名（如 `Locomotor=Drive`）— **Belonit**。
- 缺省图标（`MissingCameo`）— **Uranusian**。
- 电厂增强器（`PowerPlantEnhancer`）— **secsome**、**NetsuNegi**（范围）、**Ollerus**（`MaxCount`）、**Trsdy**（EMP）。
- 电厂受损系数（`PowerPlant.DamageFactor`）— **Otamaa**、**Ollerus**。
- 建筑生产队列 — **CrimRecya**。
- INI `$Include` / `$Inherits` — **Morton**。
- 遭遇战 AI 去硬编码（残局变卖/全体出击、MCV 部署集结）— **TaranDahl**。
- 并行 AI 生产队列限制 — **Trsdy**、**NetsuNegi**（按类别）、
  **Starkku**（按 TechnoType）。
- 遭遇战无限颜色 — **Morton**。
- AttachEffect — **Starkku**（目标过滤 — **Flactine**，附加事件 606 — **FS-21**）。
- 暴击系统 — **Starkku**（逻辑）、**AutoGavy**（弹头暴击）、**Trsdy**
  （护盾 `ImmuneToCrit`）。

完整贡献者名单见
[Phobos CREDITS.md](https://github.com/Phobos-developers/Phobos/blob/develop/CREDITS.md)。
VERA 的 include/inheritance 遍历还改编自 baixiong443 的 ra2-mono `IniFile`，
见 [INI 指令文档](../ini-directives.md#source-mapping)。

## VERA20K 移植署名

各扩展功能的 Rust 移植由 VERA20K 贡献者以合并的 pull request 完成；下面用
GitHub 用户名标注每个链条对应的合并 PR 作者：

- **1372704087** — Phobos 侧的全部功能：护盾、自定义辐射类型、三条矿石
  链、超武科技门槛、数字显示、放置预览、侧边栏 GDI/NOD 坐标、步兵自动
  部署、`FlyNoWobbles`、暂停界面居中、建筑生产队列、遭遇战 AI 去硬编码、
  并行 AI 队列限制、遭遇战无限颜色、超武侧边栏、匍匐速度、建筑类型选择、
  计时器闪烁配色、缺省图标、电厂增强器、`Locomotor=` 语义别名、选中
  闪烁、`AllowDamageOnSelf`、`DecloakDamagedTargets`、抛射体重力、空袭
  标记颜色、AttachEffect 与暴击链、`$Include`/`$Inherits` 原生钩子；以及
  Ares 的第四 Side 默认值（SideExt）、国家展示、国家空投覆盖与国家 AI
  电厂。
- **zzzzzzh123** — Ares 的 opt-in 构建能力本身、EMP、水陆类型转换与
  脚本 68、自定义装甲、补充字符串表、按类型 SHP 建筑调色板、EVA 语音
  标签、通用生产前置、`FactoryOwners`、载具驾驶员与 KillDriver、扩展 IFV
  模式、类型选择编组、NoManualFire、Ares AttachEffect 链（地面弹头效果、
  附带装甲/火力/速度/装填、自身附带潜影、强制显形、动画弹头与资源、弹头
  阵营过滤）、HunterSeeker、阵营存续、零费与初始/虚拟充能超武、扩展 House
  盟友列表、步兵装填与按武器弹药、按类型晋升音效、击杀赏金。
- **baixiong443** — GenericWarhead、超武建筑门槛、单段/两段超武费用与
  奖励、UnitDelivery、INI include 与继承（含 ra2-mono `IniFile` 改编）。
- **hrusten** — Ares 超武掩码/AI 模式原生读取、赏金标签的原生槽位、
  读档后的辐射衰减时钟修复。
- **Krisztiaan** — 护盾伤害/恢复钩子顺序修复。
- **CCDawn** — 战术视图右上角的 Phobos 构建描述文字。
- **EncoreAlpha** — 保留的 `EMPulseWarhead` 规则状态。

## VeraX 与自有功能

技术细节：[VeraX 扩展](../verax.md)

VeraX 当前编译自动装车（AutoLoad）与遭遇战地图热重载两个功能。下面的半透明
迷雾与寻路步骤显示是相关自有功能，不属于 `verax` 特性。

- **自动装车（AutoLoad）**：思路参考 Bilibili UP 主
  [335956233](https://space.bilibili.com/335956233) 的视频。VERA 自行实现
  分配、距离与命令策略，没有使用对方的二进制或源码。
- **半透明迷雾**：VERA 自有的客户端图形选项
  （`[graphics] translucent_fog=true`，默认关闭以保持原版外观）。地形在
  迷雾中仍然可见，但未看见的敌方单位、敌方子弹和围墙会被隐藏；它不受
  `verax` 特性控制。
- **步兵寻路步骤显示**：不是扩展功能，而是 `dev-overlays` 开发构建中的
  诊断叠层（`Ctrl+Shift+O`），绘制选中移动单位的剩余路径格子与步数标签。
  思路参考 Bilibili UP 主
  [1995852436](https://space.bilibili.com/1995852436) 的视频；正式构建
  （`--no-default-features`）不包含，未使用第三方二进制或源码。

## NPatch / NPSE

技术细节：[NPatch 扩展支持](../npatch.md)

NPatch（NewPatch）是由前 RockPatch 开发者 **CnCVK（VK）** 制作的、基于
RockPatch 的尤里复仇扩展平台，有多个版本：

- **NPatch 1.00**：最初的正式版，由 CnCVK 发布。
- **NPatch Extended（NPExt）**：由 **LH_Mouse** 发布；手册为 NPExt #169
  （2011 年），并在其中致谢 pd 与 CnCVK。VERA 的旧版
  `EnableAIBuildLimitation` 路径引用 NPExt `ext173` 的读取器
  （`0xC2D132`，门控 `0xC2CF67`）。
- **NPatch Special Edition（NPSE）**：CnCVK 的 NPatch 下载页上的安装包
  #0020；**NPSE 2.0 #359** 的说明书与更新日志作者为 **lydjjlyd**。
- **NPatch Autumn Edition（NPAE）**：同一页面的安装包 #0011。

NPSE 2.0 说明书鸣谢测试者 **我爱二次元**、**蓝天白云fd**、**好人先昇**、
**行少**、**凌敏SHOS**。`0xC3xxxx` 原生地址来自该说明书及其发布版本。

VERA20K 只实现文档记录的行为，不加载或修改 `gamemd.exe`，也不分发 NPatch
二进制。
