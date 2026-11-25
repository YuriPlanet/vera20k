---
layout: default
title: Kratos 扩展支持
lang: zh-CN
---

# Kratos 兼容

[English](../kratos.md) · **简体中文**

VERA20K 正在用 Rust 原生移植 [Kratos PP](https://github.com/ChrisLv-CN/KratosPP) ——
《尤里的复仇》的 C++ 扩展。和 Ares 一样，每个构建（包括发布构建）都内置此扩展。默认的 Auto 设置会根据游戏数据决定是否启用。

## Kratos 开关

当一局游戏的规则、美术（ART）、模式或地图里含有只有 Kratos 读取器才会读的键时，Kratos 在这局
游戏中生效（`src/rules/kratos_switch.rs`）。卧倒射击点取自规则、模式和地图覆盖后的最终步兵 Image，其 ART 键随处理后的规则一起保存；开关在投影规则时一次性判定并存入规则集，所以存档、回放和联机中的每一方都能从
同一份文件得到同一答案。判定用的键会写入日志，例如
`Kratos plays in this game: [GI] PrimaryCrawlingFLH is a Kratos key`，模组作者据此能看出原因。

如果同一个键也被原版（stock）、Ares 或 Phobos 的读取器读过，它就不会开启 Kratos：Kratos 读取器
和 Phobos 一样复用这些读取器，而两个扩展都读的键对双方都不算数。原版数据里没有任何 Kratos 键，
所以原版对局的表现与原版游戏一致。

`RuleSet::kratos()` 就是 sim 和表现层读取的唯一答案。还没有任何已移植功能读取的 Kratos 键不会
开启开关，因此只往 INI 里加一个键本身不会改变游戏。

## Kratos 设置

Kratos PP 没有总开关：装上 `Kratos.dll` 就生效，连它的默认值一起。VERA 没有 DLL，所以在
Options 的扩展页提供一个启动级设置：VERA.INI 的 `[VERA] Kratos=Auto|On|Off`。

- **Auto**（默认）：由游戏数据决定（上面的键）。
- **On**：每局都启用 Kratos，因此只依赖 Kratos 默认值（比如卧倒时 `Z=20`）的模组，不写键也能拿到。
- **Off**：一律不启用；Kratos 键不生效，Kratos 模组按原版读。

模组也可以在 `mod.ini` 里写 `[Extensions] Kratos=yes`，让本次启动启用 Kratos，等同于模组自带该扩展；
玩家设置为 Off 时 VERA 会拒绝该模组，并提示如何重新开启。该设置和 Ares 的一样在启动时读取，
并随对局加载的规则一起保存，所以存档、回放和联机各方得到同一答案。

## 卧倒射击点

Kratos PP v0.2.4 会调整最终规则为 `Fearless=no` 的步兵的枪口坐标。下面三个键应写在步兵最终
`Image=` 对应的 ART 段中。任意一个都能让 Auto 启用 Kratos。

```ini
[GI]
PrimaryCrawlingFLH=20,0,20
ElitePrimaryCrawlingFLH=30,0,20
SecondaryCrawlingFLH=0,0,20
```

三个数依次表示前后、左右和高度偏移，单位是 lepton。站立时读取主武器或副武器对应的站姿坐标。
卧倒坐标省略时，沿用站姿的前后和左右偏移，高度取 20。达到精英等级后，即便仍在使用普通武器，
也会采用精英坐标。

精英副武器是个特例。v0.2.4 在两种姿态下都读取 `EliteSecondaryFireFLH`，显式填写的高度在卧倒时
仍然生效。省略该键时，卧倒采用 `SecondaryFireFLH` 的前后和左右偏移，高度取 20。
`EliteSecondaryCrawlingFLH` 在此版本中无效。这些设置只作用于武器槽 0 和 1。
[上游 ART 读取代码](https://github.com/ra2diy/KratosPP/blob/v0.2.4/src/Ext/TechnoType/CrawlingFLHData.h#L19-L39)

VERA 按射手自身的姿态计算枪口位置，弹丸起点、枪口动画和开火音效都使用这一坐标。
同类型步兵可以独立站立或卧倒。[上游姿态和等级判定](https://github.com/ra2diy/KratosPP/blob/v0.2.4/src/Ext/TechnoType/CrawlingFLH.cpp#L17-L90)

## 只打空中的抛射体

在 `[ProjectileType]`（抛射体类型）段写 `AAOnly=yes`，该抛射体就拒绝朝不在空中的目标开火：

```ini
[MissileAA]
AA=yes
AAOnly=yes
```

Kratos 把这个检查放在 `TechnoClass_CanFire` 钩子（`0x6FC339`，拒绝时返回 `0x6FCB7E` =
ILLEGAL）：当目标不在空中、开火者的 `LandTargeting` 不为 `1`（`Land_Not_OK`，原版对陆地目标本来就会
拒绝）且抛射体带 `AA=yes` 时终止发射。没有 `AA=yes` 时该键不起作用，与 Kratos 一致
（[源码](https://github.com/ra2diy/KratosPP/blob/v0.2.4/src/Ext/TechnoType/Status/CanFireCheck.cpp)）。

VERA 把这个键读进 `ProjectileType::aa_only`，在 `sim::combat::fire_error` 里与同一个钩子的
Phobos、Ares 分支并列判定。它只是原版已有类型上的一个开关，所以放在该类型的读取器里，
而不是单独开一个模块。

## 武器要钱

在 `[WeaponType]` 写 `NoMoneyNoTalk`，开火方所属不够这笔钱就无法开火；写负数则是「钱太多就不
打」，超过该数值的绝对值时拒绝：

```ini
[GoldShell]
NoMoneyNoTalk=1000
```

同一段写 `DontNeedMoney=yes` 跳过该检查。数值在每次请求开火时读取，所以所属攒够钱后下一次
尝试就能开火。

Kratos 在 `TechnoStatus::CanFire_CheckWeapon` 里做这件事，运行于 `TechnoClass::CanFire`
钩子（`0x6FC339`，拒绝返回 `0x6FCB7E` = ILLEGAL），并且它的 AE 也能提供该数值
（`src/Ext/TechnoType/Status/CanFireCheck.cpp`）。VERA 在 `kratos_read` 里读取这两个武器键，
并在 `sim::combat::fire_error` 中与同一钩子的其它分支并列判定。提供数值的 AttachEffect 尚未
移植：只认武器自己写的键。

## 冻结类型

在 `[TechnoType]` 写 `Wreck=yes`，该类型就是不可使用的残骸：不移动、不索敌，炮塔也不转。

```ini
[HTNK_HULK]
Wreck=yes
```

在 Kratos 里这个键只是逐单位 `Freezing` 字节的三个来源之一
（`src/Ext/TechnoType/Status/Freeze.cpp`），另外两个是活着的 `Freeze` 脚本和
`VectorFreezeActive` 闩锁。`Freezing` 成立时，单位每次更新都会清空目标并强制停下
（`TechnoStatus::OnUpdate_Freeze`），`TechnoClass::IsUnderEMP`（`0x70EFD0`）回答 true，
引擎的运动与朝向钩子也拒绝推进它（`0x4CCBB3`、`0x54AED0`、`0x6622E0`，以及 `0x736A95`
的炮塔自转分支）。

VERA 在 `rules::kratos_freeze` 里从类型推导该状态，这对 `Wreck` 是精确的：残骸存活期间一直
冻结，因此不需要逐单位标记，也不需要存档字段。将来那两个逐单位来源会跟自己的所有者一起进来，
在同一个地方增加运行时输入而不是第二个状态。目前有三个所有者读取它：

- `world::object_turn`：地面与空中 locomotor 的 Process 在做任何工作之前返回，单位保持坐标、
  朝向和待执行的路线；
- `world::techno_ai::target_scan`：既不索敌也不反击；
- `movement::turret::facing_update`：`TurretSpins` 类型跳过自转分支，与 `0x736A95` 钩子跳到
  `0x736AD5` 完全一致，因此普通的回位路径照常运行。

残留：`TechnoClass::IsUnderEMP` 这个谓词本身没有接冻结，所以直接询问它的代码（EMP 火花、
部署、navcom）仍会把残骸当作可操作对象。上面三个所有者覆盖了原版拒绝推进冻结单位的全部位置，
也就是这个键真正的作用范围。

## 原作者与项目

VERA20K 用 Rust 重新实现行为，不加载 `Kratos.dll`，也不重新分发 Kratos 二进制；每个已移植的
功能都按代码旁注明的 Kratos 源码翻译。

This product includes software developed by the Kratos Authors.

[Kratos PP](https://github.com/ChrisLv-CN/KratosPP) 是 **DPKratos** 的 C++ 移植版，作者
**双杀步枪（ChrisLv_CN）**（开发者），策划 **艾木魁（M.Kenosis）**，测试 **冲冲冲14782**。
它构建在 **Xkein** 的 [DynamicPatcher](https://github.com/Xkein/YRDynamicPatcher) 之上，
并鸣谢 Ares 与 Phobos 的开发者。Kratos PP 以 LGPLv3 授权
（[许可证](https://github.com/YuriPlanet/vera20k/blob/main/LICENSES/KratosPP.txt)），
VERA20K 的 GPLv3 组合作品保留该许可证。

本页所述功能对应的原作者：

- 卧倒射击点（`CrawlingFLH`）— **双杀步枪（ChrisLv_CN）**，
  `src/Ext/TechnoType/CrawlingFLH.cpp` 与 `CrawlingFLHData.h`。
- 只打空中的抛射体（`AAOnly`）— **双杀步枪（ChrisLv_CN）**，
  `src/Ext/TechnoType/Status/CanFireCheck.cpp`。
- 武器要钱（`NoMoneyNoTalk`）— **双杀步枪（ChrisLv_CN）**，
  `src/Ext/TechnoType/Status/CanFireCheck.cpp`。
- 冻结类型（`Wreck`）— **双杀步枪（ChrisLv_CN）**，`src/Ext/TechnoType/Status/Freeze.cpp`
  与 `src/Hooks/TechnoExtHook.cpp`。

## VERA20K 移植署名

各扩展功能的 Rust 移植由 VERA20K 贡献者以合并的 pull request 完成；下面用 GitHub 用户名
标注每个链条对应的合并 PR 作者：

- **1372704087** — Kratos 的逐局开关、卧倒射击点（`CrawlingFLH`）、只打空中的抛射体
  （`AAOnly`）、武器要钱（`NoMoneyNoTalk`）与冻结类型（`Wreck`）。

## 添加一个功能

已移植的功能把它的键放在 `kratos_read` 里读取，这些读取会被标记为 Kratos 读取器；开关只统计没有
被其它读取器读到的键。键名、默认值和钳制值来自 Kratos PP 以及 Kratos 所挂钩的 `gamemd.exe`，
不是凭清单猜的；原生地址写在 Rust 代码旁。没有该键的游戏保持原版路径。

### 一个功能一个模块

每项功能的键、默认值和判定都放在 `src/rules/<扩展>_<功能>.rs`，测试放在同目录的
`<扩展>_<功能>_tests.rs`。读取逻辑遵循 Kratos 对应的 `Read` 方法，模拟在用到该值时调用此模块的判定。

例如，`kratos_crawling_flh.rs` 对应 `Ext/TechnoType/CrawlingFLHData.h`。读取器把两组坐标写入
`WeaponOffsets.crawling` 和 `WeaponOffsets.elite_crawling`。`get_weapon_record` 通过借用这两个字段
的视图调用 `flh(slot, elite, prone)`。玩家存档的布局记录字段名、类型和顺序，因此这两个字段保留原来的
名称、类型和顺序。

类型数据保存在共享的 `RuleSet` 中，即使场上有两万个单位，同一类型的单位也共用一组偏移量。
