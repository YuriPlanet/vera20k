# IniCounter：模块注册 INI 的示例

演示[模块注册 INI](../../../docs/rfcs/0001-wasm-extension-modules.zh-CN.md#6h-ini-注册)：modder 只写 INI，模块实现功能。

社区里的一个想法："拥有雷达时，每训练 10 个动员兵，就从战车工厂免费送一辆犀牛坦克"。模块把它做成通用的计数器，全部在 `rulesmd.ini` 里配置：

```ini
[Demo.Counters]        ; 模块注册的注册表，每一项是一个计数器节
1=SovietDraft

[SovietDraft]
Count=E2               ; 计哪些单位的出生（逗号分隔）
Threshold=10           ; 多少个换一次奖励
Requires=NARADR        ; 可选：拥有这种建筑时才计数
Reward=HTNK            ; 奖励的单位
Factory=NAWEAP         ; 可选：奖励出现在这种建筑旁（不填则在任意建筑旁）
Houses=Russians        ; 可选：只对这些国家生效（不填则所有所属方）
Stage=4                ; 可选，仅演示用：模块自己给电脑方造好建筑和 4 个计数单位
```

示例 `rulesmd.ini` 用 `Threshold=3`，方便很快看到效果。值写错时模块会停止，提示哪个节哪个键。

## 模块做了什么

- 初始化时注册注册表 `Demo.Counters`、全局事件 `Demo.RewardGranted` 和对象事件 `Demo.RewardArrived`。
- 每个 `IniEntry` 读出计数器节，用代码建一个绑定到计数类型的 `Birth` 触发器。
- 计数按所属方存在模块内存里。达到阈值且满足 `Requires` 时，用 `CreateObject` 造出奖励，再引发两个事件：
  - `Demo.RewardGranted`（全局，value 为计数器序号，house 为该所属方）：由代码里的 `Trigger::on_module_event` 触发器接收。INI 触发器总是绑定到挂了 `Wasm.Tags` 的类型上，而全局事件的触发器不能绑定，所以全局事件只能在代码里接收。
  - `Demo.RewardArrived`（对象事件，在新坦克上）：由 `rulesmd.ini` 里 `[HTNK]`、`[MTNK]` 的 `Wasm.Tags` 带的 INI 触发器接收。
- 目前引擎的 `CreateObject` 还不支持以工厂为目标（从门口开出），模块先试工厂，得到 `Unsupported` 后改为放在工厂旁边的空格子上。

## 构建与安装

```sh
cargo build --release
```

把 `target/wasm32-unknown-unknown/release/ini_counter.wasm` 改名为 `rules.wasm`，和 `mod.ini` 一起放进 `Mods/IniCounter/`。mod 的 `rulesmd.ini` 会整个替换原版的，所以要从完整副本开始，把这里的节加进去（`Wasm.Tags` 写进已有的 `[HTNK]`、`[MTNK]`）。

第一次成功时日志会出现 `DEMO-OK entry`、`DEMO-OK count`、`DEMO-OK reward`、`DEMO-OK event`。
