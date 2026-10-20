# Native 同步命令 opcode dispatch 取证

> 证据级别声明：**Native established（静态）· retail + Steam 双证一致** ——
> 以下来自两个受支持的 gamemd.exe（均在 `tools/native_oracle.py` 白名单内）：
> - **retail** `1cdd1180e49024fb...`（`E:/Ra2Game412/gamemd.exe`，5,286,504 B）
> - **Steam** `3e81a61775d2745d...`（`E:/Games/红警2重聚未来/Reunion/gamemd.exe`，5,286,208 B）
>
> 用 `tools/native_inspect`（capstone 5.0.7）dump。**retail 复跑已通过**（见 §0）：
> `EventClass::Execute` 主窗口 + 三段尾区共 **1646 条指令、逐字节 0 差异**，
> switch table **46 个目标 0 差异**。两个二进制总大小虽差 296 B，但该分发函数区域
> **布局与字节完全相同（同一 VA `0x4C6CB0`）**，故本表不再是"Steam 单证候选映射"。
>
> **仍未做**：(1) reachability 全 caller 证明；(2) dynamic trace；(3) 真实网络流采样。
> 静态反汇编只能确认立即数值与分支目标，**不能**证明：(a) 某 opcode 在真实网络流里
> 是否可达；(b) 是否存在多个 opcode 编码同一语义的 alias；(c) 分支内某 call 是否真的在
> 该 opcode 路径上被执行。另注意 §2 曾因线性 call 提取越过 `ret` 而出错，已由 §2b
> 的 ret 边界净体修正。

## 0. retail 复跑验证（retail `1cdd1180...` vs Steam `3e81a617...`）

**定位方式**：retail 与 Steam 是不同二进制，VA 可能漂移。取 Steam 函数开头 13 字节机器码
`81 ec 70 03 00 00 33 c0 53 55 56 8b f1`（=`sub esp,0x370; xor eax,eax; push ebx/ebp/esi; mov esi,ecx`）
作签名，在 retail 上 `find-bytes` 命中**唯一一处**：

```
native_inspect find-bytes "81 ec 70 03 00 00 33 c0 53 55 56 8b f1"
  -> address 5008560 (0x4C6CB0), file_offset 814256     ← 与 Steam 完全一致
```

**复跑结果（逐字节 diff）**：

| 区域 | 指令数 (Steam/Retail) | 字节差异 |
|------|----------------------|----------|
| 主窗口 `0x4c6cb0` +4096B | 1261 / 1261 | **0** |
| 尾1 `0x4c7ca0` +336B | 94 / 94 | **0** |
| 尾2 `0x4c7ded` +552B | 151 / 151 | **0** |
| 尾3 `0x4c8011` +368B | 140 / 140 | **0** |
| **合计** | **1646 / 1646** | **0** |
| switch table `0x4c8114` 46 目标 | 46 / 46 | **0** |

**结论**：`EventClass::Execute` 的整张 46-opcode 分发表在 retail 与 Steam 上**字节级完全一致**。
由此本报告的 opcode 全集（0x01..0x2e）、5 个 no-op、全部分支目标、以及 §2b 各分支体
（含 0x0c/0x22 写 `[0xa8eda0]`、0x10≡0x2e、0x11 独立体）均获**双证支撑**，
不再受"Steam 单证"限制。**仍未消除**的是静态方法的固有限制（上表 (a)(b)(c)）——
那些需要 dynamic trace / 真实网络流，不因双证而消失。

## 1. 总分发点

`EventClass::Execute @ 0x4C6CB0`（Steam）。序言 `sub esp, 0x370`。命令记录首字节是 opcode，
分发形如下：

```
mov al, [esi]                 ; al = record byte 0 = opcode
dec eax                       ; eax = opcode - 1            (0x00-based index)
cmp eax, 0x2d                 ; 允许 opcode ∈ 0x01..0x2e (即 1..46)
ja  <epilogue 0x4c8109>       ; 越界 -> 直接返回（无副作用）
jmp dword ptr [eax*4 + 0x4c8114]   ; 184 字节 switch table
```

**结论：native 同步命令 opcode 全集是有界的 0x01..0x2e（46 个）**。这与 VERA 里
"native record 只认 6 组 opcode" 的认知一致——但那 6 组只是 VERA 已移植的子集，
不是 native 全集。

switch table `0x4c8114`（46 个 32 位目标，读自 `.switch_table.json`）：

| op | 目标 | 分类 |
|----|------|------|
| 0x01 | 0x4c6cf0 | 实分支 |
| 0x02 | 0x4c6d4d | 实分支 |
| 0x03 | 0x4c6dee | 实分支 |
| 0x04 | 0x4c71ca | 实分支 |
| 0x05 | 0x4c71ca | 实分支（与 0x04 同址） |
| 0x06 | 0x4c74cb | 实分支 |
| 0x07 | 0x4c7859 | 实分支 |
| 0x08 | 0x4c7bdf | 实分支 |
| 0x09 | 0x4c76bc | 实分支 |
| 0x0a | 0x4c7823 | 实分支 |
| 0x0b | 0x4c70e1 | 实分支 |
| 0x0c | 0x4c7929 | 实分支 |
| 0x0d | 0x4c794b | 实分支 |
| 0x0e | 0x4c711b | 实分支 |
| 0x0f | 0x4c7140 | 实分支 |
| 0x10 | 0x4c7163 | 实分支 |
| 0x11 | 0x4c718c | 实分支 |
| 0x12 | 0x4c78d6 | 实分支 |
| 0x13 | 0x4c7903 | 实分支 |
| 0x14 | 0x4c6fde | 实分支 |
| 0x15 | 0x4c6ed2 | 实分支 |
| 0x16 | 0x4c6f12 | 实分支 |
| 0x17 | 0x4c6fb3 | 实分支 |
| 0x18 | 0x4c6e48 | 实分支 |
| **0x19** | **0x4c8109** | **no-op（跳函数尾声）** |
| **0x1a** | **0x4c8109** | **no-op** |
| 0x1b | 0x4c79ff | 实分支 |
| **0x1c** | **0x4c8109** | **no-op** |
| 0x1d | 0x4c7a14 | 实分支 |
| 0x1e | 0x4c6daa | 实分支 |
| 0x1f | 0x4c7b4d | 实分支 |
| **0x20** | **0x4c800c** | **尾内分支（>0x4c7ca0，需另读）** |
| **0x21** | **0x4c80b1** | **尾内分支** |
| **0x22** | **0x4c80f6** | **尾内分支** |
| 0x23 | 0x4c7b73 | 实分支 |
| **0x24** | **0x4c7f35** | **尾内分支** |
| **0x25** | **0x4c8109** | **no-op** |
| **0x26** | **0x4c8109** | **no-op** |
| **0x27** | **0x4c7cf5** | **尾内分支** |
| 0x28 | 0x4c7bf1 | 实分支 |
| 0x29 | 0x4c7c12 | 实分支 |
| 0x2a | 0x4c6cde | 实分支 |
| 0x2b | 0x4c6cde | 实分支（与 0x2a 同址） |
| 0x2c | 0x4c6cde | 实分支（与 0x2a 同址） |
| 0x2d | 0x4c6e2e | 实分支 |
| 0x2e | 0x4c7188 | 实分支 |

