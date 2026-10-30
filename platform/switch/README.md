# VERA20k Switch 移植

这是原生 Horizon / 大气层移植入口，使用独立 Cargo workspace 构建。
当前分支为 `feature/switch-wgpu29`，客户端版本为 `0.0.3 beta`。本分支也调整了共享
渲染代码及桌面依赖；Horizon 专用依赖补丁通过生成的 Switch Cargo 配置启用。

客户端包含菜单、战役、遭遇战、音频和账号 SaveData 支持，仍处于实验阶段。
菜单与部分对局已有 Eden 冒烟验证，完整战术操作、长时间运行稳定性和真机兼容性
仍需验证。已知问题包括帧率波动、冻结或闪退报告，以及部分单位显示问题。

本版 Horizon 输入新增 USB Hub 键鼠、右摇杆镜头平移和双指中点拖动地图；
鼠标键位同时支持中键与前进/后退，滚轮复用现有滚轮语义。剧情影片会从资源根
读取 UTF-16LE `subtitlemd.txt` / `subtitle.txt`，按影片自身帧时钟绘制字幕。
零售字幕、MIX 和其他游戏资源不入 Git。

当前 Switch 客户端由主线程调度共享 Rust 游戏核心和 wgpu/Vulkan 绘制；Rayon
三线程池用于部分寻路和目标扫描，体素图集用 scoped `std::thread` 分摊姿态光栅化，
没有独立模拟/渲染线程。实际性能取决于地图、资源和运行环境。

## 完整客户端入口

`--client` 直接编译共享 `vera20k` 客户端库，以 Horizon 窗口后端驱动同一个
`App`，保留完整菜单、对局、输入、音频与存档代码。此入口没有 GPU 探针的
60 秒超时。整体编译、链接和菜单到对局的流程已有初步验证。

```bash
python3 build.py --client
```

当前客户端版本为 `0.0.3 beta`，NSP 命名格式为
`dist/vera20-client-v<VERSION>.nsp`，本版产物为
`dist/vera20-client-v0.0.3.nsp`，标题 ID
`0100f52556458000`。运行日志在客体 `sdmc:/switch/vera20k/logs/client.log`，
错误结果在 `sdmc:/switch/vera20k/client.last-error.txt`。资源优先从包内 RomFS
读取，配置和日志目录为
`sdmc:/switch/vera20k`。存档根为 `vera20k-user:/saves`，挂载到所选账号的
系统 SaveData；NACP 要求选择用户，预设 256 MiB 数据空间和 64 MiB 日志空间。
随构建生成的 NRO 留作诊断，当前客户端启动要求 NSP 的账号上下文。
窗口后端来源与当前手柄映射见 [winit-horizon/UPSTREAM.md](winit-horizon/UPSTREAM.md)。

完整客户端的 NSP 主机菜单图标和 NRO 图标统一使用 `icon.jpg`，
格式为 256×256 RGB JPEG。

## 把游戏资源一起打入 NSP

完整客户端链接构建默认把本机资源目录打入 Program RomFS，`--check` 只检查编译。
可显式指定不同资源目录：

```bash
# 从仓库根目录执行。
cd platform/switch
python3 build.py --client --nsp \
  --nsp-assets target/emulator/master-xdg-data/eden/sdmc/game
```

也可以把 `--nsp-assets` 指向单独准备的资源目录。目录根必须有非空的
`ra2.mix`、`ra2md.mix`（小写文件名）；指定目录的全部文件和子目录原样入包，
因此请只放需要随包携带的资源。原始资源留在本机，不加入 Git。

客户端链接构建默认打入游戏资源，并按 `vera20-client-v<VERSION>.nsp` 命名。
NSP 大小取决于实际资源目录，旧版固定大小和 SHA256 不再适用于 `0.0.3 beta`；
每次构建后请以 `dist/vera20-client-v0.0.3.nsp` 的 `stat` / `sha256sum` 结果为准。
构建也会生成诊断 NRO；NRO 本身不内置这些资源。

