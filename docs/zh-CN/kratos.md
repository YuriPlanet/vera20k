---
layout: default
title: Kratos 扩展支持
lang: zh-CN
---

# Kratos 兼容

[English](../kratos.md) · **简体中文**

VERA20K 正在用 Rust 原生移植 [Kratos PP](https://github.com/ChrisLv-CN/KratosPP) ——
《尤里的复仇》的 C++ 扩展。和 Ares 一样，它存在于每个构建（包括发布构建）中，由游戏自身的数据
决定是否启用；没有编译特性，也没有启动设置。

## Kratos 开关

当一局游戏的规则、美术（ART）、模式或地图里含有只有 Kratos 读取器才会读的键时，Kratos 在这局
游戏中生效（`src/rules/kratos_switch.rs`）。ART 是在规则处理阶段读取的，因此这些键随处理后的
规则一起保存；开关在投影规则时一次性判定并存入规则集，所以存档、回放和联机中的每一方都能从
同一份文件得到同一答案。判定用的键会写入日志，例如
`Kratos plays in this game: [GI] PrimaryCrawlingFLH is a Kratos key`，模组作者据此能看出原因。

如果同一个键也被原版（stock）、Ares 或 Phobos 的读取器读过，它就不会开启 Kratos：Kratos 读取器
和 Phobos 一样复用这些读取器，而两个扩展都读的键对双方都不算数。原版数据里没有任何 Kratos 键，
所以原版对局的表现与原版游戏一致。

`RuleSet::kratos()` 就是 sim 和表现层读取的唯一答案。还没有任何已移植功能读取的 Kratos 键不会
开启开关，因此只往 INI 里加一个键本身不会改变游戏。

## 卧倒射击点

`InfantryClass::Crawling`（`+0x6DB`）决定步兵射击时枪口从哪里出。Kratos
（`src/Ext/TechnoType/CrawlingFLH.cpp`）在单位 Image 对应的 ART 段里保留四个键：

```ini
[GI]
PrimaryCrawlingFLH=20,0,20
ElitePrimaryCrawlingFLH=20,0,20
SecondaryCrawlingFLH=0,0,20
EliteSecondaryCrawlingFLH=0,0,20
```

- 卧倒时，武器槽 0 / 1 使用对应的卧倒坐标，而不是 `PrimaryFireFLH` / `SecondaryFireFLH`
  （精英等级用 `Elite*` 那对）。站立时仍用原版坐标。
- 每个卧倒坐标默认等于该槽的射击坐标、且 `Z=20`（`CrawlingFLHData::Read`），所以一个键都不写的
  类型，卧倒时枪口也会降低。
- `Fearless` 步兵被跳过：`CrawlingFLH::Setup` 会为它们禁用自己。
- 只改动原版的两个武器槽位；Kratos 只改写 `GetWeapon(0)` 与 `GetWeapon(1)`。

VERA 在 `rules::weapon_offsets` 读取这些键，在 `sim::combat::combat_weapon`
（`get_weapon_record`）应用它们，因此弹丸起点、枪口动画和开火音效使用同一个坐标。Kratos 不生效的
对局保持 gamemd 的坐标。

## 原作者与项目

VERA20K 用 Rust 重新实现行为，不加载 `Kratos.dll`，也不重新分发 Kratos 二进制；每个已移植的
功能都按代码旁注明的 Kratos 源码翻译。

This product includes software developed by the Kratos PP author.

[Kratos PP](https://github.com/ChrisLv-CN/KratosPP) 是 **DPKratos** 的 C++ 移植版，作者
**双杀步枪（ChrisLv_CN）**（开发者），策划 **艾木魁（M.Kenosis）**，测试 **冲冲冲14782**。
它构建在 **Xkein** 的 [DynamicPatcher](https://github.com/Xkein/YRDynamicPatcher) 之上，
并鸣谢 Ares 与 Phobos 的开发者。Kratos PP 以 LGPLv3 授权
（[许可证](https://github.com/YuriPlanet/vera20k/blob/main/LICENSES/KratosPP.txt)），
VERA20K 的 GPLv3 组合作品保留该许可证。

本页所述功能对应的原作者：

- 卧倒射击点（`PrimaryCrawlingFLH` 等四个键）— **双杀步枪（ChrisLv_CN）**，
  `src/Ext/TechnoType/CrawlingFLH.cpp` 与 `CrawlingFLHData.h`。

## VERA20K 移植署名

各扩展功能的 Rust 移植由 VERA20K 贡献者以合并的 pull request 完成；下面用 GitHub 用户名
标注每个链条对应的合并 PR 作者：

- **1372704087** — Kratos 的逐局开关与卧倒射击点（`CrawlingFLH`）。

## 添加一个功能

已移植的功能把它的键放在 `kratos_read` 里读取，这些读取会被标记为 Kratos 读取器；开关只统计没有
被其它读取器读到的键。键名、默认值和钳制值来自 Kratos PP 以及 Kratos 所挂钩的 `gamemd.exe`，
不是凭清单猜的；原生地址写在 Rust 代码旁。没有该键的游戏保持原版路径。