**统计**：46 个 opcode 中，**5 个（0x19/0x1a/0x1c/0x25/0x26）直接跳 epilogue = no-op**
（native 收到这些 opcode 就静默丢弃）；**5 个（0x20/0x21/0x22/0x24/0x27）落在 0x4c7ca0
之后的函数尾部**（本轮 4096 字节主窗口未覆盖，需另读 `.event_tail.json` 或扩大窗口）；
其余 **36 个** 落在主函数体内，各有实分支。

同址合流：0x04≡0x05、0x2a≡0x2b≡0x2c —— 说明这几组 opcode 语义等价（一个主 opcode
带别名），是"alias"线索，正好回应 §1 的 (2) 限制：静态可看到它们跳同一址，但不能据此
断言它们在真实网络流里都一定出现。

## 2. 各 opcode 分支的 handler call（主窗口内 36 个实分支）

下列 call 为"分支头部起、线性向下前 4 条 call"，**可能跨越条件跳转**，只能当
"该 opcode 语义的候选佐证"，不是确证。地址后 `; op` 标注。

- **0x01** `0x4c6cf0`：`call 0x6e7a80; 0x6386e0; 0x452260; 0x6e7a80`
- **0x02** `0x4c6d4d`：`call 0x6e7a80; 0x6386e0; 0x452360; 0x6e6f20`
- **0x03** `0x4c6dee`：`call 0x4f9a50; 0x4f9f70; 0x4f9b50; 0x50c8c0`
- **0x04/05** `0x4c71ca`：`call 0x6e6f20; 0x637e00; 0x6386e0; 0x6e6ff0` — **MegaMission 族**
- **0x06** `0x4c74cb`：`call 0x6e6f20; 0x6386e0; 0x565730; [edx+0x2b0]`
- **0x07** `0x4c7859`：`call 0x6e6f20; 0x6386e0; [edx+0x174]; 0x4fae50` — **Scatter（VERA 已证 0x07）**
- **0x08** `0x4c7bdf`：`call 0x4fc980; 0x4068e0; 0x4068e0; 0x53ece0`
- **0x09** `0x4c76bc`：`call 0x6e6f20; 0x6386e0; 0x565730; [eax+0x2b0]`
- **0x0a** `0x4c7823`：`call 0x6e6f20; 0x6386e0; 0x438720; 0x6e6f20`
- **0x0b** `0x4c70e1`：`call 0x4fb0e0; 0x4fa350; 0x4fa910; 0x4faa10`
- **0x0c** `0x4c7929`：**零 call**，仅 `test [0xa8d5f8],2; mov [0xa8eda0],1`（置游戏速度档位 1，见 §2b）
- **0x0d** `0x4c794b`：`call 0x734e60; 0x7ca405; 0x7ca564; 0x7c5f00`（含 x87 缩放）— **对象命令，非 0x0c alias**
- **0x0e** `0x4c711b`：`call 0x4fa350; 0x4fa910; 0x4faa10; 0x6e7a80`
- **0x0f** `0x4c7140`：`call 0x4fa910; 0x4faa10; 0x6e7a80; 0x6386e0`
- **0x10** `0x4c7163`：`call 0x4faa10; 0x6e7a80; 0x6386e0; [edx+0x4f0]`
- **0x11** `0x4c718c`：`call 0x6e7a80; 0x6386e0; [edx+0x4f0]; 0x6e6f20`
- **0x12** `0x4c78d6`：**净体仅 `call 0x4fae50` 一条**（读 `word [esi+0xb]`+`word [esi+0xd]` 组坐标对）— 后三条 0x4068e0/0x734e60/0x7ca405 是线性提取串进 0x13 的，已剔除
- **0x13** `0x4c7903`：`call 0x4068e0; 0x734e60; 0x7ca405; 0x7ca564` — **Exit（VERA 已证 0x13）**
- **0x14** `0x4c6fde`：`call 0x578080; 0x565730; 0x7c8e17; 0x7c8e17`
- **0x15** `0x4c6ed2`：`call 0x6e6f20; 0x6386e0; [eax+0x19c]; 0x6e6f20`
- **0x16** `0x4c6f12`：`call 0x6e6f20; 0x6386e0; [eax+0x2c]; [edx+0x2c]`
- **0x17** `0x4c6fb3`：`call 0x4fce80; 0x578080; 0x565730; 0x7c8e17` — **SellWall 候选（VERA 0x17）**
- **0x18** `0x4c6e48`：`call 0x734e60; 0x7ca564; 0x5d3ba0; 0x4f42f0`
- **0x1b** `0x4c79ff`：`call 0x6c8c40; 0x6c8c40; 0x734e60; 0x734e60`
- **0x1d** `0x4c7a14`：`call 0x6c8c40; 0x6c8c40; 0x734e60; 0x734e60` — **与 0x1b 同 call（alias）**
- **0x1e** `0x4c6daa`：`call 0x6e6f20; 0x6386e0; 0x6e6e20; 0x70c610`
- **0x1f** `0x4c7b4d`：`call 0x7c8b3d; 0x4068e0; 0x50b730; 0x6646c0`
- **0x23** `0x4c7b73`：`call 0x4068e0; 0x50b730; 0x6646c0; 0x50b730`
- **0x28** `0x4c7bf1`：`call 0x4068e0; 0x4068e0; 0x53ece0; 0x53f0e0`
- **0x29** `0x4c7c12`：`call 0x4068e0; 0x53ece0; 0x53f0e0; 0x4068e0`
- **0x2a/2b/2c** `0x4c6cde`：`call 0x637e00; 0x6e7a80; 0x6386e0; 0x452260` — **Planning 族（VERA 已证 0x2a..0x2c）**
- **0x2d** `0x4c6e2e`：`call 0x50c8c0; 0x734e60; 0x7ca564; 0x5d3ba0` — **Cheer（VERA 已证 0x2d）**
- **0x2e** `0x4c7188`：**体 = `push 1; jmp 0x4c7165`（=0x10 体）→ 与 0x10 共享 `call 0x4faa10`**（**0x10≡0x2e alias**，差一个 push 常量 0/1）
- **0x11** `0x4c718c`：`call 0x6e7a80; 0x6386e0; [edx+0x4f0]; 0x6e6f20`（独立体，**不是** 0x2e）