客户端挂载 `romfs:/`；没有配置覆盖时优先从包内读取资源，找不到包内资源时
回退到 `sdmc:/game`。配置、日志和录像保存在
`sdmc:/switch/vera20k`；普通存档改用账号 SaveData。首次发现旧 SD `saves` 目录时，
导入其中 `.bin` / `.sav` 文件，保留原件及账号下已有文件；导入完成标记按账号保存。
旧 `config.toml` 的 `[paths] ra2_dir` 继续具有优先权；
测试包内资源时，应先备份并移开旧配置，让默认资源发现生效。

已在独立 Eden 配置下验证：SD 中没有 `game` 目录，NSP 能启动完整主菜单并
进入 URBAN 遭遇战，完整侧栏和单位可见。首次配置需沿用现有的
`disable_macro_jit=true` / `disable_macro_hle=true`，同时将各自的
`\default` 设为 `false`，否则 Eden 会恢复默认值。
`run_eden.sh --client` 仍是旧 NRO 诊断入口；验证当前客户端须将 Eden 的 `-g`
指向上述 NSP，以提供账号存档上下文。

USB 键盘、鼠标仅在底座/TV 模式启用；掌机模式使用手柄和触摸。
切回掌机会释放 USB 按键状态。手柄 A/B、单指触摸和 USB 鼠标共用点击事件，
任一来源释放时不会打断其他来源仍按住的按钮。双指拖图整段抑制触摸点击。
编辑框获得文本焦点时自动打开系统软键盘，实体 Plus 也可再次唤起，且不再合成为
Escape；确认后的 UTF-8 文本提交给当前聊天、玩家名或联机输入框。键盘是阻塞调用，
由事件轮询代为执行，因此获取焦点不会卡住状态转换；双指平移手势会取消待弹出的
键盘。USB 键盘本身仍可直接输入。

`v0.0.3 beta` 手柄映射：左摇杆移动指针，右摇杆平移镜头；十字键上／下选择
编队 1／2，左为警戒（G），右为路径点模式（Z）。选中单位后，按住 R 再按
上／下可建立编队 1／2；按住十字键右并依次点击地面设置路径点，松开后执行。
X 为部署（D），Y 为切换同盟（A），L／R 为 Shift／Ctrl，ZL／ZR 为 Tab／Space，
Plus／Minus 为 Escape／F10。以上功能遵循当前游戏热键配置；剧情用实体 Plus
跳过，不添加屏幕 `+` 按钮。

Horizon 对局呈现默认设置 30 FPS 上限；可在主菜单 `Options → Vera Options →
Developer Options` 勾选 `60 FPS Limit`，切换为 60 FPS 上限，取消勾选恢复 30。
选择保存在 `VERA.INI` 的 `[VERA] Switch60Fps=`，切换即时生效，只影响画面
呈现，不改变模拟速度或屏幕刷新率；实际帧率仍取决于性能。原有 `FPS Counter`
保持每次启动默认隐藏。所有呈现路径都接入
SaltyNX 共享页上报，无服务时跳过；已有 NX-FPS 上报者活跃时交由它负责。
外部插件读数、实际帧率及用户存档隔离仍需用最终 NSP 在真机确认。

FPS 上报诊断记录在 `logs/client.log`，日志目标为 `switch::fps`，包括共享页连接、
IPC/映射错误和首次有效读数。缺少 SaltyNX 不阻止启动，后台每 5 秒重试。
安装版 NSP 的 Session 限制为 `1/1` 时，上报器只记录 `0x10801` 并跳过；
不要尝试用特权 SVC `0x7E` 提升限额（真机返回 `2168-0008` 并终止游戏）。
主机上的针对性 C 检查可从仓库根运行：

```bash
cc -std=gnu11 -Wall -Wextra -Werror -Iplatform/switch/tests/include \
  platform/switch/tests/fps_report_test.c -o /tmp/vera-fps-test
/tmp/vera-fps-test retry
/tmp/vera-fps-test publish
/tmp/vera-fps-test publish-offset
```

