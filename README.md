# vpsctl

一个给 AI agent 用的 skill：**从零把一台 VPS 变成可用的 VLESS + REALITY 节点**。
覆盖买机器时该看什么、SSH 怎么配、脚本怎么跑、客户端怎么发、怎么加固、怎么验收。

不是网上一堆"一键脚本"的又一个副本。差别在于：**每一步都写了判据和踩过的坑**，
包括那些看起来配好了其实没生效的地方（配置属主、dest 与 servername 必须同步、
单流测速的假结论）。脚本只是把这些判据固化下来。

## 它管到哪一步

```
选机器（地域/规格/流量/促销陷阱/协议风险）
  → 配 SSH（公钥/别名/主机密钥变更的处理）
  → 装 Xray REALITY（生成密钥 → 渲染配置 → 校验 → 自测 → 安装）
  → 放行端口
  → 发客户端（每台设备独立 uuid + shortId；vless 链接 / 二维码 / 手填字段）
  → 加固（fail2ban + ufw，不动 SSH 登录方式）
  → 验收（服务端回环、端口可达、客户端出口 IP、国内直连延迟）
```

下单和付款不在范围内——读价、算成本、给选项可以，确认购买交回人。

## 用法

### 当作 agent skill 用

把本目录放进 agent 的 skills 目录（或直接让 agent 读 `SKILL.md`），然后说
"帮我搭一台代理 VPS"。agent 会按 `SKILL.md` 的流程问清 4 件事（用途/预算/设备数/支付），
再逐段执行。

### 手动跑

**首选方式是 `vpsctl` 单二进制**（见 `cli/`，支持 Linux/macOS/Windows 当遥控器）：

```bash
curl -fsSL https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | bash
vpsctl deploy --dry-run --clients "laptop,phone"
vpsctl deploy --clients "laptop,phone" && vpsctl doctor
```

下面是等价的脚本流程（没有二进制时用）：

```bash
# 1. 先看不改（生成密钥对 + 三条示例链接，不碰系统）
bash scripts/rebuild.sh --dry-run

# 2. 自测（本机 8443 起临时实例，逐客户端回环验出口 IP，完事清理）
bash scripts/rebuild.sh --selftest

# 3. 真装
sudo bash scripts/rebuild.sh --clients "laptop,phone" --port 443 --sni www.cloudflare.com

# 4. 加设备
./scripts/add_client.sh --host my-vps --email tablet

# 5. 加固
bash scripts/harden_vps.sh my-vps

# 6. 看每个客户端用了多少流量（服务端已默认开统计）
sudo install -m 755 scripts/traffic.py /usr/local/bin/xray-traffic
sudo xray-traffic

# 7. 可选：让账本在重启前自动保存 + 每小时落一次（xray 自身不做持久化）
sudo bash scripts/enable-traffic-accounting.sh
sudo xray-traffic --accumulate      # 跨重启累计
```

## 关键设计

**默认开按客户端统计。** 配置里带 `stats` + `api`（只绑 `127.0.0.1:10085`）+ `policy`，
所以建完就能看出"哪台设备在跑流量"。计数在内存里、重启清零，长期总量仍以 `vnstat -m` 为准；
`traffic.py --save FILE` 可按小时留快照。

**换机现生成密钥，不沿用旧私钥。** 换机必然换 IP，客户端本来就要重新导入；保住私钥只省下改
`pbk` 一项，收益为零，代价是多一个要维护的 secrets 文件。脚本会把所有客户端的新链接一次性打印出来。

**客户端不需要私钥。** 客户端只需要 `publicKey` + `uuid` + `shortId` + `SNI` + `fp`；
私钥只在服务端。私钥不能反推，但反过来可以（`xray x25519 -i`）。

**不装管理面板。** ≤4 台设备时，加设备就是加一条 client；面板是第二个公网入口，不划算。

## 前置

- 一台 Linux VPS（Debian/Ubuntu 系实测；xray 官方支持更多发行版）
- 能 ssh 上去，普通用户 + sudo
- 本机有 `bash` / `python3` / `openssl`（脚本依赖）

## 许可

MIT，见 `LICENSE`。
