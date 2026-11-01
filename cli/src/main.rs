//! vpsctl —— 个人自用 VPS 代理管理（Xray REALITY）
//!
//! 设计目标：一个静态二进制，管一台自己的小机器。不做面板、不做机场、不暴露后台入口。
//!
//! 跨平台：Windows / macOS / Linux 都能当"遥控器"（走系统 ssh 连服务器）；
//! `--local` 只能在 Linux 上跑（服务器端操作依赖 systemd）。

mod checks;
mod client;
mod config;
mod deploy;
mod harden;
mod host;
mod keys;
mod manage;
mod proxy;
mod traffic;
mod upgrade;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use host::Target;

#[derive(Parser)]
#[command(
    name = "vpsctl",
    version,
    about = "个人自用 VPS 代理管理（Xray REALITY）",
    long_about = "一个二进制管一台自己的机器：部署 / 用户 / 体检 / 流量账本 / 升级 / 加固。\n\
                  默认通过系统 ssh 连远端（复用 ~/.ssh/config 里的别名），--local 表示就在这台机器上执行。"
)]
struct Cli {
    /// ssh 别名或 user@host（也可以用环境变量 VPSCTL_HOST）
    #[arg(long, global = true, env = "VPSCTL_HOST")]
    host: Option<String>,

    /// 直接在本机执行（只支持 Linux，需要 systemd）
    #[arg(long, global = true, conflicts_with = "host")]
    local: bool,

    /// 不用 sudo 执行远端命令（已经是 root 时用）
    #[arg(long, global = true)]
    no_sudo: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 看服务/端口/客户端/运行时长
    Status,
    /// 全面体检：二进制、配置、服务、端口、端到端回环
    Doctor,
    /// 逐条凭证回环自测（真实连一次，取出口 IP）
    Selftest {
        /// 只测某个 email
        #[arg(long)]
        email: Option<String>,
    },
    /// 按客户端看流量
    Traffic {
        /// 累加进跨重启账本
        #[arg(long)]
        accumulate: bool,
        /// 机器可读
        #[arg(long)]
        json: bool,
        /// 不打印表格（给 systemd 用）
        #[arg(long)]
        quiet: bool,
        /// 账本文件（默认 /var/lib/vpsctl/traffic.json）
        #[arg(long)]
        state: Option<String>,
        /// 装自动记账：ExecStop + 每小时 timer
        #[arg(long)]
        enable_accounting: bool,
        /// 服务器上 vpsctl 的路径
        #[arg(long, default_value = "/usr/local/bin/vpsctl")]
        bin: String,
    },
    /// 客户端（凭证）管理
    User {
        #[command(subcommand)]
        sub: UserCmd,
    },
    /// 升级 xray 二进制（备份 → 校验 → 替换 → 回滚）
    Upgrade {
        /// 指定版本，如 26.3.27（默认升到实测可用版本）
        #[arg(long)]
        version: Option<String>,
        /// 升到上游 latest（可能打断老客户端，升完必须用真机验证）
        #[arg(long, conflicts_with = "version")]
        latest: bool,
        /// 直接给下载地址（镜像/离线包）
        #[arg(long)]
        url: Option<String>,
    },
    /// 只看有没有新版，不动手
    Checkupgrade,
    /// 回滚到最近一次升级前的二进制
    Rollback {
        /// 指定备份文件（默认取最新的 xray.bak-*）
        #[arg(long)]
        file: Option<String>,
    },
    /// 版本与客户端兼容性一览
    Versions,
    /// 显示服务端公钥，并与 xray 自己算的对照（校验密钥算法实现一致）
    Keys,
    /// 从零部署（装 xray + 生成密钥 + 写配置 + systemd）
    Deploy(DeployArgs),
    /// 加固：fail2ban + ufw
    Harden {
        /// SSH 端口，放行用
        #[arg(long, default_value_t = 22)]
        ssh_port: u16,
    },
    /// 把本机当客户端（默认关闭）
    Proxy {
        #[command(subcommand)]
        sub: ProxyCmd,
    },
    /// 客户端配置：从服务端现状渲染出 Clash / 链接 / 二维码
    Client {
        #[command(subcommand)]
        sub: ClientCmd,
    },
}