这些检查验证上报器的状态和字段写入，不能代替真机插件读数验收。

资源打包不会补齐源目录缺少的音乐、影片或字幕。按需准备
`theme.mix`、`thememd.mix`、`movmd03.mix`、`movies01.mix`、`movies02.mix`，
以及 UTF-16LE `subtitlemd.txt`、`subtitle.txt`。
随机地图生成仍有向资源目录写 `RandMap.Sed` / 预览文件的路径，在只读 RomFS
模式下尚未适配，当前请使用已有地图。

## 已实现并验证

`dist/vera20k-core.nro` 是运行 **原项目 vera20k-core** 的移植探针，
不是之前独立的 Rust hello-world，也还不是可玩的图形客户端。

早期核心探针已在 Eden 0.2.1 / Vulkan 验证：

- Rust `std`、文件系统、线程、TLS、系统随机数。
- 生产 MIX 文件加载与文件名查询（Horizon 使用堆内存后备映射）。
- 生产 INI 解析与 CRC。
- Horizon 本地文件时间显示；newlib 不提供 `newlocale/strftime_l`，使用本地时间的数字日期。
- 三线程 Rayon 池及归约。
- 生产 `Simulation` 构造、状态哈希和 16 次 `SimRuntime` 空场景帧事务。
- 原生 libnx 默认窗口、EGL / GLES2 上下文和 1280×720 渐变三角形。
- 手柄 A 键输入及 `+` 正常退出；独立输入层覆盖面键、肩键、菜单键和十字键。
- 检查零售资源根 `sdmc:/game` 中两套主 MIX；存在时通过生产 `AssetManager`
  挂载档案并查找 `rulesmd.ini`。早期核心探针只验证了资源缺失分支。

主机与 Switch ARM64 的状态哈希一致：

| 阶段 | 两端哈希 |
| --- | --- |
| 构造，seed=12345 | `cf10ddbe65985acf` |
| 16 帧后 | `3789926a66da78b4` |

这是空场景与指定路径的兼容性证据，不代表零售地图、完整战斗或全部跨平台确定性已经验证。
2026-10-22 已通过 FTP 取回真机核心 NSP 报告并执行 `verify.py --format nsp --report ...`：
上述核心检查、A 键输入和正常退出全部通过，两个状态哈希与主机一致。
独立 Rust 基础测试 NRO 也已通过真机验证；核心 NRO 与基础测试 NSP 尚未取得真机报告。

## 构建与运行

准备 devkitA64、libnx 及 Switch portlibs，并用 `DEVKITPRO` 指向 SDK 根目录。
本入口还需要：

```bash
rustup toolchain install nightly-2026-10-04 --profile minimal --component rust-src,rustfmt,llvm-tools-preview
```

克隆固定版本的 Horizon 适配代码，并用 `VERA_SWITCH_UPSTREAM` 指定其路径：

```bash
git clone https://github.com/JohnUzoka/bevy-horizon.git bevy-horizon
git -C bevy-horizon checkout 3eac779b232ee47cd0eb957676c64386ac7b0f1d
export VERA_SWITCH_UPSTREAM="$(cd bevy-horizon && pwd)"
```

GPU 和完整客户端构建还需要本地化的 NVK 静态库 `libvulkan_bh.a`。
用 `VERA_SWITCH_NVK_LIB` 指定库所在目录；驱动及模拟器补丁来源见
[UPSTREAM.md](UPSTREAM.md)。这些补丁需要单独准备，构建脚本不会自动构建驱动。
用 `EDEN_APP` 指定 Eden 可执行文件；GPU 和客户端诊断模式需要支持相应补丁的版本。

```bash
# 从仓库根目录执行。
cd platform/switch
python3 build.py
bash run_eden.sh
```

键盘 A 对应 Switch A，键盘 M 对应 `+`。退出后报告在：
`target/emulator/xdg-data/eden/sdmc/switch/vera20k/core-probe-nro.txt`。
截图在 `target/emulator/evidence/input-layer.png`。

