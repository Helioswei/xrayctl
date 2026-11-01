//! 体检：服务状态 / 端到端回环自测。
//!
//! 端到端用的办法：在服务器上临时起一个 xray **客户端**实例（socks 绑回环），
//! 用它连自己 443，然后 `curl -x socks5h://` 取出口 IP。能出 IP 才算这条凭证真的通。

use crate::config::{public_ip, ServerConf, DEFAULT_CFG};
use crate::host::Target;
use anyhow::{bail, Result};
use serde_json::json;

pub struct Status {
    pub active: bool,
    pub ports: Vec<String>,
    pub clients: usize,
    pub stats_ok: bool,
    pub uptime_min: Option<i64>,
    pub listening_443: bool,
}

pub fn status(t: &Target) -> Result<Status> {
    let conf = ServerConf::load(t, DEFAULT_CFG)?;
    let port = conf.port()?;
    let active = t.ok("systemctl is-active xray").map(|s| s == "active").unwrap_or(false);
    let ports = t
        .ok(&format!(
            "{}ss -lntH | awk '{{print $4}}' | grep -E ':(443|{})$' | sort -u | tr '\\n' ' '",
            t.su(),
            crate::config::API_PORT
        ))
        .unwrap_or_default();
    let listening_443 = t
        .run(&format!("{}ss -lntH | grep -q ':{}\\b'", t.su(), port))?
        .code
        == 0;
    let stats_ok = crate::traffic::query(t).is_ok();
    Ok(Status {
        active,
        ports: ports.split_whitespace().map(|s| s.to_string()).collect(),
        clients: conf.clients().len(),
        stats_ok,
        uptime_min: crate::traffic::uptime_minutes(t),
        listening_443,
    })
}

pub fn cmd_status(t: &Target) -> Result<()> {
    let s = status(t)?;
    let conf = ServerConf::load(t, DEFAULT_CFG)?;
    println!("目标      {}", t.describe());
    println!("服务      {}", if s.active { "active" } else { "未运行 ✗" });
    println!("端口      {}（vless {} / api {}）", s.ports.join(" "), conf.port()?, crate::config::API_PORT);
    println!(
        "运行时长  {}",
        s.uptime_min
            .map(|m| {
                if m >= 60 {
                    format!("{} 小时 {} 分", m / 60, m % 60)
                } else {
                    format!("{} 分钟", m)
                }
            })
            .unwrap_or_else(|| "未知".into())
    );
    println!("客户端    {} 个", s.clients);
    for c in conf.clients() {
        println!("          - {} ({})", c.email, &c.id[..c.id.len().min(8)]);
    }
    println!("统计接口  {}", if s.stats_ok { "可用" } else { "不可用（配置缺 stats/api/policy）" });
    println!("SNI       {}", conf.sni()?);
    Ok(())
}

/// 在服务器上把某条凭证跑一遍：返回 (出口IP, 是否等于服务器公网IP)
pub fn e2e(t: &Target, conf: &ServerConf, email: &str, socks_port: u16) -> Result<(String, bool)> {
    let clients = conf.clients();
    let client = clients
        .iter()
        .find(|c| c.email == email)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("没有 email={} 的客户端", email))?;
    let sids = conf.short_ids();
    // shortIds 与 clients 按下标对应（部署时就是这么写的）；数量不齐时回落到第一条
    let idx = clients.iter().position(|c| c.email == email).unwrap_or(0);
    let sid = sids.get(idx).or_else(|| sids.first()).cloned().unwrap_or_default();

    let cconf = json!({
        "log": {"loglevel": "error"},
        "inbounds": [{"listen":"127.0.0.1","port":socks_port,"protocol":"socks",
                      "settings":{"udp":false,"auth":"noauth"}}],
        "outbounds": [{
            "protocol": "vless",
            "settings": {"vnext": [{"address":"127.0.0.1","port":conf.port()?,
                          "users":[{"id":client.id,"encryption":"none","flow":client.flow}]}]},
            "streamSettings": {"network":"tcp","security":"reality","realitySettings":{
                "fingerprint":"chrome","serverName":conf.sni()?,
                "publicKey":conf.public_key()?,"shortId":sid,"spiderX":"/"}}
        }]
    });

    let tmp = format!("/tmp/xrayctl-e2e-{}.json", socks_port);
    // 写成当前登录用户可读（临时凭证，用完即删）：否则 600 root 会让 xray/rm 都碰不了
    t.write_file(&tmp, &serde_json::to_string_pretty(&cconf)?, "600", None)?;
    t.ok(&format!("{}chown $(id -u):$(id -g) {}", t.su(), tmp))?;
    let script = format!(
        "set -e; /usr/local/bin/xray run -c {tmp} >/dev/null 2>&1 & P=$!; sleep 2; \
         IP=$(curl -s --max-time 10 -x socks5h://127.0.0.1:{sp} https://api.ipify.org || true); \
         kill $P 2>/dev/null || true; rm -f {tmp}; echo \"$IP\"",
        tmp = tmp,
        sp = socks_port
    );
    let out = t.ok(&script)?;
    let server_ip = public_ip(t)?;
    Ok((out.clone(), out == server_ip))
}