### 关键 handler 复用指纹

> ⚠️ **本节下面的"簇成员"清单已按 ret 边界净体（§2b）修正过**。§2 原始的"分支头部起前 4 条
> call"线性提取，对**短分支会越过 `ret` 串进下一个分支**（例：0x0c 体仅 5 条指令、零 call，
> 却因线性提取把紧跟其后的 0x0d 的 call 算进了 0x0c；0x12 同理把 0x13 的 call 串了进来）。
> 因此 0x0c/0x12 的"call 指纹"行是污染的，请以 §2b 的净体为准。

几个 call 地址在多个 opcode 里重复出现，是强语义指纹（**净体核实后**）：

- `0x734e60 / 0x7ca405 / 0x7ca564` 三连：**0x0d, 0x13, 0x18, 0x2d**（及尾区 0x24）共享。
  高度疑似"**把一条同步命令写入 OutList / 广播**"的公共出口（0x13=Exit、0x2d=Cheer 都在，
  印证"写入同步流"是共同后置动作）。**（0x0c/0x12 不在此簇——它们的净体没有这条 call。）**
- `0x637e00`：**0x04(MegaMission)、0x2a(Planning)** 共享 —— 疑似"按 owner house 解析
  命令目标"。
- `0x4068e0`：**0x13, 0x1f, 0x23, 0x28, 0x29**（及 0x20/0x21/0x24 尾区）共享 —— 通用收尾/校验。
  **（0x08 的净体只调 0x4fc980，不碰 0x4068e0；0x12 的净体只调 0x4fae50。）**
- `0x6386e0`：对象解析型分支都调一次 —— "取命令记录目标对象 / 校验长度"。

### 2b. ret 边界净体（高价值未接 opcode 的逐条核实）

下列每个"分支体"都**严格取到该分支自己的 `ret` 为止**（不越界），是比 §2 线性提取更可靠的
证据。标注净体读到的 payload 偏移（相对记录 `[esi]`）与末位 handler。

| op | 净体要点（读到的 payload 偏移 → handler） | 语义推断 | 证据级 |
|----|------|----------|--------|
| **0x0c** | 零 payload；guard `test [0xa8d5f8],2` 不成立时 `mov [0xa8eda0],1`（全局） | **置游戏速度=1**（见下 0x22 佐证） | Native established |
| **0x22** | 零 payload；同一 guard `test [0xa8d5f8],2` → `mov [0xa8eda0],7` | **置游戏速度=7**；与 0x0c 同 guard 同目标，值不同 | Native established |
| 0x0d | `[esi+7]`存 `[0xa8eb60]`；`[esi+2]`(house)解析目标；x87 `fld/fmul` 缩放；`0x734e60` 簇 | 带 house+浮点缩放的对象命令（**不是**改速，也不是 0x0c alias） | Native established |
| 0x12 | 读 `word [esi+0xb]` + `word [esi+0xd]` 组一个坐标对 → `call 0x4fae50` | 单坐标型命令（非 OutList 簇） | Native established |
| 0x13 | `push [0xa8ed84]; call 0x4068e0; mov byte [0xa83d48],1` | Exit（置局内退出标记）✅ VERA 已证 | Native established |
| 0x0e | `[esi+7]`目标 + `[esi+0xb]`参数 + `[esi+0xf]`标志；`push 0` → `call 0x4fa350` | 目标+参数型（阶梯簇顶端） | Native established |
| 0x0f | 同 0x0e 读法；`call 0x4fa910`（无 push 0） | 同族 | Native established |
| 0x10 | 同上 + `push 0` → `call 0x4faa10` | 同族 | Native established |
| **0x2e** | 体=`push 1; jmp 0x4c7165`（=0x10 体） | **0x10 的 alias**（差一个 push 常量 0 vs 1，共享 0x4faa10） | Native established |
| 0x11 | `0x6e7a80` 解析对象 → `call [edx+0x4f0]`（虚表） | 对象虚调用型（独立，**非** 0x2e 体） | Native established |
| 0x0b | `word [esi+0x13]`+`word [esi+0x15]`(坐标) + `[esi+0xb]`+`[esi+0xf]` → `call 0x4fb0e0` | 带坐标+目标+标志型 | Native established |
| 0x06 | `[esi+0xac]` cmp 0x12/0x13 拒；`[esi+0x9c]`三元组 → `call 0x565730`（虚表） | 带"拒绝某些对象态"的目标型 | Native established |
| 0x09 | 与 0x06 同 guard 同 0x565730，多读 `[esi+0x504]` 拒 0 | **0x06 的近 alias**（参数更宽） | Native established |
| 0x08 | 零 payload，`call 0x4fc980`（单 call 体） | 对象级命令 | Native established |
| 0x1e | `0x6e6f20` 解析对象 → `[esi+0xc]` → `call 0x70c610` | 带 1 参数对象型 | Native established |