常规核心验证可使用 Eden 0.2.1 发布版。
另行构建 Eden master 用于排查 NVK 图形兼容性，不是 Vera 核心构建的前置步骤。

启动脚本使用独立的 Eden 配置。首次运行若该配置没有 `prod.keys`，
会依次从 `SWITCH_KEYS`、`~/.switch/prod.keys`、默认 Eden 的
`~/.local/share/eden/keys/prod.keys` 导入本机已有密钥，副本权限为 0600。
密钥副本保存在被 Git 忽略的 `target/emulator/xdg-data/eden/keys/`。
若这些位置均无密钥，脚本会明确报错。这些是可配置的默认查找位置，不要求特定的
个人目录结构。密钥内容不应提交到 Git 或随安装包分发。

`--gpu` 和 `--client` 模式从 `VERA_RETAIL_DIR` 读取原版游戏资源，
未设置时使用通用默认目录 `$HOME/RA2`。例如：

```bash
VERA_RETAIL_DIR=/path/to/retail bash run_eden.sh --gpu
```

`build.py --host-test` 在主机执行同一个生产核心探针。
`build.py --check` 仅执行交叉编译检查。
`build.py --check-libc-abi` 直接复用固定上游版本的 ABI 检查器，在本机比较 SDK C 头与 Rust 类型。
需要 Python 的 `pyelftools`；构建后检查最终 ELF 的 TLS 对齐。
运行 `python3 verify.py` 可核对已保存的模拟器报告、按键/退出状态与主机状态哈希。

## 生产 GPU 上下文接入

`dist/vera20k-gpu.nro` 通过 Switch 原生 `NWindow` 直接调用项目的
`src/render/gpu.rs`，不复制 GPU 初始化和取帧实现。
当前分支已升级到 wgpu 29.0.4 / Vulkan-only；编译检查通过，仍需在 Eden 和真机
复验最终 NSP。以下是 wgpu 27 阶段已建立的生产渲染路径回归证据：带补丁的
Eden master 通过 NVK 使用生产 `BatchRenderer` 显示 12 个合成 RGBA 精灵。
像素回读覆盖透明边缘、纹理颜色、全部实例以及相机平移和 2 倍缩放。
同一 GPU 探针还从模拟器内的 `temperat.mix` 经生产
`AssetManager`、`load_theater` 和 `load_tile_images` 加载 INI、三套 PAL 与
5 个 TMP。地图 fixture 再通过生产 `MapFile` 解析 9 条 `IsoMapPack5` 记录，
经 Size diamond 与 `ResolvedTerrainGrid` 物化 15 格，并由主循环以生产
`build_visible_instances` 和逐像素 R8 Z 深度绘制全部 15 个 TMP 地形实例。
最终运行呈现 2007 帧、约 33.4 秒后以 A 键和 `+` 正常退出。截图见
`target/emulator/evidence/production-terrain.png`。
这个合成 fixture 仍用于验证颜色、UV 和逐像素 Z 深度回归。

同一入口还从 `sdmc:/game` 的生产 `AssetManager` 挂载真实
`ra2md.mix`、`ra2.mix` 和数字媒体 MIX，加载零售 TEMPERATE theater。
`sdmc:/switch/vera20k/maps/ai_dred_bombard.map` 是 74,307 字节的真实
`IsoMapPack5` 地图，解析出 4,560 格和 4,560 条 lookup，全部在 838 个
theater slot 内解析，fallback 为 0；其中 234 格使用 `water09.tem`
（tile 322）。主循环把文件地图的 UV 绑定到零售图集并绘制 1,749 个初始可见
实例。实际水陆像素回读分别为 `[17,20,25,255]` 和 `[85,111,17,255]`，
最后呈现 2,853 帧，A 键计数 1，`+` 正常退出。这证明零售 MIX、TEMPERATE
地形、文件地图与主画面图集绑定已连通；SHP/VXL、游戏对象和完整客户端仍未接入。

