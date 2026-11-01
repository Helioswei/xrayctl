# 伪装目标（dest）怎么挑

REALITY 的 `dest` 是"你的服务器在没有正确凭证时，会把流量转发去哪"。它决定这台的连接看起来像在访问谁。
选错的表现不是报错，而是**握手卡住**或**连上但没网**，很难排查，所以要按判据挑。

## 判据

| 项 | 要求 |
|---|---|
| 协议 | TLS 1.3 + ALPN 含 `h2` |
| 证书 | 有效、剩余 > 90 天 |
| 国内可达 | 从国内能正常访问（否则 SNI 过滤会盯上） |
| 位置 | 离你的 VPS 近（同地域 / 同 ASN / 邻段最佳） |
| 站型 | 无需登录的静态/大站；非 301/302 跳转站；不套 Cloudflare/Fastly |
| 避开 | 金融、需登录、敏感站 |

## 实测结论（2026-10，腾讯云东京，Xray 26.3）

| dest | 结果 |
|---|---|
| `www.cloudflare.com:443` | ✅ 首选，`xray run -test` 无告警 |
| `www.apple.com` | ⚠️ 能通，但 xray 告警"可能被 GFW 封 IP" |
| `dl.google.com` | ⚠️ 技术可用，域名国内被墙，SNI 过滤会盯上 |
| `www.microsoft.com` | ❌ 握手卡在发证书，日志 `handshake did not complete successfully` |

**改 dest 必须同步改客户端 `servername`**，否则客户端走回落路径，症状是"突然连不上"。

## 验证候选站

```bash
echo | openssl s_client -connect www.example.com:443 -servername www.example.com \
  -alpn h2,http/1.1 -tls1_3 2>/dev/null | grep -E 'Protocol|ALPN|subject='
```

> `openssl` **不读 `http_proxy`**：在国内直连验境外站必超时，会被误判成"不支持 TLS1.3"。
> 要显式指定代理：`openssl s_client -proxy 127.0.0.1:7897 -connect <IP>:443 -servername <域名> -alpn h2,http/1.1 -tls1_3`

## 找候选：从 VPS 的邻居网段找

目标是"让流量看起来最像本地"，所以优先挑和 VPS 同一张网里的站：

1. bgp.he.net 输入 **VPS 的 IP** → 记下 ASN 与所在 Prefix
2. 点进 ASN → Prefixes → 挑同 /24 或邻居网段的 IP
3. 对这些 IP 反查托管域名（Reverse IP / Shodan）
4. 逐个按上面判据验：`openssl` 检查 + `curl -s https://<域名>/cdn-cgi/trace` 确认没套 CDN
5. 合格再写进 `dest` + `serverNames`，并留 1–2 个备用目标

## 被探测的风险与缓解

社区有一种探测法：用"你的 VPS IP + 你的 SNI"访问目标站，再看目标站登录日志里的来源 IP——
出现 VPS IP 就说明这个站不是你的。缓解三条：

1. 选**无需登录**的大站/静态站
2. **防火墙阻断节点向目标站的探测回源**
3. 定期换目标站
