#!/usr/bin/env python3
"""traffic.py — 按客户端（email）查看 xray 流量

前提：服务端配置里开了统计（rebuild.sh 默认已开）：
    "stats": {}
    "api": {"tag": "api", "services": ["HandlerService", "StatsService"]}
    "policy": {"levels": {"0": {"statsUserUplink": true, "statsUserDownlink": true}}}
    + 一个绑 127.0.0.1:10085 的 dokodemo-door inbound（tag=api）+ 指向它的 routing 规则

用法：
    sudo python3 traffic.py              表格（按总量降序）
    sudo python3 traffic.py --json       机器可读
    sudo python3 traffic.py --save FILE  追加一行快照到 FILE（TSV）
    sudo python3 traffic.py --api H:P    指定 api 地址（默认 127.0.0.1:10085）
    sudo python3 traffic.py --accumulate 累计账本（跨重启累加，见下）

累计账本（--accumulate）：
    xray 的计数在内存里，重启归零，且它只提供查询接口、不能写入/恢复。
    所以"重启自动保存"只能在外面做：每次运行算出「本次启动周期内的增量」，
    累加进 state 文件。判断是否换了启动周期，用 systemd 的 ActiveEnterTimestamp。
      · systemd ExecStop= 里调一次 → 停服前抓最后一份，不丢数据
      · systemd timer 每小时调一次 → 崩溃/断电也能少丢
    默认 state：/var/lib/xray-traffic/state.json（600）

安装：sudo install -m 755 traffic.py /usr/local/bin/xray-traffic

⚠️ 不带 --accumulate 时看到的是"本次启动以来"，重启即清零。
   长期总量仍以 `vnstat -m`（整机）为准；要客户端维度就靠账本。
"""
import json
import os
import subprocess
import sys
import time

API = "127.0.0.1:10085"
SAVE = None
AS_JSON = False
ACCUMULATE = False
QUIET = False
STATE = "/var/lib/xray-traffic/state.json"

args = sys.argv[1:]
while args:
    a = args.pop(0)
    if a == "--json":
        AS_JSON = True
    elif a == "--save" and args:
        SAVE = args.pop(0)
    elif a == "--api" and args:
        API = args.pop(0)
    elif a == "--accumulate":
        ACCUMULATE = True
    elif a == "--quiet":
        QUIET = True
    elif a == "--state" and args:
        STATE = args.pop(0)
    elif a in ("-h", "--help"):
        print(__doc__)
        sys.exit(0)
    else:
        sys.exit("未知参数: %s" % a)


def human(n):
    n = float(n)
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if n < 1024 or unit == "TiB":
            return "%d %s" % (n, unit) if unit == "B" else "%.2f %s" % (n, unit)
        n /= 1024.0


def query():
    try:
        out = subprocess.run(
            ["xray", "api", "statsquery", "--server=" + API, "-pattern", "user>>>"],
            capture_output=True, text=True, timeout=20,
        )
    except FileNotFoundError:
        sys.exit("找不到 xray 可执行文件（/usr/local/bin/xray）")
    except subprocess.TimeoutExpired:
        sys.exit("查询超时：api %s 没响应" % API)
    if out.returncode != 0 or not out.stdout.strip():
        sys.exit("查询失败（api %s）：%s" % (API, (out.stderr or out.stdout).strip()[:300]))
    return json.loads(out.stdout)


def service_boot_stamp():
    """xray 本次启动的时刻，用作"是否换过启动周期"的判据。"""
    try:
        ts = subprocess.run(
            ["systemctl", "show", "xray", "-p", "ActiveEnterTimestamp", "--value"],
            capture_output=True, text=True, timeout=10,
        ).stdout.strip()
        return ts or "unknown"
    except Exception:
        return "unknown"


def service_uptime_minutes():
    try:
        ts = subprocess.run(
            ["systemctl", "show", "xray", "-p", "ActiveEnterTimestamp", "--value"],
            capture_output=True, text=True, timeout=10,
        ).stdout.strip()
        if not ts:
            return None
        started = time.mktime(time.strptime(ts, "%a %Y-%m-%d %H:%M:%S %Z"))
        return int((time.time() - started) / 60)
    except Exception:
        return None


stat = query().get("stat", [])
users = {}
for s in stat:
    parts = s.get("name", "").split(">>>")
    if len(parts) != 4 or parts[0] != "user":
        continue
    email, direction = parts[1], parts[3]
    users.setdefault(email, {"up": 0, "down": 0})
    if direction == "uplink":
        users[email]["up"] = int(s.get("value", 0))
    elif direction == "downlink":
        users[email]["down"] = int(s.get("value", 0))

rows = sorted(users.items(), key=lambda kv: -(kv[1]["up"] + kv[1]["down"]))
up_minutes = service_uptime_minutes()
boot = service_boot_stamp()


