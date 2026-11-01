#!/usr/bin/env bash
# rebuild.sh — 在一台新的 Ubuntu VPS 上从零重建这台 REALITY 节点
#
# 用法：
#   sudo bash scripts/rebuild.sh                 # 真装：现生成密钥对 → 写配置 → 起服务 → 打印全部客户端链接
#   sudo bash scripts/rebuild.sh --dry-run       # 只生成并校验，不写系统、不重启（看新凭证长什么样）
#   sudo bash scripts/rebuild.sh --selftest      # 在本机 8443 起一个临时实例，逐客户端回环实测后清理
#   sudo bash scripts/rebuild.sh --clients "laptop,phone" --port 443 --sni www.cloudflare.com
#
# 设计：**不保留旧私钥**。换机必然换 IP，客户端本来就要重新导入；
#       现生成密钥对省掉一个要维护的 secrets 文件，仓库里也不再需要私钥。
#       唯一代价：所有客户端要导入新链接（脚本最后一次性打印出来）。
set -euo pipefail

MODE="apply"
PORT=443
SNI="www.cloudflare.com"
DEST="www.cloudflare.com:443"
CLIENTS="laptop,phone"
CFG=/usr/local/etc/xray/config.json
IP=""

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run)  MODE="dry-run"; shift ;;
    --selftest) MODE="selftest"; shift ;;
    --clients)  CLIENTS="$2"; shift 2 ;;
    --port)     PORT="$2"; shift 2 ;;
    --sni)      SNI="$2"; DEST="$2:443"; shift 2 ;;
    --dest)     DEST="$2"; shift 2 ;;
    --ip)       IP="$2"; shift 2 ;;
    -h|--help)  sed -n '2,14p' "$0"; exit 0 ;;
    *) echo "未知参数: $1"; exit 2 ;;
  esac
done

if [ "$(id -u)" -eq 0 ]; then SUDO=""; else SUDO="sudo"; fi
command -v python3 >/dev/null || { echo "缺 python3"; exit 1; }

# ---------- 1. 确保有 xray 二进制 ----------
if [ ! -x /usr/local/bin/xray ]; then
  echo "== 安装 Xray"
  if curl -fsSL --max-time 60 https://github.com/XTLS/Xray-install/raw/main/install-release.sh -o /tmp/xray-install.sh; then
    $SUDO bash /tmp/xray-install.sh install
  else
    echo "!! 拉不到安装脚本（GitHub 不可达）。手动把二进制放到 /usr/local/bin/xray 后重跑，"
    echo "   例如在能上网的机器上: ssh 旧机 'sudo cat /usr/local/bin/xray' | ssh 新机 'cat > /tmp/x && sudo install -m755 /tmp/x /usr/local/bin/xray'"
    exit 1
  fi
else
  echo "== Xray 已存在: $(/usr/local/bin/xray version | head -1)"
fi

WORK=$(mktemp -d /tmp/rebuild.XXXXXX)
chmod 700 "$WORK"
trap 'rm -rf "$WORK"' EXIT

# ---------- 2. 现生成密钥对 + 每个客户端的独立凭据 ----------
echo "== 生成密钥对与客户端凭据"
GEN=$(/usr/local/bin/xray x25519)
PRIV=$(printf '%s\n' "$GEN" | awk -F': *' '/[Pp]rivate/{print $2; exit}' | tr -d '\r')
PUB=$(printf '%s\n' "$GEN" | awk -F': *' '/(Password|PublicKey)/{print $2; exit}' | tr -d '\r')
[ -n "$PRIV" ] && [ -n "$PUB" ] || { echo "!! x25519 输出解析失败"; printf '%s\n' "$GEN"; exit 1; }

CLIENTS_ENV="$CLIENTS" PUB_ENV="$PUB" PRIV_ENV="$PRIV" PORT_ENV="$PORT" \
DEST_ENV="$DEST" SNI_ENV="$SNI" WORK_ENV="$WORK" \
python3 - <<'PY'
import json, os, secrets, sys, uuid

work   = os.environ["WORK_ENV"]
pub    = os.environ["PUB_ENV"]
priv   = os.environ["PRIV_ENV"]
port   = int(os.environ["PORT_ENV"])
dest   = os.environ["DEST_ENV"]
sni    = os.environ["SNI_ENV"]
emails = [e.strip() for e in os.environ["CLIENTS_ENV"].split(",") if e.strip()]

clients, short_ids, rows = [], [], []
for e in emails:
    uid = str(uuid.uuid4())
    sid = secrets.token_hex(8)
    clients.append({"id": uid, "flow": "xtls-rprx-vision", "email": e})
    short_ids.append(sid)
    rows.append((e, uid, sid))