**🔑 本轮最重要的更正与推进（推翻 §3 与"SetGameSpeed=0x0d"注释）：**
`[0xa8eda0]` 这个全局被 **0x0c 置 1、0x22 置 7**，guard 完全相同（`test [0xa8d5f8],2`，疑似
"当前处于某模式才允许改速"）。两个 opcode 写同一全局、只数值不同，正是**"改游戏速度"的
典型编码形态（speed 档位 1/7）**。因此：
- **SetGameSpeed 的真 opcode 更可能是 0x0c / 0x22，而非 `command.rs:780` 注释写的 0x0d。**
- 0x0d 是带 house + x87 浮点缩放的对象命令，语义上不是改速。
- **但在 retail 复跑 + dynamic trace 之前，这条仍是"强嫌疑"而非 wire 规范**（Steam 单证，
  且 0xa8eda0 是否=速度档位需 Ghidra 反编译该全局的读取方确认）。

### 2c. 未接 opcode 的 payload 偏移约定（从净体归纳，供后续补码参考）

记录 `[esi]`（esi 指向 payload 起点，7 字节 header 之后）常见偏移：
`[esi]`=opcode、`[esi+2]`=house/flags、`[esi+7]`=**目标对象句柄**（大量对象命令复用）、
`[esi+9]`=cell/坐标低、`[esi+0xb]`+`[esi+0xd]`=**坐标对**、`[esi+0xf]`=子目标/参数、
`[esi+0x13]`+`[esi+0x15]`=另一坐标对。**这些偏移是静态净体读法，不等于 native 结构体定义**，
补码时须逐条对 native 记录布局复核。

### 2d. handler 语义深挖（补码前研究，仅 retail 静态）

对最高频玩家命令对应的 handler 做了函数体反编译。**注意：以下只到"形态证据"为止，
opcode→具体 VERA `Command` 变体的对应仍未定死，见 §2e 边界说明。**

#### (1) op 0x01 / 0x02 —— **命名级坐实：Building On Line / Building Off Line**

**调用链**（retail 静态）：dispatch → `0x4c6cf0`(op 0x01) / `0x4c6d4d`(op 0x02) → 解析
payload `[esi+7]` 目标建筑句柄（`0x6e7a80`）→ 守卫（`[obj+0x90]!=0`、`[obj+0x81]==0`、
`[house+0x16c0]==0`）→ `0x452260`(op 0x01) / `0x452360`(op 0x02)。

**命名级证据**：`0x452360`（op 0x02 路径）末尾 `mov ecx, 0x819108; call 0x752700` ——
`0x819108` 处字符串 = **`EVA_BuildingOffLine`**；`0x452260`（op 0x01 路径）同位置
`0x8190c8` = **`EVA_BuildingOnLine`**。→ **op 0x01 = 建筑上线，op 0x02 = 建筑下线**。

`0x452260`（On Line）与 `0x452360`（Off Line）函数体逐项对照：

| | 0x452260 (op 0x01, On) | 0x452360 (op 0x02, Off) |
|---|---|---|
| 入口检查 | `mov al,[esi+0x660]; test al,al; **jne** skip`（已 on-line→跳过） | `mov al,[esi+0x660]; test al,al; **je** skip`（已 off-line→跳过） |
| `[esi+0x660]` 写入 | `mov byte [esi+0x660], **1**` | `mov byte [esi+0x660], **0**` |
| `[esi+0x80]` | 置 1 | 置 1 |
| house `[ecx+0x5778]` / `[edx+0x5779]` / `[ecx+0x1fc]` | 均置 1 | 均置 1 |
| 21 建筑列表循环 | 每项调 `0x425270` = `mov [obj+0x19e], **0**` | 每项调 `0x425260` = `mov [obj+0x19e], **1**` |
| 共享清理例程 | —（内联开启逻辑；独立函数版 `0x452410`） | `0x452480`（`[esi+0x6ea]=0`、`0x554a80` 停止、dirty 0x6eb/0x6ec、虚表 `[edx+0x418]`） |
| EVA 输出 | 守卫 `0x50b6f0`(house) 为真 → `EVA_BuildingOnLine` | 守卫 `0x50b6f0`(house) 为真 → `EVA_BuildingOffLine` |

**判定**：**同一个电网开关状态的开/关对**（`[esi+0x660]` 是开关；`[obj+0x19e]` 是每建筑
的离线标志，经 21 项指针列表 `[esi+0x55c]`(step 4) 与 house 侧 `[house+0xf8c]`(step 0x44,
21 项) 配对逐建筑清/置；house 侧 0x5778/0x5779/0x1fc 是 dirty/待重算标志；EVA 只对本地播）。
**补码含义**：0x01 与 0x02 必须**成对实现**、共享同一组状态标志（开关 + 每建筑离线标志
+ house dirty），不能当两个独立命令各写一遍。⚠️ VERA `Command` 暂无对应变体
（`ToggleRepair` 是单建筑维修，不是这个）。

#### (2) 阶梯簇 0x0e / 0x0f / 0x10(≡0x2e) —— 三 handler 确为不同语义

分支体给三者的**参数形状完全相同**（`[esi+7]`目标 + `[esi+0xb]`参数 + `[esi+0xf]`标志，
0x0e/0x10 多一个 `push 0`），差异全在函数内部。反编译结果：

