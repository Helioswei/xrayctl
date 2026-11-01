//! `client sync`：把服务端现状渲染成本机客户端配置。
//!
//! 解决的问题：`deploy` 生成凭证后只写服务端，客户端那几个字段要人手抄，
//! 抄错一个的症状（REALITY authentication failed）和服务端配置错、版本不兼容
//! 完全一样，排查成本极高。这里改成一条命令生成 + 自检 + 落盘。
//!
//! 产出（默认写到 ./dist）：
//!   clash-profile.yaml   Clash Verge 可直接导入的整份配置
//!   links.txt            每个客户端一条 vless 链接
//!   phone-import.txt     手机那条链接
//!   phone-qr.png         手机二维码（需要 python3 + qrcode）

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::{self, Client, Conn, ServerConf};
use crate::host::Target;

pub struct SyncOpts {
    /// 用哪个客户端生成主 profile（默认取第一个，通常选 --client 指定本机那个）
    pub client: Option<String>,
    /// 输出目录，默认 ./dist
    pub out_dir: String,
    /// 展示名，默认 "<地域>-REALITY"
    pub label: Option<String>,
    /// Clash Verge 的 profiles 目录；给 "auto" 则自动探测
    pub clash_dir: Option<String>,
    /// 顺便生成手机二维码
    pub qr: bool,
}

pub fn run(t: &Target, o: &SyncOpts) -> Result<()> {
    let conf = ServerConf::load(t, config::DEFAULT_CFG)?;
    let ip = config::public_ip(t)?;
    let port = conf.port()?;
    let sni = conf.sni()?;
    let pubkey = conf.public_key()?;
    let clients = conf.clients();
    let sids = conf.short_ids();
    if clients.is_empty() {
        bail!("服务端没有客户端，先 vpsctl user add");
    }
    if sids.is_empty() {
        bail!("服务端没有 shortId");
    }

    let label_root = o.label.clone().unwrap_or_else(|| "VPS-REALITY".to_string());

    // 主 profile 用哪个客户端：指定 > mac > 第一个
    let main = match &o.client {
        Some(name) => clients
            .iter()
            .find(|c| &c.email == name)
            .with_context(|| format!("服务端没有名为 {} 的客户端", name))?,
        None => clients
            .iter()
            .find(|c| c.email == "mac")
            .unwrap_or(&clients[0]),
    };
    let main_sid = sid_for(&clients, &sids, &main.email)?;

    let out = PathBuf::from(expand(&o.out_dir));
    std::fs::create_dir_all(&out).with_context(|| format!("建不了目录 {}", out.display()))?;

    // 1) Clash profile
    let clash = render_clash(&ClashArgs {
        name: &format!("{}-{}", label_root, main.email),
        host: &ip,
        port,
        client: main,
        public_key: &pubkey,
        short_id: &main_sid,
        sni: &sni,
        group: &label_root,
    });
    let clash_path = out.join("clash-profile.yaml");
    std::fs::write(&clash_path, &clash)?;
    self_check(&clash, main, &ip, &pubkey, &main_sid)?;
    println!("✓ {}", clash_path.display());

    // 2) 全部客户端链接
    let mut links = String::new();
    for (i, c) in clients.iter().enumerate() {
        let sid = sids.get(i).cloned().unwrap_or_default();
        let conn = Conn {
            host: ip.clone(),
            port,
            id: c.id.clone(),
            public_key: pubkey.clone(),
            short_id: sid,
            sni: sni.clone(),
            flow: c.flow.clone(),
        };
        links.push_str(&format!(
            "{}#{}@{}\n",
            conn.vless_link(&c.email),
            c.email,
            ip
        ));
    }
    let links_path = out.join("links.txt");
    std::fs::write(&links_path, &links)?;
    println!("✓ {}", links_path.display());

    // 3) 手机那条
    if let Some(phone) = clients.iter().find(|c| c.email == "phone") {
        let sid = sid_for(&clients, &sids, &phone.email)?;
        let conn = Conn {
            host: ip.clone(),
            port,
            id: phone.id.clone(),
            public_key: pubkey.clone(),
            short_id: sid.clone(),
            sni: sni.clone(),
            flow: phone.flow.clone(),
        };
        let link = conn.vless_link("phone");
        let p = out.join("phone-import.txt");
        std::fs::write(&p, &link)?;
        println!("✓ {}", p.display());

        if o.qr {
            let png = out.join("phone-qr.png");
            match make_qr(&link, &png) {
                Ok(_) => println!("✓ {}", png.display()),
                Err(e) => println!("· 二维码跳过：{}", e),
            }
        }
    }

    // 4) 写进 Clash Verge：优先覆盖「当前选中的那个 profile」
    match clash_dir(o.clash_dir.as_deref()) {
        Some(dir) => {
            let target = current_profile(&dir).unwrap_or_else(|| dir.join(format!("vpsctl-{}.yaml", ip)));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            // 覆盖前留备份，别把用户手调过的配置弄丢
            if target.exists() {
                let bak = target.with_extension(format!("yaml.bak-{}", now_stamp()));
                let _ = std::fs::copy(&target, &bak);
            }
            std::fs::copy(&clash_path, &target)
                .with_context(|| format!("写不进 {}", target.display()))?;
            println!("✓ 已写入 Clash Verge：{}", target.display());
            println!("   Verge 里点一下当前配置「刷新/重载」即生效（旧文件已留 .bak）");
        }
        None => println!("· 没找到 Clash Verge 目录，只写了仓库里那份（用 --clash-dir 指定）"),
    }

    println!("\n== 结果");
    println!("   服务端 {}:{}  客户端 {}  公钥 {}…", ip, port, main.email, &pubkey[..8.min(pubkey.len())]);
    println!("   其余设备：`vpsctl --host <那台机器> proxy install --from <本机别名>`；手机扫 phone-qr.png");
    Ok(())
}

