#!/usr/bin/env bash
# 从 Xray REALITY 配置生成每个客户端的 vless:// 分享链接 + 终端二维码（手机扫码导入）
#
# 用法： bash make_vless_share_link.sh [config.json] [端口]
#   默认： /usr/local/etc/xray/config.json  443
# 依赖： jq；二维码可选 qrencode（apt install -y qrencode）
#
# 设计要点：
#   · 所有变量写成 ${VAR} —— macOS bash 3.2 在 C locale 下会把紧跟的中文首字节算进变量名
#   · 公钥多数配置里不存在，用 xray x25519 -i <privateKey> 反推
#   · shortIds 按 client 下标取，人数多于 shortId 条数时回落到第一条
set -euo pipefail

CFG="${1:-/usr/local/etc/xray/config.json}"
PORT="${2:-443}"

command -v jq >/dev/null 2>&1 || { echo "需要 jq"; exit 1; }
[ -f "${CFG}" ] || { echo "找不到配置：${CFG}"; exit 1; }

IP=$(curl -4 -s --max-time 8 https://ifconfig.me || curl -4 -s --max-time 8 https://api.ipify.org || true)
[ -n "${IP}" ] || { echo "取不到公网 IP，手动传入或检查网络"; exit 1; }

SNI=$(jq -r '.inbounds[0].streamSettings.realitySettings.serverNames[0]' "${CFG}")
PRIV=$(jq -r '.inbounds[0].streamSettings.realitySettings.privateKey' "${CFG}")

# 公钥：先看配置里有没有（少数模板会写），没有再反推
PBK=$(jq -r '.inbounds[0].streamSettings.realitySettings.settings.publicKey // empty' "${CFG}" 2>/dev/null || true)
if [ -z "${PBK}" ]; then
  PBK=$(xray x25519 -i "${PRIV}" 2>/dev/null | awk '/PublicKey|Password/{print $NF; exit}' || true)
fi
[ -n "${PBK}" ] || { echo "取不到公钥：确认 privateKey 正确，或手动跑 xray x25519 -i <私钥>"; exit 1; }

SID_COUNT=$(jq -r '.inbounds[0].streamSettings.realitySettings.shortIds | length' "${CFG}")

IDX=0
while IFS=$'\t' read -r UUID NAME; do
  [ -n "${UUID}" ] || continue
  [ -n "${NAME}" ] || NAME="client${IDX}"
  SID=$(jq -r --argjson i "${IDX}" --argjson n "${SID_COUNT}" \
        '.inbounds[0].streamSettings.realitySettings.shortIds[($i % $n)]' "${CFG}")
  LINK="vless://${UUID}@${IP}:${PORT}?encryption=none&security=reality&sni=${SNI}&fp=chrome&pbk=${PBK}&sid=${SID}&type=tcp&flow=xtls-rprx-vision#${NAME}"
  echo "── ${NAME}"
  echo "${LINK}"
  if command -v qrencode >/dev/null 2>&1; then
    qrencode -t ANSIUTF8 "${LINK}"
  else
    echo "（装 qrencode 可显示二维码：apt install -y qrencode）"
  fi
  echo
  IDX=$((IDX + 1))
done < <(jq -r '.inbounds[0].settings.clients[] | [(.id // ""), (.email // "")] | @tsv' "${CFG}")

echo "共 ${IDX} 个客户端。手机端选 sing-box / v2rayNG / Shadowrocket 扫码导入即可。"
