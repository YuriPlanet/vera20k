# INI Academy（军校）

演示"模块注册 INI 名字"（[RFC 0001 第 6h 节](../../../docs/rfcs/0001-wasm-extension-modules.zh-CN.md#6h-ini-注册)）：modder 只写 INI，"步兵进驻建筑就升级"的机制由模块实现。

## modder 写什么

全部写在 mod 的 `rulesmd.ini` 里（完整例子见本目录的 [`rulesmd.ini`](rulesmd.ini)）。mod 的 `rulesmd.ini` 会整个替换原版，所以要加进一份完整副本里。

```ini
[Demo.AcademyTypes]       ; 军校类型列表（模块注册的注册表）
0=EliteAcademy

[EliteAcademy]            ; 一种军校
Rank=Elite
Allowed=E1,E2,GGI
Eject=no

[NABNKR]                  ; 战斗碉堡成为军校
Demo.Academy=EliteAcademy

[E1]                      ; 升级后打个招呼
Wasm.Tags=AcademyTag

[AcademyTag]
Trigger=AcademyGraduation

[AcademyGraduation]
Event=Wasm:Demo.Promoted
Action1=Wasm:academyHello(Subject,welcome)
Repeat=Repeating
```

## 各个键

| 写法 | 位置 | 含义 |
| --- | --- | --- |
| `[Demo.AcademyTypes]` | 列表节 | 每一项是一个军校类型节的名字 |
| `Rank=` | 军校类型节 | 进驻后得到的等级：`Rookie`（新兵）、`Veteran`（老兵）、`Elite`（精英），默认 `Elite`。已经达到或高于这个等级的单位不变 |
| `Allowed=` | 军校类型节 | 可以升级的单位类型，逗号分隔；不写或留空表示任何能进驻的单位 |
| `Eject=` | 军校类型节 | `yes`：升级后建筑把驻军全部放出来（相当于玩家对建筑下"撤出"命令）；默认 `no` |
| `Demo.Academy=` | 建筑类型节（可进驻，`CanBeOccupied=yes`） | 这种建筑是哪种军校；必须是列表里的名字 |
| `Event=Wasm:Demo.Promoted` | 触发器节 | 单位升级时在该单位上发生的事件；`Other` 是建筑，`value` 是新等级（0、1、2） |
| `Wasm:academyHello(目标,文字)` | 触发器动作 | 在日志里向目标说这段文字 |

- 只有步兵进驻（`Occupier=yes` 的步兵进入可进驻建筑）才算，进运输工具不算。
- 值写错（等级名不对、`Allowed` 里有不存在的类型、`Demo.Academy` 指向不存在的军校类型）时，模块停止，日志指出哪个节的哪个键。

## 构建

```sh
cargo build --release
```

把 `target/wasm32-unknown-unknown/release/ini_academy.wasm` 改名为 `rules.wasm`，和 `mod.ini`、`rulesmd.ini` 一起放进 mod 文件夹（见 [docs/wasm-modules/guide.md](../../../docs/wasm-modules/guide.md)）。

无人操作时，模块会在玩家基地附近放一座战斗碉堡和几个步兵，命令他们进驻，每个功能第一次生效时在日志里写一行 `DEMO-OK …`，供引擎测试 `ini_demo_mods_reach_their_marks` 检查。