| handler | opcode | 特征调用 |
|---|---|---|
| `0x4fa350` | 0x0e | `0x48dcd0`(×2 共享前置)、`0x5007a0`、虚表 `[eax+0x94]`/`[edx+0x94]`/`[eax+0x84]`、`0x4068e0`(×3)、`0x7c8e17`、`0x4c98b0` |
| `0x4fa910` | 0x0f | `0x48dcd0`、`0x4c9e60`、`0x6abc60`、`0x6a60a0` |
| `0x4faa10` | 0x10 / 0x2e | `0x48dcd0`(×2)、`0x4ca620`(×2)、`0x6abc60`、`0x6a60a0`、虚表 `[eax+0x88]`/`[edx+0x40]`、**`[eax+0x20]`×6**、 `0x6abad0`、`0x4a8bf0`、`0x4c9ff0` |

**判定**：三者虚表槽位完全不同（`0x94/0x84` vs `0x88/0x40/0x20`），且 `0x4faa10` 对
`[eax+0x20]` 重复调用 6 次（像**对一组对象逐个执行**）→ 确为三种不同语义。
其中 `0x0f` 与 `0x10` 共享 `0x6abc60`+`0x6a60a0` 这对调用，比 `0x0e` 更接近。

### 2e. 静态取证的边界与字符串命名法（为什么有些到此为止，有些能定死）

`§2d` 已把静态反汇编能给的证据给完。再往下定"handler = 哪个 VERA `Command` 变体"，
会遇到三道**静态方法绕不过去**的墙：

1. **虚表调用无法静态解析**：`call [eax+0x94]` / `[eax+0x20]` 这类间接调用，目标类取决于
   运行时对象类型。不跑起来（dynamic trace）或不做类型推导（Ghidra 虚表/RTTI 分析），
   无法确定它调的是哪个类的哪个成员函数。阶梯簇三个 handler 的核心差异**全在这里**。
2. **递归展开是指数级的**：每个 handler 又调子函数（如 `0x452480`、`0x5007a0`、`0x6abc60`），
   子函数再调子函数……静态逐层追，工作量爆炸且每层都可能引入误判。
3. **形态相似 ≠ 语义相同**：`§2d(1)` 早期只凭"镜像结构"能确证 0x01/0x02 是 toggle 对
   （硬证据），但"这个 toggle 到底是 Deploy 还是 ToggleRepair"**形态给不出答案**。

**但本轮（2026-10-20）打开了第四条路——字符串命名法**，它能把部分 handler 直接推到
**命名级坐实**，绕开上面三堵墙：

- **EVA/UI 字符串常量**：handler 末尾 `mov ecx, <strptr>; call 0x752700`（EVA 输出）模式，
  读出字符串即得语义。实例：`0x8190c8`=`EVA_BuildingOnLine`、`0x819108`=`EVA_BuildingOffLine`
  → op 0x01/0x02 直接定死为建筑上/下线（§2d(1)）。
- **日志宏的 文件名+行号**：`0x734e60(ecx=<msg>, edx, <"Event.CPP">, <line>)` 模式，
  消息文本同样是命名证据。实例：`0x820df0`=`TXT_PLAYER_CHANGED_SPEED` +
  `0x820e44`=`D:\ra2mdpost\Event.CPP`（行 1427）→ op 0x0d 定死为改速通知。
- **读者侧反查**：对"某全局被两个 opcode 写"的形态学推断，可 `find-bytes` 该全局地址的
  **读取模式**（`mov eax,[addr]` = `a1 <addr32>`），dump 读者区确认语义。实例：
  `[0xa8eda0]` 的读者 = 主循环 `0x48c8b0` 速度状态机（八 case 跳表 + 归一化 + 复位）
  → 坐实"速度控制全局"，op 0x0c/0x22 = 改速（§3 SetGameSpeed 复核）。

→ **方法论结论**：定 handler 语义按代价排序：① 字符串命名（EVA/日志/消息文本，直接定死）
→ ② 读者侧反查（写方形态学 + 读方行为，可定死全局语义）→ ③ 形态学（toggle 对/alias/同
guard，只给结构关系）→ ④ Ghidra 反编译虚表 / dynamic trace（终极手段，最贵）。
**只有 ③ 的结论不可单独用于补码**；①②已产出 Native established 级命名证据。

## 3. 与 VERA `Command` 变体对齐

VERA `Command`（`src/sim/command.rs`）现 ~36 个语义变体。VERA `decode_native_command_record`
（`world_commands.rs:502`）目前只接 7 组：

| VERA 已接 | native opcode | 状态 |
|-----------|---------------|------|
| PlanningRecord / commit | 0x2a / 0x2b / 0x2c | ✅ 已证 |
| MegaMission（含 Mission 族） | 0x04 / 0x05 | ✅ 已证（mission 字节 7..10/16/17/25 已 decode） |
| Scatter | 0x07 | ✅ 已证 |
| Cheer | 0x2d | ✅ 已证 |
| ExitMatch | 0x13 | ✅ 已证 |
| SellWallAtCell | 0x17 | ✅ 已证（候选，本轮 0x17 分支 `0x4fce80/0x578080` 与 SellWall 语义吻合） |
| **SetGameSpeed** | **0x0c / 0x22** | ✅ **已接入（2026-10-20）**：`decode_native_command_record` 按分支立即数映射 speed=1/7；lockstep 队列不再静默消化这两个 opcode，改走 execute 回调。**residual**：native 1/7 与对话框 0..=6 换算未坐实，`apply_in_game_speed` 拒 7（既有行为，`world_tests` 有记录） |
| **SetBuildingOnLine** | **0x01 / 0x02** | ✅ **已接入（2026-10-20，第二步）**：成对命令，opcode 选方向、AbstractClass token（kind 0x34）选建筑；apply 转调 `go_building_online` / `go_building_offline`（幂等 + 电力/科技树失效 + EVA）。**residual**：native 分发守卫 `[obj+0x81]`（忙/移除中）与 `[house+0x16c0]`（未定名 house 标志）无 VERA 对应物；`[obj+0x90]`（on-map）由 substrate 实体存在性承载 |

**仍未接的 native 实分支**（VERA 未映射、但有 native opcode 实分支 → 属 (b)"需补码"或
需先判 native 本地是否真同步）：

