# vpsctl

一个静态二进制管一台自己的机器：部署 / 用户 / 体检 / 流量账本 / 升级 / 回滚 / 加固。
不做面板、不做机场、不暴露后台入口。

## 只需要一个 bin：装在服务器上，本地零安装

`vpsctl` 干的是「在 Linux 上操作 xray/systemd」，所以**功能上只有一份是真正必要的**：
服务器上那个 Linux 版。本地（macOS/Windows）想省事就不用装：

```bash
bash install.sh --remote my-vps   # 装到服务器，本地什么都不装
ssh my-vps vpsctl status                     # 零参数：在服务器上就跑本机模式
alias vps='ssh my-vps vpsctl'                # 想更短，加进 ~/.zshrc
```

两者是同一份代码：不带 `--host` 且本机是 Linux → 本机执行；本机不是 Linux → 直接报错并给出上面两条用法，
不会误读本地路径。

想本地装一份当遥控器也行，`install.sh` 按 `uname` 自动挑产物（你不用选平台）：

```bash
curl -fsSL https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | bash
vpsctl --host my-vps status
```

### 只要一个文件给用户：`vpsctl.sh` 自举入口

`opensource/vpsctl.sh` 是**单个 shell 文件**，跨 macOS / Linux（Windows 走 Git Bash / WSL）。
首次运行按 `uname` 取对应平台的静态二进制、校验 sha256 后缓存，再 `exec` 过去——用户手里始终只有这一个文件。

```bash
./vpsctl.sh --host my-vps status     # 首次会拉对应平台二进制，之后直接用
VPSCTL_VERSION=v0.1.0 ./vpsctl.sh    # 钉版本
```

### 关于"一份代码到处跑"的其他方案（为什么没选）

| 方案 | 用户拿到 | 需要先装 | 代价 |
|---|---|---|---|
| **原生二进制 + 自举脚本（现方案）** | 1 个 .sh | 无（只要 sh/curl） | 每平台仍要编一次（CI 做） |
| Node 单文件 `.js` | 1 个 .js | **node** | 服务器上未必有；纯 JS 才行，带原生插件就要 node-gyp 编译 |
| Python 单文件 `.py` | 1 个 .py | **python3** | 同上，且启动慢、类型/异常处理更松 |
| npm 包（内含各平台二进制） | 1 个包名 | **npm** | 这就是 esbuild/swc 的做法，本质仍是每平台一份 |
| JVM jar | 1 个 jar | **JRE** | 启动慢，服务器上基本不装 |
| WASM/WASI | 1 个 .wasm | **wasmtime** | 沙箱里不能起子进程/调 systemd —— 这个工具全废 |

结论：**"一个文件"和"一份二进制"是两件事**。要"一个文件"，自举脚本最省；要"零运行时依赖"，
就得每平台编一份。这个工具真正的执行环境只有 Linux，所以默认只推服务器那一份。

### 为什么不能"一个文件同时跑三系统"

ELF / Mach-O / PE 是三种不同的可执行格式，任何语言都一样（Go 也要每平台编一份）。
真正的减法是**减少需要发布的平台数量**，不是把三个格式塞进一个文件：
功能只需要 Linux → 就只发 Linux（x86_64 / aarch64 两种架构，因为 CPU 指令集不同）。

## 平台

| 平台 | 当"遥控器"（连远端 Linux VPS） | `--local`（就在本机执行） |
|---|---|---|
| Linux x86_64 / aarch64 | ✅ | ✅（需要 systemd） |
| macOS Intel / Apple Silicon | ✅ | ❌（没有 systemd） |
| Windows x86_64 | ✅（用系统自带 OpenSSH 客户端） | ❌ |

也就是说：**笔记本上装一份就能管服务器**；服务器端操作（`--local`）只支持 Linux。
Windows 上 `ssh` 必须可用（Win10 起自带：设置 → 应用 → 可选功能 → OpenSSH 客户端）。