```bash
python3 build.py --gpu
VERA_RETAIL_DIR=/path/to/retail bash run_eden.sh --gpu
python3 verify.py --probe gpu
```

GPU 探针使用独立的 `master-xdg-{data,config}` 配置，报告在
`target/emulator/master-xdg-data/eden/sdmc/switch/vera20k/gpu-probe-nro.txt`，
详细 wgpu 日志为同目录的 `gpu-wgpu.log`。程序最多运行 60 秒后自动退出；
校验器还要求运行中测试过 A 键。合成地形截图为
`target/emulator/evidence/production-terrain.png`，零售地图运行截图为
`target/emulator/evidence/retail-terrain-final.png`；完整桌面运行证据为
`retail-terrain-live.png`，客户区裁剪为 `retail-terrain-live-crop.png`。

此模式使用带本地 SDL3 生命周期修复、立即数乘法支持的 Eden master，
并在配置中关闭 macro JIT/HLE；补丁来源见 [UPSTREAM.md](UPSTREAM.md)。
核心模式仍使用 Eden 0.2.1 发布版。GPU 模式目前只生成 NRO。

**启动模式：** 相册/主页菜单直接启动 NRO 是 `LibraryApplet`，内存与 Vi/NV
服务权限受限；完整零售图集会快速失败并提示使用 Application 启动。请优先从
应用菜单启动已安装的 NSP，或使用标题覆盖/转发器启动 NRO。GPU 初始化后加载
theater、地图、对象和实时场景时会持续呈现进度条，避免长时间纯黑屏。
如需保留 Mesa 详细日志，设置 `VERA_MESA_DEBUG=1`；默认不启用。
该入口使用生产 `BatchRenderer`，实例属性和合成 RGBA
纹理采样与完整绘制已通过。主画面现在直接绘制生产地图加载链生成的地形；
此前相机数据在片段阶段正确、在顶点阶段为零；
Horizon NVK 现在通过 `third_party/mesa-switch-graphics-ubo.patch` 的可选路径
把该 UBO 直接绑定为常量缓冲区，并将选项计入着色器缓存键。
生产 theater/MIX、`MapFile`、`IsoMapPack5`、Size diamond、
`ResolvedTerrainGrid`、`load_tile_images` 和主画面地形绘制链路已接入；
零售地图和零售 TEMPERATE 地形已接入；SHP/VXL、完整客户端和声音尚未接入此入口。
退出时仍有 NVK VA teardown 警告，资源释放还需排查。

## 正式 NSP 与 FTP 真机测试

通过 `SWITCH_KEYS` 指定打包所需的设备密钥文件，或使用打包器的默认查找位置
`~/.switch/prod.keys`。设备密钥不放入工程或分发包。

```bash
python3 build.py --nsp
```

生成 `dist/vera20k-core.nro` 和 `dist/vera20k-core.nsp`。
NSP 的应用 ID 为 `0100f52556455241`，直接包含核心程序，不依赖 SD 卡上的 NRO。
打包器用独立 hactool 解密并校验 Program/Control/Meta NCA 的内容哈希与 NPDM，
仅容许自制包预期的固定密钥/ACID 未签名项。核心 NSP 已安装并通过真机运行验证。

```bash
SWITCH_FTP_HOST=your-switch-hostname
python3 deploy_ftp.py --host "$SWITCH_FTP_HOST" --port 5000
```

上传核心和基础 Rust 测试的 NRO/NSP；NRO 放在各自的 `/switch/` 子目录，NSP 放在 `/nsp/`。
上传先写临时文件，再下载逐字节核对后更名；已有不同内容的目标文件会保留备份。
传输清单为 `target/hardware/deployment.json`。

上传完整客户端（自带零售资源的 NSP，即承载剧情影片的包）加 `--client`：

```bash
python3 deploy_ftp.py --host "$SWITCH_FTP_HOST" --port 5000 --client
```

它按 `dist/vera20-client-*.nsp` 取最新包，随包生成并上传 `.sha256`，
同样走临时文件加逐字节回读；不要用手动复制绕过这道校验。