pub fn cmd_selftest(t: &Target, only: Option<&str>) -> Result<()> {
    let conf = ServerConf::load(t, DEFAULT_CFG)?;
    let mut fail = 0;
    for (i, c) in conf.clients().iter().enumerate() {
        if let Some(e) = only {
            if e != c.email {
                continue;
            }
        }
        match e2e(t, &conf, &c.email, 18100 + i as u16) {
            Ok((ip, same)) => {
                if same {
                    println!("  ✓ {:<22} 出口 {}", c.email, ip);
                } else if ip.is_empty() {
                    println!("  ✗ {:<22} 不通（客户端起来但连不上服务端）", c.email);
                    fail += 1;
                } else {
                    println!("  ✗ {:<22} 出口 {} ≠ 服务器 IP", c.email, ip);
                    fail += 1;
                }
            }
            Err(e) => {
                println!("  ✗ {:<22} {}", c.email, e);
                fail += 1;
            }
        }
    }
    if fail > 0 {
        bail!("{} 条凭证自测失败", fail);
    }
    println!("全部通过。");
    Ok(())
}

pub fn cmd_doctor(t: &Target) -> Result<()> {
    println!("== doctor @ {}", t.describe());
    let mut problems: Vec<String> = Vec::new();

    // 1. xray 二进制
    match t.ok("/usr/local/bin/xray version | head -1") {
        Ok(v) => println!("  ✓ xray      {}", v),
        Err(_) => {
            println!("  ✗ xray      未安装或不可执行");
            problems.push("xray 未安装".into());
        }
    }

    // 2. 配置
    match crate::config::test_config(t, DEFAULT_CFG) {
        Ok(_) => println!("  ✓ 配置      Configuration OK"),
        Err(e) => {
            println!("  ✗ 配置      {}", e);
            problems.push("配置无效".into());
            return report(problems);
        }
    }

    // 3. 服务与端口
    let s = status(t)?;
    println!("  {} 服务      {}", if s.active { "✓" } else { "✗" }, if s.active { "active" } else { "未运行" });
    if !s.active {
        problems.push("服务未运行".into());
    }
    println!(
        "  {} 端口      {} 监听 443/实际端口",
        if s.listening_443 { "✓" } else { "✗" },
        if s.listening_443 { "在" } else { "未" }
    );
    if !s.listening_443 {
        problems.push("vless 端口未监听".into());
    }
    if !s.stats_ok {
        println!("  ✗ 统计      不可用（缺 stats/api/policy）");
        problems.push("统计接口不可用".into());
    } else {
        println!("  ✓ 统计      可用");
    }

    // 4. 端到端：用第一条凭证真连一次
    let conf = ServerConf::load(t, DEFAULT_CFG)?;
    if let Some(first) = conf.clients().first() {
        match e2e(t, &conf, &first.email, 18199) {
            Ok((ip, same)) if same => println!("  ✓ 端到端    用 {} 回环出口 {}", first.email, ip),
            Ok((ip, _)) => {
                println!("  ✗ 端到端    用 {} 出口 {}（应为服务器公网 IP）", first.email, ip);
                problems.push("端到端不通".into());
            }
            Err(e) => {
                println!("  ✗ 端到端    {}", e);
                problems.push("端到端执行失败".into());
            }
        }
    }

    // 5. 防火墙/加固（只提示，不改）
    let f2b = t.ok("systemctl is-active fail2ban").unwrap_or_default();
    let ufw = t
        .ok(&format!("{}ufw status | head -1", t.su()))
        .unwrap_or_default();
    println!("  · 加固      fail2ban={} / ufw={}", if f2b == "active" { "active" } else { "未启用" }, ufw);

    report(problems)
}

fn report(problems: Vec<String>) -> Result<()> {
    if problems.is_empty() {
        println!("\n全部检查通过。");
        Ok(())
    } else {
        println!("\n有 {} 项异常：{}", problems.len(), problems.join("、"));
        bail!("doctor 未通过")
    }
}
