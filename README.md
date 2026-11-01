# xrayctl

一个二进制管一台自己的 VPS：**部署 Xray REALITY → 发客户端凭证 → 体检 → 流量账本 → 升级/回滚 → 卸载**。
不做面板、不做机场、不暴露后台入口。

```bash
# Linux / macOS：装到当前用户（免 sudo）
curl -fsSL https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.sh | bash

# Windows PowerShell
irm https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.ps1 | iex
```

**English**: [README.en.md](README.en.md)

> 本地零安装也能用：`bash install.sh --remote my-vps` 只装服务器，之后 `ssh my-vps xrayctl status`。

## 三十秒跑通

```bash
xrayctl --host my-vps deploy --clients laptop,phone   # 装 xray + 生成密钥 + 写配置 + systemd
xrayctl --host my-vps client sync --qr                # 渲染 Clash 配置 + 全部链接 + 手机二维码
xrayctl --host my-vps selftest                        # 每条凭证真连一次，回显出口 IP
```

`client sync` 默认只写文件（`./dist`），加 `--apply` 才会接管 Clash Verge 的当前 profile（先备份）。

**为什么不让用户自己填客户端字段**：REALITY 的那几个字段抄错任何一个，报错都是同一句
`REALITY authentication failed`，和服务端配置错、版本不兼容长得一模一样。这个项目里所有
"需要人手抄"的环节都被命令取代了。

## 命令

```bash
xrayctl [--host <ssh别名> | --local] [--no-sudo] <子命令>
```

| 命令 | 作用 |
|---|---|
| `deploy` | 从零部署：装 xray（固定版本）→ 生成密钥 → 渲染配置 → `xray -test` → systemd 单元 → 起服务 |
| `client sync` | 读服务端现状，生成 Clash profile / 全部 vless 链接 / 手机二维码；`--apply` 写入 Clash Verge |
| `selftest` | 逐条凭证回环真连一次，打印出口 IP |
| `doctor` | 全面体检：二进制、配置、服务、端口、端到端回环 |
| `status` | 服务状态、监听、客户端列表、运行时长 |
| `user list / add / rm` | 每台设备一条独立凭证（独立 uuid + shortId），删一条不影响别人 |
| `traffic` | 按客户端看流量；`--json` 给脚本读；`--enable-accounting` 装每小时账本 |
| `upgrade` / `rollback` | 换 xray 二进制：备份 → 校验 → 替换，失败自动回滚 |
| `checkupgrade` / `versions` | 看有没有新版；打印服务端版本与客户端兼容阶梯 |
| `keys` | 显示公钥，并与 xray 自己算的对照 |
| `harden` | fail2ban + ufw（不动 SSH 登录方式） |
| `proxy install / on / off` | 把这台服务器本身当客户端（默认关，用时才开） |
| `uninstall` | 默认整体卸载（工具 + xray + 配置 + 账本 + 用户）；`--keep-xray` 只卸工具 |

## 平台

| 平台 | 状态 |
|---|---|
| Linux x86_64 / aarch64 | ✅ 真机验证（Ubuntu 24.04 / 26.04） |
| macOS arm64 | ✅ 真机验证（当远端遥控器） |
| macOS x86_64 | ⚠️ 构建通过，未在真机运行验证 |
| Windows x86_64 | ⚠️ 构建通过，`install.ps1` 未在真机验证；遇到问题请开 issue |

`--local`（在本机直接操作 systemd）只支持 Linux；其它平台一律当遥控器用，必须给 `--host`。

## 它管到哪一步

```
选机器（地域/规格/流量/促销陷阱）
  → 配 SSH（公钥/别名/主机密钥变更的处理）
  → 部署（装 xray → 生成密钥 → 渲染 → 校验 → 自测 → systemd）
  → 放行端口（云控制台安全组是另一道墙，工具管不到）
  → 发客户端（每设备独立 uuid + shortId；Clash / vless 链接 / 二维码）
  → 加固（fail2ban + ufw，不动 SSH 登录方式）
  → 验收（回环自测、端口可达、出口 IP、按客户端流量）
```

下单付款不在范围内：读价、算成本、给选项可以，确认购买交回人。

## 关键设计

**默认开按客户端统计。** 配置带 `stats` + `api`（只绑 `127.0.0.1:10085`）+ `policy`，
建完就能看出哪台设备在跑流量。计数在内存里、重启清零，长期总量以 `vnstat -m` 为准；
`traffic --enable-accounting` 会装每小时快照，`--accumulate` 显示跨重启累计。

**私钥只在服务端，客户端只需要 5 个公开字段。** 换机时要么现生成凭证（客户端重导），
要么用 `deploy --from-config` 复用旧配置（客户端只改地址）——两条路都给。

**不装管理面板。** 几台设备就是几条 client；面板是第二个公网入口，不划算。

## Xray 版本策略

默认装**实测可用的固定版本**，不追 `latest`：上游随时可能改握手行为甚至撤版
（有版本上线后被删除，直链直接 404），跟着 latest 跑等于把线上交给运气。

| 服务端版本 | mihomo（默认配置） | mihomo + `support-x25519mlkem768: true` | xray 客户端 |
|---|---|---|---|
| 26.6.27 及更早 | ✅ | ✅ | ✅ |
| 26.10.x | ❌ `REALITY authentication failed` | ✅ | ✅ |

断点在 26.7.11（`minClientVer`）与 26.9.9（强制 X25519MLKEM768）。`client sync` 生成的配置自带
这个 flag；手写配置的人漏掉这一行就会踩坑，而且报错看着像服务端坏了。`xrayctl versions` 直接打印这张表。

```bash
xrayctl --host my-vps upgrade --version 26.6.27   # 钉到某个版本（对老客户端最保险）
xrayctl --host my-vps upgrade --latest            # 追上游，升完必须用真机验证
xrayctl --host my-vps rollback                    # 回到上次升级前的二进制
```

## 卸载

```bash
bash install.sh --uninstall                         # 本地：删二进制 + 缓存
xrayctl --host my-vps uninstall                     # 预演：只打印将执行的动作
xrayctl --host my-vps uninstall --yes               # 整体卸载（含 xray / 配置 / 账本 / 用户）
xrayctl --host my-vps uninstall --keep-xray --yes   # 只卸工具，梯子继续跑
```

## 文档

- [SKILL.md](SKILL.md) — 给 AI agent 的可执行流程（选机器、SSH、部署、发客户端、加固、验收的判据与坑）
- [references/dest-selection.md](references/dest-selection.md) — 落地目标（sni / dest）怎么挑
- [references/client-configs.md](references/client-configs.md) — 各客户端配置与手填字段含义
- [references/client-downloads.md](references/client-downloads.md) — 客户端下载地址

## 许可

MIT，见 [LICENSE](LICENSE)。