## 安装（就是 opencode / pi 那套机制）

一条命令的本质都一样：**安装脚本 + `uname` 探测平台架构 + 从 Release 取对应资产 + 装到用户目录**。
opencode 是 `curl -fsSL https://opencode.ai/install | bash`，pi 是 `curl -fsSL https://pi.dev/install.sh | sh`
（Windows 另给 `irm .../install.ps1 | iex`）。脚本里就是 `uname -s` / `uname -m` 两个 case，
用户不需要知道自己在什么平台上。

### 开源后的分发模型

```
唯一发布源：GitHub Releases（公开仓 → 匿名 curl 直接可下，不需要 gh、不需要 token）
安装脚本：  仓库里的 install.sh（可 --base 指向任意镜像）
Windows：   install.ps1
```

脚本的取包顺序是**先直链、后 gh**：直链能通（公开仓就是这种）就完全不碰 gh；
gh 只是私有仓/内网的自用兜底，开源用户根本走不到那一步。

```bash
# macOS / Linux，装到 ~/.local/bin（/usr/local/bin 可写才装那儿，全程不要 sudo）
curl -fsSL https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | sh

# 只装服务器，本地零安装
curl -fsSL .../install.sh | sh -s -- --remote my-vps

# Windows
irm https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.ps1 | iex

# 自己编
cargo build --release --manifest-path cli/Cargo.toml
install -m 755 cli/target/release/vpsctl ~/.local/bin/vpsctl
```

### 国内网络：用 GitHub 代理，不自建 CDN

开源项目**不该为了国内用户自建 CDN**：维护成本高，而且海外用户反而被拖慢（GitHub 在海外是最快的源）。
社区通用做法是让国内用户把下载源换成 GitHub 代理前缀，脚本已经留好开关：

```bash
# 任选一个还在维护的 GitHub 代理前缀（这类服务变动频繁，用前自己验一下）
curl -fsSL <代理前缀>/https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | sh
# 或直接给安装脚本指定下载根：
bash install.sh --base <代理前缀>/https://github.com/Helioswei/vpsctl/releases/latest/download
```

`vpsctl.sh` 认同一个环境变量：`VPSCTL_BASE=<代理前缀>/https://github.com/.../releases/latest/download`。

> 说明：以前这里写过一个 `cdn.<域名>` 的例子，那只是占位，本项目**不打算自建 CDN**。
> `install.ps1` 尚无 Windows 环境实测，首次使用时请留意输出。

## 只需要一个 bin：装在服务器上，本地零安装

`vpsctl` 干的是「在 Linux 上操作 xray/systemd」，所以**功能上只有一份是真正必要的**：
服务器上那个 Linux 版。本地（macOS/Windows）想省事就不用装：

```bash
bash install.sh --remote my-vps   # 装到服务器，本地什么都不装
ssh my-vps vpsctl status                     # 零参数：在服务器上就跑本机模式
alias vps='ssh my-vps vpsctl'                # 想更短，加进 ~/.zshrc
```

两者是同一份代码：不带 `--host` 且本机是 Linux → 本机执行；本机不是 Linux → 直接报错并给出上面两条用法，
不会误读本地路径。

想本地装一份当遥控器也行，`install.sh` 按 `uname` 自动挑产物（你不用选平台）：

```bash
curl -fsSL https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | bash
vpsctl --host my-vps status
```

### 只要一个文件给用户：`vpsctl.sh` 自举入口

`opensource/vpsctl.sh` 是**单个 shell 文件**，跨 macOS / Linux（Windows 走 Git Bash / WSL）。
首次运行按 `uname` 取对应平台的静态二进制、校验 sha256 后缓存，再 `exec` 过去——用户手里始终只有这一个文件。

```bash
./vpsctl.sh --host my-vps status     # 首次会拉对应平台二进制，之后直接用
VPSCTL_VERSION=v0.1.0 ./vpsctl.sh    # 钉版本
```