| native opcode | 分支 call 指纹 | VERA 候选变体 | 处置 |
|---------------|----------------|----------------|------|
| 0x03 | 0x4f9a50/0x4f9f70 | 独立 | 同上 |
| 0x06 | 0x565730 + [edx+0x2b0] | 0x09 共享 0x565730（虚表 call，疑似同语义带参数） | 同上 |
| 0x08 | 0x4fc980/0x53ece0 | 独立 | 同上 |
| 0x09 | 0x565730 + [eax+0x2b0] | 与 0x06 共享 0x565730（alias 候选） | 同上 |
| 0x0a | 0x438720 | 独立 | 同上 |
| 0x0b | 0x4fb0e0/0x4fa350 | 独立 | 同上 |
| 0x0c | **零 payload**，guard `test [0xa8d5f8],2` → `mov [0xa8eda0],1` | **置游戏速度=1**（非 0x0d alias，见 §2b） | Native established |
| 0x0d | `[esi+7]`存 `[0xa8eb60]` + header house 解析 + `0x734e60` 簇 | **改速通知**：写 `[0xa8eb60]`=**新 GameSpeed 设置值**（格式串 `GameControls: GameSpeed = %d, ScrollRate = %d, Detail = %d`@`0x82280c` 坐实；改速者是 header house `[esi+2]`，非此全局），header house ≠ 本地 house（`[0xa83d4c]`）时对其他 house 广播 `TXT_PLAYER_CHANGED_SPEED`（Event.CPP:1427） | Native established |
| 0x0e/0x0f/0x10/0x11 | 0x4fa350/0x4fa910/0x4faa10 阶梯 | 四连，共享 0x4fa910/0x4faa10，疑似参数递增族 | 同上 |
| 0x12 | 0x4fae50 + 0x4068e0 | 独立 | 同上 |
| 0x14 | 0x578080 + 0x7c8e17 | 与 0x17 共享 0x578080（alias 候选） | 同上 |
| 0x15 | [eax+0x19c] | 虚表 call | 同上 |
| 0x16 | [eax+0x2c]/[edx+0x2c] | 虚表 call | 同上 |
| 0x18 | 0x734e60 簇 + 0x5d3ba0/0x4f42f0 | 独立 | 同上 |
| 0x1b/0x1d | 0x6c8c40 ×2 | 同 call，alias | 同上 |
| 0x1e | 0x6e6e20/0x70c610 | 独立 | 同上 |
| 0x1f/0x23 | 0x50b730/0x6646c0 | 0x1f 前置 0x7c8b3d，0x23 无 | 半 alias |
| 0x28/0x29 | 0x53ece0/0x53f0e0 | 0x28 多一次 0x4068e0 | 半 alias |
| 0x2e | **`push 1; jmp 0x10 体`**，共享 `call 0x4faa10` | **与 0x10 的 alias**（差一个 push 常量 0 vs 1） | Native established |
| 0x11 | `0x6e7a80` 解析对象 → `call [edx+0x4f0]`（虚表） | 对象虚调用型（独立，**非** 0x2e 体） | Native established |
| 0x27 | 0x4068e0/0x697e70/[0x7e1530]/0x664440 | 独立（尾内） | 需定语义 |
| 0x24 | 0x4068e0 ×2 + 0x734e60/0x7ca405/0x7ca564 簇 | 入"0x734e60 公共出口"簇 | 同上 |
| 0x20 | `mov [esi+9]` + `sub word [esi+9],cx`（递减计数器） | 疑似 tick/speed 计数器 | 需 dynamic trace |
| 0x21 | 循环 `cmp [ebp+0x6f]`（0x6f=记录长度），扫 OutList 记录 | 疑似"扫描/校验同步记录流" | 需 dynamic trace |
| **0x22** | 与 0x0c **同 guard** `test [0xa8d5f8],2` → `mov [0xa8eda0],7` | **SetGameSpeed 档位 7**：读者侧状态机 `0x48c8b0` 坐实 `[0xa8eda0]`=速度全局 | Native established |

### SetGameSpeed 复核（**结论已升级：0x0c / 0x22 才是真 opcode，0x0d 是改速通知**）

`command.rs:780` 注释称 SetGameSpeed = "live network opcode 0x0D"。本轮三条独立证据把它推翻：

1. **0x0d 的净体**（§2b）是带 house 解析 + x87 `fld/fmul` 浮点缩放 + `0x734e60` 簇的对象命令
   —— 既不像改本地帧率，也与 Exit/Cheer 同一公共出口（"写同步流"）。0x0d 不是改速。
2. **更硬的证据**：`[0xa8eda0]` 这个全局被 **0x0c 置 1、0x22 置 7**，两个 opcode 的 guard
   完全相同（`test [0xa8d5f8],2`，疑似"仅在某模式下允许改速"），且都零 payload、
   只差写入的数值。**两个 opcode 写同一全局、只数值不同 = "游戏速度档位"（1 / 7）的典型编码**。
3. **本轮新增读者侧证据**：`[0xa8eda0]` 的读取方集中在主循环 `0x48c8b0`——一段**速度
   状态机**（`mov eax,[0xa8eda0]` → `lea ecx,[eax-1]; cmp ecx,8; ja; jmp [ecx*4+0x48cc84]`
   八 case 跳表 + 特判值 8/9 + 归一化 5→1 + 复位为 0）。另有 op 0x1c 分支（`0x4c7a14`）
   也读它并调 `0x48d080`（写 `[0xa8eda0]` 的内部函数）——即本地对话框改速也走同一全局。
   → `[0xa8eda0]` = **游戏速度控制全局**，0x0c/0x22 是它的网络同步入口。

