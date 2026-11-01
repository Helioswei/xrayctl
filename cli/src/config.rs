//! 服务端配置的读取/改写，以及客户端链接生成。
//!
//! 用 serde_json::Value 而不是强类型结构：真实配置里字段会随 xray 版本增减（sniffing、xver、show…），
//! 强类型会在读别人机器上的配置时直接崩；Value 只碰我们关心的那几个路径，其余原样保留。

use crate::host::Target;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub const DEFAULT_CFG: &str = "/usr/local/etc/xray/config.json";
pub const DEFAULT_SNI: &str = "www.cloudflare.com";
pub const API_PORT: u16 = 10085;

pub struct ServerConf {
    pub raw: Value,
    pub path: String,
}

impl ServerConf {
    pub fn load(t: &Target, path: &str) -> Result<Self> {
        let txt = t.ok(&format!("{}cat {}", t.su(), path))?;
        let raw: Value = serde_json::from_str(&txt).with_context(|| format!("{} 不是合法 JSON", path))?;
        Ok(ServerConf { raw, path: path.to_string() })
    }

    pub fn vless_inbound(&self) -> Result<&Value> {
        let inbounds = self
            .raw
            .get("inbounds")
            .and_then(|v| v.as_array())
            .context("配置里没有 inbounds")?;
        inbounds
            .iter()
            .find(|ib| ib.get("protocol").and_then(|p| p.as_str()) == Some("vless"))
            .context("找不到 protocol=vless 的 inbound")
    }

    pub fn port(&self) -> Result<u16> {
        self.vless_inbound()?
            .get("port")
            .and_then(|p| p.as_u64())
            .map(|p| p as u16)
            .context("vless inbound 没有 port")
    }

    pub fn sni(&self) -> Result<String> {
        let names = self
            .vless_inbound()?
            .pointer("/streamSettings/realitySettings/serverNames")
            .and_then(|v| v.as_array())
            .context("realitySettings.serverNames 缺失")?;
        names
            .first()
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
            .context("serverNames 为空")
    }

    pub fn private_key(&self) -> Result<String> {
        self.vless_inbound()?
            .pointer("/streamSettings/realitySettings/privateKey")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .context("realitySettings.privateKey 缺失")
    }

    pub fn public_key(&self) -> Result<String> {
        crate::keys::public_from_private(&self.private_key()?)
    }