### 关于"一份代码到处跑"的其他方案（为什么没选）

| 方案 | 用户拿到 | 需要先装 | 代价 |
|---|---|---|---|
| **原生二进制 + 自举脚本（现方案）** | 1 个 .sh | 无（只要 sh/curl） | 每平台仍要编一次（CI 做） |
| Node 单文件 `.js` | 1 个 .js | **node** | 服务器上未必有；纯 JS 才行，带原生插件就要 node-gyp 编译 |
| Python 单文件 `.py` | 1 个 .py | **python3** | 同上，且启动慢、类型/异常处理更松 |
| npm 包（内含各平台二进制） | 1 个包名 | **npm** | 这就是 esbuild/swc 的做法，本质仍是每平台一份 |
| JVM jar | 1 个 jar | **JRE** | 启动慢，服务器上基本不装 |
| WASM/WASI | 1 个 .wasm | **wasmtime** | 沙箱里不能起子进程/调 systemd —— 这个工具全废 |

结论：**"一个文件"和"一份二进制"是两件事**。要"一个文件"，自举脚本最省；要"零运行时依赖"，
就得每平台编一份。这个工具真正的执行环境只有 Linux，所以默认只推服务器那一份。

### 为什么不能"一个文件同时跑三系统"

ELF / Mach-O / PE 是三种不同的可执行格式，任何语言都一样（Go 也要每平台编一份）。
真正的减法是**减少需要发布的平台数量**，不是把三个格式塞进一个文件：
功能只需要 Linux → 就只发 Linux（x86_64 / aarch64 两种架构，因为 CPU 指令集不同）。

## 平台

| 平台 | 当"遥控器"（连远端 Linux VPS） | `--local`（就在本机执行） |
|---|---|---|
| Linux x86_64 / aarch64 | ✅ | ✅（需要 systemd） |
| macOS Intel / Apple Silicon | ✅ | ❌（没有 systemd） |
| Windows x86_64 | ✅（用系统自带 OpenSSH 客户端） | ❌ |

也就是说：**笔记本上装一份就能管服务器**；服务器端操作（`--local`）只支持 Linux。
Windows 上 `ssh` 必须可用（Win10 起自带：设置 → 应用 → 可选功能 → OpenSSH 客户端）。

## 安装（和 opencode / pi 一样的机制）

一条命令的本质都一样：**一个安装脚本 + `uname` 探测平台架构 + 从 Release 取对应资产 + 装到用户目录**。
opencode 是 `curl -fsSL https://opencode.ai/install | bash`，pi 是 `curl -fsSL https://pi.dev/install.sh | sh`
（Windows 另给 `irm .../install.ps1 | iex`）——脚本里就是 `uname -s` / `uname -m` 两个 case，
不需要用户知道自己在什么平台上。

```bash
# macOS / Linux：装到 ~/.local/bin（/usr/local/bin 可写才装那儿，全程不要 sudo）
curl -fsSL https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | bash

# 走自建镜像/CDN（国内不用翻墙，约定 <base>/<asset> 与 <base>/SHA256SUMS）
bash install.sh --base https://cdn.example.com/vpsctl

# 只装服务器，本地零安装
bash install.sh --remote my-vps

# Windows（PowerShell）
irm https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.ps1 | iex

# 自己编
cargo build --release --manifest-path cli/Cargo.toml
install -m 755 cli/target/release/vpsctl ~/.local/bin/vpsctl
```

> `install.ps1` 尚无 Windows 环境实测（写完没跑过），首次在 Windows 上用时注意看输出。

## 用法（默认走 ssh，复用 ~/.ssh/config 的别名）

