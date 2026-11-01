---
name: xrayctl
description: Use when 从零搭一台自建代理 VPS（选机器 → SSH → xrayctl 部署 Xray REALITY → 发客户端 → 加固 → 验收）。
version: 1.0.0
author: Helioswei
license: MIT
metadata:
  hermes:
    tags: [vps, proxy, xray, reality, ssh, hardening]
    related_skills: [remote-server-access, china-network-tooling]
---

# 从零搭一台 REALITY 节点（xrayctl 版）

目标：一台普通 Linux VPS → 手机/电脑能上外网，全程有判据、可复验、能回滚。

工具只有一个：`xrayctl`（Rust 单二进制，装在服务器 `/usr/local/bin`，本地用 ssh 遥控）。

## 0. 开工前必须问清 4 件事

```
1 用途     订阅网页（claude.ai / ChatGPT）还是纯开发出海？
           前者看 IP 身份（机房 IP 会被风控），后者只看延迟和稳定
2 设备数   每台设备一条独立凭证，删一条不影响别人
3 支付     谁买、什么卡。工具不代下单：读价、算成本、给选项，确认购买交回人
4 出口地区 从"目标服务的边缘在哪 + 本机到哪最快"定，不按"服务主体在哪"
```

## 1. 选机器

地域别凭感觉，测两段延迟：

```bash
# 第一段：本机 → 候选机（直连，不走代理）
ping -c 5 <候选IP>

# 第二段：候选机 → 目标服务（同一业务路径）
ssh <别名> 'curl -s -o /dev/null -w "connect=%{time_connect} TLS=%{time_appconnect}\n" https://api.anthropic.com/'
```

规格：1–2 核 / 1G 内存起够用（xray 常驻几十 MB），磁盘 20G 够；流量按"订阅 + 日常"估。
促销首年便宜、续费回原价，谈价时看清。

**机器之外还有一道墙**：云控制台的防火墙/安全组。主机 ufw 开了不代表外面能进，
控制台不放行 443 时外面 `nc -z` 超时（"拒绝"和"超时"是两种故障，别混）。这步只能人工。

## 2. SSH（做任何事之前）

```bash
nc -z -G 5 -w 5 <IP> 22                        # macOS 的 nc 要 -G；Linux 只有 -w
ssh-copy-id -i ~/.ssh/id_rsa.pub ubuntu@<IP>   # 密码由用户自己输，不代填
```

`~/.ssh/config` 写别名，后续一律用别名：

```sshconfig
Host my-vps
    HostName <IP>
    User ubuntu
    IdentityFile ~/.ssh/id_rsa
    IdentitiesOnly yes
    ServerAliveInterval 30
    ConnectTimeout 10
```

主机密钥变更（重装系统 / IP 复用）：**先把新旧指纹给用户确认，再 `ssh-keygen -R` 清理**，
不要用 `StrictHostKeyChecking=no` 把判断关掉。

验证：`ssh my-vps 'whoami; hostname; . /etc/os-release && echo $PRETTY_NAME'`。

## 3. 部署

```bash
# 只生成 + 校验，不碰系统（先看一眼）
xrayctl --host my-vps deploy --clients laptop,phone --dry-run

# 真装：装 xray（固定版本）→ 生成密钥 → 渲染配置 → xray -test → systemd → 起服务
xrayctl --host my-vps deploy --clients laptop,phone

xrayctl --host my-vps doctor
xrayctl --host my-vps status
```

换机复用旧凭证（客户端只改地址，不用重新导入）：

```bash
xrayctl --host new-vps deploy --from-config ~/backup/old-config.json
```

xray 版本策略见 README：默认固定版本，**不追 latest**；`--xray-version` 可指定。

## 4. 放行端口

```bash
# 云控制台：安全组 / 防火墙加 TCP 443（人工，工具做不到）
xrayctl --host my-vps harden --ssh-port 22   # 主机侧：fail2ban + ufw
```

验收：本机 `nc -z -G 5 -w 5 <IP> 443`。

## 5. 客户端

```bash
# 生成 Clash 配置（默认只写 ./dist，不动你正在用的配置）
xrayctl --host my-vps client sync

# 要它接管 Clash Verge 当前 profile（先自动备份）
xrayctl --host my-vps client sync --apply
```

产出：`dist/clash-profile.yaml`、`dist/links.txt`（每条凭证一条 vless 链接）、
`dist/phone-import.txt`、`dist/qr-<客户端>.png`。

加/删设备：

```bash
xrayctl --host my-vps user add tablet --selfcheck   # 加完立刻真连一次
xrayctl --host my-vps user rm tablet                # 同步删 shortId，不留孤儿
```

