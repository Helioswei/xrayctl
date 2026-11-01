//! `xrayctl uninstall`：把装过的东西干净卸掉。
//!
//! 默认只卸 **xrayctl 自己**（二进制 + 账本定时器 + systemd 覆盖层），
//! 不碰 xray —— 少一个"想卸工具结果把梯子删了"的事故面。
//! 要连 xray 一起清，加 `--purge`，并且必须再加 `--yes` 才真的执行。

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

pub fn run(t: &Target, purge: bool, yes: bool) -> Result<()> {
    let mut steps: Vec<String> = Vec::new();
    for u in TIMER_UNITS {
        steps.push(format!("systemctl stop {} 2>/dev/null || true", u));
        steps.push(format!("systemctl disable {} 2>/dev/null || true", u));
        steps.push(format!("rm -f /etc/systemd/system/{}", u));
    }
    steps.push(format!("rm -f {}", DROP_IN));
    steps.push("rmdir /etc/systemd/system/xray.service.d 2>/dev/null || true".into());
    steps.push("systemctl daemon-reload".into());
    steps.push(format!("rm -f {}", BIN));

    if purge {
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
    if purge {
        println!("   模式 purge：连 xray 服务、配置、账本、xray 用户一起删");
    } else {
        println!("   模式 默认：只卸 xrayctl（保留 xray 与配置，梯子照常跑）");
    }
    for s in &steps {
        println!("   · {}", s);
    }

    if !yes {
        println!("\n以上为预演。确认执行请加 --yes{}", if purge { "（--purge 不可逆）" } else { "" });
        return Ok(());
    }

    for s in &steps {
        let _ = t.run(&format!("{}sh -c {}", t.su(), shq(s)));
    }
    println!("\n== 已卸载");

    // 验收：别嘴上说卸了
    let left = t
        .run(&format!("{}sh -c 'ls {} 2>/dev/null; systemctl is-active xray 2>/dev/null'", t.su(), BIN))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    if left.contains("xrayctl") {
        println!("!! {} 还在，检查权限", BIN);
    } else {
        println!("   {} 已移除", BIN);
    }
    if purge {
        println!("   xray 状态：{}", if left.contains("active") { "仍在跑（检查）" } else { "已停" });
    } else {
        println!("   xray 状态：{}（保留）", if left.contains("active") { "active" } else { "未运行" });
    }
    let _ = API_PORT; // 提示用：purge 后 10085 也应释放
    println!("\n   别忘：本地那条命令也在 `install.sh --uninstall` 里能卸");
    Ok(())
}

fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'"'"'"#))
}