cfg = {
    "log": {"loglevel": "warning"},
    # 按客户端统计流量：stats 开表、api 暴露查询端口、policy 才真正按 user 记数
    "stats": {},
    "api": {"tag": "api", "services": ["HandlerService", "StatsService"]},
    "policy": {"levels": {"0": {"statsUserUplink": True, "statsUserDownlink": True}}},
    "inbounds": [{
        "tag": "vless-reality",
        "listen": "0.0.0.0",
        "port": port,
        "protocol": "vless",
        "settings": {"clients": clients, "decryption": "none"},
        "streamSettings": {
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "show": False,
                "dest": dest,
                "xver": 0,
                "serverNames": [sni],
                "privateKey": priv,
                "shortIds": short_ids,
            },
        },
        "sniffing": {"enabled": True, "destOverride": ["http", "tls", "quic"]},
    }, {
        # api 只绑回环，云安全组不要放行
        "tag": "api",
        "listen": "127.0.0.1",
        "port": 10085,
        "protocol": "dokodemo-door",
        "settings": {"address": "127.0.0.1"},
    }],
    "outbounds": [
        {"protocol": "freedom", "tag": "direct"},
        {"protocol": "blackhole", "tag": "block"},
    ],
    "routing": {"rules": [
        {"type": "field", "protocol": ["bittorrent"], "outboundTag": "block"},
        {"type": "field", "inboundTag": ["api"], "outboundTag": "api"},
    ]},
}

with open(os.path.join(work, "config.json"), "w") as f:
    json.dump(cfg, f, indent=2, ensure_ascii=False)
os.chmod(os.path.join(work, "config.json"), 0o600)
with open(os.path.join(work, "clients.tsv"), "w") as f:
    for e, uid, sid in rows:
        f.write("%s\t%s\t%s\n" % (e, uid, sid))
print("   clients=%d shortIds=%d" % (len(clients), len(short_ids)))
PY

echo "== 校验生成的配置"
/usr/local/bin/xray run -test -c "$WORK/config.json" 2>&1 | tail -2

# ---------- 3. 渲染客户端链接 ----------
render_links() {
  local ip="$1"
  while IFS=$'\t' read -r email uid sid; do
    printf '  %-16s %s\n' "$email" \
      "vless://${uid}@${ip}:${PORT}?encryption=none&security=reality&sni=${SNI}&fp=chrome&pbk=${PUB}&sid=${sid}&type=tcp&flow=xtls-rprx-vision#${email}"
  done < "$WORK/clients.tsv"
}

if [ "$MODE" = "dry-run" ]; then
  echo
  echo "== [dry-run] 未写入系统、未重启服务"
  echo "   公钥 $PUB"
  echo "   配置 $WORK/config.json（退出后自动删除）"
  echo "   链接（IP 用花括号占位，真跑时会填公网 IP）："
  render_links "{NEW_IP}"
  exit 0
fi

# ---------- 4. selftest：临时实例 + 逐客户端回环 ----------
if [ "$MODE" = "selftest" ]; then
  TPORT=8443
  DEST_ENV="$DEST" SNI_ENV="$SNI" WORK_ENV="$WORK" PORT_ENV="$TPORT" PUB_ENV="$PUB" \
  python3 - <<'PY'
import json, os
work = os.environ["WORK_ENV"]
cfg = json.load(open(os.path.join(work, "config.json")))
cfg["inbounds"][0]["port"] = int(os.environ["PORT_ENV"])
cfg["inbounds"][0]["listen"] = "127.0.0.1"   # 临时实例只绑回环，不对外暴露
# 统计用的 api inbound 要摘掉：正式 xray 已经占了 10085，临时实例再绑会端口冲突起不来
cfg["inbounds"] = [ib for ib in cfg["inbounds"] if ib.get("tag") != "api"]
cfg.pop("api", None)
cfg.pop("stats", None)
cfg.pop("policy", None)
if "routing" in cfg:
    cfg["routing"]["rules"] = [r for r in cfg["routing"].get("rules", [])
                               if r.get("outboundTag") != "api"]
