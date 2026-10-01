# 剑网三网络诊断

**JX3 Network Diagnostics** 是面向 Windows 的剑网三网络诊断工具。遇到游戏卡顿、掉线或延迟波动时，选择区服和测试时长，工具会持续观测网关、公网参照、区服接入地址、游戏进程及本机其他程序的网络活动，保存日志并生成中文分析报告。

当前版本：**0.1.0**。技术栈：Tauri 2、React 19、TypeScript、Rust、Windows 网络 API / ETW。

本项目为玩家工具，与剑网三、金山官方无隶属关系。

## 主要功能

| 功能 | 说明 |
| --- | --- |
| 区服选择 | 读取官方目录，按合服后的名称归并；支持旧服名搜索、多个接入地址、在线更新和本地回退 |
| 持续测试 | 默认 10 分钟，可选 5 / 30 / 60 分钟或自定义 1–180 分钟；支持提前结束 |
| 多目标探测 | 对照网关、两个公网参照、区服接入地址和部分游戏实际 IPv4 远端；ICMP 与 TCP 建连分别统计 |
| 游戏进程 | 显示 PID、完整 EXE 路径、CPU 与内存；记录实际连接变化，退出后按原路径查找唯一候选重新关联 |
| 本机网络活动 | 记录 TCP IPv4/IPv6 连接、UDP 本地端点、各网卡流量和可读取的进程身份 |
| 每进程流量 | 通过 Windows ETW 观测 TCP/UDP 上传、下载及累计字节；权限或事件不可用时明确标注 |
| 卡顿标记 | 点击“刚刚卡了”，将体感时间与前后观测对齐 |
| 日志与报告 | 测试中持续写入 JSONL；结束后生成统计、证据、可能原因和下一步建议，支持历史记录与离线 HTML 报告 |

## 快速使用

运行环境：**Windows 10/11 x64 + Microsoft Edge WebView2 运行时**。Windows 11 及多数 Windows 10 已安装 WebView2；安装包可在需要时联网下载安装，单独 EXE 不包含运行时。

1. **先启动游戏，再打开工具**。使用已经构建好的 `jx3-network-diagnostics.exe`，或者运行安装包后启动。
2. 选择当前区服；只记得旧名称时可通过搜索找到合服后的区服。
3. 选择游戏进程并核对 EXE 路径。多开客户端时确认 PID；不关联进程也可以测试网络。
4. 选择时长，点击“开始测试”，照常玩游戏。偶发问题建议测试 30 分钟或更长。
5. 出现卡顿或掉线时点击“刚刚卡了”。在“联网程序”页面查看后台活动，展开程序可看路径与连接。
6. 到时或点击“结束并分析”，查看报告。通过“日志目录”打开离线报告和原始记录。

**两种启动顺序都支持。** 如果先开工具、再开游戏，点击“刷新进程列表”后选择游戏即可。自动候选识别基于进程名，未识别出的客户端仍可手动选择。

若提示每进程流量不可用，可尝试右键 EXE →“以管理员身份运行”。管理员身份也不保证所有受保护进程可读；不可读取的数据会保留缺失状态。

源码仓库不存放 EXE、安装包、实际诊断日志或构建缓存。首次获取源码后请按下方步骤构建；产物在本地 `release/` 目录。也可阅读[简明使用说明](docs/使用说明.txt)。

## 如何理解报告

报告按“现象、证据、可能原因、判断把握、验证建议”组织内容。第一版主要根据同一时间窗口内多个 ICMP 目标的变化缩小故障范围，TCP 建连用于独立统计和补充证据。

ICMP 超时不是游戏丢包率，TCP 建连耗时不是游戏内延迟。全程未回应的目标不能据此判为故障；中间某一跳不回应，也不能证明那一跳丢弃游戏数据。加速器可能使主动探测与游戏使用不同路径。

每进程流量是观测到的网络事件字节，存在交付延迟；代理、回环及虚拟网卡可能造成重复观测，不要把各进程或各网卡简单相加作为带宽账单。未知进程身份不归属流量。当前版本未采集游戏帧时间、TCP 重传、UDP 远端、路由器内部指标，也不对未知连接做恶意软件判断。

单端诊断提供可疑范围及对照验证建议，不能保证精确区分去程、回程与服务端内部故障。

## 日志与数据范围

应用内“日志目录”打开当前记录。默认位于 `%LOCALAPPDATA%/com.jx3.network-diagnostics/sessions/`，实际路径以界面为准。

| 文件 | 内容 |
| --- | --- |
| session.json | 配置、开始时间、区服目录来源 |
| samples.jsonl | 原始环境、每秒快照、探测结果、事件 |
| report.json | 统计和分析结果 |
| report.html | 可离线查看、浏览器打印的中文报告 |

