//! 生成云厂商「安全组导入文件」，省得在控制台手敲 443。
//!
//! 云安全组是第二道墙：主机 ufw 放行了，控制台不放行，外面照样连不上。
//! 各家的导入模板列头不一样，这里只做**已经拿到官方模板确认过的那种**，
//! 别的厂商明确告诉你「不支持」，不猜格式 —— 猜错的文件导进控制台会把现有规则覆盖掉。

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub struct RuleOpts {
    pub provider: String,
    pub port: u16,
    pub ssh_port: Option<u16>,
    pub ipv6: bool,
    pub out_dir: String,
}

/// 腾讯云模板列头（来自控制台下载的 firewallRulesTemplate*.csv，UTF-8 BOM + CRLF）
const TENCENT_HEADER: &str = "来源,协议,端口,策略,备注";

pub fn generate(o: &RuleOpts) -> Result<PathBuf> {
    let provider = o.provider.to_ascii_lowercase();
    if provider != "tencent" && provider != "tx" {
        bail!(
            "目前只内置腾讯云的模板列头（来源,协议,端口,策略,备注）。\n\
             {} 的导入格式没有官方模板核对过，不猜 —— 手加一条：入站 TCP:{} 来源 0.0.0.0/0",
            o.provider,
            o.port
        );
    }

    let mut rows: Vec<String> = Vec::new();
    // 代理端口：主规则
    rows.push(format!(
        "0.0.0.0/0,TCP,{},允许,xrayctl 代理",
        o.port
    ));
    if o.ipv6 {
        rows.push(format!("::/0,TCP,{},允许,xrayctl 代理 v6", o.port));
    }
    // SSH 端口：导入是覆盖语义时，必须把自己的登录入口一起写进去
    if let Some(sp) = o.ssh_port {
        rows.push(format!("0.0.0.0/0,TCP,{},允许,SSH 登录", sp));
        if o.ipv6 {
            rows.push(format!("::/0,TCP,{},允许,SSH 登录 v6", sp));
        }
    }

    // 腾讯云：UTF-8 BOM + CRLF，端口多个时用英文引号包住
    let mut csv = String::from("\u{feff}");
    csv.push_str(TENCENT_HEADER);
    csv.push_str("\r\n");
    for r in &rows {
        csv.push_str(r);
        csv.push_str("\r\n");
    }

    let dir = Path::new(&o.out_dir);
    std::fs::create_dir_all(dir).with_context(|| format!("建不了目录 {}", dir.display()))?;
    let path = dir.join(format!("tencent-sg-{}.csv", o.port));
    std::fs::write(&path, csv).with_context(|| format!("写不了 {}", path.display()))?;
    Ok(path)
}

/// 打印导入指引（追加 / 覆盖两种方式都说清楚）
pub fn explain(path: &Path, ssh_port: Option<u16>) {
    println!("\n== 云安全组（控制台操作，工具替不了）");
    println!("   文件 {}", path.display());
    println!("   路径 控制台 → 安全组 → 选中实例用的安全组 → 入站规则 → 导入规则");
    println!("   方式 选「追加导入」：只加这一条，不动现有规则（推荐）");
    println!("        选「覆盖导入」：文件里的规则会替换掉现有全部入站规则");
    if ssh_port.is_some() {
        println!("        ——所以这个文件里已经带上了 SSH 端口，覆盖导入也不会把自己锁在外面");
    } else {
        println!("        ——覆盖导入前先确认文件里有你的 SSH 端口，否则会把自己锁在外面");
    }
    println!("   验完  本机执行： nc -z -G 5 -w 5 <公网IP> 443（macOS 的 nc 要 -G，Linux 用 -w 5）");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tencent_csv_has_bom_header_and_rows() {
        let dir = std::env::temp_dir().join(format!("xrayctl-fw-{}", std::process::id()));
        let p = generate(&RuleOpts {
            provider: "tencent".into(),
            port: 443,
            ssh_port: Some(22),
            ipv6: false,
            out_dir: dir.to_string_lossy().to_string(),
        })
        .unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.starts_with('\u{feff}'), "必须带 BOM");
        assert!(s.contains("来源,协议,端口,策略,备注"));
        assert!(s.contains("\r\n"), "腾讯云模板是 CRLF");
        assert!(s.contains("0.0.0.0/0,TCP,443,允许"));
        assert!(s.contains("0.0.0.0/0,TCP,22,允许"), "SSH 端口要一并写入");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_provider_is_refused() {
        let r = generate(&RuleOpts {
            provider: "aliyun".into(),
            port: 443,
            ssh_port: None,
            ipv6: false,
            out_dir: "/tmp/xrayctl-should-not-exist".into(),
        });
        assert!(r.is_err());
    }
}
