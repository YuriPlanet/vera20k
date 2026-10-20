# Vera20k 长期项目笔记

## 🔑 native 取证二进制位置（本机，仓库不分发可执行文件）

| 版本 | 路径 | SHA-256 | 大小 |
|---|---|---|---|
| **retail** | `E:/Ra2Game412/gamemd.exe` | `1cdd1180e49024fb...` | 5,286,504 B |
| Steam | `E:/Games/红警2重聚未来/Reunion/gamemd.exe` | `3e81a61775d2745d...` | 5,286,208 B |

- 白名单定义在 `tools/native_oracle.py`（`NATIVE_SHA256` / `STEAM_NATIVE_SHA256`），
  非白名单二进制一律被拒（本机 `7b8a0685...` 等 20+ 个都是 mod/注入版，不可用）。
- ⚠️ **找 retail 要全盘扫**：`find /e /c -xdev -type f -iname "gamemd.exe"` + `sha256sum` 比对
  （约 4m35s，27 命中，本机仅此一个 retail）。**别只扫 `/e/Games`**——retail 在 `/e/Ra2Game412/`。
- 环境变量：`VERA20K_GAMEMD_EXE=<上述路径>`（优先，不静默回退）或 `RA2_DIR`。

## 🛠 取证工具环境

- **capstone/unicorn** 装在本机 venv：
  `E:\WorkBuddyData\.workbuddy\binaries\python\envs\native-inspect\Scripts\python.exe`
  → 跑 `native_inspect`/`native_oracle` **必须用它**（系统 python 没装，会 `ModuleNotFoundError`）。
- ⚠️ **路径坑**：bash 里 `/e/retail_bench/x.json` 能写成功，但 **Python 不认 `/e/` 风格路径**，
  Python 侧一律用 `E:/retail_bench/...`。
- 常用命令：
  ```sh
  export VERA20K_GAMEMD_EXE="E:/Ra2Game412/gamemd.exe"   # 或 Steam 那个
  python -m tools.native_inspect sections
  python -m tools.native_inspect disasm 0x4C6CB0 --bytes 4096
  python -m tools.native_inspect read    0x4c8114 --bytes 184
  python -m tools.native_inspect find-bytes "81 ec 70 03 00 00 33 c0 53 55 56 8b f1"
  ```

## 📌 跨二进制取证方法（签名定位法）

不同二进制 VA 会漂移。定位同一函数的做法：
1. 从已知侧取函数开头机器码做签名（如 `EventClass::Execute` 前 13 字节
   `81 ec 70 03 00 00 33 c0 53 55 56 8b f1` = `sub esp,0x370; xor eax,eax; push ebx/ebp/esi; mov esi,ecx`）
2. 目标二进制上 `find-bytes` 该签名
3. 同址 dump，跟已知侧逐条 diff

**已验证结论（2026-10-20）**：`EventClass::Execute` 在 retail 与 Steam 上
**同 VA `0x4C6CB0`、字节级完全一致**（1646 条指令 + 46 个 switch 目标，0 差异）。
→ 该分发表的取证已是**双证**，不再是"Steam 单证"。

## ⚠️ 取证方法论教训（重要）

**native 分支的 call 提取必须设 `ret` 哨兵！** 用"分支头部起取前 N 条 call"的线性提取，
短分支会**越过 `ret` 串进下一个分支**，把邻分支的 call 算到自己头上，**凭空造出 alias**。
曾因此误判 `0x0c≡0x0d`、把 `0x11` 的体当成 `0x2e` 的。→ 一律改用
"**每个分支体严格取到自己的 `ret` 为止**"（见 `docs/native_opcode_dispatch.md` §2b）。

## 🏗 关键架构红线（详见 `devref/ARCHITECTURE.md`）

- **`sim/` 永不依赖 `render/ui/audio/net/app`**；`net/` 可依赖 `sim/`（`architecture_guards.rs` 编译期强制）。
- 双 crate：`vera20k-core`（引擎 lib）+ `vera20k`（客户端 app/render/ui/audio）。
- **AGENTS.md 是权威契约**：`Never invent offsets/identities/behavior`；
  **research 不授权实现**（research 与 implementation 必须分开授权）；
  三层证据 = Native established / Rust regression tested / Parity demonstrated。

## 📁 交付物约定

- **`.gitignore` 是上游的，不要重写**。注意：`/docs/*` 和根 `/*.md` 几乎全被 ignore
  （只白名单 README/AGENTS/CLAUDE 等）→ **新建文档放 `devref/`**（不被 ignore），
  或 `git add -f` 强制纳入。
- **本地仓库来源（2026-10-20 老李确认）**：本地 `E:/Project/GitHub/Vera20k` = 从
  `github.com/YuriPlanet/vera20k` **下载的源码快照**（上游 `.git` 被剥离；首 commit
  `ebf51aa` 注明 "YuriPlanet/vera20k work copy, .git was stripped upstream"）。
  老李在此快照上**自建 git**，现有 **17 个本地 commit**（取证+文档），无 remote、不 push。
- **与上游关系 / 落后程度（2026-10-20 精确化）**：上游走 feature 分支 + PR 协作
  （PR 到 #2217，作者 hrusten）。本地 initial 快照 vs 上游 HEAD = **1690 文件 /
  +44.4万 / −9.1万行**，本地仅 16 个自建 commit；.git 被剥离 → **无共同祖先，merge 不可行**。
- **已 clone 完整上游**：`E:/Project/GitHub/vera20k-upstream`（origin→YuriPlanet/vera20k，
  7106 commit，HEAD `2b3240253`）。**今后开发基于此目录**（能 pull 跟上游），
  本地老副本 `Vera20k` 留作历史存档。
- **上游 opcode 实现状态 vs 本地原创**：
  - 0x0c/0x22 SetGameSpeed：上游**已实现**（`IN_GAME_OPTIONS_MAX_SPEED=6`
    `src/sim/game_options.rs:18`）→ **本地"1/7↔0..=6"未决项已由上游结掉**（上限 6，7 不采纳）。
  - 0x01/0x02 Building On/Off Line、0x0d GameSpeedChangeNotice：上游**无** → **本地原创**，
    但需按上游新架构重做（老代码补丁 am 不上，属 implementation）。
- 本地 16 commit 导出为 patch 存于 `/e/Project/GitHub/vera20k-patches/`（0001–0016，
  不含 initial）。文档类可干净落地上游 clone；代码类需重做。
