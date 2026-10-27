# WASM 联机适配

[简体中文](wasm-net.md) | [English](wasm-net.en.md)

`wasm-net` 为浏览器版本增加现有在线大厅和 UDP relay 的访问入口。默认构建、桌面联网和仅启用 `wasm` 的单机包继续使用原路径。接入方式是 WS 与 UDP 的透明转换：原大厅和 relay 服务端、消息格式、票据、加密、ACK、可靠消息和指令排序协议均不修改。

## 构建和地址

使用 [WASM 构建要求](wasm.zh-CN.md#构建) 安装 Rust target 和匹配 lockfile 的 wasm-bindgen CLI，然后执行：

```sh
# 默认单机包
python3 tools/wasm/build.py

# 联网包，默认代理 wss://wss.vera20k.com
python3 tools/wasm/build.py --network

# 自定义代理地址在编译时嵌入，修改后需要重新编译
WS_PROXY_URL=wss://gateway.example.com python3 tools/wasm/build.py --network

# 本地验证
WS_PROXY_URL=ws://localhost:8788 python3 tools/wasm/build.py --network
```

两种模式均输出到 `target/wasm-dist/`，可加 `--profile release`。联网打包添加 `network.js` 和 `lobby.js`；重新打单机包会移除这两个文件及对应导入，避免沿用上次联网产物。

`WS_PROXY_URL` 是编译期代理 origin，默认完整值为 `wss://wss.vera20k.com`。唯一 Rust 配置入口是 [`net::udp::browser_proxy_url`](../src/net/udp.rs)，通过 `option_env!` 取得构建环境；JS 不另设默认值。构建工具拒绝空值、凭据、路径、查询串、片段和非 WebSocket scheme；`ws://` 只允许回环地址。正常部署使用 `wss://`。

默认的两条 WS 地址为 `wss://wss.vera20k.com/game/primary` 和 `wss://wss.vera20k.com/game/alternate`。大厅 Fetch 使用同一 authority 的 HTTPS `/v1/*`（本地 `ws:` 对应 `http:`）。没有运行时地址设置、localStorage 或页面域名回退。默认域名只是构建约定，不表示服务已部署。`WS_PROXY_URL` 不改变大厅下发的 `RelayGrant.address`。

## 局域网（`wasm-lan`）

`python3 tools/wasm/build.py --network --lan` 构建 `wasm-lan` 包，额外打包 [`lan_rtc.js`](../tools/wasm/lan_rtc.js)。该 feature 依赖 `wasm-net`，复用同一个 `WS_PROXY_URL`；未开启时 LAN 菜单仍返回不支持。网页版局域网只在浏览器之间联机，不与桌面互通（build identity 也不同）。

网关的 `/lan/signal` 只协调握手：按公网出口地址分组（IPv4 全地址、IPv6 /64），给组员分配 `10.20.0.N` 虚拟地址，并在组内转发 WebRTC offer/answer/candidate。游戏数据报不经过网关。组内两两建立 `negotiated`、`ordered:false`、`maxRetransmits:0` 的 DataChannel，地址较小的一方发起 offer。

[`web_rtc.rs`](../src/net/web_rtc.rs) 只在 LAN 绑定（`0.0.0.0:1234/1236`）时替换物理 socket；在线绑定的临时端口仍走 WS relay。发现、加入、开局、ACK、重试和对局状态都由原 `lan_service` 与 `net::udp` 处理。广播 `255.255.255.255` 对每个已连通组员各发一份，不回环到自身（自身过滤原本按名字进行）。对端帧头 4 字节，为源端口和目的端口；不认识或未连通的对端按 UDP 丢包处理。

信令断线不会关闭已连通的 DataChannel，重连时会请求原虚拟地址；若虚拟地址变化，就视为新的 LAN 身份。浏览器默认用 mDNS 隐藏局域网 IP，开启 AP 隔离或屏蔽组播的网络可能无法直连，此时可在网关配置 `VERA_LAN_STUN`。不提供 TURN，因为 TURN 会转发游戏流量。同一网络里一台走 IPv4、一台走 IPv6 的浏览器会被分到不同组。

## 编译隔离

根 crate 的 `wasm-net` 启用 `wasm` 和 core 的 `wasm-net`；core 在浏览器 target 下增加可选 `wasm-bindgen` 依赖。生产代码的浏览器联网分支同时检查：

```rust
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
```

| 构建 | 使用的实现 |
| --- | --- |
| 桌面默认、shipping、启用 `wasm` 或 `wasm-net` | 原生 UDP、ureq 和线程调度 |
| WASM 仅启用 `wasm` | 原单机路径，联网入口返回不支持 |
| WASM 启用 `wasm-net` | WS 数据报适配、Fetch 和浏览器加载等待 |
| WASM 未启用 `wasm` | 保留原编译错误 |

独立手动 WASM workflow 分别打包单机与联网版本；桌面 workflow 不增加网关或 Node 依赖。网关位于 [`tools/wasm/gateway`](../tools/wasm/gateway/README.zh-CN.md)，独立安装和运行，不属于 Rust workspace。

## 传输和服务

```text
现有在线大厅逻辑 → Fetch → 网关 /v1/* → 原 HTTPS 大厅
现有网络协议 → 浏览器 socket → WS /game/primary → 独立 UDP socket → 原 relay
             浏览器 alternate → WS /game/alternate → 另一 UDP socket
桌面客户端 → 原 HTTPS 大厅与 UDP relay
```

一条 WS binary message 就是一份完整的原 UDP 数据报，不增加目的地址头、JSON、base64 或自定义握手，不拼接报文。认证、续期、ACK、重试、排序及对局日志始终由原所有者处理。WS OPEN 不是 relay admission；连接 Ping/Pong 不代表游戏层确认。

每个 WS 连接独占一个 connected UDP socket，存活期间源端口稳定，不同玩家和通道不共用端口。网关的 UDP 目标由管理员固定配置，必须对应大厅下发的 relay；浏览器不能指定任意转发目标。部署方法和限制见 [网关说明](../tools/wasm/gateway/README.zh-CN.md)。

`/v1/*` 代理保持现有方法、路径、Authorization、请求体、状态码和响应体，不解析或改写游戏 JSON，也不替客户端重试。跨源 CORS 由网关处理，原大厅不需要增加 OPTIONS。网关的 HTTP 部分会经手 bearer 和包含 payload key 的响应，属于大厅凭据的信任边界；它不需要 relay 签名密钥，不记录凭据，也不读取零售资源。

## 客户端边界

### socket 和恢复

[`web_socket.rs`](../src/net/web_socket.rs) 仅替换 `UdpIo` 的私有物理 socket，保留 `LanUdpTransport` 接口和原协议队列。`bind` 只创建逻辑句柄，直到原 owner 接受 relay grant 后才打开 WS；局域网菜单不能顺带连接远程代理（`wasm-lan` 只连接信令，见上文）。发送和接收端点绑定于该 grant，不使用“最后一次发送目的地”推断来源。

[`network.js`](../tools/wasm/network.js) 只缓冲原始字节。接收上限为 256 包和 256 KiB，发送通过 `bufferedAmount` 限制为 256 KiB；Rust 使用原 codec 的 `MAX_DATAGRAM`。未连接或发送积压时返回 WouldBlock，协议队列仍拥有重试责任。

物理连接失败时使用有界退避重连，清除旧连接的字节与回调，通知原 Rust owner 重新进行 relay challenge。原可靠队列、对局身份、未确认 batch 和日志 cursor 保留。断网不制造 Leave；显式退出仍走原确认流程。浏览器没有原生 I/O pump 线程，app 的分次 service 和事件循环共同推进网络。

### 大厅执行器

[`http.rs`](../src/app/shell_wol/live_service/http.rs) 的 `serial_driver!` 保留一份串行控制流程：native 展开为原同步函数和阻塞 ureq，浏览器分支展开为 async 函数；`platform_await!` 只决定平台等待形式。没有为桌面引入 async executor，也没有复制另一套 token、cursor、active_match、批次去重、resume 或 cleanup 状态机。

浏览器 I/O 在 [`http/browser.rs`](../src/app/shell_wol/live_service/http/browser.rs) 与 [`lobby.js`](../tools/wasm/lobby.js)。主控制请求仍串行执行，翻译独立调度；现有 Request/Response、地图传输与房间 reducer 继续复用。每次大厅 visit 拥有独立 Fetch 请求句柄；退休时取消请求并释放回调，不能写入后来的 visit。HTTP 保留超时、响应大小限制、无 cookie、无重定向和原错误映射。

下载地图复用 `map::source` 的校验与缓存配额。仅浏览器联网分支将磁盘操作接入现有内存文件系统，完整校验后原子写入，并沿用 `/saves` 持久化。活动地图由共享 lease 保留，最后一个消费者释放后才能被淘汰；桌面继续使用原文件锁和暂存文件发布流程。

### 加载期间让出事件循环

在线加载在浏览器联网分支使用已有 `FreshMapLoadJob` continuation。`LoadingNetworkService::milestone` 服务一次网络后，将 ACK drain 延迟到后续加载 pump；同一个 `LanService` 保存等待期限，不复制 transport 或可靠队列。

等待仍使用原先“队列超过 5”和 240 wall buckets 的边界。连续里程碑不会重置既有期限，超时不会伪造 ACK。后续加载阶段、显示准备和最终安装前会恢复该等待，并服务大厅及加载消息。浏览器完成分段加载后仍经过原远端进度及启动屏障，不能跳过准备就推进第一模拟帧。桌面保留原同步加载、20 ms 休眠和原线程路径。

相关 owner 是 [`lan_service/loading.rs`](../src/app/lan_service/loading.rs) 和 [`loading/pump.rs`](../src/app/loading/pump.rs)。这里改变的是浏览器调度，原规则读取、初始化顺序、模拟数学和 RNG 不作为适配目标修改。

## 验证和限制

定向检查入口：

```sh
node --test tools/wasm/tests/*.test.cjs
npm --prefix tools/wasm/gateway ci
npm --prefix tools/wasm/gateway test
python3 -m unittest tools.tests.test_wasm_build
```

Rust 验证通过 `python3 -m tools.cargo_run -- test -p <crate> --lib <filter>`，覆盖受影响的 net、HTTP、加载模块及直接消费者。零售相关验证设置 `VERA20K_REQUIRE_RETAIL_INI=1`；加载改动还需 `--profile fast` 零售地图加载。修改 Python 工具后执行项目要求的 `python -m tools.run_tests`。不要把单个平台的编译或 JS mock 测试当成其他平台或真实对局的验证。

发布前需要分别覆盖 native 默认、native `wasm`、native `wasm-net`、WASM `wasm` 和 WASM `wasm-net`；检查代理环境变量变化后的产物以及网络包切回单机包的清理。网关集成测试覆盖真实 HTTP/WS/UDP 的报文字节、边界、源端口、端口释放和拒绝路径。

实际双客户端完整比赛、浏览器与未修改桌面端的相同已提交帧摘要、临时断线后的追赶、明确退局及再开局仍是必要的运行验收。只有通过这些流程，才能声称联网可玩；构建成功或 UDP echo 不足以证明它们。验证结果随交付说明报告，不在此文档保留运行日志。

互联测试两端必须有相同源码和 feature 集合：既有 build identity 包含全部 Cargo feature。本方案不放宽该校验；对应浏览器联网包的桌面测试端应使用 `--no-default-features --features wasm-net` 构建，仍选择原生网络实现。不能直接假定现成的旧桌面安装包能加入新构建。

本轮接入现有在线对战和可选的浏览器间局域网（见上文），不提供与桌面互通的网页局域网、任意 IP 直连、CnCNet、新协议、随机地图生成或刷新页面后恢复模拟。浏览器后台节流、长时间同步资源操作和 WS 队头阻塞仍需实机观察；原 relay 的进程重启恢复边界保持不变。30 个协议 source 不代表浏览器已达到 30 人和 20,000 单位性能目标。
