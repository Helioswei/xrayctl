//! 从零部署：装 xray → 生成密钥/凭证 → 写配置 → systemd → 起服务 → 打印全部链接。
//! --dry-run 只生成+校验，不碰系统。

use crate::config::{build_config, public_ip, test_config, DEFAULT_CFG};
use crate::host::Target;
use anyhow::{bail, Context, Result};
use serde_json::Value;

/// 实测可用的 xray 版本（2026-10-31 逐版验证）。
///
/// 默认取「兼容地板」26.6.27：mihomo 默认配置 + xray 客户端都能连，不用改客户端。
/// 版本阶梯（实测）：
///   ≤26.6.27   mihomo 默认配置 ✅ / xray 客户端 ✅
///   26.7.11+   REALITY 引入 minClientVer，老客户端被拒（authentication failed）
///   ≥26.9.9    要求 X25519MLKEM768 后量子密钥交换 —— mihomo 必须在 reality-opts 里写
///               support-x25519mlkem768: true，否则连不上（xray 客户端不受影响）
/// 想用新版本：升客户端 + `--xray-version 26.10.29`，升完用真机连一次。
pub const TESTED_XRAY_VERSION: &str = "26.10.31";

const UNIT: &str = r#"[Unit]
Description=Xray Service
After=network.target nss-lookup.target

[Service]
User=xray
Group=xray
ExecStart=/usr/local/bin/xray run -config /usr/local/etc/xray/config.json
# 以 xray 用户跑又要绑 443：必须显式授予 CAP_NET_BIND_SERVICE，
# 否则日志里只有一句 bind 0.0.0.0:443: permission denied（真装时踩过）
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
Restart=on-failure
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
"#;

pub struct DeployOpts {
    pub clients: Vec<String>,
    pub port: u16,
    pub sni: String,
    pub dest: Option<String>,
    pub dry_run: bool,
    pub xray_url: Option<String>,
    /// 指定 xray 版本；不给就用 TESTED_XRAY_VERSION
    pub xray_version: Option<String>,
    /// 用本地现成配置部署（换机复用旧凭证，客户端不用重新导入）
    pub from_config: Option<String>,
}

pub fn run(t: &Target, o: &DeployOpts) -> Result<()> {
    if let Some(path) = o.from_config.as_deref() {
        return deploy_from_config(t, path, o);
    }
    if o.clients.is_empty() {
        bail!("至少给一个客户端名：--clients \"laptop,phone\"");
    }
    let dest = o
        .dest
        .clone()
        .unwrap_or_else(|| format!("{}:443", o.sni));

    ensure_xray(t, o.xray_url.as_deref(), o.xray_version.as_deref())?;

    // 本地生成全部凭证
    let keys = crate::keys::generate_keys();
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for email in &o.clients {
        rows.push((email.clone(), crate::keys::new_uuid(), crate::keys::new_short_id()));
    }
    let short_ids: Vec<String> = rows.iter().map(|(_, _, s)| s.clone()).collect();
    let cfg: Value = build_config(o.port, &o.sni, &dest, &keys.private, &rows, &short_ids);

    let tmp = "/tmp/xrayctl-deploy.json";
    t.write_file(tmp, &serde_json::to_string_pretty(&cfg)?, "600", Some("root:root"))?;
    test_config(t, tmp)?;
    println!("== 配置校验通过（clients={} shortIds={}）", rows.len(), short_ids.len());

    let ip = public_ip(t).unwrap_or_else(|_| "{公网IP}".into());
    if o.dry_run {
        let _ = t.ok(&format!("{}rm -f {}", t.su(), tmp));
        println!("\n== [dry-run] 未写入系统、未重启服务");
        println!("   公钥 {}", keys.public);
        print_links(&ip, o, &rows, &keys.public);
        return Ok(());
    }

    // 系统用户 + 目录 + 配置
    // 注意：整句都要在 sudo 下，否则只有 id 有权限、useradd 没有（真机踩过 Permission denied）
    t.ok(&format!(
        "{}sh -c 'id -u xray >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin xray'",
        t.su()
    ))?;
    t.ok(&format!("{}install -d -m 755 /usr/local/etc/xray", t.su()))?;
    // 旧配置先留一份：启动失败要还原，不能把现场打死
    let _ = t.ok(&format!(
        "{}[ -f {} ] && cp -p {} /tmp/xrayctl-deploy.prev || true",
        t.su(), DEFAULT_CFG, DEFAULT_CFG
    ));
    t.ok(&format!(
        "{}install -m 600 -o xray -g xray {} {}",
        t.su(),
        tmp,
        DEFAULT_CFG
    ))?;
    let _ = t.ok(&format!("{}rm -f {}", t.su(), tmp));

    // systemd
    t.write_file("/etc/systemd/system/xray.service", UNIT, "644", Some("root:root"))?;
    t.ok(&format!("{}systemctl daemon-reload", t.su()))?;
    t.ok(&format!("{}systemctl enable xray", t.su()))?;
    t.ok(&format!("{}systemctl restart xray", t.su()))?;
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let st = t.ok("systemctl is-active xray").unwrap_or_default();
    if st != "active" {
        // 还原上一份配置并把服务拉回来，避免"装一半"把线上打死
        let _ = t.ok(&format!(
            "{}if [ -f /tmp/xrayctl-deploy.prev ]; then cp -p /tmp/xrayctl-deploy.prev {} && systemctl restart xray; fi",
            t.su(), DEFAULT_CFG
        ));
        let restored = t.ok("systemctl is-active xray").unwrap_or_default();
        bail!(
            "xray 启动失败（{}）\n  已还原上一份配置，服务现在：{}\n  查原因：journalctl -u xray -n 30",
            st.trim(), restored.trim()
        );
    }
    println!("== xray active");

    // BBR + vnstat（失败不影响主流程）
    let _ = t.ok(&format!(
        "printf 'net.core.default_qdisc=fq\\nnet.ipv4.tcp_congestion_control=bbr\\n' > /tmp/99-bbr.conf && \
         {}mv /tmp/99-bbr.conf /etc/sysctl.d/99-bbr.conf && {}sysctl -q --system",
        t.su(),
        t.su()
    ));
    if t.run(&format!("{}DEBIAN_FRONTEND=noninteractive apt-get install -y -qq vnstat", t.su()))
        .map(|o| o.code == 0)
        .unwrap_or(false)
    {
        println!("== 已装 vnstat（月流量口径）");
    }

    println!("\n== 部署完成，服务端公钥 {}", keys.public);
    print_links(&ip, o, &rows, &keys.public);
    println!("\n下一步：云控制台放行 TCP {}（脚本做不了）", o.port);
    Ok(())
}

