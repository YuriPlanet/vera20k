# 浏览器 WebAssembly 预览版

[English](wasm.md) | [简体中文](wasm.zh-CN.md)

Wasm 是与 macOS、Windows 和 Linux 并列的第四个可选构建目标，复用现有客户端、规则、模拟、菜单和加载模块。默认浏览器包为单机版；可选联机传输使用独立的 `wasm-net` feature。构建成功不代表已经完成浏览器实机游玩验证。

## 构建

需要稳定版 Rust、Python 3.11 或更新版本，以及与 `Cargo.lock` 中 `wasm-bindgen` 版本一致的 `wasm-bindgen-cli`（当前为 0.2.114）。

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.114 --locked
python3 tools/wasm/build.py
```

构建工具使用 `--target wasm32-unknown-unknown --no-default-features --features wasm`，按现有 `fast` profile 将客户端库构建为 `cdylib`，再将绑定代码和不含游戏资源的宿主页面打包到 `target/wasm-dist/`。可加 `--profile release` 使用 release 优化。工具会检查绑定 CLI 与 lockfile 的版本是否一致，并为引擎设置 8 MiB 的 Wasm 栈。桌面版的产物类型、默认 feature 和构建命令保持不变。

可选联机版的构建命令：

```sh
python3 tools/wasm/build.py --network
WS_PROXY_URL=wss://gateway.example.com python3 tools/wasm/build.py --network
```

`--network` 选择 `--features wasm-net`。`WS_PROXY_URL` 会编译进该版本，默认值为 `wss://wss.vera20k.com`；修改后需要重新构建。请提供不含路径、查询串、片段或凭据的完整 origin。只有本地开发的回环地址可以使用 `ws://`，例如 `WS_PROXY_URL=ws://localhost:8788`。桌面版和浏览器单机版不使用此设置。两种模式均输出到 `target/wasm-dist/`；重新构建单机包会移除联网专用 JavaScript 及对应导入。

联机包需要单独运行的 [WS/UDP 网关](../tools/wasm/gateway/README.zh-CN.md)。默认域名只是构建设置，不保证服务已部署。网关将每条二进制 WS 消息作为一份原始 UDP 数据报转发，并代理现有 HTTPS 大厅请求。原游戏协议和服务器保持不变。状态归属、编译边界和运行验收要求见[联机实现说明](wasm-net.md)。

等价的编译命令如下，不包含 JS 打包：

```sh
python3 -m tools.cargo_run -- rustc -p vera20k --lib \
  --target wasm32-unknown-unknown --no-default-features --features wasm \
  --profile fast --locked --crate-type cdylib -- -C link-arg=-zstack-size=8388608
```

## 本地预览

```sh
python3 -m http.server 8000 --bind 127.0.0.1 --directory target/wasm-dist
```

在支持 WebGPU 的浏览器中打开 `http://localhost:8000`。WebGPU 要求安全上下文，即 HTTPS 或 localhost。选择已解压的《尤里的复仇》零售资源目录，然后点击 Start。构建包不包含游戏资源。单机版不会上传资源；联机时可以通过现有地图传输协议分享所选地图，其他零售资源仍留在本地。点击 Start 时会准备音频，以满足浏览器自动播放策略。资源字节保存在内存中，因此较大的游戏安装目录需要较多浏览器内存。

宿主页面通过当前 origin 的 IndexedDB 恢复设置和存档，并排队保存后续写入。页面会显示存储失败和待写入状态。引擎先完成内存写入，异步存储事务随后才会完成；关闭页面前应等待已保存状态。清除站点数据会删除这些副本。导入的零售资源不会持久化，重新加载页面后需要再次选择。

## 兼容边界

所有浏览器实现都要求同时启用 `wasm` Cargo feature 和 `wasm32-unknown-unknown` 目标。在桌面目标上启用 `wasm` 仍会选择桌面实现。未启用 `wasm` 的浏览器构建会被拒绝。

- `util::time` 在桌面端重新导出现有标准时钟，在浏览器端导出 `web-time`；模拟 tick 和 RNG 算法不变。
- `util::file_system` 在桌面端重新导出标准文件操作。浏览器导入资源位于 `/retail`，存档位于 `/saves`，配置位于 `/saves/profile`；MIX 归档共享导入字节，不使用内存映射。
- 浏览器通过现有并行工作接口串行执行工作。桌面端仍选择原生 Rayon 和工作线程路径。
- GPU 获取通过现有初始化任务让出执行。渲染使用离屏 sRGB 帧和 canvas 呈现 pass。浏览器纹理上传不会阻塞等待 GPU 完成。
- 浏览器入口提供 canvas、状态回调和存储回调；现有 App 继续负责启动和对局生命周期。

只有同时启用 `wasm-net` 和浏览器目标，才会启用 Fetch、WS socket 和延后的联机加载等待。桌面构建（包括启用 `wasm-net` 的构建）仍使用原生 UDP、阻塞 HTTPS 和线程调度。仅启用 `wasm` 的单机包仍会在网络入口明确返回不支持错误。浏览器不支持 LAN 发现和直接 UDP 连接。

此单线程预览版也不支持随机地图生成和同步 GPU 截图／捕获，仍以已有安装地图为目标。非美式键盘布局中需要 Shift 的标点快捷键尚未完全映射。大型图集工作和同步存档准备仍可能阻塞浏览器事件循环；运行和性能验证尚待完成。

## 检查

```sh
python3 -m unittest tools.tests.test_cargo_run
python3 -m tools.cargo_run -- test -p vera20k-core --lib --features wasm util::file_system::tests
python3 -m tools.cargo_run -- test -p vera20k-core --lib --features wasm sim::parallel_web_tests
python3 -m tools.cargo_run -- check -p vera20k --lib --bins
python3 -m tools.cargo_run -- check -p vera20k --lib --bins --features wasm
node --test tools/wasm/tests/*.test.cjs
npm --prefix tools/wasm/gateway ci
npm --prefix tools/wasm/gateway test
python3 -m unittest tools.tests.test_wasm_build
```

内存文件系统测试覆盖嵌套资源、归档字节共享、缺失文件、目录创建、排他创建存档以及恢复修改时间。串行工作测试将有序结果与原生执行进行比较。独立的手动 Wasm workflow 打包单机和联机版本，不改变桌面 workflow。网络检查覆盖传输边界和平台隔离；完整的浏览器之间及浏览器与桌面之间的对局、状态收敛、重连和对局结束后的重新进入都需要实机游玩验证。浏览器渲染结果、音频、持久化数据重新加载及存档读写也需要运行验证。