#[derive(Subcommand)]
enum ClientCmd {
    /// 读服务端配置，生成 Clash profile + 全部链接 + 手机二维码，并同步到 Clash Verge
    Sync {
        /// 用哪个客户端生成主 profile（默认取 mac，没有就取第一个）
        #[arg(long)]
        client: Option<String>,
        /// 输出目录
        #[arg(long, default_value = "dist")]
        out: String,
        /// 展示名，建议按地域起，例如 首尔-REALITY
        #[arg(long)]
        label: Option<String>,
        /// Clash Verge 的 profiles 目录；auto = 自动探测 macOS 默认路径
        #[arg(long)]
        clash_dir: Option<String>,
        /// 顺便生成手机二维码
        #[arg(long)]
        qr: bool,
    },
}

#[derive(Args)]
struct DeployArgs {
    /// 客户端名，逗号分隔：laptop,phone；用 --from-config 复用旧配置时可省略
    #[arg(long, default_value = "")]
    clients: String,
    #[arg(long, default_value_t = 443)]
    port: u16,
    #[arg(long, default_value = config::DEFAULT_SNI)]
    sni: String,
    /// REALITY 回落目标，默认 <sni>:443
    #[arg(long)]
    dest: Option<String>,
    /// 只生成+校验，不碰系统
    #[arg(long)]
    dry_run: bool,
    /// 自己给 xray 下载地址（镜像/离线）
    #[arg(long)]
    xray_url: Option<String>,
    /// 指定 xray 版本（默认装实测可用版本，见 cli/README 兼容表）
    #[arg(long)]
    xray_version: Option<String>,
    /// 用本地现成 config.json 部署（换机复用旧凭证，客户端不用重新导入）
    #[arg(long)]
    from_config: Option<String>,
}

#[derive(Subcommand)]
enum UserCmd {
    /// 列出所有客户端
    List,
    /// 新增一个客户端并打印链接
    Add {
        email: String,
        /// 加完立刻回环自测
        #[arg(long)]
        selfcheck: bool,
    },
    /// 删除客户端
    Rm { email: String },
}

#[derive(Subcommand)]
enum ProxyCmd {
    /// 安装客户端配置（默认从第一条凭证借用）
    Install {
        #[arg(long)]
        email: Option<String>,
        /// 从哪台机器读节点凭证（默认就是目标机器自己）
        #[arg(long)]
        from: Option<String>,
        #[arg(long, default_value_t = 1080)]
        socks: u16,
        #[arg(long, default_value_t = 8118)]
        http: u16,
    },
    On {
        #[arg(long, default_value_t = 1080)]
        socks: u16,
        #[arg(long, default_value_t = 8118)]
        http: u16,
    },
    Off,
    Status {
        #[arg(long, default_value_t = 1080)]
        socks: u16,
        #[arg(long, default_value_t = 8118)]
        http: u16,
    },
    /// 打印 export 行，配合 eval 使用
    Env {
        #[arg(long, default_value_t = 1080)]
        socks: u16,
        #[arg(long, default_value_t = 8118)]
        http: u16,
    },
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("错误：{:#}", e);
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let target = Target::new(cli.host.clone(), cli.local, !cli.no_sudo)?;

