# SHP 绘制局部姿态复用 — 第三轮，2026-10-20

## 范围与契约

本轮继续优化插值原型，不改模拟架构。生产改动仅在
`src/app/presentation/instances/shp.rs`：`build_shp_instances` 为一个实体的一次
只读绘制读取一份新的 RenderPose，将它的 screen、visual_lift、body、barrel、
recoil 传给原有消费者。姿态不跨绘制调用保留，也不建立 frame→entity 的全局缓存。

| 原有路径 | 本轮前完整姿态读取次数 | 本轮后 |
| --- | ---: | ---: |
| 可见步兵 / SHP Foot 的位置、升降、帧朝向 | 3 | 1 |
| 屏外 Foot 的位置和升降（后续被裁剪） | 2 | 1 |
| 带 VXL 炮塔建筑的位置、炮塔/炮管/后坐 | 4 | 1 |
| 无 VXL 炮塔的普通建筑 | 1 | 1 |

帧选择只抽出一个接受已采样 body facing 的私有 helper，仍使用原高字节及原有
序列、伪装、移动、跳跃喷射目标方向等规则。对外的 `resolve_object_shp_frame`
包装器继续自己读取新姿态，不复用其他调用的旧值。屏幕裁剪条件、精灵帧计数、
图集/纹理、绘制机会、炮塔顺序、光照、深度和附属对象流程均不改。

本轮不改变完整世界捕获、不跳过静止对象、不改变可见性或采样频率、不预测。
不改存档、命令、RNG、开关和单逻辑帧显示延迟，也没有触及 ra2-mono。

## 验证方法与证据边界

新增 `interpolation/shp_reuse_tests.rs`，冻结本轮前的逐字段重复读取策略，与
新的一次读取/局部字段复用策略比较完整已消费输入（anchor、帧朝向、炮塔朝向、
炮管、后坐及 active）。浮点比较包含位值，不只比较位置或状态哈希。

覆盖五种类别/绘制路径、frame 0/1/17/u32::MAX、ON/OFF、暂停、8 个相位
（含 signed zero、越界、NaN）、同逻辑帧原始字段改变、错过帧：共 760 组输入
对照。原有插值回退/历史/生命周期测试继续运行。新增夹具首轮曾因两组开关复用
已经修改的对象而错误要求再次回退；已改为每组独立副本，初始失败日志保留。

独立 ignored Release 探针使用零售构造的 E1、GAPOWR、GTGCAN，分别复制为
1,000 个带独立 stable ID 的受控读取对象。这不是合法的千对象对战布局。
计时仅包含实体迭代和上述已声明字段组合的读取，**不包含完整 SHP builder、
序列解析、图集、GPU、模拟、捕获或实际呈现，因此不是 FPS 测试**。

旧/新策略在同一个固定优化二进制中，使用相同世界、相位、返回字段及循环长度，
按 AB/BA 交替运行；允许编译器消除旧策略内的重复读取，不在每次旧读取之间插入
black_box 强制制造额外工作。black_box 仅放在外层对象输入和输出边界。每路径
预热 128，计时 512 次呈现式采样循环，每轮 8 对；ON/OFF 分开统计。

完整游戏组帧性能和实机画面仍须另外验收。SHP 步兵、带炮塔建筑的局部收益不能
外推为空军、VXL 车辆或整个插值模块收益，也不能与前两轮百分比相加。

## 测量结果

i5-12600KF / Windows 11 / Rust 1.97.0，Release，3 次完整探针运行；每种对象、
每个 ON/OFF、每种策略有 24 个运行均值，以下取中位数。共保留 288 条记录和
147,456 次计时采样循环，不剔除异常样本；每次循环遍历 1,000 个对象。

| 零售对象 / 路径 | 模式 | 重复读取 | 局部复用 | 被测字段组合时间下降 |
| --- | --- | ---: | ---: | ---: |
| E1，步兵 | ON | 0.17469 ms | 0.07084 ms | 59.4% |
| E1，步兵 | OFF | 0.07109 ms | 0.03033 ms | 57.3% |
| GTGCAN，建筑体素炮塔 | ON | 0.22146 ms | 0.07130 ms | 67.8% |
| GTGCAN，建筑体素炮塔 | OFF | 0.09339 ms | 0.03263 ms | 65.1% |
| GAPOWR，普通建筑对照 | ON | 0.06803 ms | 0.06682 ms | 约 0.0012 ms，非主要收益 |
| GAPOWR，普通建筑对照 | OFF | 0.02902 ms | 0.02823 ms | 约 0.0008 ms，非主要收益 |

