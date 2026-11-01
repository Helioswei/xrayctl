#!/usr/bin/env bash
# install.sh — 下载 xrayctl 并校验 sha256（不校验就不装）
#
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.sh | bash
#   bash install.sh --version vX.Y.Z --prefix ~/.local/bin
#   bash install.sh --remote my-vps        # 直接装到服务器（本地零安装，之后 ssh my-vps xrayctl ...）
#   bash install.sh --uninstall            # 卸掉本地的 xrayctl（二进制 + 缓存）
#   bash install.sh --remote my-vps --uninstall   # 卸掉服务器上的（转给 xrayctl uninstall）
#
# 支持：linux x86_64/aarch64、macOS（Intel/Apple Silicon）；Windows 用 Releases 里的 .exe
set -euo pipefail

REPO="${XRAYCTL_REPO:-Helioswei/xrayctl}"
VERSION="${XRAYCTL_VERSION:-latest}"
PREFIX="${PREFIX:-}"        # 不给就自动选：/usr/local/bin 可写用它，否则 ~/.local/bin（免 sudo）
BASE="${XRAYCTL_BASE:-}"   # 自建镜像/CDN 的 releases 根，如 https://cdn.example.com/xrayctl
REMOTE=""
UNINSTALL=""

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --prefix)  PREFIX="$2";  shift 2 ;;
    --base)    BASE="$2";    shift 2 ;;
    --repo)    REPO="$2";    shift 2 ;;
    --remote)  REMOTE="$2";   shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "未知参数 $1"; exit 2 ;;
  esac
done

# 卸载：本地删二进制 + 缓存；远端转给 xrayctl uninstall
if [ -n "${UNINSTALL:-}" ]; then
  if [ -n "$REMOTE" ]; then
    echo "在 $REMOTE 上卸载 xrayctl..."
    ssh -o BatchMode=yes "$REMOTE" 'sudo /usr/local/bin/xrayctl uninstall --yes 2>/dev/null || /usr/local/bin/xrayctl uninstall --yes' || {
      echo "远端没有 xrayctl，或需要 sudo。手动：ssh $REMOTE 'sudo xrayctl uninstall --yes'"; exit 1; }
  fi
  for f in "$HOME/.local/bin/xrayctl" "/usr/local/bin/xrayctl"; do
    if [ -f "$f" ]; then
      if [ -w "$f" ]; then rm -f "$f"; else sudo rm -f "$f"; fi
      echo "已删除 $f"
    fi
  done
  rm -rf "$HOME/.cache/xrayctl"
  echo "已清理缓存 ~/.cache/xrayctl"
  echo "（xray 服务与配置不受影响；要连它一起清：xrayctl uninstall --purge --yes）"
  exit 0
fi

# --remote：装到远端服务器（只能是 Linux），本地什么都不用装
if [ -n "$REMOTE" ]; then
  os=linux
  arch=$(ssh -o BatchMode=yes -o ConnectTimeout=10 "$REMOTE" 'uname -m')
else
  os=$(uname -s | tr '[:upper:]' '[:lower:]')
  arch=$(uname -m)
fi
case "$arch" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) echo "不支持的架构：$arch"; exit 1 ;;
esac

case "$os" in
  linux)  asset="xrayctl-linux-${arch}" ;;
  darwin) asset="xrayctl-darwin-${arch}" ;;
  *) echo "不支持的平台：$os（Windows 请从 Releases 下载 .exe）"; exit 1 ;;
esac

if [ -n "$BASE" ]; then
  # 自建镜像：约定 <base>/<asset> 与 <base>/SHA256SUMS
  base="${BASE%/}"
elif [ "$VERSION" = "latest" ]; then
  base="https://github.com/${REPO}/releases/latest/download"
else
  base="https://github.com/${REPO}/releases/download/${VERSION}"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "下载 ${asset}（${VERSION}）"
# 正常路径：直链（公开仓开箱可用，不依赖 gh / token）。
# 只有直链失败（私有仓、或网络被墙）才回退：先 gh，再报错并提示镜像用法。
tag=""; [ "$VERSION" != "latest" ] && tag="$VERSION"
if ! curl -fsSL -o "$tmp/$asset" "$base/$asset" 2>/dev/null; then
  if { [ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ] || { command -v gh >/dev/null 2>&1 && gh auth token >/dev/null 2>&1; }; } && [ -z "$BASE" ]; then
    echo "直链失败，改用 gh 取（私有仓模式）"
    gh release download $tag --repo "$REPO" --pattern "$asset" --output "$tmp/$asset" --clobber
  else
    echo "下载失败：$base/$asset" >&2
    echo "国内网络可改用镜像：" >&2
    echo "  bash install.sh --base https://<镜像前缀>/${REPO}/releases/latest/download" >&2
    exit 1
  fi
fi
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" 2>/dev/null \
  || gh release download $tag --repo "$REPO" --pattern SHA256SUMS --output "$tmp/SHA256SUMS" --clobber 2>/dev/null

echo "校验 sha256"
if command -v sha256sum >/dev/null 2>&1; then
  ( cd "$tmp" && grep " ${asset}\$" SHA256SUMS | sha256sum -c - )
else
  want=$(grep " ${asset}\$" "$tmp/SHA256SUMS" | awk '{print $1}')
  got=$(shasum -a 256 "$tmp/$asset" | awk '{print $1}')
  [ "$want" = "$got" ] || { echo "sha256 不匹配：期望 $want 实际 $got"; exit 1; }
  echo "$asset: OK"
fi

if [ -n "$REMOTE" ]; then
  scp -q "$tmp/$asset" "$REMOTE:/tmp/xrayctl.new"
  ssh "$REMOTE" "sudo install -m 755 /tmp/xrayctl.new /usr/local/bin/xrayctl && rm -f /tmp/xrayctl.new"
  echo "已装到 ${REMOTE}： $(ssh "$REMOTE" '/usr/local/bin/xrayctl --version')"
  echo
  echo "之后本地零安装，直接："
  echo "  ssh $REMOTE xrayctl status"
  echo "  alias vps='ssh $REMOTE xrayctl'     # 想更短就加这行到 ~/.zshrc"
  exit 0
fi

if [ -z "$PREFIX" ]; then
  if [ -w /usr/local/bin ] 2>/dev/null; then
    PREFIX=/usr/local/bin
  else
    PREFIX="$HOME/.local/bin"
    case ":$PATH:" in
      *":$PREFIX:"*) ;;
      *) REBUILD_PATH=1 ;;
    esac
  fi
fi

mkdir -p "$PREFIX"
if [ -w "$PREFIX" ]; then
  install -m 755 "$tmp/$asset" "$PREFIX/xrayctl"
else
  sudo install -m 755 "$tmp/$asset" "$PREFIX/xrayctl"
fi

echo "已安装： $("$PREFIX/xrayctl" --version)  →  $PREFIX/xrayctl"
if [ "${REBUILD_PATH:-0}" = "1" ]; then
  echo
  echo "把下面这行加进 ~/.zshrc 或 ~/.bashrc 才能直接敲 xrayctl："
  echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi
echo
echo "试一下："
echo "  xrayctl --host <你的ssh别名> status"
echo "  xrayctl --host <你的ssh别名> doctor"
echo
echo "只想装服务器、本地不装：bash install.sh --remote <ssh别名>"
