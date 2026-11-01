//! 按客户端的流量：查询、跨重启累计账本、开机/定时记账的自动安装。
//!
//! 为什么要自己记账：xray 的统计在内存里，API 只提供查询、没有写入/恢复接口，
//! 重启即清零。所以只能在外面按「启动周期内增量」累加。两个触发点：
//!   · systemd ExecStop（停服/重启前抓最后一份，那时 API 还活着）
//!   · systemd timer（每小时，防崩溃/断电）

use crate::host::Target;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const STATE_PATH: &str = "/var/lib/vpsctl/traffic.json";

#[derive(Default, Clone, Copy)]
pub struct Bytes {
    pub up: u64,
    pub down: u64,
}

impl Bytes {
    fn total(&self) -> u64 {
        self.up + self.down
    }
}

pub fn query(t: &Target) -> Result<BTreeMap<String, Bytes>> {
    let script = format!(
        "/usr/local/bin/xray api statsquery --server=127.0.0.1:{} -pattern 'user>>>' 2>/dev/null",
        crate::config::API_PORT
    );
    let out = t.run(&script)?;
    if out.code != 0 || out.stdout.trim().is_empty() {
        bail!(
            "查询流量失败：{}（检查 xray 是否在跑、配置里有没有 stats/api/policy 三件套）",
            if out.stderr.trim().is_empty() {
                "无输出"
            } else {
                out.stderr.trim()
            }
        );
    }
    let v: Value = serde_json::from_str(&out.stdout).context("statsquery 返回的不是 JSON")?;
    let mut map: BTreeMap<String, Bytes> = BTreeMap::new();
    for s in v.get("stat").and_then(|s| s.as_array()).unwrap_or(&vec![]) {
        let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let parts: Vec<&str> = name.split(">>>").collect();
        if parts.len() != 4 || parts[0] != "user" {
            continue;
        }
        let val = s.get("value").and_then(|v| v.as_u64()).unwrap_or(0);
        let e = map.entry(parts[1].to_string()).or_default();
        match parts[3] {
            "uplink" => e.up = val,
            "downlink" => e.down = val,
            _ => {}
        }
    }
    Ok(map)
}

pub fn boot_stamp(t: &Target) -> String {
    t.ok("systemctl show xray -p ActiveEnterTimestamp --value")
        .unwrap_or_else(|_| "unknown".into())
}

/// 运行时长：用「系统 uptime - 服务的 monotonic 启动点」算，避开时区解析坑
/// （systemctl 的 ActiveEnterTimestamp 带 CST，被 date -d 当成美国中部时间，实测差 -820 分钟）
pub fn uptime_minutes(t: &Target) -> Option<i64> {
    let mono: i64 = t
        .ok("systemctl show xray -p ActiveEnterTimestampMonotonic --value")
        .ok()?
        .parse()
        .ok()?;
    if mono == 0 {
        return None;
    }
    let sys_up: f64 = t.ok("cut -d' ' -f1 /proc/uptime").ok()?.parse().ok()?;
    let mins = ((sys_up * 1_000_000.0) as i64 - mono) / 60_000_000;
    if mins < 0 {
        None
    } else {
        Some(mins)
    }
}

pub fn human(n: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < units.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", n)
    } else {
        format!("{:.2} {}", v, units[unit])
    }
}

fn load_state(t: &Target, path: &str) -> Value {
    let txt = t
        .ok(&format!("{}cat {} 2>/dev/null", t.su(), path))
        .unwrap_or_default();
    serde_json::from_str(&txt).unwrap_or_else(|_| json!({"boot": null, "totals": {}, "last": {}}))
}

fn save_state(t: &Target, path: &str, state: &Value) -> Result<()> {
    let dir = std::path::Path::new(path)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/var/lib".into());
    let txt = serde_json::to_string_pretty(state)?;
    // 目录与文件都归 xray：systemd 的 ExecStop 是以 User=xray 跑的，
    // 归 root 会导致停服时写不进去（旧 python 版本就踩过 Permission denied）
    t.ok(&format!("{}install -d -m 700 -o xray -g xray {}", t.su(), dir))?;
    t.write_file(path, &txt, "600", Some("xray:xray"))
        .with_context(|| format!("写账本 {} 失败", path))
}

