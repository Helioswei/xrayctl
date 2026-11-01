#!/usr/bin/env bash
# reality-selfcheck.sh — 在 VPS 本机验证 REALITY 是否真的能用
#
# 原理：本机起一个真正的客户端连 127.0.0.1:<port>。能通说明
#   端口监听 / 密钥配对 / shortId / 目标站 全部正确；
#   剩下不通只可能是云防火墙或 GFW。
#
# 用法: bash reality-selfcheck.sh <uuid> <publicKey> <shortId> <sni> [port]
# 依赖: curl, bash；自动下载 mihomo 到 /tmp 并在退出时清理
set -euo pipefail

UUID="${1:?用法: reality-selfcheck.sh <uuid> <publicKey> <shortId> <sni> [port]}"
PUB="${2:?缺 publicKey（xray x25519 输出的 Password 那行）}"
SID="${3:?缺 shortId}"
SNI="${4:?缺 sni（服务端 dest 的域名）}"
PORT="${5:-443}"

case "$(uname -m)" in
  x86_64|amd64) ARCH=amd64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *) echo "不支持的架构: $(uname -m)"; exit 1 ;;
  esac

WORK=/tmp/reality-selfcheck
rm -rf "$WORK"; mkdir -p "$WORK"
BIN="$WORK/mihomo"

URL=$(curl -s --max-time 20 https://api.github.com/repos/MetaCubeX/mihomo/releases/latest \
  | grep -oE "https://[^\"]*mihomo-linux-${ARCH}-compatible-v[^\"]*\\.gz" | head -1)
[ -n "$URL" ] || { echo "取 mihomo 下载地址失败（服务器出网或 API 限流）"; exit 1; }
curl -sL --max-time 120 -o "$WORK/m.gz" "$URL"
gunzip -f "$WORK/m.gz"
SRC=$(find "$WORK" -maxdepth 1 -type f ! -name '*.gz' | head -1)
mv "$SRC" "$BIN"; chmod +x "$BIN"

cat > "$WORK/config.yaml" <<YAML
mixed-port: 7899
allow-lan: false
mode: rule
log-level: warning
proxies:
  - name: selfcheck
    type: vless
    server: 127.0.0.1
    port: ${PORT}
    uuid: ${UUID}
    network: tcp
    udp: true
    tls: true
    flow: xtls-rprx-vision
    servername: ${SNI}
    client-fingerprint: chrome
    reality-opts:
      public-key: ${PUB}
      short-id: ${SID}
proxy-groups:
  - name: PROXY
    type: select
    proxies: [selfcheck]
rules:
  - MATCH,PROXY
YAML

"$BIN" -d "$WORK" -f "$WORK/config.yaml" > "$WORK/run.log" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true; rm -rf "$WORK"' EXIT
sleep 3

CODE=$(curl -s --max-time 15 -x http://127.0.0.1:7899 -o /dev/null -w '%{http_code}' https://www.google.com/ || true)
IP=$(curl -s --max-time 15 -x http://127.0.0.1:7899 https://ipinfo.io/ip 2>/dev/null || true)

if [ "$CODE" = "200" ] && [ -n "$IP" ]; then
  echo "OK  ready=200  出口IP=$IP"
  echo "   服务端配置正确；客户端仍不通就查云防火墙端口规则或 GFW。"
  exit 0
fi

echo "FAIL ready=${CODE:-none} 出口IP=${IP:-none}"
echo "--- 服务端日志 ---"
journalctl -u xray -n 20 --no-pager 2>/dev/null | tail -12 || true
echo "--- mihomo 日志 ---"
tail -8 "$WORK/run.log" 2>/dev/null || true
echo "提示：'handshake did not complete successfully' 多为 dest/密钥/shortId 不匹配（先换 dest 验证）；"
echo "      'authentication failed or validation criteria not met' 多为密钥或 shortId 不对。"
exit 1
