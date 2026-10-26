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
- 建筑生产队列 — **CrimRecya**。
- INI `$Include` / `$Inherits` — **Morton**。
- 遭遇战 AI 去硬编码（残局变卖/全体出击、MCV 部署集结）— **TaranDahl**。
- 并行 AI 生产队列限制 — **Trsdy**、**NetsuNegi**（按类别）、
  **Starkku**（按 TechnoType）。
- 遭遇战无限颜色 — **Morton**。

完整贡献者名单见
[Phobos CREDITS.md](https://github.com/Phobos-developers/Phobos/blob/develop/CREDITS.md)。
VERA 的 include/inheritance 遍历还改编自 baixiong443 的 ra2-mono `IniFile`，
见 [INI 指令文档](../ini-directives.md#source-mapping)。

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
