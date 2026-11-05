# 开发计划：协议实现（开发者 A）

## 依据与边界

- **依据：** 本目录的 `spec.md`，下文称"方案"。它是唯一依据。方案有歧义、矛盾或错误时，**不要自行决定**：停下来，把问题、证据和建议的修改文字报告给协调者。协调者改了方案之后再继续。
- **工作目录：** `/Users/libi/Documents/workspace/other/vera20k-relay-transport-protocol`，分支 `feature/relay-adaptive-transport-protocol`。只在这个目录里工作，不碰其他 worktree。
- **另一条线：** 开发者 B 并行实现端到端测试（`e2e-plan.md`），只依据方案和你的 A1 接口来写判定规则。B 发现的不一致会通过协调者转给你。
- **范围：** 方案 §1~§8、§10~§12、§15 的第 1、2、4 步。§9（回合长度自适应）**不做**。

## 里程碑

每个里程碑结束时：focused 测试全部通过，提交一次（不 push），向协调者报告。

### A1：接口骨架（最先交付，B 依赖它）

**只写公开类型、函数签名和文档注释，函数体可以是 `todo!()`。必须能编译。** 文档注释里写清：调用时机、参数单位、返回值含义、对应方案哪一节。B 会照着这些注释写测试，所以注释就是接口契约。

**1. 客户端，`vera20k-relay-protocol` 的 `ClientSession`。** 做成不做 IO 的状态机，时间一律由参数 `now_ms: u64`（单调毫秒）传入，内部不读系统时钟。至少要有以下能力，名字可以自定，但能力不能少：

- **收发都带时间：**
  - 所有**编码**数据报的方法，以及**接收**数据报的方法，都带 `now_ms`。接收时传的是套接字收到的时刻。
  - 用途是发送环记时间（§4.2）和 RTT 计算（§4.4）。现有的 `probe`、`confirm`、`receive`、`configure_ordering`、`pull_ordering`、`control_ordering_pause`、`leave_ordering`、`data` 都要改。
- **由排序层（`SequencedSession`）提供内容：**
  - 封一个新批次，返回批次 ID；
  - 交一组 checksum 样本；
  - 交一次 pace 测量结果；
  - 设置连续水位 `recv_next`；
  - 报告出现缺口，或者缺口消失；
  - 设置或清除 RESTORING；
  - 设置当前的 `turn_ms`（丢包状态下的确认间隔）；
  - 通知已开局，以及收到了第一个 PACK（§6.0）。
- **由状态机决定什么时候发：**
  - `poll_uplink(now_ms, out) -> bool`：按 §6.3 判断此刻是否要发一个 `UPLINK`；要发就编码进 `out`。一次调用最多产生一个包；调用方循环调用，直到返回 false。
  - `next_uplink_deadline() -> Option<u64>`：没有新输入时，下一次可能需要发包的时刻。
  - `heartbeat_uplink(now_ms, out) -> bool`：给 IO 线程补发心跳用（§6.6）。
- **观测：** `link_stats() -> ClientLinkStats`。
  - 上行状态、`up_loss‰`、是否严重丢包、`srtt` / `rttvar` / `base`、DELAYED；
  - §10.1 的全部计数器；
  - 下行接收窗口的 `high` / `bits`。

**2. 中继，`RelayServer`。**

- `process_at`、`drain_ordering_push` 已经带 `now_ms`，保持不变。
- 新增 `reader_link_stats(room: [u8; 16], source: u8) -> Option<ReaderLinkStats>`：
  - 读者状态、`down_loss‰`、是否严重丢包、当前 K、`acked_next`、`sent_next`、重传队列长度、PTO 退避次数；
  - 按 kind 0~4 分开计的发送包数和字节数；
  - 上行接收窗口的 `high` / `bits`。

**3. 事件：** `ServerEvent` 和 `OrderEvent` 的变化，包括 ACK、STATUS 新增的字段，以及删除 PushState。

**交付：** 提交 A1，在报告里列出全部公开签名，把文档注释原文贴出来。

### A2：测量与编解码

