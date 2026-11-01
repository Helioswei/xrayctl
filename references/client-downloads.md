# 客户端下载：哪个平台用什么、从哪下、国内怎么下

先回答一个高频混淆：**v2rayNG 只有 Android 版**，没有 Windows/macOS 版。
电脑上同作者的软件叫 **v2rayN**（也是 2dust 出的，但是另一个项目，.NET 写的，只支持 Windows/Linux）。
macOS 上一般不用这两个，用 Clash Verge Rev 或 sing-box。

## 对照表（版本号为本项目实测时的上游最新，2026-10-31）

| 平台 | 客户端 | 上游仓库 | 版本 | 下哪个文件 |
|---|---|---|---|---|
| **Android** | v2rayNG | `2dust/v2rayNG` | 2.3.14 | `v2rayNG_<ver>-fdroid_arm64-v8a.apk`（绝大多数手机）；不确定就 `universal.apk` |
| Android | NekoBox | `MatsuriDayo/NekoBoxForAndroid` | 1.4.2 | `NekoBox-<ver>-arm64-v8a.apk` |
| Android | Hiddify | `hiddify/hiddify-app` | 4.1.1 | `Hiddify-Android-arm64.apk` |
| Android | sing-box (SFA) | `SagerNet/sing-box` | 1.14.4 | `SFA-<ver>-arm64-v8a.apk` |
| **iOS** | sing-box (SFI) | `SagerNet/sing-box` | 1.14.4 | App Store 搜 sing-box（免费，需非中国区 Apple ID） |
| iOS | Shadowrocket | 闭源付费 | — | App Store，需外区 ID，约 $2.99 |
| **macOS** | Clash Verge Rev | `clash-verge-rev/clash-verge-rev` | v2.5.8 | `*.dmg`（Apple Silicon 选 aarch64） |
| macOS | sing-box (SFM) | `SagerNet/sing-box` | 1.14.4 | `SFM-*.pkg` |
| **Windows** | v2rayN | `2dust/v2rayN` | 7.25.6 | `v2rayN-windows-64.zip`（含核心的包名带 `-with-core`） |
| Windows | Clash Verge Rev | 同上 | v2.5.8 | `*_x64-setup.exe` |
| **Linux** | v2rayN | `2dust/v2rayN` | 7.25.6 | `v2rayN-linux-64.deb` / `.zip` |
| Linux | Clash Verge Rev | 同上 | v2.5.8 | `.rpm` / AppImage |
| **服务端/命令行** | xray-core | `XTLS/Xray-core` | 26.x | `Xray-linux-64.zip` |

下载页统一是 `https://github.com/<仓库>/releases/latest`。

> 本表只写"实测当时的版本"。要最新版，直接看 releases/latest，别照抄版本号。

## 国内怎么下（这是真正的门槛）

GitHub Releases 在国内经常连不上（`curl` 超时、`RST`）。三条路，按可靠性排：

**1. 有代理就直接下（最稳）**

```bash
export https_proxy=http://127.0.0.1:7897
curl -L -O https://github.com/2dust/v2rayNG/releases/download/2.3.14/v2rayNG_2.3.14-fdroid_arm64-v8a.apk
```

**2. 从 GitHub API 找资产名，避免手打错**

```bash
export https_proxy=http://127.0.0.1:7897
curl -s https://api.github.com/repos/2dust/v2rayNG/releases/latest \
  | python3 -c "import json,sys;[print(a['name']) for a in json.load(sys.stdin)['assets']]"
```

**3. 手机装 APK 的鸡生蛋问题**：手机没代理就下不了 GitHub，用 Mac 当临时服务器传过去

```bash
cd ~/Downloads && python3 -m http.server 8899 --bind 0.0.0.0     # 必须后台跑
ipconfig getifaddr en0                                          # Mac 的局域网 IP
# 手机浏览器打开 http://<那个IP>:8899/v2rayNG_xxx.apk
```

三个坑：① 端口可能被上次遗留的 `http.server` 占着，先 `curl -o /dev/null -w '%{http_code}'` 自测再发地址；
② 传完就关掉，别长期把目录暴露在局域网；③ APK 几十 MB，别塞进 git 仓库。

## 别做的事

- **不要从第三方 APK 站/网盘下**：这类客户端能改系统 VPN、读全部流量，捆绑一次就是长期后门。
- 认准 `github.com/2dust/...`、`github.com/SagerNet/...`、`github.com/clash-verge-rev/...` 这类官方 release。
- 校验随包发布的 `.sig` 文件（v2rayNG/v2rayN 用 GPG 签名），或对 `sha256sum`；
  上游不提供 sha256 时，至少确认 release 页面上的发布者与仓库同名。

## 装完连什么

客户端装好只是壳，连接参数要按 `references/client-configs.md` 填：
mihomo/Clash 用 yaml，v2rayN/v2rayNG 直接粘贴 `vless://` 链接或扫码。