struct ClashArgs<'a> {
    name: &'a str,
    host: &'a str,
    port: u16,
    client: &'a Client,
    public_key: &'a str,
    short_id: &'a str,
    sni: &'a str,
    group: &'a str,
}

/// 渲染整份 Clash 配置。字段顺序与手工维护的 profile 保持一致，
/// 这样 diff 一眼能看出差异（support-x25519mlkem768 必须放在 reality-opts 里）。
fn render_clash(a: &ClashArgs) -> String {
    format!(
        r#"mixed-port: 7897
allow-lan: false
mode: rule
log-level: info
ipv6: false

dns:
  enable: true
  ipv6: false
  enhanced-mode: fake-ip
  fake-ip-range: 198.18.0.1/16
  nameserver:
    - https://doh.pub/dns-query
    - https://dns.alidns.com/dns-query
  proxy-server-nameserver:
    - https://doh.pub/dns-query
    - https://dns.alidns.com/dns-query

proxies:
  - name: "{name}"
    type: vless
    server: {host}
    port: {port}
    uuid: {uuid}
    network: tcp
    udp: true
    tls: true
    flow: {flow}
    servername: {sni}
    client-fingerprint: chrome
    reality-opts:
      public-key: {pbk}
      support-x25519mlkem768: true
      short-id: {sid}

proxy-groups:
  - name: {group}
    type: select
    proxies:
      - "{name}"
      - DIRECT

rules:
  - GEOIP,LAN,DIRECT,no-resolve
  - GEOSITE,cn,DIRECT
  - GEOIP,CN,DIRECT
  - MATCH,{group}
"#,
        name = a.name,
        host = a.host,
        port = a.port,
        uuid = a.client.id,
        flow = a.client.flow,
        sni = a.sni,
        pbk = a.public_key,
        sid = a.short_id,
        group = a.group,
    )
}

