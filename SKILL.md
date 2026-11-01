---
name: vps-reality-setup
description: Use when 从零搭一台自建代理 VPS（买机器 → SSH → 装 Xray REALITY → 客户端 → 加固 → 验收）。面向 agent 的可执行流程，脚本随包。
version: 1.0.0
author: Helioswei
license: MIT
metadata:
  hermes:
    tags: [vps, proxy, xray, reality, ssh, hardening]
---

# 从零搭一台自建代理 VPS

给 agent 用的端到端流程：**买机器 → 配 SSH → 装 Xray REALITY → 发客户端 → 加固 → 验收**。
每一步都给判据，不确定的地方明确写"要问用户"，不猜。

> 适用范围：单人或小圈子自用（1–5 台设备）。商业转售、公开分享违反多数云厂商 AUP，不在本流程范围内。

## 0. 开工前必须问清的 4 件事

| 问题 | 为什么必须先问 |
|---|---|
| **主要用途**：浏览网页 / 看视频 / 跑 API（Claude、OpenAI 等）/ 只是要个境外 IP | 决定地域、带宽、流量档；跑 API 和看 4K 的要求差 10 倍 |
| **预算与时长**：月付还是年付、能不能接受首年促销后续费翻倍 | 大促价多是首年价，续费回原价；决定要不要买长周期 |
| **设备数**：自己几台、是否多人共用 | ≤4 台不装面板，加设备就是加一条 client |
| **支付与实名**：能不能实名、有没有境外卡 | 国内云要实名；境外卡可走 Vultr/DO 按小时计费 |

**不要替用户下单。** 读价格、算成本、给选项可以做；绑卡、付款、点"确认购买"交回用户。

## 1. 选机器

### 地域按"目标服务的边缘节点在哪"选，不按"服务主体在哪"

对美系 API（Anthropic/OpenAI），东京往往比美国西岸更快——流量落在东京的边缘节点，不出太平洋。
选之前用两段法实测，别凭地理直觉：

```bash
# 第一段：本机 → 候选机（直连，不走代理）
ping -c 3 -q <候选IP>
# 第二段：候选机上 → 目标 API（TCP 握手，同业务路径）
ssh <host> 'for i in 1 2 3; do curl -4 -s -o /dev/null -w "%{time_connect} " --max-time 8 https://api.anthropic.com; done'
```

用户感知延迟 = 第一段 + 第二段。报数字必须注明：哪两点之间、什么协议、几次采样。

### 规格与套餐

| 项 | 判据 |
|---|---|
| 规格 | 1 核 1G 撑 5–10 人日常够用；CPU 从不是瓶颈 |
| 带宽 | 20–30Mbps 峰值够个人；标"200Mbps"的多是共享峰值，晚高峰会掉 |
| 流量 | 1TB/月够 3–5 人；看视频是最快的消耗项（1080p ≈ 1.5–3GB/h，4K ≈ 7GB/h） |
| 地域 | 东京/首尔对国内延迟最好；避开跨洲（250ms+） |
| 计费 | 先比同门相邻档，小规格常是性价比最差的一档 |
| 退款 | 国内云境外轻量常有 5 天无理由全额退，首单可用；折扣套餐不退 |

### 两个必须提醒用户的风险

1. **协议条款**：多数国内云 AUP 禁止"非法代理服务器"，实测通常不管，但**公开分享或转售**有封号风险，且他人滥用的投诉记在账号持有人头上。
2. **促销价陷阱**：1.7–2.5 折基本都是首年价，续费回日常价；"限 N 个"是资格门槛，能否享受要看结算页。

## 2. 配置 SSH（做任何事之前）

```bash
# 1) 确认端口可达（timed out = 防火墙丢包；refused = 端口通但没服务）
nc -z -v <IP> 22

# 2) 装公钥
ssh-copy-id -i ~/.ssh/id_ed25519.pub <user>@<IP>

# 3) 写 ~/.ssh/config 别名，后续命令一律用别名
Host my-vps
    HostName <IP>
    Port 22
    User <user>
    IdentityFile ~/.ssh/id_ed25519
    IdentitiesOnly yes
    ServerAliveInterval 30
    ServerAliveCountMax 3
    ConnectTimeout 10

# 4) 验证
ssh my-vps 'whoami; hostname; uname -sr; sudo -n true && echo sudo-ok'
```

**主机密钥变更不要自己删**：`ssh-keygen -F <IP>` 能看到旧记录。密钥变了只有两种可能——用户重装过系统，或者中间人。**先问用户**，确认后再 `ssh-keygen -R <IP>` + `ssh-keyscan`。