**op 0x0d 的真实身份（本轮新证据）**：分支体 `0x4c794b` 写 `[0xa8eb60] = [esi+7]`
（**新 GameSpeed 设置值**，由格式串 `GameControls: GameSpeed = %d, ScrollRate = %d, Detail = %d`@`0x82280c` 坐实；改速者是 header house `[esi+2]`，非此值），然后仅当 header house ≠ 本地 house（`cmp esi,[0xa83d4c]; je 尾`）时调
`0x734e60(ecx=0x820df0, edx=0, 0x820e44, 0x593)`。两处字符串 = **`TXT_PLAYER_CHANGED_SPEED`**
与 **`D:\ra2mdpost\Event.CPP`**（文件名+行号 1427 的日志/消息宏）→ **0x0d = "某玩家改变了
游戏速度"的跨玩家通知**，与 0x0c/0x22（写速度值本体）是同一机制的两个动作。
`command.rs:780` 注释把它当成"SetGameSpeed 的独立时序"不准确：0x0d 是通知，不是速度值命令。

→ **SetGameSpeed 速度值的真 opcode 是 0x0c（档位 1）与 0x22（档位 7），不是注释里的 0x0d。**
`command.rs:780` 那条注释应视为**误记 / 旧证**，改写 0x0d 的语义前先把它改掉。

⚠️ 仍留的余地（**retail 复跑后已收窄**）：~~单一 Steam 样本~~ —— §0 已证明 retail
与 Steam 在这段代码上字节级一致，**"样本单一"的顾虑已消除**。读者侧状态机已坐实
`[0xa8eda0]` 是速度控制全局；剩下唯一未定的是 **1/7 与对话框档位（VERA `0..=6`）的换算
关系**（native 网络命令只发 1 和 7 两档，而对话框是 0..6——多档映射待 dynamic trace）。
→ 结论已达：**0x0c/0x22 是改速 opcode、0x0d 是改速通知**；补码时 0x0c/0x22 映射
`SetGameSpeed`，0x0d 单独处理（广播通知）。

### 接入落地（2026-10-20，Stage 2 联机队列接入 第一步）

已按上表接入 VERA（`8642466` 之后的新改动）：
- `sim/command.rs`：新增 `GAME_SPEED_SLOW_OPCODE=0x0c` / `GAME_SPEED_FAST_OPCODE=0x22` 常量
  （注释含分支地址与 guard）。
- `sim/world/world_commands.rs::decode_native_command_record`：两 opcode → `Command::SetGameSpeed`，
  speed 取分支立即数（1 / 7）——**值来自分支体而非 payload**，与 native 逐字节对应。
- `net/lockstep.rs`：删除 `queue_consumes_without_execute`（0x0c/0x22 不再静默消化），
  两 opcode落入 generic execute 回调；原两个“marked_without_event_execute”测试改写为
  “reaches_event_execute_as_set_game_speed”。
- 新增测试：decode 映射（0x0c→1、0x22→7、未注册 house→None）+ 全链
  （decode→ingress→消费，7 被拒的 residual 行为与既有记录一致）。
  测试结果：`game_speed` 群 12/12、`net::lockstep` 32/32 全过。

### 接入落地第二步（2026-10-20，Building On/Off Line 0x01 / 0x02）

按"顺序 1 然后 2"推进的第二项。除 §2d(1) 已有的命名级证据外，本轮补了
**分支体级字节证据**（retail 静态，`disasm 0x4C6CF0`）：

```
0x4c6cf0  lea ecx,[esi+7]        ; ecx = payload 起点（7 字节 header 之后）
0x4c6cf7  call 0x6e7a80          ; 句柄解析：先 cmp byte [esi+4],0x34
                                 ;   （kind 必须 0x34，否则空 → 分支尾丢弃），
                                 ;   再在 0xb0e840 有序表上二分查找对象
0x4c6cfc  mov esi,eax            ; esi = 目标对象
0x4c6cfe  xor edx,edx
0x4c6d00  mov ecx,esi
0x4c6d02  call 0x6386e0
0x4c6d07  test esi,esi ; je 尾   ; 空对象丢弃
0x4c6d09  mov al,[esi+0x90] ; test al,al ; je 尾   ; on-map 守卫
0x4c6d17  mov al,[esi+0x81] ; test al,al ; jne 尾  ; 忙/移除中守卫
0x4c6d25  mov ecx,[esi+0x520]                        ; house
0x4c6d2b  mov al,[ecx+0x16c0] ; test al,al ; jne 尾  ; house 标志守卫
0x4c6d33  mov ecx,esi
0x4c6d38  call 0x452260          ; GoOnline（0x02 分支同一形态 → 0x452360 GoOffline）
```

守卫字段的读者/写者侧反查（本轮新增）：
- `[obj+0x90]`：写点 `0x4220b0` 在把对象加入全局列表后置 1 → **on-map/active 标志**。
  VERA 对应物 = substrate 实体存在（decode 的 `entities.get`）。
- `[obj+0x81]`：写点 `0x44ec5e`（生产回调前后临时置位再恢复）与 `0x5438db`
  （从全局向量移除时置 1）→ **忙/移除中标志**。VERA 执行期无此重入态 →
  **open residual**。
- `[house+0x16c0]`：5 个读者（`0x44535b`/`0x4471cb`/`0x449518`/`0x4c6d8a`/`0x5f48fa`，
  均为 `[vt+0x2c]==6` 形态后读），写点未定位（存档字段组 0x16c0/0x16c1/0x16c2 之一，
  `0x460ac0` 处与建筑类型名一起流式写出）→ **未定名，open residual**。

已落地改动：
- `sim/command.rs`：新增 `BUILDING_ON_LINE_OPCODE=0x01` / `BUILDING_OFF_LINE_OPCODE=0x02`
  常量、`BuildingOnLineRecord` 类型化视图（token 与 Scatter 同形：value i32 @ payload[0..4] +
  kind 0x34 @ [4]）、`Command::SetBuildingOnLine { entity_id, on }`（enum 末尾追加，
  既有变体序号不变）。
- `sim/world/world_commands.rs::decode_native_command_record`：0x01/0x02 →
  `SetBuildingOnLine`（house 已知 + 实体存在；kind≠0x34 / 未注册 house / 未知实体 → None）。
  **未加 owner 检查**——native 分支体同样没有，输入门禁是客户端职责。
- apply 尾部：`SetBuildingOnLine` → `go_building_online` / `go_building_offline`
  （幂等、电力/科技树失效、Stuff、工厂队列、human house 的 EVA 均已在 VERA 侧坐实）。
