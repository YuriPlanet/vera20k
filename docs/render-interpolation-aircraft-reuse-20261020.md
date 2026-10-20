# 飞机机体与阴影采样复用 — 第四轮，2026-10-20

## 改动

`RenderInterpolation::sample_entity_and_coord` 一次读取完整当前姿态、一次验证历史，
返回原有 `sample_entity` 的姿态和 `entity_coord` 的调用方坐标结果。保留全部字段
同帧变更检查；坐标仍用原有 f64 运算再 round，不从屏幕位移或已取整位置反推。
即使是历史与资格不一致的异常组合，也保留两个旧接口原本不同的 eligibility gate。

`units.rs` 的飞机阴影准备将这份新鲜姿态交给后续机体，单次 draw 内复用，不跨帧
存储。飞机/沉没/NoShadow/资源及地形入口保持不变；普通车辆继续在原有裁剪后才
读取姿态，不新增屏外读取。阴影几何、桥面邻居检查、坡度、朝向、深度、图集与
机体/阴影裁剪公式不改。若已读姿态后才发现没有受支持的阴影 locomotor，也可
把这份姿态交给机体，而不是再读一次。

有阴影、机体通过裁剪的路径：ON 完整姿态读取 3→1；OFF 为 2→1。机体被裁剪
的阴影准备路径：ON 为 2→1，OFF 原本就是 1。无阴影的普通车辆保持原路径。

## 受控 CPU 探针

i5-12600KF / Windows 11 / Rust 1.97.0，Release。同一固定二进制内对照未改的旧
独立接口与新组合接口，AB/BA 交替，每路径预热128、计时512次；每批1,000个
零售构造的受控对象克隆，不是合法千飞机战斗。3次完整运行、每用例每模式24对，
保留480条记录、245,760次计时迭代，不剔除异常样本。

**仅测姿态与坐标读取组合，不含完整 builder、阴影几何、图集、捕获、模拟、GPU
或实际呈现，不是游戏 FPS。**

| 路径（每批1,000对象） | 旧 ON | 新 ON | 局部时间下降 |
| --- | ---: | ---: | ---: |
| ORCA 飞行，机体+阴影输入 | 0.20249 ms | 0.10979 ms | 45.8% |
| ORCA 地面，机体+阴影输入 | 0.17285 ms | 0.09873 ms | 42.9% |
| ORCA 机体被裁剪后的阴影输入 | 0.13863 ms | 0.10591 ms | 23.6% |
| V3ROCKET 飞行，机体+阴影输入 | 0.19647 ms | 0.10758 ms | 45.2% |
| MTNK 无飞机阴影，对照 | 0.08173 ms | 0.08142 ms | 基本不变 |

OFF 的飞行 ORCA 为 0.08036→0.05090 ms；V3ROCKET 为0.07701→0.05047 ms。
全部240对策略的权威哈希一致。该局部收益不能外推为整帧增益或与其他轮次相加。

## 验证和证据

- 57,600组完整 RenderPose（含全部浮点位值）及取整坐标对照：高度、桥面、帧号
  回绕、空/过期/同帧变更历史、limbo资格、ON/OFF、暂停、无限速度及异常相位。
- 使用原版 aircraft_shadow corpus，将两路采样结果送入真实阴影函数，比较全部
  anchor位值、world_z、facing、slope；OFF/相位1另与原版输出核对。
- Release插值31通过、6 ignored；呈现层242通过、87 ignored。新手动探针已显式
  运行3次，其他 ignored 项未冒称执行。原版阴影测试1项通过，Kirov离屏GPU1项通过。
- 未更改模拟、命令、RNG、存档、选项或插值时序；未改 ra2-mono。未进行整帧旧/新
  像素或实机流畅度验收，未提交/推送/创建PR。

证据：`logs/interpolation-opt4-20261020/`。固定`tests.exe`，`probe-r1..3.log`，
`summarize.ps1`、`final-comparison.json`保留原始记录、配对差和范围；本轮前源文件
为`before-interpolation.rs`与`before-units.rs`。

复现探针：

```powershell
cargo test -p vera20k --lib --release --no-default-features `
  app::presentation::interpolation::aircraft_reuse_tests::render_interpolation_aircraft_joint_sample_cpu_probe `
  -- --ignored --exact --nocapture --test-threads=1
```

需设置`RA2_DIR`及`VERA20K_REQUIRE_RETAIL_INI=1`。正常游戏opt1构建不等于计时用
Release测试二进制。用户另要求继续坦克方向，将单独建立第五轮基线与尖峰报告。

正常构建45.60秒通过，已有3条警告不变；格式与差异检查通过。已核对游戏未运行，
将正常opt1 exe/PDB更新到`target/debug`，旧版备份为证据目录的`runtime-before.*`，
未代用户启动游戏。该第四轮exe为32,557,056 bytes，SHA256：
`95BC4B89DFB213317F7BDAE6400395A9B596FF9512205C78B15F512A85B8FBDD`。
计时测试exe SHA256：`098E6C78125FB85DB82C01CE56BBEFEDCE8870A474187C39C29D1A825900591B`。

随后第五轮更新了本机程序；第四轮exe/PDB现保存在
`logs/interpolation-opt5-20261020/runtime-before.*`，第五轮报告记录最终程序哈希。