国内云默认是普通用户 + 免密 sudo，不是 root：所有安装命令加 `sudo`。

## 2.5 首选：用 vpsctl 单二进制（而不是一堆脚本）

`cli/` 里是 Rust 写的 `vpsctl`，一个静态二进制替代下面的脚本组合，跨平台（Win/macOS/Linux 都能当遥控器）。

```bash
# 装（从 Releases，带 sha256 校验）
curl -fsSL https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | bash
export VPSCTL_HOST=my-vps

vpsctl deploy --dry-run --clients "laptop,phone"   # 只生成+校验
vpsctl deploy --clients "laptop,phone"             # 真装（装 xray + 生成密钥 + systemd）
vpsctl status / doctor / selftest                  # 体检，selftest 会真的连一次
vpsctl user add tablet --selfcheck                 # 加设备并立即验证
vpsctl traffic / traffic --accumulate              # 按客户端用量 / 跨重启账本
vpsctl upgrade / rollback                          # 升级与回滚
vpsctl harden --ssh-port 22
vpsctl proxy install --from my-vps                 # 把另一台机器变成客户端
```

下面 3–7 节是**同一件事的手工/脚本流程**，留作对照：vpsctl 不在手边（无网络、架构不支持）时照做即可。
两条路径产出的配置是同一份结构，可以混用。

## 3. 装 Xray REALITY

```bash
# 先看不改：生成密钥对 + 每个客户端独立 uuid/shortId → 渲染配置 → xray -test → 打印链接
bash scripts/rebuild.sh --dry-run

# 再自测：本机 127.0.0.1:8443 起临时实例，逐客户端回环实测出口 IP，完事清理
bash scripts/rebuild.sh --selftest

# 真装：写配置（属主 xray、600）→ BBR → vnstat → 起服务 → 查监听 → 打印全部客户端链接
sudo bash scripts/rebuild.sh --clients "laptop,phone" --port 443 --sni www.cloudflare.com
```

脚本干的事与顺序（出问题就按这个顺序回查）：

1. 没有 xray 就装（GitHub 不可达时改传二进制：`ssh 旧机 'sudo cat /usr/local/bin/xray' | ssh 新机 'cat > /tmp/x && sudo install -m755 /tmp/x /usr/local/bin/xray'`）
2. `xray x25519` 生成密钥对（服务端存 privateKey，客户端用 publicKey）
3. 每客户端独立 `uuid` + `shortId`
4. 渲染 `config.json` → `xray run -test` 校验
5. `install -m 600 -o xray -g xray`（**属主错会 permission denied，而 -test 却通过**）
6. BBR + vnstat（月流量口径：tx = 计费出流量）
7. `systemctl enable --now xray`，检查端口监听
8. 一次性打印所有客户端 `vless://` 链接

**为什么换机不沿用旧私钥**：换机必然换 IP，客户端本来就要重新导入，保住私钥只省下改 `pbk` 一项。
现生成省掉一个要维护的 secrets 文件；唯一代价是每台设备导入一次新链接。
只有"IP 不变的重建"（重装系统/换盘）才需要沿用旧 `config.json`。

## 4. 放行端口（必须人工，脚本做不了）

云控制台 → 实例 → 防火墙 → 添加规则：TCP / `<port>` / 源 `0.0.0.0/0` / 允许。

判据：`nc -z -v <IP> <port>` 返回 **timed out** = 防火墙丢包（未放行）；**refused** = 端口可达但无服务。
22 通而 443 超时 = 典型未放行，不是被墙。

## 5. 客户端

加设备永远是"加一条独立 client"，不改 dest、不动私钥、不装面板：

```bash
./scripts/add_client.sh --host my-vps --email laptop --dry-run   # 先校验
./scripts/add_client.sh --host my-vps --email laptop             # 真加（重启 xray，瞬断 1–2 秒）
./scripts/add_client.sh --host my-vps --email laptop --selfcheck # 体检这条凭证
```

把脚本打印的 `vless://` 链接交给用户，或用 `scripts/make_vless_share_link.sh` 生成链接 + 终端二维码。

**三种客户端形态的完整配置见 `references/client-configs.md`**：mihomo/Clash 的 yaml（含导入步骤与验证命令）、
xray-core 的 client json、vless 链接的字段含义。要发给用户时直接从那里抄对应的一份，别手写。

手填这四项时最常错，错任一项都连不上：

```
security=reality   sni=<serverNames>   pbk=<publicKey>   sid=<shortId>
外加 flow=xtls-rprx-vision   fp=chrome
```