- `sim/snapshot.rs`：SNAPSHOT_VERSION 433→434（追加 enum 变体改变 bincode schema）。
- lockstep 无需改动：0x01/0x02 本就落在 generic execute 臂。
- 新增测试：decode 方向映射（0x01→on、0x02→off、foreign house→None、未知实体→None、
  错误 kind→None）；apply（下线→latch 落 + power/tech recheck + EVA_BuildingOffLine、
  幂等二次→false、上线恢复 + EVA_BuildingOnLine、foreign 建筑可切但 computer house 无 EVA、
  非建筑→拒）；decode→apply 全链一帧。

## 4. 结论与红线

1. **native 同步命令 opcode 全集有界 = 0x01..0x2e（46）**，其中 **5 个是 no-op**
   （0x19/0x1a/0x1c/0x25/0x26 直跳 epilogue）。VERA 已接 7 组 = 全集的 1/6 余。
2. 那 ~20 个"无编码"的 VERA typed 命令，按 AGENTS.md 红线拆成两类：
   - **(a) 已 0x04/0x05 MEGAMISSION 编码、只是 VERA 未映射回 typed 变体**：扩
     `MegaMissionRecord`/`decode_native_command_record` 的 mission 映射即可，**不发明 opcode**。
   - **(b) 有独立 opcode、VERA 未接**（本表 §3 那 20+ 行）：要补码。定语义按代价排序：
     ① 字符串命名（EVA/日志/消息文本）② 读者侧反查（写方+读方配对）——本轮已用这两招
     定死 0x01/0x02（Building On/Off Line）、0x0c/0x22（改速档位）、0x0d（改速通知）；
     仍剩的才上 ③ dynamic trace 或 Ghidra 反编译虚表。**不能在缺 gamemd 证据时硬编。**
3. **alias / 成对强信号**（净体核实后）：0x04≡0x05、0x2a≡0x2b≡0x2c、**0x10≡0x2e**（共享
   `call 0x4faa10`，差 push 常量 0/1）、0x1b≡0x1d、0x06↔0x09（共享 0x565730）、
   0x14↔0x17（共享 0x578080）、0x28↔0x29（共享 0x53ece0/0x53f0e0）。
   补码时必须成组处理，不能按单 opcode 猜。
   ⚠️ **0x0c/0x22 不是 alias 而是"档位对"**（同 guard 同全局 `[0xa8eda0]`，值 1/7，
   读者侧状态机 `0x48c8b0` 已坐实是速度档位）；**0x01/0x02 是电网开关对**（非 alias，
   EVA 字符串已定死 Building On/Off Line）。
4. **尾内 5 分支已补读**（`.event_tail.json` 0x4c7ca0..0x4c7ded + `.event_tail2/3.json`
   0x4c7ded..0x4c8109）：0x27=`0x4068e0/0x697e70`、0x24=入 `0x734e60` 公共出口簇、
   0x20=递减 `[esi+9]` 计数器、0x21=扫 OutList 记录（`cmp [ebp+0x6f]`）、
   **0x0c=`test [0xa8d5f8],2`→`mov [0xa8eda0],1`、0x22=`test [0xa8d5f8],2`→`mov [0xa8eda0],7`——
   两者同 guard 同全局、只数值不同，是 SetGameSpeed 速度档位（1/7）的真 opcode**。
   至此 46 个 opcode 全部分类完毕（36 主分支 + 5 no-op + 5 尾分支）。
5. **retail 复跑通过（§0）**：以上 1–4 全部在 retail `1cdd1180...` 上复跑验证，
   1646 条指令 + 46 个 switch 目标**逐字节 0 差异**，双证一致。

## 5. 边界声明（不授权实现）

本报告**只做取证，不写 encode/decode**（AGENTS.md："Research/review alone does not
authorize implementation"）。进入实现需**单独授权**，且每项 (b) 命令在写码前应补齐
"该 handler 语义 = 某 VERA Command 变体"的 native 证据（dynamic trace 或 Ghidra 反编译
handler）。5 个 no-op opcode 与任何"native 本地不同步"命令，一律记 residual，不硬对齐。

**retail 复跑消去的是"样本偏差"，不是"静态方法的局限"**：双证一致只说明这段 native 代码
在两个受支持二进制里字节相同；opcode 的**语义**（它到底对应哪个 VERA `Command` 变体）
仍须 dynamic trace / Ghidra 反编译 handler 来定，不因双证而可以免。

## 附：取证工件

**Steam 侧**（`3e81a617...`）：
- `.event_execute.json` — `EventClass::Execute @ 0x4C6CB0` 主窗口 4096B 反汇编（capstone 5.0.7）
- `.switch_table.json` — switch table `0x4c8114` 46×4B 目标
- `.event_tail.json` — 函数尾 0x4c7ca0..0x4c7ded（0x24/0x27 已解析）
- `.event_tail2.json` — 0x4c7ded..0x4c8011（0x24 公共出口簇确认）
- `.event_tail3.json` — 0x4c8011..0x4c8109（0x20/0x21/0x22 + epilogue 0x4c8109 确认）

**retail 侧**（`1cdd1180...`，用于 §0 复跑）：
- `retail_execute.json` / `retail_switch.json` / `retail_tail.json` / `retail_tail2.json` / `retail_tail3.json`
  （与 Steam 侧同址 dump，用于逐字节 diff）

**二进制位置**（本机，均不在仓库内——仓库不分发可执行文件）：
- retail：`E:/Ra2Game412/gamemd.exe`（`1cdd1180...`，5,286,504 B）
- Steam：`E:/Games/红警2重聚未来/Reunion/gamemd.exe`（`3e81a617...`，5,286,208 B）
- 取证环境变量：`VERA20K_GAMEMD_EXE=<上述路径>`（本机 capstone/unicorn 在
  `E:\WorkBuddyData\.workbuddy\binaries\python\envs\native-inspect\` venv，
  须用它的 `Scripts\python.exe`）