fn print_links(ip: &str, o: &DeployOpts, rows: &[(String, String, String)], pubkey: &str) {
    println!("\n== 客户端链接（同一份配置，任选客户端导入）");
    for (email, id, sid) in rows {
        let conn = crate::config::Conn {
            host: ip.to_string(),
            port: o.port,
            id: id.clone(),
            public_key: pubkey.to_string(),
            short_id: sid.clone(),
            sni: o.sni.clone(),
            flow: "xtls-rprx-vision".into(),
        };
        println!("  {:<16} {}", email, conn.vless_link(email));
    }
}

fn ensure_xray(t: &Target, url: Option<&str>, version: Option<&str>) -> Result<()> {
    if t.run("test -x /usr/local/bin/xray").map(|o| o.code == 0).unwrap_or(false) {
        let v = t.ok("/usr/local/bin/xray version | head -1").unwrap_or_default();
        println!("== xray 已存在：{}", v);
        return Ok(());
    }
    let arch = t.ok("uname -m")?;
    let asset = match arch.trim() {
        "x86_64" | "amd64" => "Xray-linux-64.zip",
        "aarch64" | "arm64" => "Xray-linux-arm64-v8a.zip",
        other => bail!("不支持的架构 {}（可手动装 xray 后重试）", other),
    };
    let ver = version.unwrap_or(TESTED_XRAY_VERSION);
    let url = url
        .map(|u| u.to_string())
        .unwrap_or_else(|| {
            format!(
                "https://github.com/XTLS/Xray-core/releases/download/v{}/{}",
                ver, asset
            )
        });
    println!("== 下载 xray v{}：{}", ver, url);
    let script = format!(
        "set -e; {su}apt-get install -y -qq unzip >/dev/null 2>&1 || true; \
         curl -fsSL --max-time 180 -o /tmp/xray.zip {url}; \
         rm -rf /tmp/xray-unz && mkdir -p /tmp/xray-unz; unzip -oq /tmp/xray.zip -d /tmp/xray-unz; \
         {su}install -m 755 /tmp/xray-unz/xray /usr/local/bin/xray; rm -rf /tmp/xray.zip /tmp/xray-unz",
        su = t.su(),
        url = url
    );
    let out = t.run(&script)?;
    if out.code != 0 {
        bail!(
            "下载/安装 xray 失败：{}\n（机器出不了 GitHub 时，用 --xray-url 指定镜像或先手动装）",
            out.stderr.trim()
        );
    }
    let v = t.ok("/usr/local/bin/xray version | head -1").unwrap_or_default();
    println!("== 装好 {}", v);
    Ok(())
}