    match cli.cmd {
        Cmd::Status => checks::cmd_status(&target),
        Cmd::Doctor => checks::cmd_doctor(&target),
        Cmd::Selftest { email } => checks::cmd_selftest(&target, email.as_deref()),
        Cmd::Traffic {
            accumulate,
            json,
            quiet,
            state,
            enable_accounting,
            bin,
        } => {
            let state_path = state.unwrap_or_else(|| traffic::STATE_PATH.to_string());
            if enable_accounting {
                return traffic::enable_accounting(&target, &bin);
            }
            if accumulate {
                let (totals, raw) = traffic::accumulate(&target, &state_path)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&raw)?);
                } else if !quiet {
                    let footer = format!("账本：{}（跨重启累计）", state_path);
                    traffic::print_table(&totals, &footer);
                }
                return Ok(());
            }
            let cur = traffic::query(&target)?;
            if json {
                let obj: serde_json::Map<String, serde_json::Value> = cur
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            serde_json::json!({
                                "uplink": v.up, "downlink": v.down, "total": v.up + v.down
                            }),
                        )
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"clients": obj}))?
                );
                return Ok(());
            }
            let mins = traffic::uptime_minutes(&target);
            let footer = match mins {
                Some(m) => format!("统计区间：xray 本次启动至今约 {} 分钟（重启即清零）\n长期总量看 vnstat -m", m),
                None => "统计区间：xray 本次启动至今（重启即清零）".to_string(),
            };
            traffic::print_table(&cur, &footer);
            Ok(())
        }
        Cmd::User { sub } => match sub {
            UserCmd::List => manage::cmd_list(&target),
            UserCmd::Add { email, selfcheck } => manage::cmd_add(&target, &email, selfcheck),
            UserCmd::Rm { email } => manage::cmd_rm(&target, &email),
        },
        Cmd::Upgrade { version, latest, url } => {
            upgrade::run(&target, version.as_deref(), url.as_deref(), latest)
        }
        Cmd::Checkupgrade => upgrade::check(&target),
        Cmd::Rollback { file } => upgrade::rollback(&target, file.as_deref()),
        Cmd::Versions => {
            let cur = target
                .ok("/usr/local/bin/xray version | head -1")
                .unwrap_or_else(|_| "未安装".into());
            println!("服务端 xray   {}", cur);
            println!("实测可用版本  v{}（deploy 默认装这个）", deploy::TESTED_XRAY_VERSION);
            println!();
            println!("服务端版本阶梯（2026-10-31 逐版实测）：");
            println!("  ≤26.6.27   mihomo 默认 ✅   xray 客户端 ✅        ← 默认装这个");
            println!("  26.7.11+   REALITY 加 minClientVer，老客户端被拒");
            println!("  ≥26.9.9    要求 X25519MLKEM768：mihomo 必须写");
            println!("             reality-opts.support-x25519mlkem768: true（xray 客户端不受影响）");
            println!();
            println!("升完用手机/Clash 真连一次；不通就 `vpsctl rollback`。");
            Ok(())
        }
        Cmd::Keys => {
            let conf = config::ServerConf::load(&target, config::DEFAULT_CFG)?;
            let mine = conf.public_key()?;
            println!("公钥（vpsctl 计算）  {}", mine);
            let theirs = target.ok(&format!(
                "{}xray x25519 -i {} 2>/dev/null | grep -iE 'Password|Public' | awk '{{print $NF}}'",
                target.su(),
                conf.private_key()?
            ))?;
            println!("公钥（xray 计算）    {}", theirs);
            if theirs.trim() == mine {
                println!("✓ 两者一致");
                Ok(())
            } else {
                anyhow::bail!("两者不一致，密钥算法实现有偏差")
            }
        }
        Cmd::Deploy(a) => deploy::run(
            &target,
            &deploy::DeployOpts {
                clients: a
                    .clients
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                port: a.port,
                sni: a.sni,
                dest: a.dest,
                dry_run: a.dry_run,
                xray_url: a.xray_url,
                xray_version: a.xray_version,
                from_config: a.from_config,
            },
        ),
        Cmd::Client { sub } => match sub {
            ClientCmd::Sync { client, out, label, clash_dir, qr } => client::run(
                &target,
                &client::SyncOpts {
                    client,
                    out_dir: out,
                    label,
                    clash_dir,
                    qr,
                },
            ),
        },
        Cmd::Harden { ssh_port } => harden::run(&target, ssh_port),
        Cmd::Proxy { sub } => match sub {
            ProxyCmd::Install { email, from, socks, http } => {
                let src = match &from {
                    Some(h) => Some(Target::new(Some(h.clone()), false, !cli.no_sudo)?),
                    None => None,
                };
                proxy::install(
                    &target,
                    &proxy::Ports { socks, http },
                    email.as_deref(),
                    src.as_ref(),
                )
            }
            ProxyCmd::On { socks, http } => proxy::on(&target, &proxy::Ports { socks, http }),
            ProxyCmd::Off => proxy::off(&target),
            ProxyCmd::Status { socks, http } => {
                proxy::status(&target, &proxy::Ports { socks, http })
            }
            ProxyCmd::Env { socks, http } => {
                println!("{}", proxy::env(&proxy::Ports { socks, http }));
                Ok(())
            }
        },
    }
    .with_context(|| format!("vpsctl 执行失败（目标 {}）", target.describe()))
}