- **编解码：** `RecvWindow`（§4.1，包括不计入窗口的三种数据报），两端的发送环（§4.2），`UPLINK` / ACK / STATUS 的编解码（§3.2~§3.4，包括 §3.2 的全部 Malformed 规则）。
- **测量算法：** 丢包判定和折算（§4.3），RTT / 基准 RTT（§4.4），RTO（§4.5），PTO（§4.6），跳续期时的重置（§4.7）。
- **状态机：** 链路状态机（§5），客户端判上行；中继判下行的那一半先写好，A4 接入。
- **测试：**
  - 方案 §13.1 的第 1~5 项；
  - **字节级样例：** 用附录 A 的包 14、18、19、20、24、25 作为编解码的固定样例。字段值和长度要和附录完全一致（不含 MAC 和密文本身）。

### A3：上行链路（方案 §15 第 1 步）

- **中继：** 处理 `UPLINK`（§7.1 的第 1~3 步、§8 的全部规则和回复规则）。删除 request 1/4/6/7 和 PushState。
- **客户端状态机：** §6.0~§6.6 的全部发送条件、回复处理、订阅确认重发。
- **游戏客户端接入：**
  - `src/net/live_commands/sequenced.rs`：改成只提供内容、读记录流；删除 `PushDelivery` 的三种状态（Probing / Active / PullOnly），删除连发 2 份和固定 100 ms 重发的逻辑。
  - `src/net/udp.rs`：收到数据报时把套接字时刻传进状态机，IO 线程按 §6.6 补发心跳。
  - 现有的 `live_commands_tests.rs` 里的 `sequenced_*` 测试按新协议改写。原来锁定旧行为的断言（比如"首发 2 份"），改成方案规定的新行为。
- **测试：** §13.1 的第 6 项；客户端调度的每一条发送条件，各有一个边界测试。

### A4：下行链路（方案 §15 第 2 步）

- **中继：**
  - `ReaderLink`（§7.1）、下行丢包判定、按确认重传（§7.3）；
  - 捎带（§7.4，**捎带不能增加包数**）、按状态开关 8 ms 补推（§7.5）、尾部探测（§7.6）；
  - 读者断开时停推，恢复后从水位处重新推送（§5.5）；
  - 每轮推送的顺序和预算归属（§7.2）。
- **客户端：** 缺口修复改为 §7.7 的规则（先发 `UPLINK`，宽限期过后才发 PULL）。
- **观测：** 中继每分钟统计（§10.2）。
- **测试：** §13.1 的第 7~9 项。

### A5：收尾（方案 §15 第 4 步）

- `services/relay-protocol/examples/relay_compat.rs` 改用新接口，保持能编译。新协议不兼容旧版本，PR 需要打 `relay-compat-break` 标签，在报告里说明。
- `services/relay/README.md` 的协议章节按方案更新。
- **`relay_load` 不要改：** 它由 B 负责迁移。删除旧接口后，如果它编译不过，在报告里说明。

## 规约（来自仓库 AGENTS.md，必须遵守）

- **Cargo：** 一律用 `python3 -m tools.cargo_run -- <参数>`，从工作目录执行；它会和其他 worktree 排队等锁，不要并行编译，也不要杀掉别人的编译。测试一律带 `--lib`，只跑相关模块：
  - `python3 -m tools.cargo_run -- test -p vera20k-relay-protocol --lib`
  - `python3 -m tools.cargo_run -- test -p vera20k-core --lib net::`
- **格式化：** 只格式化改过的文件：`export PATH=$HOME/.cargo/bin:$PATH; rustfmt --edition 2024 <文件>`。
- **测试规模：** 每个修复配一到三个聚焦的回归测试，用边界情况，不做扫参。
- **不改的部分：** 方案 §1.1 列出的不变项。
- **提交：** 提交信息说清改了什么。**不加** `Co-Authored-By` 行。不 push，不开 PR。
- **运行：** 不启动游戏界面，只跑测试和无界面程序。
- **沟通：** 用中文报告：做了什么、测试结果（附命令和通过数）、和方案不一致或方案没覆盖的地方。
