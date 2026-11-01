//! 执行目标：本机（--local）或通过 ssh 的远端服务器。
//!
//! 设计取舍：不引入 SSH 库，直接调系统 `ssh`（Linux/macOS 自带，Windows 10+ 也有 OpenSSH 客户端），
//! 这样 ~/.ssh/config 里的别名、密钥、agent、ProxyJump 全部直接复用，也不用担心和 libssh2 的版本差异。

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone)]
pub struct Target {
    pub host: Option<String>,
    pub sudo: bool,
}

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Target {
    pub fn new(host: Option<String>, local: bool, sudo: bool) -> Result<Self> {
        if local && host.is_some() {
            bail!("--local 和 --host 只能选一个");
        }
        if local && cfg!(windows) {
            bail!("Windows 上不支持 --local（服务器端操作依赖 systemd，只能在 Linux 上跑）");
        }
        // 没给 --host：如果本机就是 Linux，直接在本机执行（`ssh my-vps vpsctl status` 零参数可用）；
        // 否则只会去读本机的 /usr/local/etc/xray，必然是错的，早点拦住并给用法。
        if host.is_none() && !local && !cfg!(target_os = "linux") {
            bail!(
                "没指定目标。本机不是 Linux，不能在本机执行服务器端操作。\n\n\
                 两种用法（都只需要服务器上那份二进制）：\n  \
                 1) 装到服务器，本地零安装：  ssh my-vps vpsctl status\n  \
                 2) 本地当遥控器：            vpsctl --host my-vps status\n\n  \
                 本地安装： curl -fsSL https://raw.githubusercontent.com/Helioswei/vpsctl/main/install.sh | bash\n  \
                 装到服务器： bash install.sh --remote my-vps"
            );
        }
        Ok(Target { host, sudo })
    }

    pub fn describe(&self) -> String {
        match &self.host {
            Some(h) => format!("ssh://{}", h),
            None => "local".to_string(),
        }
    }

    /// sudo 前缀（--no-sudo 时为空）
    pub fn su(&self) -> &'static str {
        if self.sudo {
            "sudo "
        } else {
            ""
        }
    }

    pub fn run_with_input(&self, script: &str, stdin_data: Option<&str>) -> Result<Out> {
        let mut cmd = match &self.host {
            Some(h) => {
                let mut c = Command::new("ssh");
                c.arg("-o").arg("BatchMode=yes")
                    .arg("-o").arg("ConnectTimeout=10")
                    .arg(h)
                    .arg("--")
                    .arg(script);
                c
            }
            None => {
                let mut c = Command::new("sh");
                c.arg("-c").arg(script);
                c
            }
        };
        cmd.stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("无法执行 {}（ssh 或 sh 不可用？）", self.describe()))?;
        if let Some(data) = stdin_data {
            child
                .stdin
                .as_mut()
                .context("拿不到子进程 stdin")?
                .write_all(data.as_bytes())?;
        }
        let out = child.wait_with_output().context("等待命令结束失败")?;
        Ok(Out {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    }

    pub fn run(&self, script: &str) -> Result<Out> {
        self.run_with_input(script, None)
    }

    /// 成功则返回 stdout（已 trim），失败带上 stderr 报错
    pub fn ok(&self, script: &str) -> Result<String> {
        let out = self.run(script)?;
        if out.code != 0 {
            bail!(
                "命令失败（exit {}）：{}\n{}",
                out.code,
                first_line(script),
                out.stderr.trim()
            );
        }
        Ok(out.stdout.trim().to_string())
    }

    /// 把文本写到远端文件：走 stdin + tee，避免在命令行里拼引号
    pub fn write_file(&self, path: &str, content: &str, mode: &str, owner: Option<&str>) -> Result<()> {
        let script = format!(
            "set -e; {su}tee {path} > /dev/null; {su}chmod {mode} {path}{chown}",
            su = self.su(),
            path = path,
            mode = mode,
            chown = match owner {
                Some(o) => format!("; {}chown {} {}", self.su(), o, path),
                None => String::new(),
            }
        );
        let out = self.run_with_input(&script, Some(content))?;
        if out.code != 0 {
            bail!("写入 {} 失败：{}", path, out.stderr.trim());
        }
        Ok(())
    }
}

fn first_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.len() > 100 {
        format!("{}…", &line[..100])
    } else {
        line.to_string()
    }
}
