# 交接手册

给下一个接手的人（或 agent）：这份文档说明**仓库长什么样、怎么构建、当前哪里是好的、哪里有坑**。看完它应该能在半小时内独立跑起来。

---

## 1. 这是什么

VERA20k —— 《命令与征服：尤里的复仇》的 Rust 重写。目标：复刻 `gamemd.exe` 的行为，同时撑到 2 万单位、30 家。

- 引擎是 `vera20k-core`（`src/`）：`util / assets / rules / map / sim / net` + headless/skirmish 根。
- 应用是 `vera20k`（`src/lib.rs`）：`app / render / ui / audio / asset_tools`，重新导出 core 的模块。
- `autoplay/` 是**引擎之外**的策略 crate（`vera20k-autoplay`）：`policy.rs` + `manager/*` + `profile/` + `job/`。
- `tools/` 是构建与验证工具（`tools.cargo_run` 是唯一的 Cargo 入口）。

## 2. 工作树

| 路径 | 分支 | 用途 |
|---|---|---|
| `E:\Projects\vera20k` | `main-ci` | 主检出，构建缓存在它的 `target/owned-worktrees/`，**不要改** |
| `E:\Projects\vera20k-autoplay-v4` | `feature/autoplay-v4` | **当前** |
| `E:\Projects\vera20k-autoplay-v5` | `feature/autoplay-v5` | 参考只读；从 v4 更早的点分出，不含 v4 的上游合并 |

每个工作树都需要 `config.toml`（游戏目录）与 `ini/`（retail INI），两者都在 `.gitignore` 里，用 `python -m tools.prepare_checkout` 生成。

## 3. 构建与测试

**唯一入口**：`python -m tools.cargo_run -- <cargo 参数>`。它串行化所有工作树的构建，别并行跑。

```bash
# 构建游戏（fast 出可跑版本，release 出计时版本）
python -m tools.cargo_run -- build --profile fast --bin vera20k

# 引擎测试（大多数逻辑测试在这里）
python -m tools.cargo_run -- test -p vera20k-core --lib --profile fast -- <过滤>

# 应用测试
python -m tools.cargo_run -- test -p vera20k --lib --profile fast -- <过滤>

# 策略 crate 测试
python -m tools.cargo_run -- test -p vera20k-autoplay --lib --profile fast
```

必须知道的四件事：

1. **一律 `--lib`**。仓库规定：`cargo test` 只跑库测试，不跑整树。
2. **加 `--profile fast`**。这个缓存路径下 `debug` profile 构建不了 C 依赖（ring / libmimalloc-sys）：`cl: 命令行 error D8050 :无法执行"c1xx.dll"`。`fast` 里有现成的 C 产物。放在 cargo 参数之后。
3. **测试分组是 feature**：`test-focus,tests-app,tests-app-frontend,tests-app-input-a-c,tests-app-input-d-z,tests-app-match,tests-app-presentation*,tests-render-a-r,tests-render-s-z,tests-root,tests-ui,tests-asset-tools,tests-audio`。过滤器前缀能映射到组时 runner 会自动加 `--features`；映射不到的（如 `sim::control::...`）**不会**被编译进去——`sim` 的测试在 **core** crate，要 `-p vera20k-core`。
4. **构建/测试后清理缓存**：`python -m tools.cargo_run --trim-cache`。空间紧时删 `<target>/owned-worktrees/<hash>/incremental`。

零售 INI 相关的测试在没有 `ini/` 时会跳过并打印 `SKIPPED`；要让缺失变成失败，设 `VERA20K_REQUIRE_RETAIL_INI=1`。

## 4. 架构（改代码前必须知道）

```
sim/behavior  L1 执行：目标词表、单一命令漏斗、托管微操、ready 门
sim/control   L2 控制：棋盘(ControlBoard)、Brain port、预算、投影(Observation)、
                   感知(perception)、选址(placement)、外部契约(external.rs)、托管表(micro)
sim/strategy  L3 决策：assist 大脑（20 文件，~15k 行）——游戏的默认大脑
autoplay/     引擎之外的策略 crate：policy.rs + manager/*（v2 策略）
```

依赖方向 L3 → L2 → L1，有 `architecture_guards.rs` 测试守着。**L2 永远不指名具体大脑**：`sim/control/brain.rs` 只声明 `Brain` trait。