| 平台 | 客户端 |
|---|---|
| iOS | sing-box（免费，非中国区 ID）/ Shadowrocket（付费） |
| Android | **v2rayNG（只有 Android，没有电脑版）** / NekoBox / Hiddify / sing-box |
| Windows | v2rayN（与 v2rayNG 同作者，另一个项目）/ Clash Verge Rev |
| macOS | Clash Verge Rev / sing-box |
| Linux | v2rayN / Clash Verge Rev |

客户端从哪下、国内怎么下（含手机传 APK 的办法）、版本对照见 `references/client-downloads.md`；
**别从第三方 APK 站下**——这类软件能接管全部流量，捆绑一次就是长期后门。

**不装 3x-ui 面板**（≤4 台设备时）：面板是第二个公网入口，为加设备开一个能被扫描的后台不划算。
设备数 > 4 或需要自助管理时再考虑，且必须改默认端口 + 强密码。

### 5.1 反过来：让服务器自己当客户端（服务器走代理上网）

服务器需要访问 GitHub / pypi / npm 时，给它装个 xray 客户端连自建节点，**只绑回环、默认关闭**：

```bash
# 1) 写客户端配置（inbound socks 127.0.0.1:1080 + http 127.0.0.1:8118，outbound 指向节点）
# 2) 装 systemd 服务，并确保默认不自启
sudo systemctl disable --now xray-client
# 3) 装开关
sudo install -m 755 scripts/proxy-toggle.sh /usr/local/bin/proxy
```

然后把下面这段加到 `~/.bashrc`——**不加这段，`proxy on` 改不了当前 shell 的环境变量**（它是子进程）：

```bash
proxy() {
  case "$1" in
    on)  command /usr/local/bin/proxy on || return 1
         eval "$(command /usr/local/bin/proxy env)" ;;
    off) command /usr/local/bin/proxy off
         unset http_proxy https_proxy all_proxy no_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY ;;
    *)   command /usr/local/bin/proxy "$@" ;;
  esac
}
```

用法：

```bash
proxy on          # 起服务 + export 到当前 shell → 之后直接 git pull / hermes update
proxy status      # 看开没开、出口 IP 是否已是节点
proxy off         # 停服务 + 清变量
proxy env         # 只打印 export 行
proxy run <cmd>   # 只给这一条命令挂
```

**三条必须知道的边界**（踩过）：

| 场景 | 会不会生效 | 正确做法 |
|---|---|---|
| 交互式 shell 里 `proxy on` 后跑命令 | 生效 | — |
| **非交互式**（`ssh host 'cmd'`、cron、脚本） | 不生效——shell 函数根本没加载，变量不会 export | `proxy run <cmd>`，或显式 `curl -x http://127.0.0.1:8118` |
| systemd 服务（网关、守护进程） | 不生效 | 单独写 drop-in 的 `Environment=` |

也不要写进 `/etc/environment` 做全局代理：apt 和国内镜像会被拖到境外绕一圈，反而更慢。
监听只绑 `127.0.0.1`，云安全组不放行这两个端口。

验证链路真的通了（别靠"看起来配上了"）：

```bash
bash -ic 'proxy on >/dev/null; curl -s -o /dev/null -w "%{http_code}\n" https://api.github.com/rate_limit; curl -s https://ipinfo.io/ip; proxy off >/dev/null'
# 期望：200 + 节点 IP（不是服务器自己的 IP）
```

> 注意：不加 `bash -ic` 直接 `ssh host 'proxy on; curl ...'` 属于非交互式，函数不加载，
> 你会看到 github 200 但出口还是服务器自己的 IP——这正是最容易误判"已经走代理了"的地方。

## 6. 加固

```bash
bash scripts/harden_vps.sh my-vps
```

装 fail2ban（Ubuntu 24+ 无 `/var/log/auth.log`，backend 必须 `systemd`）+ 开 ufw，
**不改 SSH 登录方式**（用户说了保持现状就别重提改端口/关密码）。

两个必验点，不验就只是"看起来配上了"：

```bash
# 封禁真进内核了吗
sudo fail2ban-client set sshd banip 203.0.113.99 && sudo nft list ruleset | grep 203.0.113.99
sudo fail2ban-client set sshd unbanip 203.0.113.99
# 加固有没有掐断业务
sudo ufw status verbose    # 22 / <port> 都在
```

攻击面量化（别凭感觉说"有风险"）：`sudo ss -tlnp | awk 'NR>1 && $4 !~ /^127\./'`、
`sudo journalctl -u ssh --since '7 days ago' | grep -c 'Failed password'`。没人管过的公网机器 7 天 500+ 次失败登录是常态。