    pub fn clients(&self) -> Vec<Client> {
        self.vless_inbound()
            .ok()
            .and_then(|ib| ib.pointer("/settings/clients"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|c| Client {
                        email: c.get("email").and_then(|e| e.as_str()).unwrap_or("").to_string(),
                        id: c.get("id").and_then(|e| e.as_str()).unwrap_or("").to_string(),
                        flow: c.get("flow").and_then(|e| e.as_str()).unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn short_ids(&self) -> Vec<String> {
        self.vless_inbound()
            .ok()
            .and_then(|ib| ib.pointer("/streamSettings/realitySettings/shortIds"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect())
            .unwrap_or_default()
    }

    /// 新增一条客户端：独立 uuid + 独立 shortId（互不影响，删一台不牵连别人）
    pub fn add_client(&mut self, email: &str) -> Result<(String, String)> {
        if self.clients().iter().any(|c| c.email == email) {
            bail!("客户端 {} 已存在", email);
        }
        let id = crate::keys::new_uuid();
        let sid = crate::keys::new_short_id();
        let inbound = self
            .raw
            .get_mut("inbounds")
            .and_then(|v| v.as_array_mut())
            .context("配置里没有 inbounds")?
            .iter_mut()
            .find(|ib| ib.get("protocol").and_then(|p| p.as_str()) == Some("vless"))
            .context("找不到 vless inbound")?;

        inbound
            .pointer_mut("/settings/clients")
            .and_then(|v| v.as_array_mut())
            .context("clients 不是数组")?
            .push(json!({"id": id, "flow": "xtls-rprx-vision", "email": email}));

        if let Some(sids) = inbound
            .pointer_mut("/streamSettings/realitySettings/shortIds")
            .and_then(|v| v.as_array_mut())
            {
            sids.push(Value::String(sid.clone()));
        }
        Ok((id, sid))
    }

    /// 删客户端时同时删掉它对应的 shortId（按下标），否则会在列表里留下孤儿条目
    pub fn remove_client(&mut self, email: &str) -> Result<()> {
        let inbound = self
            .raw
            .get_mut("inbounds")
            .and_then(|v| v.as_array_mut())
            .context("配置里没有 inbounds")?
            .iter_mut()
            .find(|ib| ib.get("protocol").and_then(|p| p.as_str()) == Some("vless"))
            .context("找不到 vless inbound")?;

        let clients = inbound
            .pointer_mut("/settings/clients")
            .and_then(|v| v.as_array_mut())
            .context("clients 不是数组")?;
        let before = clients.len();
        let idx = clients
            .iter()
            .position(|c| c.get("email").and_then(|e| e.as_str()) == Some(email));
        let Some(idx) = idx else {
            bail!("没有 email={} 的客户端", email);
        };
        clients.remove(idx);
        debug_assert_eq!(clients.len(), before - 1);

        if let Some(sids) = inbound
            .pointer_mut("/streamSettings/realitySettings/shortIds")
            .and_then(|v| v.as_array_mut())
        {
            if idx < sids.len() {
                sids.remove(idx);
            }
        }
        Ok(())
    }

    pub fn to_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.raw).unwrap_or_default()
    }
}

#[derive(Clone)]
pub struct Client {
    pub email: String,
    pub id: String,
    pub flow: String,
}

/// 客户端要的东西：地址、端口、uuid、公钥、shortId、SNI
pub struct Conn {
    pub host: String,
    pub port: u16,
    pub id: String,
    pub public_key: String,
    pub short_id: String,
    pub sni: String,
    pub flow: String,
}

impl Conn {
    pub fn vless_link(&self, label: &str) -> String {
        format!(
            "vless://{id}@{host}:{port}?encryption=none&security=reality&sni={sni}\
             &fp=chrome&pbk={pbk}&sid={sid}&type=tcp&flow={flow}#{label}",
            id = self.id,
            host = self.host,
            port = self.port,
            sni = self.sni,
            pbk = self.public_key,
            sid = self.short_id,
            flow = self.flow,
            label = label
        )
    }
}

/// 读服务端自己的公网 IP
pub fn public_ip(t: &Target) -> Result<String> {
    let out = t.ok("curl -4 -s --max-time 8 https://api.ipify.org || curl -4 -s --max-time 8 https://ifconfig.me")?;
    if out.is_empty() {
        bail!("取不到公网 IP（机器无出网？）");
    }
    Ok(out)
}

/// 生成新配置（部署用）：REALITY inbound + 按客户端统计 + 屏蔽 BT
pub fn build_config(
    port: u16,
    sni: &str,
    dest: &str,
    private_key: &str,
    clients: &[(String, String, String)], // (email, uuid, shortId)
    short_ids: &[String],
) -> Value {
    let client_arr: Vec<Value> = clients
        .iter()
        .map(|(email, id, _)| json!({"id": id, "flow": "xtls-rprx-vision", "email": email}))
        .collect();

    json!({
        "log": {"loglevel": "warning"},
        "stats": {},
        "api": {"tag": "api", "services": ["HandlerService", "StatsService"]},
        "policy": {"levels": {"0": {"statsUserUplink": true, "statsUserDownlink": true}}},
        "inbounds": [
            {
                "tag": "vless-reality",
                "listen": "0.0.0.0",
                "port": port,
                "protocol": "vless",
                "settings": {"clients": client_arr, "decryption": "none"},
                "streamSettings": {
                    "network": "tcp",
                    "security": "reality",
                    "realitySettings": {
                        "show": false,
                        "dest": dest,
                        "xver": 0,
                        "serverNames": [sni],
                        "privateKey": private_key,
                        "shortIds": short_ids
                    }
                },
                "sniffing": {"enabled": true, "destOverride": ["http", "tls", "quic"]}
            },
            {
                "tag": "api",
                "listen": "127.0.0.1",
                "port": API_PORT,
                "protocol": "dokodemo-door",
                "settings": {"address": "127.0.0.1"}
            }
        ],
        "outbounds": [
            {"protocol": "freedom", "tag": "direct"},
            {"protocol": "blackhole", "tag": "block"}
        ],
        "routing": {"rules": [
            {"type": "field", "protocol": ["bittorrent"], "outboundTag": "block"},
            {"type": "field", "inboundTag": ["api"], "outboundTag": "api"}
        ]}
    })
}

/// 校验 xray 配置是否合法（返回 xray 的原始输出，失败时含原因）
pub fn test_config(t: &Target, path: &str) -> Result<String> {
    let out = t.run(&format!("{}/usr/local/bin/xray run -test -c {} 2>&1", t.su(), path))?;
    let combined = format!("{}{}", out.stdout, out.stderr);
    if !combined.contains("Configuration OK") {
        bail!("配置校验失败：\n{}", combined.trim());
    }
    Ok("Configuration OK".to_string())
}