逐秒刷新日志、每 5 秒请求落盘。正常停止和关闭完成收尾；异常退出留下已落盘样本，历史标记为“未完成”。第一版保留日志，不提供中断会话自动重新分析。根据机器联网活动量，长时间测试的日志可能较大。

日志只保存在本机，包含程序路径、连接地址和观测数据，不保存网络正文，也不自动上传。工具不会修改网络设置或结束其他程序。手动分享日志前，可以先检查其中的路径和地址信息。

区服目录来源：[剑网三官方服务器列表](https://jx3comm.xoyocdn.com/jx3hd/zhcn_hd/serverlist/serverlist.ini)。内置目录日期为 2026-10-01；目录采用 GBK 编码，保留官方原始字节。接入 IP 不等于物理服务器数量，也不一定是进入地图或副本后的实际连接地址。

## 获取源码与构建

开发环境：

- Node.js 20.19+ 或 22.12+，以及 npm。
- Rust stable，MSVC 工具链。
- Visual Studio 2022 Build Tools 的“使用 C++ 的桌面开发”组件和 Windows SDK。
- Microsoft Edge WebView2 运行时。

这是私有仓库，克隆需要具有仓库访问权限的 GitHub 账号。

```powershell
git clone https://github.com/Vintcet/jx3-network-diagnostics.git
cd jx3-network-diagnostics
npm ci
npm run dev
```

`npm run dev` 会通过脚本查找 Visual Studio 构建环境并启动 Tauri。`npm run dev:web` 只预览界面，浏览器无法访问本工具的 Windows 采集接口。

构建正式版和安装包：

```powershell
npm run build
```

| 本地产物 | 用途 |
| --- | --- |
| `release/jx3-network-diagnostics.exe` | 单文件程序，依赖已安装的 WebView2 |
| `release/jx3-network-diagnostics-0.1.0-x64-setup.exe` | Windows 安装包 |
| `release/使用说明.txt` | 面向使用者的简明说明 |
| `release/SHA256SUMS.txt` | EXE 和安装包校验值 |

`.cargo/config.toml` 使用 rsproxy Cargo 镜像；`package-lock.json` 固定前端依赖版本。`scripts/tauri.test.conf.json` 只用于自动化测试，正式构建不应传入该配置。

## 验证

```powershell
npm run check
npm run build:web
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

当前已完成核心逻辑测试、真实 Windows 短时采集、桌面 IPC 操作和界面布局检查，详见[验证记录](docs/验证记录.md)。朋友电脑上的真实游戏掉线、不同权限、加速器和多开场景仍需要现场对照测试。

后端短时自检可在工程目录运行：

```powershell
cargo run --manifest-path src-tauri/Cargo.toml -- --self-check --seconds 12
```

自检沿用真实采集、落盘和分析流程，使用内置区服，关联工具自身进程，日志写入工作目录 `.tmp/native-check`。`--stop-early` 验证提前停止。它不会模拟用户真实游戏卡顿；上线验收仍需在朋友电脑上边玩边测。

真实桌面界面测试需要 Python、Playwright，以及本机 Edge。使用单独的调试配置构建后运行：

```powershell
npm run tauri -- build --debug --no-bundle --config scripts/tauri.test.conf.json
python scripts/test-desktop.py
```

测试只启动本工具实例，通过本机 49387 端口连接 WebView2，完成后关闭。正式版不配置调试端口。

`scripts/test-ui.py` 用于浏览器界面检查，运行前需另行启动 `npm run dev:web`。`scripts/make-icon.py` 是可选图标生成脚本，需要 Pillow；构建直接使用已经提交的图标，不要求安装 Pillow。

## 结构

- `src/`：React 界面、图表、进程表格、历史和报告。
- `src-tauri/src/platform.rs`：Windows 进程、连接表、网卡、Wi-Fi。
- `src-tauri/src/traffic.rs`：ETW 网络事件，仅解析元数据。
- `src-tauri/src/probe.rs`：ICMP、TCP、逐跳探测。
- `src-tauri/src/session.rs`：后台调度、日志、会话和历史。
- `src-tauri/src/analysis.rs`：统计、诊断规则和离线报告。
- `src-tauri/src/catalog.rs`：官方 GBK 目录解析、缓存和回退。

## 反馈问题

反馈时请注明 Windows 版本、工具版本、游戏区服、有线或 Wi-Fi、是否使用加速器，以及故障发生时间。优先附报告中的异常结论和对应时间段；原始日志由你自行决定是否分享。