/// 自检：渲染结果必须包含全部关键字段，缺一个就直接报错（不要写出去）。
fn self_check(yaml: &str, c: &Client, ip: &str, pbk: &str, sid: &str) -> Result<()> {
    let need = [
        ("server", ip),
        ("uuid", &c.id),
        ("public-key", pbk),
        ("short-id", sid),
        ("support-x25519mlkem768", "true"),
        ("flow", &c.flow),
    ];
    for (field, val) in need {
        if !yaml.contains(val) {
            bail!("渲染自检失败：{} 里找不到 {}", field, val);
        }
    }
    if !yaml.contains("security") && !yaml.contains("reality-opts") {
        bail!("渲染自检失败：没有 reality-opts");
    }
    Ok(())
}

fn sid_for(clients: &[Client], sids: &[String], email: &str) -> Result<String> {
    let idx = clients
        .iter()
        .position(|c| c.email == email)
        .with_context(|| format!("客户端 {} 不存在", email))?;
    sids.get(idx)
        .cloned()
        .with_context(|| format!("shortId 数量({})少于客户端位置({})", sids.len(), idx))
}

fn make_qr(link: &str, png: &Path) -> Result<()> {
    let script = format!(
        "import qrcode,sys\nimg=qrcode.make(sys.argv[1])\nimg.save(sys.argv[2])\n"
    );
    let st = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(link)
        .arg(png)
        .status()
        .context("需要 python3 + qrcode（pip install qrcode[pil]）")?;
    if !st.success() {
        bail!("qrcode 生成失败");
    }
    Ok(())
}

/// Clash Verge 的 profile 目录：显式给 > macOS 默认
fn clash_dir(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(d) = explicit {
        if d != "auto" {
            return Some(PathBuf::from(expand(d)));
        }
    }
    let home = std::env::var("HOME").ok()?;
    let mac = PathBuf::from(home)
        .join("Library/Application Support/io.github.clash-verge-rev.clash-verge-rev/profiles");
    if mac.is_dir() {
        return Some(mac);
    }
    None
}

fn expand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home, rest);
        }
    }
    p.to_string()
}

/// 找出 Clash Verge 当前选中的 profile 文件（profiles.yaml 里的 current: <uid>）
fn current_profile(dir: &Path) -> Option<PathBuf> {
    // profiles.yaml 在上一级目录
    let meta = dir.parent()?.join("profiles.yaml");
    let txt = std::fs::read_to_string(meta).ok()?;
    let uid = txt
        .lines()
        .find_map(|l| l.trim().strip_prefix("current:").map(|v| v.trim().to_string()))?;
    if uid.is_empty() {
        return None;
    }
    let f = dir.join(format!("{}.yaml", uid));
    if f.exists() { Some(f) } else { None }
}

fn now_stamp() -> String {
    // 不引 date 库：直接从系统时间算个足够用的时间戳
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> &'static Client {
        Box::leak(Box::new(Client {
            email: "mac".into(),
            id: "11111111-2222-3333-4444-555555555555".into(),
            flow: "xtls-rprx-vision".into(),
        }))
    }

    fn args(c: &'static Client) -> ClashArgs<'static> {
        ClashArgs {
            name: "VPS-REALITY-mac",
            host: "203.0.113.9",
            port: 443,
            client: c,
            public_key: "PBKPBKPBK",
            short_id: "abcdef0123456789",
            sni: "www.cloudflare.com",
            group: "VPS-REALITY",
        }
    }

    #[test]
    fn clash_has_all_fields() {
        let c = client();
        let a = args(c);
        let y = render_clash(&a);
        assert!(self_check(&y, c, a.host, a.public_key, a.short_id).is_ok());
        // flag 必须在 reality-opts 段内，不能飘到 proxy 顶层
        let ro = y.split("reality-opts:").nth(1).unwrap();
        assert!(ro.contains("support-x25519mlkem768: true"));
        assert!(y.contains("short-id: abcdef0123456789"));
        assert!(y.contains("flow: xtls-rprx-vision"));
    }

    #[test]
    fn self_check_catches_typo() {
        let c = client();
        let a = args(c);
        let y = render_clash(&a).replace("PBKPBKPBK", "WRONG");
        assert!(self_check(&y, c, a.host, a.public_key, a.short_id).is_err());
    }
}