def accumulate(cur_users, boot_id):
    """把「本次启动周期的增量」累加进账本；返回(累计表, 本次增量表)。"""
    try:
        with open(STATE) as f:
            state = json.load(f)
    except Exception:
        state = {"boot": None, "totals": {}, "last": {}}
    if state.get("boot") != boot_id:      # xray 重启过 → 计数从 0 开始
        state["last"] = {}
        state["boot"] = boot_id
    # 关键：以已有账本为基础累加。若某台设备本轮没流量（甚至一个都没连），
    # 不能把它的历史累计丢掉——第一版就是在这里写成 totals={} 导致数据被清空。
    totals = {e: {"up": t.get("up", 0), "down": t.get("down", 0)}
              for e, t in state.get("totals", {}).items()}
    delta = {}
    for email, v in cur_users.items():
        prev = state["last"].get(email, {"up": 0, "down": 0})
        d_up = max(0, v["up"] - prev.get("up", 0))
        d_dn = max(0, v["down"] - prev.get("down", 0))
        delta[email] = {"up": d_up, "down": d_dn}
        t = state["totals"].get(email, {"up": 0, "down": 0})
        totals[email] = {"up": t.get("up", 0) + d_up, "down": t.get("down", 0) + d_dn}
        state["last"][email] = {"up": v["up"], "down": v["down"]}
    state["totals"] = totals
    state["updated"] = time.strftime("%Y-%m-%d %H:%M:%S")
    try:
        os.makedirs(os.path.dirname(STATE), exist_ok=True)
        tmp = STATE + ".tmp"
        with open(tmp, "w") as f:
            json.dump(state, f, indent=2)
        os.chmod(tmp, 0o600)
        os.replace(tmp, STATE)
    except Exception as e:
        sys.stderr.write("写账本失败：%s\n" % e)
    return totals, delta


if ACCUMULATE:
    totals, delta = accumulate(users, boot)
    if AS_JSON:
        print(json.dumps({"state": STATE, "boot": boot, "totals": totals, "last_delta": delta},
                         ensure_ascii=False, indent=2))
        sys.exit(0)
    if QUIET:
        sys.exit(0)
    all_tot = sum(v["up"] + v["down"] for v in totals.values()) or 1
    print("\n%s 累计账本（跨重启，state=%s）" % (time.strftime("%Y-%m-%d %H:%M"), STATE))
    print("%-22s %12s %12s %12s %7s" % ("客户端", "上传", "下载", "合计", "占比"))
    print("-" * 70)
    for email, v in sorted(totals.items(), key=lambda kv: -(kv[1]["up"] + kv[1]["down"])):
        tot = v["up"] + v["down"]
        print("%-22s %12s %12s %12s %6.1f%%" % (
            email[:22], human(v["up"]), human(v["down"]), human(tot), 100.0 * tot / all_tot))
    print("-" * 70)
    print("%-22s %12s %12s %12s" % (
        "合计", human(sum(v["up"] for v in totals.values())),
        human(sum(v["down"] for v in totals.values())), human(all_tot)))
    sys.exit(0)

if SAVE:
    header = not os.path.exists(SAVE)
    with open(SAVE, "a") as f:
        if header:
            f.write("timestamp\temail\tuplink_bytes\tdownlink_bytes\n")
        stamp = time.strftime("%Y-%m-%d %H:%M:%S")
        for email, v in sorted(users.items()):
            f.write("%s\t%s\t%d\t%d\n" % (stamp, email, v["up"], v["down"]))
    print("已追加快照 → %s（%d 个客户端）" % (SAVE, len(users)))

if AS_JSON:
    print(json.dumps({
        "api": API,
        "uptime_minutes": up_minutes,
        "clients": [
            {"email": e, "uplink": v["up"], "downlink": v["down"], "total": v["up"] + v["down"]}
            for e, v in rows
        ],
        "total": sum(v["up"] + v["down"] for v in users.values()),
    }, ensure_ascii=False, indent=2))
    sys.exit(0)

if not rows:
    print("还没有任何客户端流量（统计从 xray 上次重启开始计）")
    sys.exit(0)

total_all = sum(v["up"] + v["down"] for v in users.values()) or 1
print()
print("%-22s %12s %12s %12s %7s" % ("客户端", "上传", "下载", "合计", "占比"))
print("-" * 70)
for email, v in rows:
    tot = v["up"] + v["down"]
    print("%-22s %12s %12s %12s %6.1f%%" % (
        email[:22], human(v["up"]), human(v["down"]), human(tot), 100.0 * tot / total_all))
print("-" * 70)
print("%-22s %12s %12s %12s %6.1f%%" % (
    "合计", human(sum(v["up"] for v in users.values())),
    human(sum(v["down"] for v in users.values())), human(total_all), 100.0))
if up_minutes is not None:
    print("\n统计区间：xray 本次启动至今约 %d 分钟（重启即清零）" % up_minutes)
else:
    print("\n统计区间：xray 本次启动至今（重启即清零）")
print("长期总量以 vnstat 为准：vnstat -m")
