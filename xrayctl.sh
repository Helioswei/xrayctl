#!/bin/sh
# xrayctl.sh — 单文件入口。用户只拿这一个文件，不用选平台、不用自己编。
#
# 做法：首次运行按 OS/架构取对应二进制（sha256 校验）缓存到本地，之后直接 exec。
# 也就是说"一份文件给用户"和"原生静态二进制"可以同时成立：壳是跨平台的，肉按需取。
#
# 用法：
#   ./xrayctl.sh --host my-vps status
#   sh xrayctl.sh status                  # 在 Linux 服务器上跑（本机模式）
#   XRAYCTL_VERSION=vX.Y.Z ./xrayctl.sh    # 钉版本
#
# Windows：用 Git Bash / WSL 跑本脚本，或在服务器上直接装二进制。
set -eu

REPO="${XRAYCTL_REPO:-Helioswei/xrayctl}"
VERSION="${XRAYCTL_VERSION:-latest}"
ASSET_ROOT="${XRAYCTL_BASE:-${XRAYCTL_ASSET_ROOT:-}}"   # 自建镜像/CDN 根，如 https://cdn.example.com/xrayctl
MIRRORS="${XRAYCTL_MIRROR:-https://gh-proxy.com/ https://ghfast.top/}"   # 国内代理前缀，直链失败时依次试

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$arch" in
  x86_64|amd64)  arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) echo "不支持的架构：$arch" >&2; exit 1 ;;
esac
case "$os" in
  linux)   asset="xrayctl-linux-${arch}" ;;
  darwin)  asset="xrayctl-darwin-${arch}" ;;
  msys*|cygwin*|mingw*)
    echo "Windows 上请用 Releases 里的 xrayctl-windows-x86_64.exe（PowerShell 里直接跑）" >&2
    exit 1 ;;
  *) echo "不支持的平台：$os" >&2; exit 1 ;;
esac

CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/xrayctl/${VERSION}"
BIN="$CACHE/xrayctl"
if [ ! -x "$BIN" ]; then
  mkdir -p "$CACHE"
  if [ -n "$ASSET_ROOT" ]; then
    base="$ASSET_ROOT"
  elif [ "$VERSION" = "latest" ]; then
    base="https://github.com/${REPO}/releases/latest/download"
  else
    base="https://github.com/${REPO}/releases/download/${VERSION}"
  fi
  echo "首次运行，取 ${asset}（${VERSION}）…" >&2
  tag=""; [ "$VERSION" != "latest" ] && tag="$VERSION"

  sha_of() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
    else shasum -a 256 "$1" | awk '{print $1}'; fi
  }

  # 先取校验清单（直链 → 镜像 → gh）
  if ! curl -fsSL --connect-timeout 8 --max-time 60 -o "$CACHE/SHA256SUMS" "$base/SHA256SUMS" 2>/dev/null; then
    for m in $MIRRORS; do
      [ -z "$m" ] && continue
      curl -fsSL --connect-timeout 8 --max-time 60 -o "$CACHE/SHA256SUMS" "${m}${base}/SHA256SUMS" 2>/dev/null && break
    done
  fi
  if [ ! -s "$CACHE/SHA256SUMS" ] && [ -z "$ASSET_ROOT" ] && command -v gh >/dev/null 2>&1 && gh auth token >/dev/null 2>&1; then
    gh release download $tag --repo "$REPO" --pattern SHA256SUMS --output "$CACHE/SHA256SUMS" --clobber 2>/dev/null || true
  fi
  want=$(grep " ${asset}$" "$CACHE/SHA256SUMS" 2>/dev/null | awk '{print $1}' || true)
  [ -z "$want" ] && { echo "拿不到 ${asset} 的 sha256，拒绝执行" >&2; exit 1; }

  got_ok=""
  try_get() { # $1=url $2=来源名
    curl -fsSL --connect-timeout 8 --max-time 180 -o "$BIN.tmp" "$1" 2>/dev/null || return 1
    [ "$(sha_of "$BIN.tmp")" = "$want" ] || { echo "  来源 $2：校验不符，换下一个" >&2; return 1; }
    got_ok="$2"; return 0
  }
  try_get "$base/$asset" "直链" || {
    for m in $MIRRORS; do
      [ -z "$m" ] && continue
      try_get "${m}${base}/$asset" "镜像 $m" && break
    done
  }
  if [ -z "$got_ok" ] && [ -z "$ASSET_ROOT" ] && command -v gh >/dev/null 2>&1 && gh auth token >/dev/null 2>&1; then
    gh release download $tag --repo "$REPO" --pattern "$asset" --output "$BIN.tmp" --clobber
    [ "$(sha_of "$BIN.tmp")" = "$want" ] || { echo "gh 取到的文件校验不符" >&2; rm -f "$BIN.tmp"; exit 1; }
    got_ok="gh"
  fi
  [ -z "$got_ok" ] && { echo "下载失败：$base/$asset（可用 XRAYCTL_MIRROR 或 XRAYCTL_BASE 指定镜像）" >&2; rm -f "$BIN.tmp"; exit 1; }
  chmod +x "$BIN.tmp" && mv "$BIN.tmp" "$BIN"
fi

exec "$BIN" "$@"
