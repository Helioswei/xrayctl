//! 加固：fail2ban（SSH 撞库）+ ufw（只放行 ssh 端口和 vless 端口）。
//! 只做这两件，不动 SSH 端口/密码登录策略——那是使用者的选择，不是脚本该替他决定的。

use crate::config::{ServerConf, DEFAULT_CFG};
use crate::host::Target;
use anyhow::{bail, Result};

pub fn run(t: &Target, ssh_port: u16, extra: &[u16]) -> Result<()> {
    let port = ServerConf::load(t, DEFAULT_CFG)
        .map(|c| c.port())
        .unwrap_or(Ok(443))?;

    let jail = r#"[DEFAULT]
backend = systemd
bantime = 1h
findtime = 10m
maxretry = 5
bantime.increment = true
bantime.factor = 2
bantime.maxtime = 1w

[sshd]
enabled = true
"#;
    t.write_file("/tmp/xrayctl-jail.local", jail, "600", Some("root:root"))?;

    let script = format!(
        r#"set -e
{su}apt-get update -qq
{su}DEBIAN_FRONTEND=noninteractive apt-get install -y -qq fail2ban ufw
{su}cp /tmp/xrayctl-jail.local /etc/fail2ban/jail.local
{su}rm -f /tmp/xrayctl-jail.local
{su}systemctl enable --now fail2ban
{su}systemctl restart fail2ban
sleep 1
{su}ufw --force default deny incoming
{su}ufw --force default allow outgoing
{su}ufw allow {ssh}/tcp
{su}ufw allow {port}/tcp
{extras}{su}ufw --force enable
echo "--- 结果 ---"
echo "fail2ban: $(systemctl is-active fail2ban)"
{su}fail2ban-client status sshd | sed -n '2,6p'
{su}ufw status | head -6"#,
        su = t.su(),
        ssh = ssh_port,
        port = port,
        extras = extra
            .iter()
            .map(|p| format!("{}ufw allow {}/tcp\n", t.su(), p))
            .collect::<String>()
    );
    let out = t.run(&script)?;
    let all = format!("{}{}", out.stdout, out.stderr);
    println!("{}", all.trim());
    if out.code != 0 {
        bail!("加固失败（exit {}）", out.code);
    }

    // 加固后业务必须还通：用第一条凭证回环验一次
    let Ok(conf) = ServerConf::load(t, DEFAULT_CFG) else {
        println!("\n（这台机器没有 xray 服务端配置，跳过回环自测）");
        return Ok(());
    };
    if let Some(c) = conf.clients().first() {
        match crate::checks::e2e(t, &conf, &c.email, 18188) {
            Ok((ip, same)) if same => println!("\n加固后连通性正常（出口 {}）", ip),
            Ok((ip, _)) => bail!("加固后不通了：出口 {}", ip),
            Err(e) => bail!("加固后自测失败：{}", e),
        }
    }
    Ok(())
}