## 6.1 按客户端看流量（rebuild.sh 已默认开启）

配置里靠三样东西才统计得出来，缺一条就全为 0：

```jsonc
"stats": {},                                                  // 打开统计表
"api": {"tag": "api", "services": ["HandlerService", "StatsService"]},
"policy": {"levels": {"0": {"statsUserUplink": true, "statsUserDownlink": true}}},
// 还要一个 inbound + 一条 routing 规则，把 api 挂到回环端口上
{"tag": "api", "listen": "127.0.0.1", "port": 10085, "protocol": "dokodemo-door", "settings": {"address": "127.0.0.1"}}
{"type": "field", "inboundTag": ["api"], "outboundTag": "api"}
```

`rebuild.sh` 生成的配置已经包含这四项。**10085 只绑回环，安全组不要放行。**

查询用 `scripts/traffic.py`（装成 `/usr/local/bin/xray-traffic`）：

```bash
sudo install -m 755 scripts/traffic.py /usr/local/bin/xray-traffic
sudo xray-traffic                # 表格：每个 email 的上传/下载/合计/占比
sudo xray-traffic --json         # 机器可读
sudo xray-traffic --save /var/log/xray-traffic.tsv   # 追加快照（TSV，便于事后汇总）
```

**为什么值得配个脚本**：`xray api statsquery` 返回的是一坨原始 JSON，字段名是 `user>>>mac>>>traffic>>>uplink`，
想知道"谁用得最多""这些数字覆盖多长时间"都得自己算。脚本干三件事：改成人能看的表、按总量排序算占比、
带上 xray 本次运行的时长（否则看到 39 KiB 会以为统计坏了）。

**两条硬限制**（写进文档免得用户误判）：

- 计数在**内存**里，`systemctl restart xray` 即清零——重启后看到的不是"一天用了多少"。
- 长期总量看 `vnstat -m`（整机收发，不含客户端维度）。

### 6.1.1 "重启自动保存"能做到什么程度

**xray 自己做不到**：`xray api` 只有查询类命令（`stats` / `statsquery` / `statssys` …），
**没有写入或恢复接口**，所以不存在"重启前存、重启后读回来"这条路。任何宣称能接续的，
都是在外面自己记账。

做法是在外面加两个触发点，拼出"近似自动"：

```bash
sudo install -m 755 scripts/traffic.py /usr/local/bin/xray-traffic
sudo bash scripts/enable-traffic-accounting.sh        # 写 drop-in + timer，启用
```

它装两样东西：

| 触发点 | 作用 |
|---|---|
| `ExecStop=-xray-traffic --accumulate --quiet`（xray.service drop-in） | 停服/重启**前**抓最后一份（那时 API 还活着，这是唯一能拿到最后数据的时机） |
| `xray-traffic.timer`（每小时） | 崩溃/断电也不至于丢一整天 |

`--accumulate` 的算法，两个细节决定它对不对：

1. **增量而非快照**：记 `last`，本次值减上次值才是增量；累加进 `totals`（`/var/lib/xray-traffic/state.json`）。
2. **用 `ActiveEnterTimestamp` 判断换没换启动周期**：变了说明重启过、计数归零，此时 `last` 清空，
   本次值直接算增量；`totals` **保留**。

> 踩过的坑①：账本目录归 root 时，systemd 的 `ExecStop` 是以 `User=xray` 跑的，写不进去
> （日志里 `Permission denied: state.json.tmp`）。目录和文件都要归 `xray:xray`。
>
> 踩过的坑②：第一版把 `state["totals"] = totals` 写成了"只包含本轮出现的 email"，
> 于是 xray 刚重启、还没客户端连上时跑一次，**整个账本被清空**。修法是以已有 `totals` 为基础再累加。
> 教训：累加类脚本必须先在"空输入"上测一遍。

验证方式（别只看有没有文件）：

```bash
sudo xray-traffic --accumulate            # 记下累计值
sudo systemctl restart xray && sleep 5
sudo xray-traffic --accumulate            # 累计值必须 ≥ 刚才那个数，不能回到 0
```

## 7. 验收

```bash
# 服务端自身（不依赖任何客户端凭证）
bash scripts/rebuild.sh --selftest
bash scripts/reality-selfcheck.sh <uuid> <publicKey> <shortId> <sni> <port>

# 端口可达
nc -z -v <IP> <port>

# 客户端连上后：出口应是节点 IP
curl -x http://127.0.0.1:7897 https://ipinfo.io/json
# 国内站应该是直连（0.1–0.3s）
curl -x http://127.0.0.1:7897 -o /dev/null -w '%{time_total}\n' https://www.baidu.com/
```