**服务端自己也当客户端**（只绑回环、默认关）：

```bash
xrayctl --host my-vps proxy install --from my-vps
xrayctl --host my-vps proxy on      # socks5 127.0.0.1:1080 / http 8118
xrayctl --host my-vps proxy off
xrayctl --host my-vps proxy env     # 打印 export，给 CLI 程序用
```

Node 程序不读系统代理，必须显式 `http_proxy/https_proxy`。

## 6. 加固

```bash
xrayctl --host my-vps harden --ssh-port 22
```

内容是 fail2ban + ufw（只放行 SSH 端口与 443），**不动 SSH 登录方式**——改端口/关密码登录属于用户决策，
工具不越权。验完看 `ufw status` 与 `fail2ban-client status sshd`。

## 7. 验收（三层，缺一层都可能假绿）

```bash
# ① 服务端自身
xrayctl --host my-vps doctor

# ② 逐条凭证回环真连（临时实例走完整握手，取出口 IP）
xrayctl --host my-vps selftest

# ③ 真实客户端：Mac 载入 profile → 打开 https://ipinfo.io（应是节点 IP）
#    手机扫码导入 → 同上；国内站应直连（0.1–0.3s）
```

**回环自测全绿 ≠ 客户端能用**：xray 自家客户端对服务端版本不挑，mihomo 挑。
升级服务端后必须用真实客户端连一次。

## 8. 流量账本

```bash
xrayctl --host my-vps traffic                     # 本次运行按客户端累计
xrayctl --host my-vps traffic --json              # 给脚本读
xrayctl --host my-vps traffic --enable-accounting # 每小时快照 + 停止前兜底
xrayctl --host my-vps traffic --accumulate        # 跨重启累计
```

xray 计数在内存里，重启清零；长期总量以 `vnstat -m` 为准。账本目录属主必须是 `xray`
（服务以 `User=xray` 跑，root 拥有的目录它写不进）。

## 常见坑（按踩到频率排序）

| 坑 | 表现 | 解法 |
|---|---|---|
| **服务端版本追 latest** | 26.7.11 起加 `minClientVer`、26.9.9 起强制 X25519MLKEM768；mihomo 报 `REALITY authentication failed`，而 xray 客户端与回环自测全绿 | 装固定版本；用新版必须让客户端带 `support-x25519mlkem768: true`（`client sync` 自带）；`rollback` 兜底 |
| **云控制台没放行** | 外面 `nc 443` 超时，主机 ufw 看着正常 | 两道墙分开查：控制台安全组 + 主机 ufw |
| **配置属主错** | 服务 `failed`，日志 `permission denied` | 配置 `600 xray:xray`；手改最容易忘 |
| dest 与 serverNames 不一致 | 连上但没网（走回落） | 服务端 `dest`/`serverNames` 与客户端 `servername` 同时改 |
| `bind 443: permission denied` | 非 root 用户跑服务、没给能力 | unit 加 `AmbientCapabilities=CAP_NET_BIND_SERVICE` + `CapabilityBoundingSet=` + `NoNewPrivileges=true` |
| 换机后客户端全断 | 只改了服务端 | `client sync` 重新生成；或 `deploy --from-config` 复用旧凭证 |
| `publicKey` 找不到 | config.json 只有 `privateKey` | `xrayctl keys`（内部 `xray x25519 -i` 反推） |
| 单流测速下结论 | 以为节点只有 1/10 带宽 | 跨境单连接受窗口限制；同文件 4–8 段并行合计再判断 |
| 晚高峰掉速 | 误判节点坏了 | 服务端自身直连测一次，分清"节点慢"和"跨境此刻慢" |
| 多设备共用一个 uuid | 删一条连坐 | 每台设备一条独立 client |

字段改名：Xray 25+ 客户端配置里 `publicKey` 改名 `password`（值相同）；
mihomo/Clash 仍叫 `reality-opts.public-key`，vless 链接里叫 `pbk`。

## 文件

| 路径 | 用途 |
|---|---|
| `cli/` | Rust 源码（`xrayctl` 本体） |
| `install.sh` / `install.ps1` / `xrayctl.sh` | 三种安装入口（本地安装 / Windows / 单文件自举） |
| `references/dest-selection.md` | 伪装目标怎么挑 |
| `references/client-configs.md` | mihomo / xray-core / vless 链接三种配置与排错 |
| `references/client-downloads.md` | 各平台客户端下载 |
| `.github/workflows/release.yml` | 打 tag 出全平台二进制 + SHA256SUMS |
