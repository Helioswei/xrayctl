# 客户端配置：三种形态，同一套凭证

服务端只有一个真实来源：`/usr/local/etc/xray/config.json`（含 `privateKey`、`clients`、`shortIds`）。
客户端要的东西全部从它派生：

| 服务端字段 | 客户端字段（叫法随客户端变） | 说明 |
|---|---|---|
| `privateKey` | —— 客户端**不需要** | 只有服务端持有 |
| （由私钥反推） | `publicKey`（mihomo）/ `password`（xray-core 25+）/ `pbk`（vless 链接） | 同一个值，三种叫法 |
| `clients[].id` | `uuid`（mihomo）/ `id`（xray-core）/ 链接里的 userinfo | 每台设备一条，可单独删 |
| `shortIds[]` | `short-id` / `shortId` / `sid` | 按 client 下标取，只需命中列表中的一个 |
| `serverNames[0]` | `servername` / `serverName` / `sni` | 与 `dest` 的域名一致 |
| 端口 | `port` | 通常 443 |

公钥反推（服务端不存公钥）：

```bash
sudo xray x25519 -i $(jq -r '.inbounds[0].streamSettings.realitySettings.privateKey' /usr/local/etc/xray/config.json)
# 输出里的 Password (PublicKey) 那行就是 pbk
```

## 一、mihomo / Clash（桌面、手机 Clash 系）

```yaml
proxies:
  - name: "my-vps"
    type: vless
    server: <VPS_IP>
    port: 443
    uuid: <UUID>
    network: tcp
    udp: true
    tls: true
    flow: xtls-rprx-vision
    servername: <SNI>
    client-fingerprint: chrome
    reality-opts:
      public-key: <PBK>
      short-id: <SHORTID>

proxy-groups:
  - name: PROXY
    type: select
    proxies: ["my-vps", DIRECT]

rules:
  - GEOSITE,cn,DIRECT
  - GEOIP,CN,DIRECT
  - MATCH,PROXY
```

导入（Clash Verge Rev）：订阅 → 新建 → 类型 Local → 粘贴整段 yaml → 保存 → 启用。
国内域名靠 `GEOSITE,cn` 直连；策略组里留一个 `DIRECT` 当"临时关梯子"的手动开关。

验证别只看界面：

```bash
curl -s -x http://127.0.0.1:7897 https://ipinfo.io/json   # 出口城市应是机房所在地
curl -s -o /dev/null -x http://127.0.0.1:7897 -w '%{time_total}\n' https://www.baidu.com/  # 0.1–0.3s = 直连规则生效
```

> 改盘上的 profile 文件不会自动生效：Verge 只在启动/应用时把 profile 渲染成运行时配置。
> 服务端换了 `dest`/`SNI` 后，客户端仍会发旧 SNI，症状是"服务端好的、客户端全断"。

## 二、xray-core（命令行、服务器当客户端、路由器）

```json
{
  "log": { "loglevel": "warning" },
  "inbounds": [
    { "tag": "socks-in", "listen": "127.0.0.1", "port": 1080,
      "protocol": "socks", "settings": { "udp": false, "auth": "noauth" } },
    { "tag": "http-in", "listen": "127.0.0.1", "port": 8118,
      "protocol": "http", "settings": {} }
  ],
  "outbounds": [{
    "tag": "vps",
    "protocol": "vless",
    "settings": { "vnext": [{
      "address": "<VPS_IP>", "port": 443,
      "users": [{ "id": "<UUID>", "encryption": "none", "flow": "xtls-rprx-vision" }]
    }]},
    "streamSettings": {
      "network": "tcp", "security": "reality",
      "realitySettings": {
        "fingerprint": "chrome",
        "serverName": "<SNI>",
        "publicKey": "<PBK>",
        "shortId": "<SHORTID>",
        "spiderX": "/"
      }
    }
  }]
}
```

```bash
xray run -test -c client.json          # 先校验
xray run -c client.json &              # 起一个本地 socks1080 + http8118
curl -x socks5h://127.0.0.1:1080 https://ipinfo.io/ip   # 出口应是 VPS IP
```

> **25+ 版本字段改名**：客户端写 `publicKey` 会被接受，但官方输出的键名已是 `password`；
> 两者同义。mihomo 里仍叫 `reality-opts.public-key`，vless 链接里叫 `pbk`。

要"平时关、用时开"，用 `scripts/proxy-toggle.sh` 包成 systemd 服务 + `proxy on/off`。

## 三、vless 链接 / 二维码（手机）

一行搞定，用自己的配置生成（不要在别处抄）：

```bash
# 在服务端跑：读取 config.json，反推公钥，为每个 client 打印链接
bash scripts/make_vless_share_link.sh /usr/local/etc/xray/config.json 443

# 二维码（可选，需 qrencode；Python 亦可：pip install segno）
```

链接里的字段含义（手填时最容易错的四项）：

```
vless://<UUID>@<IP>:443
  ?encryption=none          # 必须 none
  &security=reality         # 拼写就是 reality
  &sni=<serverNames[0]>     # 与服务端 dest 域名一致
  &fp=chrome                # 指纹
  &pbk=<公钥>               # 私钥反推出来的那个
  &sid=<shortId>            # 必须命中服务端 shortIds 列表
  &type=tcp
  &flow=xtls-rprx-vision    # 与服务端 client 的 flow 一致
  #备注名
```

手机端：v2rayNG / Hiddify / NekoBox（Android）、sing-box / Shadowrocket（iOS）。
导入方式：复制链接 → 客户端"从剪贴板导入"，或扫二维码。

## 每台设备一条独立 client

```bash
./scripts/add_client.sh --host my-vps --email tablet --dry-run   # 先校验，不改线上
./scripts/add_client.sh --host my-vps --email tablet             # 真加（重启 xray，瞬断 1–2 秒）
./scripts/add_client.sh --host my-vps --email tablet --selfcheck # 体检这条凭证
```

好处：某台设备泄露/不用了，删掉那一条即可，其他人不受影响；也便于按 `email` 看流量。

## 出错时先查这四处

| 症状 | 最可能的原因 |
|---|---|
| 连不上、客户端没报错 | `pbk` / `sid` / `sni` 有任一项与服务端不一致 |
| 连上但打不开网页 | `sni` 与服务端 `dest` 域名不一致（走回落路径），或 `flow` 不匹配 |
| 客户端能连、服务端日志握手失败 | 服务端 `dest` 站点不可用（换 `www.cloudflare.com`） |
| 只有某台设备不行 | 那条 client 的 uuid 被删/重复，或 shortId 不在列表里 |
