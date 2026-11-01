#!/usr/bin/env bash
# proxy-toggle.sh — 把"本机当客户端"的 xray 做成可手动开关的代理（默认关闭，用时才开）
#
# 场景：一台服务器要访问境外服务（GitHub / pypi / npm），装个 xray 客户端连自建节点，
#       只监听 127.0.0.1，平时停着，需要时开——比全局代理干净：apt 和国内站不受影响。
#
# 安装：
#   sudo install -m 755 proxy-toggle.sh /usr/local/bin/proxy
#   再把下面的 shell 函数追加到 ~/.bashrc（否则 proxy on 改不了当前 shell 的环境变量）
#
# 用法：
#   proxy status      看状态（服务是否在跑 + 出口 IP 是否走了节点）
#   proxy on          开（函数版还会 export 到当前 shell，之后直接 hermes update / git pull）
#   proxy off         关，并确保重启后不自启
#   proxy env         打印 export 行，用来 eval
#   proxy run <cmd>   只给这一条命令挂代理
#
# shell 函数（追加到 ~/.bashrc）：
#   proxy() {
#     case "$1" in
#       on)  command /usr/local/bin/proxy on || return 1
#            eval "$(command /usr/local/bin/proxy env)"
#            echo "当前 shell 已挂代理" ;;
#       off) command /usr/local/bin/proxy off
#            unset http_proxy https_proxy all_proxy no_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY ;;
#       *)   command /usr/local/bin/proxy "$@" ;;
#     esac
#   }
set -uo pipefail

SVC="${PROXY_SVC:-xray-client}"          # systemd 服务名
SOCKS="${PROXY_SOCKS:-127.0.0.1:1080}"
HTTP="${PROXY_HTTP:-127.0.0.1:8118}"
NO_PROXY_LIST="${PROXY_NO_PROXY:-localhost,127.0.0.1,::1}"

svc_active() { systemctl is-active --quiet "$SVC"; }
exit_ip() { curl -s --max-time 8 -x "socks5h://$SOCKS" https://ipinfo.io/ip 2>/dev/null | tr -d '\n'; }

cmd_status() {
  if svc_active; then
    echo "服务: 运行中 ($SVC active)"
    ip=$(exit_ip)
    if [ -n "$ip" ]; then echo "出口: $ip ← 走节点"
    else echo "出口: 取不到（服务在跑但连不通，查 sudo journalctl -u $SVC -n 20）"; fi
  else
    echo "服务: 已关闭 ($SVC inactive) —— 直连"
    echo "本机出口: $(curl -s --max-time 6 https://ipinfo.io/ip 2>/dev/null || echo 取不到)"
  fi
  echo "自启: $(en=$(systemctl is-enabled "$SVC" 2>/dev/null); echo "${en:-unknown}")"
  echo "端口: socks5 $SOCKS / http $HTTP（仅回环）"
  [ -n "${https_proxy:-}" ] && echo "当前 shell: https_proxy=$https_proxy" || echo "当前 shell: 未设代理变量"
}

cmd_on() {
  sudo systemctl start "$SVC" || { echo "启动失败"; exit 1; }
  sleep 1
  ip=$(exit_ip)
  if [ -n "$ip" ]; then echo "已开启，出口: $ip"
  else echo "服务已起但连不通，检查: sudo journalctl -u $SVC -n 20"; exit 1; fi
  echo
  echo "用法二选一："
  echo "  eval \"\$(proxy env)\"       # 当前 shell 生效"
  echo "  proxy run <命令>            # 只给这一条命令挂"
}

cmd_off() {
  sudo systemctl stop "$SVC" 2>/dev/null
  sudo systemctl disable "$SVC" 2>/dev/null
  echo "已关闭，且重启后不会自启"
}

cmd_env() {
  cat <<EOF
export http_proxy=http://$HTTP
export https_proxy=http://$HTTP
export all_proxy=socks5h://$SOCKS
export no_proxy=$NO_PROXY_LIST
EOF
}

cmd_run() {
  [ $# -gt 0 ] || { echo "用法: proxy run <命令...>"; exit 2; }
  svc_active || { echo "代理没开，先 proxy on"; exit 1; }
  env http_proxy="http://$HTTP" https_proxy="http://$HTTP" all_proxy="socks5h://$SOCKS" \
      no_proxy="$NO_PROXY_LIST" "$@"
}

case "${1:-status}" in
  status|st) cmd_status ;;
  on)        cmd_on ;;
  off)       cmd_off ;;
  env)       cmd_env ;;
  run)       shift; cmd_run "$@" ;;
  *) sed -n '2,20p' "$0" ;;
esac