json.dump(cfg, open(os.path.join(work, "selftest-server.json"), "w"), indent=2)
# 每个客户端一条临时 socks，指向 127.0.0.1:临时端口
for i, line in enumerate(open(os.path.join(work, "clients.tsv"))):
    email, uid, sid = line.rstrip("\n").split("\t")
    c = {
        "log": {"loglevel": "warning"},
        "inbounds": [{"listen": "127.0.0.1", "port": 10900 + i, "protocol": "socks",
                      "settings": {"udp": False, "auth": "noauth"}}],
        "outbounds": [{
            "protocol": "vless",
            "settings": {"vnext": [{"address": "127.0.0.1", "port": int(os.environ["PORT_ENV"]),
                         "users": [{"id": uid, "encryption": "none", "flow": "xtls-rprx-vision"}]}]},
            "streamSettings": {"network": "tcp", "security": "reality", "realitySettings": {
                "fingerprint": "chrome", "serverName": os.environ["SNI_ENV"],
                "publicKey": os.environ["PUB_ENV"], "password": os.environ["PUB_ENV"],
                "shortId": sid, "spiderX": "/"}},
        }],
    }
    json.dump(c, open(os.path.join(work, "cli-%d.json" % i), "w"), indent=2)
PY

  echo "== [selftest] 在 127.0.0.1:$TPORT 起临时服务端"
  /usr/local/bin/xray run -c "$WORK/selftest-server.json" >/dev/null 2>&1 &
  SRV=$!
  sleep 2
  FAIL=0
  i=0
  while IFS=$'\t' read -r email uid sid; do
    /usr/local/bin/xray run -c "$WORK/cli-$i.json" >/dev/null 2>&1 &
    CLI=$!
    sleep 1
    OUT=$(curl -s --max-time 8 -x "socks5h://127.0.0.1:$((10900 + i))" https://ipinfo.io/ip || true)
    if [ -n "$OUT" ]; then echo "  ✓ $email 出口 $OUT"; else echo "  ✗ $email 不通"; FAIL=1; fi
    kill "$CLI" 2>/dev/null || true
    i=$((i + 1))
  done < "$WORK/clients.tsv"
  kill "$SRV" 2>/dev/null || true
  echo "   临时实例已清理（未动系统服务）"
  [ "$FAIL" -eq 0 ] && echo "== selftest 全部通过" || { echo "== selftest 有失败"; exit 1; }
  exit 0
fi

# ---------- 5. 正式套用 ----------
echo "== 写入配置（属主必须是 xray，否则服务 permission denied 而 -test 却通过）"
$SUDO install -d -m 755 /usr/local/etc/xray
$SUDO install -m 600 -o xray -g xray "$WORK/config.json" "$CFG"

echo "== BBR"
printf 'net.core.default_qdisc=fq\nnet.ipv4.tcp_congestion_control=bbr\n' | $SUDO tee /etc/sysctl.d/99-bbr.conf >/dev/null
$SUDO modprobe tcp_bbr 2>/dev/null || true
$SUDO sysctl --system >/dev/null 2>&1 || true
echo "   $(sysctl -n net.ipv4.tcp_congestion_control) / $(sysctl -n net.core.default_qdisc)"

echo "== vnstat"
if ! command -v vnstat >/dev/null 2>&1; then
  $SUDO DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1 || true
  $SUDO DEBIAN_FRONTEND=noninteractive apt-get install -y -qq vnstat >/dev/null 2>&1 || true
fi
$SUDO systemctl enable --now vnstat >/dev/null 2>&1 || true
echo "   vnstat: $(systemctl is-active vnstat 2>/dev/null || echo unknown)"

echo "== 启动 Xray"
$SUDO systemctl enable xray >/dev/null 2>&1 || true
$SUDO systemctl restart xray
sleep 2
echo "   状态: $(systemctl is-active xray)"
$SUDO ss -lntp | grep -q ":${PORT}" || { echo "   !! ${PORT} 没在监听，看 journalctl -u xray -n 50"; exit 1; }

# ---------- 6. 打印全部客户端链接 ----------
[ -n "$IP" ] || IP=$(curl -s --max-time 8 https://ipinfo.io/ip 2>/dev/null || true)
[ -n "$IP" ] || IP="(取不到公网IP，手动填)"
echo
echo "== 客户端链接（一次性发给所有设备，漏掉哪台就断哪台）"
render_links "$IP"
echo
echo "   公钥 $PUB"
echo "   伪装目标 $DEST    SNI $SNI    端口 $PORT"
echo "   私钥只在 $CFG（属主 xray 600），不进仓库"
echo
echo "还剩两件手动事："
echo "  1) 云控制台防火墙放行 $PORT（TCP / 0.0.0.0/0）"
echo "  2) 每台客户端导入上面自己的链接（Mac 用写 yaml 或 Verge 导入，手机 v2rayNG 从剪贴板导入）"
