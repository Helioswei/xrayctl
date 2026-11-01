#!/usr/bin/env bash
# add_client.sh — 给已有的 REALITY 节点追加一个独立客户端
# 不改 dest、不动私钥、不装面板；每台设备独立 UUID + shortId，可单独删、单独看流量。
#
# 用法:
#   ./add_client.sh --host my-vps --email laptop            # 真加（会重启 xray，现有客户端瞬断 1-2 秒）
#   ./add_client.sh --host my-vps --email test --dry-run    # 只校验配置，不改线上
#   ./add_client.sh --host my-vps --email phone --selfcheck # 只体检已有客户端
#
# 需要：能 ssh 到目标机（密钥登录），目标机上 xray 已装、config.json 在 /usr/local/etc/xray/
set -euo pipefail

HOST=""
EMAIL=""
MODE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --host)      HOST="$2"; shift 2 ;;
    --email)     EMAIL="$2"; shift 2 ;;
    --dry-run)   MODE="--dry-run"; shift ;;
    --selfcheck) MODE="--selfcheck"; shift ;;
    -h|--help)   sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "未知参数: $1"; exit 2 ;;
  esac
done

[ -n "$HOST" ]  || { echo "必须给 --host <ssh别名或user@host>"; exit 2; }
[ -n "$EMAIL" ] || { echo "必须给 --email <名字>"; exit 2; }
DIR="$(cd "$(dirname "$0")" && pwd)"

if [ "$MODE" = "--selfcheck" ]; then
  ssh "$HOST" "sudo python3 - '$EMAIL' --selfcheck" < "$DIR/_server_add.py"
  exit $?
fi

UUID=$(uuidgen | tr 'A-Z' 'a-z' 2>/dev/null || python3 -c 'import uuid;print(uuid.uuid4())')
SHORTID=$(openssl rand -hex 8)

echo "→ 目标: $HOST"
echo "→ 客户端: $EMAIL"
echo "  uuid=$UUID  shortId=$SHORTID"

ssh "$HOST" "sudo python3 - '$UUID' '$SHORTID' '$EMAIL' $MODE" < "$DIR/_server_add.py"
