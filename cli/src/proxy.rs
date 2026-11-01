//! 反过来用：把这台服务器当客户端（默认关闭，用时才开）。
//!
//! 场景：海外/国内服务器需要访问 GitHub、pypi 时，装一个本地 xray 客户端连自建节点，
//! 只监听 127.0.0.1，平时停着。注意 systemd 服务拿不到 shell 里的 export，
//! 所以 `on` 只是起服务，环境变量由 `xrayctl proxy env` 打印、由 shell 自己 eval。

use crate::config::{public_ip, ServerConf, DEFAULT_CFG};
use crate::host::Target;
use anyhow::Result;
use serde_json::json;

const CLIENT_CFG: &str = "/usr/local/etc/xray-client/config.json";
const UNIT: &str = r#"[Unit]
Description=Xray client (与自建节点互联，仅本地代理)
After=network.target

[Service]
User=root
ExecStart=/usr/local/bin/xray-client run -config /usr/local/etc/xray-client/config.json
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
"#;

pub struct Ports {
    pub socks: u16,
    pub http: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Ports { socks: 1080, http: 8118 }
    }
}

/// 读本机（服务器上）的 REALITY 配置，生成指向自己的客户端配置
pub fn install(
    t: &Target,
    p: &Ports,
    email: Option<&str>,
    creds_from: Option<&Target>,
) -> Result<()> {
    // 凭证来源默认是这台机器自己；把别的机器当客户端时用 --from 指定节点
    let src = creds_from.unwrap_or(t);
    let conf = ServerConf::load(src, DEFAULT_CFG)?;
    let clients = conf.clients();
    let client = match email {
        Some(e) => clients
            .iter()
            .find(|c| c.email == e)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("没有 email={}", e))?,
        // 没指定就用约定的客户端名，避免顺手用掉 mac 那份凭证造成两份配置打架
        None => {
            let preferred = ["cn-hermes", "proxy", "cn"];
            clients
                .iter()
                .find(|c| preferred.contains(&c.email.as_str()))
                .or_else(|| clients.first())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("服务端没有任何客户端，先 user add"))?
        }
    };
    let idx = clients.iter().position(|c| c.email == client.email).unwrap_or(0);
    let sid = conf
        .short_ids()
        .get(idx)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("找不到该客户端对应的 shortId"))?;
    let ip = public_ip(src)?;

    let cconf = json!({
        "log": {"loglevel": "warning"},
        "inbounds": [
            {"tag":"socks-in","listen":"127.0.0.1","port":p.socks,"protocol":"socks",
             "settings":{"udp":false,"auth":"noauth"}},
            {"tag":"http-in","listen":"127.0.0.1","port":p.http,"protocol":"http","settings":{}}
        ],
        "outbounds": [{
            "tag": "vps",
            "protocol": "vless",
            "settings": {"vnext": [{"address": ip, "port": conf.port()?,
                "users": [{"id": client.id, "encryption": "none", "flow": client.flow}]}]},
            "streamSettings": {"network":"tcp","security":"reality","realitySettings":{
                "fingerprint":"chrome","serverName":conf.sni()?,
                "publicKey":conf.public_key()?,"shortId":sid,"spiderX":"/"}}
        }]
    });

    t.ok(&format!("{}install -d -m 755 /usr/local/etc/xray-client", t.su()))?;
    t.write_file(CLIENT_CFG, &serde_json::to_string_pretty(&cconf)?, "600", Some("root:root"))?;
    // 客户端二进制：有 xray 就复制一份，已经是 xray-client 就跳过，都没有就下载
    let has_xray = t.run("test -x /usr/local/bin/xray").map(|o| o.code == 0).unwrap_or(false);
    let has_client = t.run("test -x /usr/local/bin/xray-client").map(|o| o.code == 0).unwrap_or(false);
    if has_xray {
        t.ok(&format!(
            "{}install -m 755 /usr/local/bin/xray /usr/local/bin/xray-client",
            t.su()
        ))?;
    } else if !has_client {
        let arch = t.ok("uname -m")?;
        let asset = if arch.trim() == "aarch64" { "Xray-linux-arm64-v8a.zip" } else { "Xray-linux-64.zip" };
        t.ok(&format!(
            "set -e; curl -fsSL -o /tmp/xc.zip https://github.com/XTLS/Xray-core/releases/latest/download/{asset}; \
             rm -rf /tmp/xc && mkdir /tmp/xc && unzip -oq /tmp/xc.zip -d /tmp/xc; \
             {}install -m 755 /tmp/xc/xray /usr/local/bin/xray-client; rm -rf /tmp/xc /tmp/xc.zip",
            t.su()
        ))?;
    }
    t.write_file("/etc/systemd/system/xray-client.service", UNIT, "644", Some("root:root"))?;
    t.ok(&format!("{}systemctl daemon-reload", t.su()))?;
    t.ok(&format!("{}systemctl disable --now xray-client", t.su()))?;

    println!("已安装（默认关闭，不自启）");
    println!("  客户端凭证  {}", client.email);
    println!("  监听        127.0.0.1:{} (socks5) / 127.0.0.1:{} (http)", p.socks, p.http);
    println!("\n开关： xrayctl proxy on | off | status | env");
    Ok(())
}

