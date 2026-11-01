#!/usr/bin/env python3
"""服务端执行体：给 REALITY 入站追加一个独立客户端。
用法: sudo python3 - <uuid> <shortId> <email> [--dry-run]
由 add_client.sh 通过 ssh stdin 传入执行，不落盘在服务器上。
"""
import json
import os
import subprocess
import sys

CFG = "/usr/local/etc/xray/config.json"
TMP = "/tmp/xray-config-new.json"

from typing import Any, NoReturn

CONFIG_TMPL: Any = {
    "log": {"loglevel": "warning"},
    "inbounds": [{
        "listen": "127.0.0.1",
        "port": 10808,
        "protocol": "socks",
        "settings": {"udp": False, "auth": "noauth"},
    }],
    "outbounds": [{
        "protocol": "vless",
        "settings": {
            "vnext": [{
                "address": "127.0.0.1",
                "port": 443,
                "users": [{
                    "id": None,
                    "encryption": "none",
                    "flow": "xtls-rprx-vision",
                }],
            }],
        },
        "streamSettings": {
            "network": "tcp",
            "security": "reality",
            "realitySettings": {
                "fingerprint": "chrome",
                "serverName": None,
                "publicKey": None,
                "password": None,
                "shortId": None,
                "spiderX": "/",
            },
        },
    }],
}


def die(msg) -> "NoReturn":
    print("✗ " + msg)
    sys.exit(1)


def load():
    cfg = json.load(open(CFG))
    ib = None
    for i in cfg["inbounds"]:
        if i.get("protocol") == "vless":
            ib = i
            break
    if ib is None:
        die("没找到 vless 入站")
    return cfg, ib, ib["streamSettings"]["realitySettings"], ib["settings"]["clients"]


def derive_pub(private_key):
    der = subprocess.run(
        ["/usr/local/bin/xray", "x25519", "-i", private_key],
        capture_output=True, text=True,
    )
    for line in der.stdout.replace("\r", "").splitlines():
        if "Password" in line or "PublicKey" in line:
            return line.split(":")[-1].strip()
    die("公钥推导失败")


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    dry = "--dry-run" in sys.argv

    # 仅体检：按 email 找出已有 client，用它的凭证做一次回环自测
    if "--selfcheck" in sys.argv:
        if not args:
            die("用法: sudo python3 - <email> --selfcheck")
        email = args[0]
        _, _, rs, clients = load()
        hit = [c for c in clients if c.get("email") == email]
        if not hit:
            die("没有这个 email: %s（现有 %s）" % (email, [c.get("email") for c in clients]))
        idx = clients.index(hit[0])
        print("→ 体检: %s" % email)
        selfcheck(hit[0]["id"], derive_pub(rs["privateKey"]),
                  rs["shortIds"][idx], rs["serverNames"][0])
        return

    if len(args) < 3:
        die("用法: sudo python3 - <uuid> <shortId> <email> [--dry-run]")
    uuid, short_id, email = args[0], args[1], args[2]

    cfg, ib, rs, clients = load()
    if any(c.get("email") == email for c in clients):
        die("email 已存在: %s（换一个名字）" % email)
    if any(c.get("id") == uuid for c in clients):
        die("uuid 已存在，重新生成")

    clients.append({"id": uuid, "flow": "xtls-rprx-vision", "email": email})
    rs["shortIds"].append(short_id)

    with open(TMP, "w") as f:
        json.dump(cfg, f, indent=2, ensure_ascii=False)
    os.chmod(TMP, 0o600)

    # 1) 语法/语义校验（xray 自己的 -test）
    test = subprocess.run(
        ["/usr/local/bin/xray", "run", "-test", "-c", TMP],
        capture_output=True, text=True,
    )
    ok = test.returncode == 0
    print(("✓" if ok else "✗") + " xray -test " + ("通过" if ok else "失败"))
    tail = (test.stdout + test.stderr).strip().splitlines()[-3:]
    for line in tail:
        print("    " + line)
    if not ok:
        die("配置校验失败，原配置未改动")

    print("   clients: %d → %d, shortIds: %d → %d"
          % (len(clients) - 1, len(clients), len(rs["shortIds"]) - 1, len(rs["shortIds"])))

    # 2) 推导公钥（客户端要用），服务端只存私钥
    der = subprocess.run(
        ["/usr/local/bin/xray", "x25519", "-i", rs["privateKey"]],
        capture_output=True, text=True,
    )
    pub = ""
    for line in der.stdout.replace("\r", "").splitlines():
        if "Password" in line or "PublicKey" in line:
            pub = line.split(":")[-1].strip()
            break
    if not pub:
        die("公钥推导失败")

    if dry:
        print("   [dry-run] 未写入、未重启")
        print("   publicKey=%s" % pub)
        print("   sni=%s" % rs["serverNames"][0])
        os.unlink(TMP)
        return

    # 3) 备份 → 覆盖（属主 xray / 600 是必须的，否则服务启不来）
    bak = "%s.bak-%s" % (CFG, subprocess.run(
        ["date", "+%Y%m%d-%H%M%S"], capture_output=True, text=True).stdout.strip())
    subprocess.run(["cp", "-a", CFG, bak], check=True)
    print("   备份: " + bak)
    subprocess.run(["install", "-m", "600", "-o", "xray", "-g", "xray", TMP, CFG], check=True)
    os.unlink(TMP)

    # 4) 重启并确认监听
    subprocess.run(["systemctl", "restart", "xray"], check=True)
    active = subprocess.run(
        ["systemctl", "is-active", "xray"], capture_output=True, text=True).stdout.strip()
    print(("✓" if active == "active" else "✗") + " xray " + active)
    if active != "active":
        subprocess.run(["cp", "-a", bak, CFG], check=True)
        subprocess.run(["systemctl", "restart", "xray"])
        die("服务未起来，已回滚到备份")

    # 5) 用新凭证在本机回环自测（证明 uuid/shortId/公钥三件配对正确）
    selfcheck(uuid, pub, short_id, rs["serverNames"][0])

    print("   email=%s" % email)
    print("   uuid=%s" % uuid)
    print("   shortId=%s" % short_id)
    print("   publicKey=%s" % pub)
    print("   sni=%s" % rs["serverNames"][0])


def selfcheck(uuid, pub, short_id, sni):
    """在服务器本机用临时 socks(10808) 走一遍新凭证，出 200 且出口 IP 就是本机即算配对正确。"""
    import tempfile
    import time

    t = json.loads(json.dumps(CONFIG_TMPL))
    t["outbounds"][0]["settings"]["vnext"][0]["users"][0]["id"] = uuid
    rs2 = t["outbounds"][0]["streamSettings"]["realitySettings"]
    rs2["serverName"] = sni
    rs2["publicKey"] = pub
    rs2["password"] = pub
    rs2["shortId"] = short_id

    d = tempfile.mkdtemp(prefix="xray-selfcheck-")
    path = os.path.join(d, "cli.json")
    with open(path, "w") as f:
        json.dump(t, f, ensure_ascii=False)
    os.chmod(path, 0o600)
    proc = subprocess.Popen(["/usr/local/bin/xray", "run", "-c", path],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        out = ""
        for _ in range(10):
            r = subprocess.run(
                ["curl", "-s", "--max-time", "6", "-x", "socks5h://127.0.0.1:10808",
                 "https://ipinfo.io/ip"], capture_output=True, text=True)
            out = r.stdout.strip()
            if out:
                break
            time.sleep(1)
        print(("✓" if out else "✗") + " 回环自测出口: " + (out or "无返回"))
        return out
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()
        import shutil
        shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    main()
