#!/bin/sh
# xrayctl.sh — 单文件入口。用户只拿这一个文件，不用选平台、不用自己编。
#
# 做法：首次运行按 OS/架构取对应二进制（sha256 校验）缓存到本地，之后直接 exec。
# 也就是说"一份文件给用户"和"原生静态二进制"可以同时成立：壳是跨平台的，肉按需取。
#
# 用法：
#   ./xrayctl.sh --host my-vps status
#   sh xrayctl.sh status                  # 在 Linux 服务器上跑（本机模式）
#   XRAYCTL_VERSION=v0.1.0 ./xrayctl.sh    # 钉版本
#
# Windows：用 Git Bash / WSL 跑本脚本，或在服务器上直接装二进制。
set -eu

REPO="${XRAYCTL_REPO:-Helioswei/xrayctl}"
VERSION="${XRAYCTL_VERSION:-latest}"
ASSET_ROOT="${XRAYCTL_BASE:-${XRAYCTL_ASSET_ROOT:-}}"   # 自建镜像/CDN 根，如 https://cdn.example.com/xrayctl

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
  # 正常路径是直链（公开仓开箱可用）；私有仓或网络受限才回退 gh
  tag=""; [ "$VERSION" != "latest" ] && tag="$VERSION"
  if ! curl -fsSL -o "$BIN.tmp" "$base/$asset" 2>/dev/null; then
    if [ -z "$ASSET_ROOT" ] && { [ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ] || { command -v gh >/dev/null 2>&1 && gh auth token >/dev/null 2>&1; }; }; then
      gh release download $tag --repo "$REPO" --pattern "$asset" --output "$BIN.tmp" --clobber
      gh release download $tag --repo "$REPO" --pattern SHA256SUMS --output "$CACHE/SHA256SUMS" --clobber 2>/dev/null || true
    else
      echo "下载失败：$base/$asset（国内可设 XRAYCTL_BASE 指向镜像）" >&2
      exit 1
    fi
  fi
  curl -fsSL -o "$CACHE/SHA256SUMS" "$base/SHA256SUMS" 2>/dev/null || true
  # 校验：拿不到 SHA256SUMS 就拒绝执行，宁可失败也不装来路不明的二进制
  if [ -s "$CACHE/SHA256SUMS" ]; then
    want=$(grep " ${asset}\$" "$CACHE/SHA256SUMS" | awk '{print $1}')
    if command -v sha256sum >/dev/null 2>&1; then
      got=$(sha256sum "$BIN.tmp" | awk '{print $1}')
    else
      got=$(shasum -a 256 "$BIN.tmp" | awk '{print $1}')
    fi
    [ -n "$want" ] && [ "$want" = "$got" ] || { echo "sha256 校验失败" >&2; rm -f "$BIN.tmp"; exit 1; }
  else
    echo "拿不到 SHA256SUMS，拒绝执行" >&2; rm -f "$BIN.tmp"; exit 1
  fi
  chmod +x "$BIN.tmp" && mv "$BIN.tmp" "$BIN"
fi

exec "$BIN" "$@"
