# IniAura：用 INI 配置光环

演示"模块注册 INI"（[RFC 0001 第 6h 节](../../../docs/rfcs/0001-wasm-extension-modules.zh-CN.md#6h-ini-注册)）：模块只提供积木，光环全部由 modder 在 INI 里写，代码里没有任何写死的光环。

## 模块注册的名字

| 名字 | 种类 | 用法 |
| --- | --- | --- |
| `Demo.AuraTypes` | 注册表 | 列出光环条目节 |
| `Demo.Aura` | TechnoType 上的键 | 该类型发出哪些光环，逗号分隔 |
| `Demo.AuraEntered` | 对象事件 | 某单位第一次被某光环覆盖时引发（`value` 为光环序号，`Other` 为光环来源） |
| `auraHello` | 模块动作 | 记录参数文本，如 `Wasm:auraHello(Subject,welcome)` |

光环条目节的键：

| 键 | 含义 |
| --- | --- |
| `Radius` | 半径（格），1 到 20 |
| `Property` | `FirepowerMultiplier`、`SpeedMultiplier`、`ArmorMultiplier` 或 `ROFMultiplier` |
| `Op` | `add`、`multiply`（默认）或 `override` |
| `Value` | 小数，如 `1.25` |
| `Affects` | `allies`（默认，含自己）或 `enemies` |
| `Interval` | 刷新间隔（帧），15 到 900，默认 30 |

值不合法时模块停止，日志指出节和键。

## 工作方式

每个光环每 `Interval` 帧刷新一次，各光环错开；一次刷新每帧最多处理 8 个光环源，给范围内的单位加持续 `Interval + 30` 帧的修改器。修改器组按光环区分，同一光环的多个光环源只会互相替换，不会叠加。

示例 `rulesmd.ini` 有两种光环：`CommandAura`（友军火力 ×1.25）和 `SlowField`（敌军速度 ×0.6），挂在犀牛坦克、灰熊坦克、美国大兵和动员兵上。开局时模块在玩家旁边放几个双方单位，无需操作即可看到效果。

## 使用

1. `cargo build --release`，把 `target/wasm32-unknown-unknown/release/ini_aura.wasm` 改名为 `rules.wasm`。
2. 用 **Options → Mods → New Mod** 新建 mod（带完整的原版 `rulesmd.ini`），把本目录 `rulesmd.ini` 的内容并进去：`[MTNK]` 等已有节只加键。
3. 放入 `rules.wasm` 和 `mod.ini`，开一局遭遇战。

日志中的 `DEMO-OK entry/key/apply/event/action` 分别表示：读到光环条目、读到类型上的键、加上修改器、引发事件、INI 触发器调用了模块动作。
