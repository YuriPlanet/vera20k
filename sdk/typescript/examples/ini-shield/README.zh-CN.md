# INI Shield（护盾）

演示"模块注册 INI 名字"（[RFC 0001 第 6h 节](../../../../docs/rfcs/0001-wasm-extension-modules.zh-CN.md#6h-ini-注册)）：modder 只写 INI，护盾机制由模块实现。

## modder 写什么

全部写在 mod 的 `rulesmd.ini` 里（完整例子见本目录的 [`rulesmd.ini`](rulesmd.ini)）。mod 的 `rulesmd.ini` 会整个替换原版，所以要加进一份完整副本里。

```ini
[Demo.ShieldTypes]        ; 护盾类型列表（模块注册的注册表）
0=HeavyShield

[HeavyShield]             ; 一种护盾
Strength=200
Regen=10
Delay=150

[HTNK]                    ; 给犀牛坦克加护盾
Demo.Shield=HeavyShield
Wasm.Tags=DemoShieldTag

[DemoShieldTag]
Trigger=DemoShieldRecharge

[DemoShieldRecharge]      ; 护盾被打破时立刻回 40 点
Event=Wasm:Demo.ShieldBroken
Action1=Wasm:restoreShield(Subject,40)
Repeat=Repeating
```

## 各个键

| 写法 | 位置 | 含义 |
| --- | --- | --- |
| `[Demo.ShieldTypes]` | 列表节 | 每一项是一个护盾类型节的名字 |
| `Strength=` | 护盾类型节 | 护盾值，必填，至少 1。单位受到的伤害先由护盾承受 |
| `Regen=` | 护盾类型节 | 每 15 帧回复的护盾值，默认 0 |
| `Delay=` | 护盾类型节 | 最后一次受击后，过多少帧才开始回复，默认 0 |
| `Demo.Shield=` | 单位类型节（TechnoType） | 这种单位使用哪种护盾；必须是列表里的名字 |
| `Event=Wasm:Demo.ShieldBroken` | 触发器节 | 护盾降到 0 时在该单位上发生的事件 |
| `Wasm:restoreShield(目标,数值)` | 触发器动作 | 给目标回复"数值"点护盾，不超过 `Strength` |

- 护盾在单位第一次受击时充满，之后按 `Regen`/`Delay` 回复；打破后过了 `Delay` 也会重新回复。
- 值写错（不是整数、超出范围、`Demo.Shield` 指向不存在的类型）时，模块停止，日志指出哪个节的哪个键。

## 构建

```sh
cd sdk/typescript && npm install
node bin/vera20k.js build examples/ini-shield
```

得到 `build/rules.wasm`（TypeScript 写的，和 Rust 版 `sdk/examples/ini-shield` 行为相同），和 `mod.ini`、`rulesmd.ini` 一起放进 mod 文件夹（见 [docs/wasm-modules/guide.md](../../../../docs/wasm-modules/guide.md)）。

无人操作时，模块会在玩家基地附近放几辆带护盾的坦克和步兵（美方对电脑方），让它们互相攻击，每个功能第一次生效时在日志里写一行 `DEMO-OK …`，供引擎测试 `ini_demo_mods_reach_their_marks` 检查。