/// 增量累加：以已有 totals 为基础，绝不因为"本轮某个客户端没流量"而清零
pub fn accumulate(t: &Target, state_path: &str) -> Result<(BTreeMap<String, Bytes>, Value)> {
    let cur = query(t)?;
    let boot = boot_stamp(t);
    let mut state = load_state(t, state_path);

    let same_boot = state.get("boot").and_then(|b| b.as_str()) == Some(boot.as_str());
    if !same_boot {
        state["boot"] = json!(boot);
        state["last"] = json!({});
    }

    // 以已有账本为基础（关键：不能写 state["totals"] = 本轮可见的客户端）
    let mut totals: BTreeMap<String, Bytes> = BTreeMap::new();
    if let Some(obj) = state.get("totals").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            totals.insert(
                k.clone(),
                Bytes {
                    up: v.get("up").and_then(|x| x.as_u64()).unwrap_or(0),
                    down: v.get("down").and_then(|x| x.as_u64()).unwrap_or(0),
                },
            );
        }
    }

    let mut last_json = state.get("last").cloned().unwrap_or_else(|| json!({}));
    for (email, cur_v) in &cur {
        let prev = last_json.get(email);
        let prev_up = prev.and_then(|p| p.get("up")).and_then(|x| x.as_u64()).unwrap_or(0);
        let prev_dn = prev.and_then(|p| p.get("down")).and_then(|x| x.as_u64()).unwrap_or(0);
        let d_up = cur_v.up.saturating_sub(prev_up);
        let d_dn = cur_v.down.saturating_sub(prev_dn);
        let e = totals.entry(email.clone()).or_default();
        e.up += d_up;
        e.down += d_dn;
        last_json[email] = json!({"up": cur_v.up, "down": cur_v.down});
    }

    let mut totals_json = serde_json::Map::new();
    for (k, v) in &totals {
        totals_json.insert(k.clone(), json!({"up": v.up, "down": v.down}));
    }
    let stamp = t.ok("date '+%Y-%m-%d %H:%M:%S'").unwrap_or_default();
    let new_state = json!({
        "boot": state.get("boot").cloned().unwrap_or(json!(null)),
        "totals": totals_json,
        "last": last_json,
        "updated": stamp
    });
    save_state(t, state_path, &new_state)?;
    Ok((totals, new_state))
}

pub fn print_table(data: &BTreeMap<String, Bytes>, footer: &str) {
    let total_all: u64 = data.values().map(|b| b.total()).sum();
    let denom = if total_all == 0 { 1 } else { total_all };
    println!();
    println!("{:<22} {:>12} {:>12} {:>12} {:>7}", "客户端", "上传", "下载", "合计", "占比");
    println!("{}", "-".repeat(70));
    let mut rows: Vec<_> = data.iter().collect();
    rows.sort_by_key(|(_, b)| std::cmp::Reverse(b.total()));
    for (email, b) in &rows {
        println!(
            "{:<22} {:>12} {:>12} {:>12} {:>6.1}%",
            truncate(email, 22),
            human(b.up),
            human(b.down),
            human(b.total()),
            100.0 * b.total() as f64 / denom as f64
        );
    }
    println!("{}", "-".repeat(70));
    println!(
        "{:<22} {:>12} {:>12} {:>12}",
        "合计",
        human(data.values().map(|b| b.up).sum()),
        human(data.values().map(|b| b.down).sum()),
        human(total_all)
    );
    if !footer.is_empty() {
        println!("\n{}", footer);
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}

/// 装 systemd 记账：ExecStop 抓最后一份 + 每小时一次
pub fn enable_accounting(t: &Target, bin: &str) -> Result<()> {
    let bin_check = t.run(&format!("test -x {}", bin))?;
    if bin_check.code != 0 {
        bail!("{} 不存在或不可执行（先把 linux 版 vpsctl 装上去）", bin);
    }
    let drop_dir = "/etc/systemd/system/xray.service.d";
    t.ok(&format!("{}mkdir -p {}", t.su(), drop_dir))?;
    t.write_file(
        &format!("{}/traffic-accounting.conf", drop_dir),
        &format!(
            "[Service]\n# 停服前抓最后一份（此时 API 还活着）；失败不阻塞停服\nExecStop=-{} --local traffic --accumulate --quiet --state {}\n",
            bin, STATE_PATH
        ),
        "644",
        None,
    )?;
    t.write_file(
        "/etc/systemd/system/vpsctl-traffic.service",
        &format!(
            "[Unit]\nDescription=vpsctl 流量账本快照\nAfter=xray.service\n\n[Service]\nType=oneshot\nExecStart={} --local traffic --accumulate --quiet --state {}\n",
            bin, STATE_PATH
        ),
        "644",
        None,
    )?;
    t.write_file(
        "/etc/systemd/system/vpsctl-traffic.timer",
        "[Unit]\nDescription=每小时把 xray 客户端流量记进账本\n\n[Timer]\nOnCalendar=hourly\nAccuracySec=1min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n",
        "644",
        None,
    )?;
    t.ok(&format!("{}systemctl daemon-reload", t.su()))?;
    t.ok(&format!(
        "{}systemctl enable --now vpsctl-traffic.timer",
        t.su()
    ))?;
    println!("已启用账本自动保存：");
    println!("  · xray 停止/重启前（ExecStop drop-in）");
    println!("  · 每小时一次（vpsctl-traffic.timer）");
    println!("  状态文件：{}", STATE_PATH);
    Ok(())
}
