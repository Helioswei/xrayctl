//! `xrayctl uninstall`：把装过的东西干净卸掉。
//!
//! 默认**整体卸载**：xrayctl 自己 + xray 服务/配置/账本/xray 用户，
//! 这样"卸了这个工具"就等于"这台机器的代理彻底没了"，不留半截状态。
//! 只想卸工具、留着梯子继续跑，加 `--keep-xray`。
//! 任何情况下不加 `--yes` 都只打印将要做的事。

use anyhow::Result;

use crate::config::{DEFAULT_CFG, API_PORT};
use crate::host::Target;

const BIN: &str = "/usr/local/bin/xrayctl";
const STATE_DIR: &str = "/var/lib/xrayctl";
const DROP_IN: &str = "/etc/systemd/system/xray.service.d/xrayctl.conf";
const TIMER_UNITS: &[&str] = &[
    "xrayctl-traffic.timer",
    "xrayctl-traffic.service",
];

pub fn run(t: &Target, keep_xray: bool, yes: bool) -> Result<()> {
    let mut steps: Vec<String> = Vec::new();

    // 账本定时器与 systemd 覆盖层
    for u in TIMER_UNITS {
        steps.push(format!("systemctl stop {} 2>/dev/null || true", u));
        steps.push(format!("systemctl disable {} 2>/dev/null || true", u));
        steps.push(format!("rm -f /etc/systemd/system/{}", u));
    }
    steps.push(format!("rm -f {}", DROP_IN));
    steps.push("rmdir /etc/systemd/system/xray.service.d 2>/dev/null || true".into());
    steps.push("systemctl daemon-reload".into());
    steps.push(format!("rm -f {}", BIN));

    if !keep_xray {
        steps.push("systemctl stop xray 2>/dev/null || true".into());
        steps.push("systemctl disable xray 2>/dev/null || true".into());
        steps.push("rm -f /etc/systemd/system/xray.service".into());
        steps.push("systemctl daemon-reload".into());
        steps.push(format!("rm -rf {}", DEFAULT_CFG.replace("/config.json", "")));
        steps.push("rm -f /usr/local/bin/xray /usr/local/bin/xray.* 2>/dev/null || true".into());
        steps.push(format!("rm -rf {}", STATE_DIR));
        steps.push("userdel xray 2>/dev/null || true".into());
        steps.push("systemctl reset-failed xray 2>/dev/null || true".into());
    }

    println!("== 将要执行");
    println!("   目标 {}", t.describe());
    if keep_xray {
        println!("   模式 --keep-xray：只卸 xrayctl，保留 xray 服务与配置（梯子继续跑）");
    } else {
        println!("   模式 默认：整体卸载（xrayctl + xray 服务/配置/账本/xray 用户）");
    }
    for s in &steps {
        println!("   · {}", s);
    }

    if !yes {
        println!("\n以上为预演。确认执行请加 --yes{}", if keep_xray { "" } else { "（不可逆）" });
        return Ok(());
    }

    for s in &steps {
        let _ = t.run(&format!("{}sh -c {}", t.su(), shq(s)));
    }
    println!("\n== 已卸载");

    // 验收：别嘴上说卸了
    let left = t
        .run(&format!("{}sh -c 'ls {} 2>/dev/null; systemctl is-active xray 2>/dev/null; ls /usr/local/bin/xray 2>/dev/null'", t.su(), BIN))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    if left.contains("xrayctl") {
        println!("!! {} 还在，检查权限", BIN);
    } else {
        println!("   {} 已移除", BIN);
    }
    if keep_xray {
        println!("   xray 状态：{}（保留）", if left.contains("active") { "active" } else { "未运行" });
    } else {
        let gone = !left.contains("active") && !left.contains("/usr/local/bin/xray");
        println!("   xray 状态：{}", if gone { "已停、二进制已删" } else { "仍在（检查）" });
    }
    let _ = API_PORT; // 提示用：整体卸载后 10085 也应释放
    println!("\n   本地那条命令在 `install.sh --uninstall` 里");
    Ok(())
}

fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'\''"#))
}