全部 144 对策略运行的权威哈希一致。E1 ON 的旧运行均值范围为
0.17333–0.17684 ms，新为 0.07004–0.10453 ms，较慢新样本仍保留；GTGCAN ON
旧为 0.22048–0.22425 ms，新为 0.07063–0.07214 ms。普通建筑原本就只读取一次，
微小差值可能含分支/编译布局/环境波动，不称为重要提速。

这是每批一千对象约 0.10–0.15 ms 的局部节省，而不是整个绘制快了 60%–68%。
大世界全局捕获在本轮完全未改；第二轮的 20k 捕获测量不能作为本轮新结果。
原有 body+status+coordinate 配对探针的调用组合也没改成本轮组合，不混用数字。

## 已完成的检查

- 新增 760 组完整已消费字段对照通过。
- Release `render_interpolation`：29 passed、5 ignored；上述新性能探针显式运行
  3 次通过，不把其他 ignored 项称作已运行。
- Release `app::presentation::`：240 passed、86 ignored，包含原有建筑 VXL 提交、
  SHP 帧/阴影、历史和呈现生命周期的聚焦测试，非完整套件。
- 5 项显式离屏 GPU 检查通过：Kirov 插值/第二帧上传、全局调色板、降落伞 remap、
  维修附属效果。不是完整 SHP builder 旧/新画面比较，更不是实机画面验收。
- 相关文件 `rustfmt --check` 通过。源代码的局部复用没有改变计算公式；没有整帧
  像素等价或实机 FPS 的声明。
- 正常 `cargo build -p vera20k --bin vera20k` 通过（45.16 秒），已有 3 条警告未改；
  `git diff --check` 通过。并未运行全部测试套件。

## 复现及原始数据

证据目录：`logs/interpolation-opt3-20261020/`。

```powershell
$env:RA2_DIR = 'H:\红警尤里复仇可观看动画\红警2'
$env:VERA20K_REQUIRE_RETAIL_INI = '1'
$env:VERA20K_REQUIRE_RETAIL_ASSETS = '1'
cargo test -p vera20k --lib --release --no-default-features `
  app::presentation::interpolation::shp_reuse_tests::render_interpolation_shp_local_reuse_cpu_probe `
  -- --ignored --exact --nocapture --test-threads=1
```

实际计时固定使用该目录的 `tests.exe`，三次日志为 `probe-r1..3.log`。
同目录 `summarize.ps1` 为只读汇总程序，`final-comparison.json` 保留范围和配对差值。
测试 exe SHA256：`A75719E0EA8AADC2CAA58EE33538D0A02B7A5D954907F8A7CE938736BFA9CAF5`。
本轮前 `before-shp.rs` 和 `before-interpolation.rs` 保留为可核对快照；初始失败
日志与最终成功日志均保留。本轮没有提交、推送或创建 PR。

## 实际游戏产物

已确认没有正在运行的 vera20k 进程，并核对替换前为第二轮的已知哈希，随后更新
`H:\Trae CN\vera20k\target\debug\vera20k.exe` 与匹配的 PDB：

- 正常 dev opt1 游戏 exe：32,564,736 bytes。
- SHA256：`411259BC584874E1145514CB9BC754075176A656397EE26785A26A4FDD09C609`。
- 替换前 exe/PDB 位于证据目录 `runtime-before.exe` / `runtime-before.pdb`；该旧
  exe SHA256 为 `0F65240028ED3A714228E8254D12AD906A0B61C1F85FE94B97C363F7518D3239`。
- 本轮最终源文件快照为 `final-shp.rs`、`final-interpolation.rs`、
  `final-shp_reuse_tests.rs`。`history.rs`、`scene.rs`、第二轮投影 helper 的哈希
  均与本轮前一致；interpolation.rs 的本轮差异仅挂接测试模块。
- `runtime-deployment.json` 保存实际复制时间、路径、exe/PDB 校验和。

正常游戏是 opt1，不是被测的 Release 测试二进制。未代用户启动游戏，保留全屏、
游戏资源目录、FPS/TPS 显示、Options 开关和 Ctrl+Alt+I。实机验收由用户运行。
