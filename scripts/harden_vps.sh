#!/usr/bin/env bash
# harden_vps.sh — 给自建代理 VPS 装 fail2ban + 开 ufw，**不改 SSH 登录方式**
#
# 用法: bash harden_vps.sh <ssh-host>        # 例: bash harden_vps.sh my-vps
#
# 做什么:
#   0. 打印对外监听端口 + 近 7 天失败登录数（量化攻击面）
#   1. 备份 sshd_config
#   2. 装 fail2ban，写 jail.local（backend=systemd，绕过缺失的 /var/log/auth.log）
#   3. ufw: 默认 deny incoming，只放行实测在用的 22/tcp 与 443/tcp（先 allow 再 enable）
#   4. 用测试 IP 验证封禁真的落进内核，再解封
# 不做（留给用户的决定）: 改 SSH 端口 / 关密码登录 / 关 root 登录
set -euo pipefail

HOST="${1:?用法: bash harden_vps.sh <ssh-host>}"
SSH="ssh -o BatchMode=yes -o ConnectTimeout=10 $HOST"

say() { printf '\n== %s ==\n' "$*"; }

say '0. 免密 sudo 以及攻击面'
$SSH 'sudo -n true' || { echo '✗ 需要免密 sudo；先手工配好再跑本脚本'; exit 1; }
$SSH 'echo "-- 对外监听 --"; sudo ss -tlnp | awk "NR>1 && \$4 !~ /^127\./ {print \$4}"; \
      echo "-- 近 7 天 SSH 失败登录 --"; sudo journalctl -u ssh --since "7 days ago" 2>/dev/null | grep -c "Failed password" || true; \
      echo "-- 现有防护 --"; command -v fail2ban-client >/dev/null && echo "fail2ban: 已装" || echo "fail2ban: 未装"; sudo ufw status | head -1'

say '1-2. 备份 + 装 fail2ban + jail.local'
$SSH 'set -e
sudo cp -n /etc/ssh/sshd_config /etc/ssh/sshd_config.bak-$(date +%Y%m%d-%H%M%S) || true
sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq fail2ban
sudo tee /etc/fail2ban/jail.local >/dev/null <<"EOF"
[DEFAULT]
backend = systemd
bantime = 1h
findtime = 10m
maxretry = 5
bantime.increment = true
bantime.factor = 2
bantime.maxtime = 1w
ignoreip = 127.0.0.1/8 ::1

[sshd]
enabled = true
port = 22
EOF
sudo fail2ban-client -t >/dev/null
sudo systemctl enable --now fail2ban
sleep 1
sudo systemctl is-active fail2ban'

say '3. ufw（先 allow 再 enable，顺序反了会把自己关在外面）'
$SSH 'set -e
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow 22/tcp
sudo ufw allow 443/tcp
sudo ufw --force enable
sudo ufw status verbose'

say '4. 验封禁真进内核（临时 IP 203.0.113.99）'
$SSH 'sudo fail2ban-client set sshd banip 203.0.113.99 >/dev/null
sleep 1
if sudo nft list ruleset 2>/dev/null | grep -q 203.0.113.99; then echo "✓ 封禁已落到 nft"; else echo "⚠ 封禁未见于 nft —— 检查 /etc/fail2ban/action.d 与 ufw 共存"; fi
sudo fail2ban-client set sshd unbanip 203.0.113.99 >/dev/null
sudo fail2ban-client status sshd | tail -3'

say '5. 别忘了从本地验业务没断'
cat <<'TIP'
  curl -s -x http://127.0.0.1:7897 https://ipinfo.io/json   # 出口应仍为该机器 IP
  若 VPS 还跑着别的服务（80 / 其它端口），按第 0 步的实测监听表补 ufw allow
TIP
