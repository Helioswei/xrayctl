//! 客户端管理：列表 / 新增 / 删除，以及把改好的配置安全地推上去。
//!
//! 推配置的固定动作：备份 → 写临时文件 → `xray run -test` → install（属主 xray）→ 重启 → 检查；
//! 任何一步失败就回滚备份。配置文件属主必须是 xray:xray，否则服务 permission denied（踩过）。

use crate::config::{ServerConf, DEFAULT_CFG};
use crate::host::Target;
use anyhow::{bail, Result};

pub fn cmd_list(t: &Target) -> Result<()> {
    let conf = ServerConf::load(t, DEFAULT_CFG)?;
    let sids = conf.short_ids();
    let clients = conf.clients();
    println!("{:<22} {:<38} {:<18}", "email", "uuid", "shortId");
    println!("{}", "-".repeat(80));
    for (i, c) in clients.iter().enumerate() {
        println!(
            "{:<22} {:<38} {:<18}",
            c.email,
            c.id,
            sids.get(i).cloned().unwrap_or_else(|| "-".into())
        );
    }
    println!("\n共 {} 个客户端", clients.len());
    Ok(())
}

pub fn cmd_add(t: &Target, email: &str, selfcheck: bool) -> Result<()> {
    let mut conf = ServerConf::load(t, DEFAULT_CFG)?;
    let (id, sid) = conf.add_client(email)?;
    apply(t, &conf)?;
    println!("已添加 {}：uuid={} shortId={}", email, id, sid);

    let ip = crate::config::public_ip(t)?;
    let conn = crate::config::Conn {
        host: ip,
        port: conf.port()?,
        id,
        public_key: conf.public_key()?,
        short_id: sid,
        sni: conf.sni()?,
        flow: "xtls-rprx-vision".into(),
    };
    println!("\n{}", conn.vless_link(email));

    if selfcheck {
        let conf2 = ServerConf::load(t, DEFAULT_CFG)?;
        match crate::checks::e2e(t, &conf2, email, 18177) {
            Ok((out_ip, same)) if same => println!("\n自测通过：出口 {}", out_ip),
            Ok((out_ip, _)) => bail!("自测失败：出口 {}（应为服务器 IP）", out_ip),
            Err(e) => bail!("自测执行失败：{}", e),
        }
    }
    Ok(())
}

pub fn cmd_rm(t: &Target, email: &str) -> Result<()> {
    let mut conf = ServerConf::load(t, DEFAULT_CFG)?;
    conf.remove_client(email)?;
    apply(t, &conf)?;
    println!("已删除 {}", email);
    Ok(())
}

/// 把内存里的配置写回服务器，带备份、校验、重启、回滚
pub fn apply(t: &Target, conf: &ServerConf) -> Result<()> {
    let stamp = t.ok("date +%Y%m%d-%H%M%S")?;
    let backup = format!("{}.bak-{}", conf.path, stamp);
    t.ok(&format!("{}cp {} {}", t.su(), conf.path, backup))?;

    let tmp = "/tmp/xrayctl-config.json";
    t.write_file(tmp, &conf.to_pretty(), "600", Some("root:root"))?;

    if let Err(e) = crate::config::test_config(t, tmp) {
        let _ = t.ok(&format!("{}rm -f {}", t.su(), tmp));
        bail!("新配置没通过校验，未写入（原配置未动）：\n{}", e);
    }

    t.ok(&format!(
        "{}install -m 600 -o xray -g xray {} {}",
        t.su(),
        tmp,
        conf.path
    ))?;
    t.ok(&format!("{}rm -f {}", t.su(), tmp))?;

    let restart = t.run(&format!("{}systemctl restart xray", t.su()))?;
    if restart.code != 0 {
        rollback(t, &backup, &conf.path)?;
        bail!("重启失败，已回滚到 {}：{}", backup, restart.stderr.trim());
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    let state = t.ok("systemctl is-active xray").unwrap_or_default();
    if state != "active" {
        rollback(t, &backup, &conf.path)?;
        bail!("重启后服务不是 active（{}），已回滚到 {}", state, backup);
    }
    Ok(())
}

fn rollback(t: &Target, backup: &str, path: &str) -> Result<()> {
    t.ok(&format!("{}install -m 600 -o xray -g xray {} {}", t.su(), backup, path))?;
    let _ = t.run(&format!("{}systemctl restart xray", t.su()));
    Ok(())
}