接入：`src/app/input/dispatch.rs` 的 `install_auto_play()` —— 热键装 `AutoplaySession`，默认 `AssistBrain`，`VERA20K_BRAIN=v2` 切 `autoplay::policy::Policy`。会话（`app/match_runtime/autoplay_session.rs`）是**唯一**步进 host 的地方，并通过 `sidebar_projection::cameo_mirror` 把命令的演示半边写进侧栏。

## 5. 当前状态

### 已验证（2026-11-20）

- `--profile fast` 构建通过，`vera20k.exe` 约 86 MB。
- `cargo test -p vera20k-autoplay --lib`：52 passed。
- `cargo test -p vera20k-core --lib -- sim::strategy::assist`：166 passed。
- `retail` → `assist` 改名完成，无行为改动。

### 未验证

- **对局强度**。所有胜负数字都来自 2026-11-02 的 v2 时期（见 `history/status-v2.md`），此后策略改过，数字没有重测。
- **实机**。改完后没有跑过客户端。

### 已知坏的 / 停用的

- **驻扎能力**（`autoplay/src/manager/abilities.rs` 的 `garrison`）：实测在 1v7 lane 上会抽走防线上的步枪兵（own 84 → 17），已在 `propose` 里注释掉。代码保留，带 `#[allow(dead_code)]` 与原因。
- **三处策略规则未接线**：`WALL_BREAKER_PRIORITY`、`MAIN_BATTLE_MAX_ENEMIES`、`SCOUTS_OUT`（`manager/military.rs`）。同上，保留并记录。

## 6. 坑（都踩过）

| 坑 | 现象 | 规避 |
|---|---|---|
| 磁盘 | `Build blocked: 9.51 GiB free ... 12.00 GiB required` | `--trim-cache`；删 `incremental`；别把参照数据放进 git |
| debug profile | `cl: 命令行 error D8050` | 用 `--profile fast` |
| 测试组 | 过滤器没匹配到组 → `Repository tests must select --lib` | 显式 `-p`、显式 `--features`；sim 测试在 core |
| 录像路径 | 游戏写 `logs/ra2.log` 和 `replays/` 到**当前工作目录**，不是 exe 目录 | 启动时 `cd` 到可写目录 |
| 录像 startup | 没有 startup 的录像不能重放 | 只有带 startup 的能重放 |
| 30 席位上限 | `SkirmishLaunchSession::parse` 拒绝 raw slot ≥ 30 | 96 席位的录像本 build 读不了 |
| rules hash | 覆盖 rules/art/VERARULES.INI，任何 INI 处理改动都会变 | 用 `--allow-rules-mismatch` 时靠 state hash 兜底 |
| 重复下令 | 逐帧改判会每帧发命令 | 走 `tracked.rs`，别绕过 |
| 静默跳过的测试 | 依赖 retail fixture 的测试在没有 `ini/` 时直接 `return`，算 PASS | 设 `VERA20K_REQUIRE_RETAIL_INI=1` |

## 7. 怎么跑

```bash
# 游戏（从工作树目录启动，保证 logs/ 与 replays/ 落在可写位置）
cd E:\Projects\vera20k-autoplay-v4
& E:\Projects\vera20k\target\owned-worktrees\<hash>\fast\vera20k.exe

# 无头对局（策略打分）
python -m tools.cargo_run -- run --profile fast -p vera20k-autoplay --bin autoplay-match -- ^
  --ra2-dir "I:\SteamLibrary\steamapps\common\Command & Conquer Red Alert II" ^
  --map rmap38631.yrm --opponents 29 --difficulty easy --seed 24301 --ticks 36000 --country america

# 录像分析
autoplay-replay --ra2-dir <retail> --replay <file.json> --out <dir> [--allow-rules-mismatch]
```

## 8. 参照数据（不在 git 里）

`autoplay/reference/`（被 `.gitignore` 排除，约 140 MB）：

- `replay_tick214095.json` + `ra2_tick214095.log` —— 1v29 胜局（有 startup，可重放）
- `replay_tick151530.json` + `ra2_tick151530.log` —— 超武 FFA 胜局（无 startup）
- `replay_tick211801.json` + `ra2_tick211801.log` —— 1v95 胜局（96 席位，本 build 读不了）
- `decode_records.py` / `read_replay_json.py` / `check_acceptance.py` / `type_table.json`

## 9. 下一步

见 `plan.md`。