/// 用现成配置部署：换机器时把旧机器的 config.json 原样搬过去，
/// 凭证（UUID / 私钥 / shortId）全不变 → 手机、Clash、其他机器都不用重新导入，只改 server IP。
fn deploy_from_config(t: &Target, path: &str, o: &DeployOpts) -> Result<()> {
    let txt = std::fs::read_to_string(path)
        .with_context(|| format!("读不到本地配置 {}", path))?;
    let cfg: Value = serde_json::from_str(&txt)
        .with_context(|| format!("{} 不是合法 JSON", path))?;

    // 打印将要部署的凭证概况，让人一眼确认没搬错
    let conf = crate::config::ServerConf { raw: cfg.clone(), path: path.to_string() };
    let clients = conf.clients();
    let sids = conf.short_ids();
    if clients.is_empty() {
        bail!("{} 里没有 vless 客户端", path);
    }
    println!("== 复用配置：{} 个客户端 / {} 个 shortId", clients.len(), sids.len());
    for c in &clients {
        println!("   - {} ({})", c.email, &c.id[..8.min(c.id.len())]);
    }
    println!("== 公钥 {}", conf.public_key()?);

    if cfg.get("stats").is_none() {
        println!("!! 注意：这份配置没有 stats/api，按客户端流量统计会失效");
    }

    ensure_xray(t, o.xray_url.as_deref(), o.xray_version.as_deref())?;

    let tmp = "/tmp/xrayctl-deploy.json";
    t.write_file(tmp, &serde_json::to_string_pretty(&cfg)?, "600", Some("root:root"))?;
    test_config(t, tmp)?;
    println!("== 配置校验通过");

    if o.dry_run {
        let _ = t.ok(&format!("{}rm -f {}", t.su(), tmp));
        println!("\n== [dry-run] 未写入、未重启");
        return Ok(());
    }

    // 注意：整句都要在 sudo 下，否则只有 id 有权限、useradd 没有（真机踩过 Permission denied）
    t.ok(&format!(
        "{}sh -c 'id -u xray >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin xray'",
        t.su()
    ))?;
    t.ok(&format!("{}install -d -m 755 /usr/local/etc/xray", t.su()))?;
    let _ = t.ok(&format!(
        "{}[ -f {} ] && cp -p {} /tmp/xrayctl-deploy.prev || true",
        t.su(), crate::config::DEFAULT_CFG, crate::config::DEFAULT_CFG
    ));
    t.ok(&format!(
        "{}install -m 600 -o xray -g xray {} {}",
        t.su(), tmp, crate::config::DEFAULT_CFG
    ))?;
    let _ = t.ok(&format!("{}rm -f {}", t.su(), tmp));

    t.write_file("/etc/systemd/system/xray.service", UNIT, "644", Some("root:root"))?;
    t.ok(&format!("{}systemctl daemon-reload", t.su()))?;
    t.ok(&format!("{}systemctl enable xray", t.su()))?;
    t.ok(&format!("{}systemctl restart xray", t.su()))?;
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let st = t.ok("systemctl is-active xray").unwrap_or_default();
    if st.trim() != "active" {
        let _ = t.ok(&format!(
            "{}if [ -f /tmp/xrayctl-deploy.prev ]; then cp -p /tmp/xrayctl-deploy.prev {} && systemctl restart xray; fi",
            t.su(), crate::config::DEFAULT_CFG
        ));
        bail!("xray 起不来（{}）。查：journalctl -u xray -n 30", st.trim());
    }
    println!("== xray active");

    // 端口放行 + 打印新机器的客户端链接（凭证不变，只有 IP 变了）
    let port = conf.port().unwrap_or(o.port);
    let _ = t.ok(&format!(
        "{}command -v ufw >/dev/null && ufw allow {}/tcp >/dev/null 2>&1 || true",
        t.su(), port
    ));
    let ip = crate::config::public_ip(t).unwrap_or_else(|_| "{公网IP}".into());
    println!("\n== 部署完成，公网 IP {}", ip);
    println!("   客户端只需把 server 地址改成 {}", ip);
    Ok(())
}