```bash
export VPSCTL_HOST=my-vps        # 也可以每条命令加 --host my-vps

vpsctl status                    # 服务/端口/客户端/运行时长
vpsctl doctor                    # 二进制、配置、服务、端口、端到端回环
vpsctl selftest                  # 逐条凭证真连一次，打印出口 IP
vpsctl traffic                   # 本次启动以来各设备用量
vpsctl traffic --accumulate      # 跨重启累计账本
vpsctl user list
vpsctl user add laptop --selfcheck
vpsctl user rm laptop
vpsctl upgrade                   # 备份 → 校验 → 替换 → 失败回滚
vpsctl rollback                  # 升级后客户端连不上时用这条
vpsctl deploy --dry-run --clients "laptop,phone"
vpsctl harden --ssh-port 22
vpsctl proxy install --from my-vps   # 让另一台机器当客户端（凭证从节点机器读）
vpsctl proxy on / off / status / env
```

## 设计要点

- **凭证纯本地生成**：X25519 密钥对（`x25519-dalek`）+ UUID + shortId，不用 ssh 到服务器跑 `xray uuid`。
  已和 `xray x25519 -i` 的输出逐字节对齐（`vpsctl keys` 会并排对照）。
- **所有写操作都是"备份 → 临时文件 → xray -test → install -o xray → 重启 → 检查 → 失败回滚"**。
  配置属主必须 `xray:xray`，否则服务 `permission denied`（踩过）。
- **不引 SSH 库**：直接调系统 `ssh`，ProxyJump / agent / 多密钥全部照旧生效。
- **不做全局代理**：`proxy on` 只起本地服务，环境变量由 `proxy env` 打印给 shell 自己 eval。

## xray 版本策略：默认装「兼容地板」，不追 latest

`deploy` 默认装 **v26.6.27**（`deploy.rs` 的 `TESTED_XRAY_VERSION`）——它是实测里
「不用改任何客户端就能连通」的最高版本。要别的版本用 `--xray-version`，或 `--xray-url <镜像>`。

### 实测版本阶梯（2026-10-31，逐版验证）

| 服务端 | mihomo 默认配置 | mihomo + `support-x25519mlkem768: true` | xray 客户端 |
|---|---|---|---|
| 26.3.27 / 26.4.25 / 26.6.27 | ✅ | ✅ | ✅ |
| 26.7.11 / 26.7.28 / 26.9.9 / 26.10.21 / 26.10.29 | ❌ `REALITY authentication failed` | ✅ | ✅ |

原因（上游 XTLS/Xray-core#6477）：

- **≥26.7.11**：REALITY 引入 `minClientVer` 校验，老客户端版本号不够直接被拒；
  服务端写 `minClientVer: "1.0.0"` 可以放宽（官方不推荐）。
- **≥26.9.9**：要求 TLS 里带 X25519MLKEM768（后量子混合密钥交换），mihomo 默认不发，
  必须在 `reality-opts` 下写 `support-x25519mlkem768: true`。实测 26.10.29 只改
  `minClientVer` 不够，必须客户端开这个开关。
- xray 自己的客户端两种版本都能连 → **回环自测永远发现不了这个问题**。

查看现状：`vpsctl versions`。切版本：`vpsctl upgrade --version 26.10.29`，出问题 `vpsctl rollback`。

## 升级的坑（实测）

`26.3.27 → 26.10.29` 服务端升级后，**Clash(mihomo) 客户端全部握手失败**，而服务端自己的
回环自测（xray 客户端连 xray 服务端）是通过的——两边实现不一致时，回环测不出问题。

所以流程是：**升级 → 用真实客户端（手机 v2rayNG / 桌面 Clash）连一次 → 不通立刻 `vpsctl rollback`**。
`vpsctl upgrade` 不带参数时升到实测版本；`--latest` 才会升到上游最新，并会先打印警告。

## 开发

```bash
cargo test        # 含密钥往返、shortId 格式
cargo build --release
```

发布：打 tag（`v*`）触发 `.github/workflows/release.yml`，产出
linux-x86_64(musl) / linux-aarch64(musl) / darwin-x86_64 / darwin-aarch64 / windows-x86_64 + `SHA256SUMS`。