分别从 hbmenu 启动 NRO、从主菜单启动已安装的 NSP，按 A 后按 `+` 退出。
重开 FTP 后下载报告：

```bash
python3 deploy_ftp.py --host "$SWITCH_FTP_HOST" --port 5000 --fetch-reports
python3 verify.py --format nro --report target/hardware/reports/core-probe-nro.txt
python3 verify.py --format nsp --report target/hardware/reports/core-probe-nsp.txt
```

NRO 与 NSP 分别写 `sdmc:/switch/vera20k/core-probe-{nro,nsp}.txt`。

## 环境核对与后续移植缺口

| 项目 | 当前情况 |
| --- | --- |
| devkitA64 / libnx / newlib | 构建前需准备并配置 `DEVKITPRO` |
| Rust 标准库 / ARM64 libc | 已构建；对 SDK 头文件的 348 项 ABI 对比为 0 不匹配 |
| MIX、INI、模拟核心 / Rayon | 核心 NSP 已通过模拟器和真机验证；范围为合成资源与空场景 |
| Switch SDL2 / EGL / GLES 库 | 早期核心图形探针需要对应 portlibs |
| Switch 原生窗口 / EGL / GLES2 图形层 | 已在 NRO 和 NSP 的 Eden 运行中画出 1280×720 画面；尚不是 Vera 生产渲染器 |
| wgpu 29.0.4 | 桌面使用 29；Switch 固定 29.0.4、Vulkan-only 并使用 Horizon 补丁。核心与客户端编译通过，真实地图和完整客户端仍需最终 NSP 真机复验 |
| rodio 0.22 / cpal 0.17 | 使用 Horizon 补丁后依赖编译检查通过；尚未链接和播放声音 |
| 桌面 winit / Wayland / rustix | 无法直接用于 Horizon；包含窗口栈的实际依赖检查在 rustix 0.38 报错，winit 本身也没有 Horizon backend |
| Switch NVK 静态驱动 | 已从 Mesa `606441c90e` 交叉编译，生成本地化 `libvulkan_bh.a`；实例、能力枚举、Vi 表面和逻辑设备已通过 |
| NVK 最小探针 | 本地 Eden master `989768a` 加入低 32 位 `IMUL (imm)` 支持并关闭 macro JIT/HLE 后，旋转三角形、30 秒呈现和自动退出通过 |
| 原版 RA2 / YR 游戏资源 | 已接入本地 YR 资源；真实主 MIX、TEMPERATE theater 和文件地图地形已通过，游戏对象与完整启动尚未验证 |
| 设备密钥配置 | 由 `SWITCH_KEYS` 或默认查找位置提供，不随源码分发 |

依赖检查可以复现：

```bash
# 包含桌面窗口栈：当前预期失败，显示实际的平台缺口
python3 build.py --check-client-deps

# 不包含 winit，按 Switch 原生前端选择 Vulkan-only：当前通过
python3 build.py --check-client-deps --without-winit
```

检查从桌面 Cargo.toml 读取依赖，并锁定桌面 Cargo.lock 中的对应版本。
上游的 libc、memmap2、wgpu 29.0.4、raw-window-handle、cpal 补丁仅通过生成的 Switch
配置启用。当前分支的共享渲染 API 和桌面依赖也有调整，桌面平台兼容性需要单独验证。
补丁源码和许可来源见 `UPSTREAM.md`。

顶层 MIX 使用 mmap，影片解码直接使用 MIX 内的共享切片，不再复制整段 BIK；
但进程 RSS 仍会计入实际访问过的文件映射页，释放影片对象不等于立即归还这些页。
地图加载会复用 LZO 输出容量并避免拼接整段 Base64，仍需用真机日志测量峰值内存。
此探针不验证大型纹理图集、所有文件操作或真实对局性能。

下一阶段是接入生产地形与 SHP/VXL 渲染，并逐步接通客户端事件、菜单与音频。
需要原版资源后再执行真实地图加载、渲染和游戏操作验证。
