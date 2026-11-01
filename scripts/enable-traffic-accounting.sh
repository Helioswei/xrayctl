#!/usr/bin/env bash
# enable-traffic-accounting.sh — 让「按客户端流量账本」在 xray 重启时自动保存，并每小时落一次
#
# 为什么需要外部做：xray 的统计在内存里，且 API 只提供查询（没有写入/恢复接口），
# 所以做不到"重启后自己接上"。这里用两个触发点拼出"近似自动"：
#   1) systemd ExecStop：停服/xray 重启前抓最后一份（那时 API 还活着）
#   2) systemd timer：每小时抓一次，防崩溃/断电丢太多
# 增量由 traffic.py --accumulate 自己算，跨重启累加进 state 文件。
#
# 用法： sudo bash enable-traffic-accounting.sh [/usr/local/bin/xray-traffic]
set -euo pipefail

BIN="${1:-/usr/local/bin/xray-traffic}"
[ -x "$BIN" ] || { echo "找不到可执行文件：$BIN（先 sudo install -m755 scripts/traffic.py $BIN）"; exit 1; }
[ "$(id -u)" -eq 0 ] || { echo "需要 root（sudo）"; exit 1; }

DROPIN=/etc/systemd/system/xray.service.d
mkdir -p "$DROPIN"

cat > "$DROPIN/traffic-accounting.conf" <<EOF
[Service]
# 停服前抓最后一份（此时 xray 还在跑、API 可查）。前缀 - 表示失败也不阻塞停服。
ExecStop=-$BIN --accumulate --quiet
EOF

cat > /etc/systemd/system/xray-traffic.service <<EOF
[Unit]
Description=xray 客户端流量账本快照
After=xray.service

[Service]
Type=oneshot
ExecStart=$BIN --accumulate --quiet
EOF

cat > /etc/systemd/system/xray-traffic.timer <<'EOF'
[Unit]
Description=每小时把 xray 客户端流量记进账本

[Timer]
OnCalendar=hourly
AccuracySec=1min
Persistent=true

[Install]
WantedBy=timers.target
EOF

systemctl daemon-reload
systemctl enable --now xray-traffic.timer >/dev/null
systemctl start xray-traffic.service || true

echo "已启用："
echo "  · xray 停止/重启前自动记账（ExecStop）"
echo "  · 每小时落一次（xray-traffic.timer）"
echo
echo "看当前累计： sudo $BIN --accumulate"
echo "看本次启动以来： sudo $BIN"