**速度别用单流下结论**：跨境单连接受窗口限制，常只能拿到带宽的 1/10；同一文件中 4–8 段并行、
合计远高于单流即确认是单流现象。晚高峰（20:00–24:00）掉速是共享峰值套餐常态，白天复测再判断。

## 常见坑（按踩到频率排序）

| 坑 | 表现 | 解法 |
|---|---|---|
| **服务端版本别追 latest** | 26.7.11 起 REALITY 加 `minClientVer`；26.9.9 起要求 X25519MLKEM768。mihomo 默认配置会报 `REALITY authentication failed`，而 xray 客户端正常、回环自测全绿 | 默认装兼容地板 26.6.27；要用新版就在客户端 `reality-opts` 加 `support-x25519mlkem768: true`；`vpsctl rollback` 兜底 |
| **升级后客户端全断** | 服务端 `active`、回环自测通过，但手机/Clash 连不上（26.3.27→26.10.29 实测：mihomo 握手失败，xray 客户端正常） | 回环自测不等于兼容性测试：**升级后用真实客户端连一次**，不通就 `vpsctl rollback` |
| 配置属主错 | 服务 `failed` 但 `xray run -test` 通过；日志 `open config.json: permission denied` | `install -m 600 -o xray -g xray`——手改配置时最容易忘，写成 `-o root -g root` 必炸 |
| dest 与 serverNames 不一致 | 连上但没网（走回落路径） | 两端同刻改：服务端 `dest`/`serverNames`，客户端 `servername` |
| dest 选错 | 握手卡住 | 首选 `www.cloudflare.com:443`；apple/icloud 会被 xray 告警；microsoft 握手失败 |
| 换机后客户端全断 | 改了服务端没改客户端 | 公钥/uuid/shortId 三处任一变了都要同步 |
| `publicKey` 找不到 | config.json 里只有 `privateKey` | `xray x25519 -i <privateKey>` 反推 |
| 客户端是 CLI/Node 程序 | 以为系统代理生效了，其实没走 | Node 不读系统代理，显式给 `http_proxy`/`https_proxy` |
| 晚高峰测速 | 误判节点坏了 | 服务端自身直连测一次，分清"节点慢"和"跨境此刻慢" |

字段改名的坑：Xray 25+ 把客户端 `publicKey` 改名 `password`（x25519 输出的 `Password (PublicKey)` 行），
mihomo/Clash 仍叫 `reality-opts.public-key`，值相同。

## 文件

| 文件 | 用途 |
|---|---|
| `scripts/rebuild.sh` | 从零重建节点：生成密钥 → 渲染配置 → 校验 → 自测 → 安装 → 打印全部客户端链接 |
| `scripts/add_client.sh` | 给已有节点加一个独立客户端（含 `--dry-run` / `--selfcheck`） |
| `scripts/_server_add.py` | 上面脚本的服务端执行体（经 ssh stdin 传入，不落盘） |
| `scripts/harden_vps.sh` | fail2ban + ufw，不动 SSH 登录方式 |
| `scripts/reality-selfcheck.sh` | 在 VPS 本机跑真客户端回环验证 |
| `scripts/make_vless_share_link.sh` | 生成 `vless://` 链接 + 终端二维码 |
| `scripts/traffic.py` | 按客户端查流量（表格/JSON/快照/跨重启累计账本） |
| `scripts/enable-traffic-accounting.sh` | 让账本在 xray 重启前自动保存 + 每小时落一次（drop-in + timer） |
| `scripts/proxy-toggle.sh` | 把本机当客户端时的 `proxy on/off/status` 开关（只绑回环，默认关闭） |
| `references/dest-selection.md` | 伪装目标怎么挑（含候选筛选脚本用法） |
| `references/client-configs.md` | 三种客户端配置模板：mihomo yaml / xray-core json / vless 链接 + 导入与排错 |
| `cli/` | Rust 单二进制 `vpsctl`（部署/用户/体检/流量/升级/加固/反代客户端） |
| `install.sh` | 从 Releases 安装 vpsctl（sha256 校验） |
| `.github/workflows/release.yml` | 打 tag 出全平台二进制 + SHA256SUMS |
| `references/client-downloads.md` | 各平台客户端对照（含 v2rayNG 只有 Android）+ 国内下载办法 + 校验 |
