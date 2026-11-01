//! 升级 xray 二进制：备份 → 下载 → 用现有配置校验新二进制 → 替换 → 重启 → 不行就回滚。
//! 之前没有这一步，是这个工具链唯一的真短板。

use crate::host::Target;
use anyhow::{bail, Result};

pub fn run(t: &Target, version: Option<&str>, url: Option<&str>, latest: bool) -> Result<()> {
    let before = t.ok("/usr/local/bin/xray version | head -1").unwrap_or_else(|_| "未安装".into());
    println!("当前版本  {}", before);

    let arch = t.ok("uname -m")?;
    let asset = match arch.trim() {
        "x86_64" | "amd64" => "Xray-linux-64.zip",
        "aarch64" | "arm64" => "Xray-linux-arm64-v8a.zip",
        other => bail!("不支持的架构 {}", other),
    };
    let url = url
        .map(|u| u.to_string())
        .unwrap_or_else(|| match version {
            Some(v) => {
                let tag = if v.starts_with('v') { v.to_string() } else { format!("v{}", v) };
                format!("https://github.com/XTLS/Xray-core/releases/download/{}/{}", tag, asset)
            }
            None => format!(
                "https://github.com/XTLS/Xray-core/releases/download/v{}/{}",
                crate::deploy::TESTED_XRAY_VERSION,
                asset
            ),
        });
    println!("下载地址  {}", url);
    if latest {
        println!();
        println!("⚠️  你在升到 latest。实测 26.10.29 的服务端 REALITY 握手与 mihomo/Clash 客户端不兼容");
        println!("   （报错 REALITY authentication failed，xray 自己的客户端却正常）。");
        println!("   升完必须用真实客户端连一次：不通就 `xrayctl rollback`。");
        println!();
    }

    let stamp = t.ok("date +%Y%m%d-%H%M%S")?;
    let backup = format!("/usr/local/bin/xray.bak-{}", stamp);
    let script = format!(
        r#"set -e
{su}cp /usr/local/bin/xray {bak}
{su}apt-get install -y -qq unzip >/dev/null 2>&1 || true
curl -fsSL --max-time 300 -o /tmp/xray-up.zip {url}
rm -rf /tmp/xray-up && mkdir -p /tmp/xray-up
unzip -oq /tmp/xray-up.zip -d /tmp/xray-up
NEW=/tmp/xray-up/xray
echo "-- 新二进制版本：$($NEW version | head -1)"
TEST=$({su}$NEW run -test -c {cfg} 2>&1 || true)   # 配置是 600 xray:xray，必须 sudo 才读得到
if ! echo "$TEST" | grep -q "Configuration OK"; then
  echo "--- 新二进制校验输出 ---"
  echo "$TEST" | tail -8
  echo "新二进制不认现有配置，已放弃升级（旧二进制未动）"
  rm -rf /tmp/xray-up /tmp/xray-up.zip
  exit 3
fi
{su}install -m 755 $NEW /usr/local/bin/xray
{su}systemctl restart xray || true
sleep 1
if [ "$(systemctl is-active xray)" != "active" ]; then
  echo "重启失败，回滚到 {bak}"
  {su}install -m 755 {bak} /usr/local/bin/xray
  {su}systemctl restart xray || true
  sleep 1
  echo "回滚后状态：$(systemctl is-active xray)"
  exit 4
fi
rm -rf /tmp/xray-up /tmp/xray-up.zip
echo "OK""#,
        su = t.su(),
        bak = backup,
        url = url,
        cfg = crate::config::DEFAULT_CFG
    );
    let out = t.run(&script)?;
    let all = format!("{}{}", out.stdout, out.stderr);
    for line in all.lines().filter(|l| !l.trim().is_empty()) {
        println!("  {}", line);
    }
    if out.code != 0 {
        bail!("升级失败（exit {}），旧二进制备份在 {}", out.code, backup);
    }
    let after = t.ok("/usr/local/bin/xray version | head -1")?;
    println!("升级后版本  {}", after);
    println!("备份        {}", backup);
    Ok(())
}

/// 只检查有没有新版（不下载、不改动）
pub fn check(t: &Target) -> Result<()> {
    let cur = t.ok("/usr/local/bin/xray version | head -1")?;
    let latest = t.ok(
        "curl -fsSL --max-time 20 https://api.github.com/repos/XTLS/Xray-core/releases/latest \
         | grep -m1 '\"tag_name\"' | cut -d'\"' -f4",
    )?;
    println!("当前      {}", cur);
    println!("最新 tag  {}", latest);
    Ok(())
}

/// 回滚到最近一次升级前的备份（升级后客户端连不上时用这条）
pub fn rollback(t: &Target, file: Option<&str>) -> Result<()> {
    let bak = match file {
        Some(f) => f.to_string(),
        None => t.ok("ls -t /usr/local/bin/xray.bak-* 2>/dev/null | head -1")?,
    };
    if bak.trim().is_empty() {
        bail!("没有可用的备份（/usr/local/bin/xray.bak-*）");
    }
    println!("当前  {}", t.ok("/usr/local/bin/xray version | head -1").unwrap_or_default());
    println!("回滚到 {}", bak);
    t.ok(&format!("{}install -m 755 {} /usr/local/bin/xray", t.su(), bak))?;
    t.ok(&format!("{}systemctl restart xray", t.su()))?;
    std::thread::sleep(std::time::Duration::from_millis(1000));
    println!("状态  {}", t.ok("systemctl is-active xray").unwrap_or_default());
    println!("版本  {}", t.ok("/usr/local/bin/xray version | head -1").unwrap_or_default());
    println!("\n重要：回滚只恢复了服务端，请立刻用真实客户端（手机/Clash）连一次确认。");
    Ok(())
}
