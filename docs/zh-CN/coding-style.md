---
layout: default
title: VERA20k 代码规范
lang: zh-CN
---

# VERA20k 代码规范

以 VERA20k 上游 `main` 的 `a7edf68c5` 为样本，适用于引擎、客户端、GUI 和项目工具的新增及修改。提交与验证要求以仓库约定为准。 [项目约定](../../AGENTS.md) · [Rust 指南](../../.agents/skills/_shared/rust-guidelines.md)

## Rust 格式与命名

- 引擎、客户端和 GUI 使用 Rust 2024。采用 rustfmt 的默认缩进和仓库配置的 100 列宽。只格式化修改过的叶子文件，原文件已有格式差异时手动保持改动范围。 [Cargo.toml](../../Cargo.toml) · [rustfmt.toml](../../rustfmt.toml) · [工具说明](../../tools/README.md)
- 模块、函数和字段用 `snake_case`，类型和枚举变体用 `UpperCamelCase`，常量用 `SCREAMING_SNAKE_CASE`。名字说明用途或量纲，例如 `PlatformState`、`render_dimensions`、`pointer_in_client`、`RA2_LOGIC_FRAMES_PER_SECOND`。含义不同的索引用独立类型，例如 `CountryIdx` 和 `SideIdx`。 [平台状态](../../src/app/state/platform.rs) · [规则索引](../../src/rules/ruleset.rs) · [定点数学](../../src/util/fixed_math.rs)
- 默认使用模块私有项，按调用范围开放 `pub(super)`、`pub(crate)` 或 `pub`。优先借用、切片和迭代器，确实需要独立持有数据时再复制。先查已有函数、类型和测试辅助代码，沿用其调用入口。 [项目约定](../../AGENTS.md) · [帧边界](../../src/sim/world/frame_effects.rs)

## 模块职责与状态

- `vera20k-core` 承载资源、规则、地图、模拟和网络核心，`vera20k` 负责应用编排、渲染、界面与音频。依赖保持单向，`sim/` 只通过已有边界交付模拟事实，窗口、GPU 和声音设备由客户端管理。 [核心入口](../../src/core_lib.rs) · [客户端入口](../../src/lib.rs) · [帧边界](../../src/sim/world/frame_effects.rs)
- 每份可变状态由一个模块负责初始化、更新和清理。对调用方提供有业务含义的操作。派生缓存写明来源、刷新和失效时机。窗口焦点、绘制计时等进程状态留在表现层，存档与同步校验只纳入各自定义的状态。 [平台状态](../../src/app/state/platform.rs) · [GPU 帧提交](../../src/render/frame_handoff.rs)
- 模拟保持跨平台确定性。分数值优先用 `SimFixed`，在转换点注明单位、取整和溢出行为。保留 RNG 的流和调用顺序，以及有语义的帧阶段和对象访问顺序。热路径优先复用缓冲区和批处理。玩法扩展沿用已有运行时开关，例如 Ares 的 `rules.ares()`。 [定点数学](../../src/util/fixed_math.rs) · [项目约定](../../AGENTS.md)

## 注释、输入与错误

- 用 `//!` 交代模块职责和依赖边界，用 `///` 说明调用约束、单位、所有权和副作用。移植原版行为时，在对应代码旁注明已核实的原版函数名、地址和关键条件。改变行为时同步更新引用同一地址的注释。 [INI 读取器](../../src/rules/ini_value.rs) · [帧边界](../../src/sim/world/frame_effects.rs)
- 外来 INI、地图、存档和网络数据先验证长度、取值和索引。INI 复用 `IniSection` 的读取器，默认值、钳位和分层顺序按原版调用确认。可恢复失败返回 `Result` 或 `Option`，解析层用 `thiserror` 保留结构化错误，应用层用 `anyhow::Context` 补上失败步骤。 [INI 读取器](../../src/rules/ini_value.rs) · [资源错误](../../src/assets/error.rs) · [客户端初始化](../../src/app/initialize.rs)
- 每个 `unsafe` 块和 `unsafe impl` 写明 `// SAFETY:` 条件，公共 `unsafe fn` 提供 `# Safety`。处理 `Result` 和 `#[must_use]`，把有副作用的操作放在断言外。需要覆盖 lint 时优先用带理由的 `#[expect(...)]`。 [工作区 lint](../../Cargo.toml)

## 测试与工具

- 回归测试覆盖生产入口和触发边界，名称写出可观察结果，例如 `a_pointer_in_the_zoom_border_is_off_the_cursor`。Rust 测试沿用所在 crate 的 `#[cfg(all(test, tests_<group>))]` 分组，使用完整模块路径过滤，并确认运行实际匹配到测试。 [指针测试](../../src/app/input/hardware_cursor.rs) · [测试分组](../../tools/README.md#test-groups)
- Python 工具沿用周围脚本的 `snake_case`、`pathlib.Path` 和参数列表形式的 `subprocess.run`。外部命令失败时显式传递错误。平台差异集中在适配边界，通过目标 `cfg` 选择对应实现。 [测试分组工具](../../tools/group_tests.py) · [文件系统适配](../../src/util/file_system.rs)

```sh
rustfmt --edition 2024 --check src/app/input/hardware_cursor.rs
python -m tools.cargo_run -- test -p vera20k --lib app::input::hardware_cursor::
```

## Switch 分支补充

- Switch 专用依赖、链接和打包逻辑放在 `platform/switch/`。共享代码里的 Horizon 分支应注明系统能力及受影响的调用。跨 Rust/C 的接口沿用现有 `platform_*` 名称，协议结构用尺寸和偏移断言检查。修改平台路径后用 Switch 构建入口验证，画面和输入以实机或模拟器观察为准。 [Switch 构建说明](../../platform/switch/README.md) · [C 协议布局](../../platform/switch/fps_report.c) · [光标适配](../../src/app/input/hardware_cursor.rs)