pub fn on(t: &Target, p: &Ports) -> Result<()> {
    t.ok(&format!("{}systemctl start xray-client", t.su()))?;
    std::thread::sleep(std::time::Duration::from_millis(800));
    let ip = exit_ip(t, p).unwrap_or_default();
    if ip.is_empty() {
        anyhow::bail!("服务起了但连不通（journalctl -u xray-client -n 20）");
    }
    println!("代理已开启，出口 {}", ip);
    println!("\n当前 shell 生效： eval \"$(xrayctl proxy env)\"");
    println!("单条命令：       xrayctl proxy run <命令>（等价 PROXY 前缀）");
    Ok(())
}

pub fn off(t: &Target) -> Result<()> {
    t.ok(&format!("{}systemctl stop xray-client", t.su()))?;
    t.ok(&format!("{}systemctl disable xray-client", t.su()))?;
    println!("代理已关闭，且重启后不自启");
    Ok(())
}

pub fn status(t: &Target, p: &Ports) -> Result<()> {
    let active = systemctl_state(t, "is-active");
    let enabled = systemctl_state(t, "is-enabled");
    println!("服务      {}", active);
    println!("自启      {}", enabled);
    println!("监听      127.0.0.1:{} / :{}", p.socks, p.http);
    if active == "active" {
        if let Ok(ip) = exit_ip(t, p) {
            println!("出口      {}", ip);
        }
    } else {
        let direct = t.ok("curl -s --max-time 6 https://api.ipify.org").unwrap_or_default();
        println!("出口      {}（直连）", direct);
    }
    Ok(())
}

pub fn env(p: &Ports) -> String {
    format!(
        "export http_proxy=http://127.0.0.1:{http}\n\
         export https_proxy=http://127.0.0.1:{http}\n\
         export all_proxy=socks5h://127.0.0.1:{socks}\n\
         export no_proxy=localhost,127.0.0.1,::1\n\
         export HTTP_PROXY=$http_proxy HTTPS_PROXY=$https_proxy ALL_PROXY=$all_proxy NO_PROXY=$no_proxy",
        http = p.http,
        socks = p.socks
    )
}

/// is-active / is-enabled 在"未运行/未启用"时退出码非 0，但 stdout 仍有值
fn systemctl_state(t: &Target, what: &str) -> String {
    t.run(&format!("systemctl {} xray-client", what))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default()
}

fn exit_ip(t: &Target, p: &Ports) -> Result<String> {
    t.ok(&format!(
        "curl -s --max-time 10 -x socks5h://127.0.0.1:{} https://api.ipify.org",
        p.socks
    ))
}
