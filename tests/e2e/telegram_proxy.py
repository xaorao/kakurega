#!/usr/bin/env python3
"""The Telegram bridge reaches Telegram through the proxy the environment names, and not around it; the local service never goes through one."""
import json
import os
import shutil
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/telegram-proxy-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
data = work / "data"
kaku = str(repo / "target/debug/kaku")
TOKEN = "123456789:FAKE-token-for-proxy-test-AAAA"
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


connects = []
heads = []
socks_seen = []
srv = socket.socket()
srv.bind(("127.0.0.1", 0))
srv.listen(8)


def proxy():
    while True:
        try:
            c, _ = srv.accept()
        except OSError:
            return
        try:
            c.settimeout(3)
            # one recv can return only the start of the request line: read until the line is complete
            raw = b""
            while b"\r\n\r\n" not in raw and len(raw) < 8192:
                part = c.recv(4096)
                if not part:
                    break
                raw += part
            head = raw.decode("latin1", "replace")
            connects.append(head.split("\r\n")[0])
            heads.append(head)
            c.sendall(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\n\r\n")
        except Exception:
            pass
        finally:
            c.close()


threading.Thread(target=proxy, daemon=True).start()

socks_srv = socket.socket()
socks_srv.bind(("127.0.0.1", 0))
socks_srv.listen(8)


def socks5():
    """A minimal SOCKS5 proxy that records what it is asked for and then refuses the connection."""
    while True:
        try:
            c, _ = socks_srv.accept()
        except OSError:
            return
        try:
            c.settimeout(3)
            greeting = c.recv(256)
            if len(greeting) < 3 or greeting[0] != 5:
                socks_seen.append(("not socks5", greeting[:8]))
                continue
            c.sendall(b"\x05\x00")
            req = c.recv(512)
            if len(req) >= 7 and req[0] == 5 and req[1] == 1 and req[3] == 3:
                # the name is handed to the proxy to resolve (socks5h)
                n = req[4]
                socks_seen.append(("name", req[5:5 + n].decode("latin1"), int.from_bytes(req[5 + n:7 + n], "big")))
            elif len(req) >= 10 and req[0] == 5 and req[1] == 1 and req[3] in (1, 4):
                # an address already resolved on this computer (socks5)
                size = 4 if req[3] == 1 else 16
                socks_seen.append(("address", req[4:4 + size].hex(), int.from_bytes(req[4 + size:6 + size], "big")))
            else:
                socks_seen.append(("unexpected request", req[:8]))
            c.sendall(b"\x05\x05\x00\x01\x00\x00\x00\x00\x00\x00")
        except Exception:
            pass
        finally:
            c.close()


threading.Thread(target=socks5, daemon=True).start()

direct = []
fake_tg = socket.socket()
fake_tg.bind(("127.0.0.1", 0))
fake_tg.listen(8)


def fake_telegram():
    """Counts every connection made to it and answers 401, as Telegram does for a token it does not know."""
    while True:
        try:
            c, _ = fake_tg.accept()
        except OSError:
            return
        try:
            c.settimeout(2)
            raw = c.recv(4096)
            direct.append(raw.split(b"\r\n")[0].decode("latin1", "replace"))
            body = b'{"ok":false,"error_code":401,"description":"Unauthorized"}'
            c.sendall(b"HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: " + str(len(body)).encode() + b"\r\n\r\n" + body)
        except Exception:
            pass
        finally:
            c.close()


threading.Thread(target=fake_telegram, daemon=True).start()
service = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    base_env = {k: v for k, v in os.environ.items() if "proxy" not in k.lower()}
    base_env["TELEGRAM_BOT_TOKEN"] = TOKEN
    proxy_url = f"http://127.0.0.1:{srv.getsockname()[1]}"

    def bridge(extra, seconds=5):
        p = subprocess.Popen([kaku, "telegram", "--data-dir", str(data)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(base_env, **extra))
        time.sleep(seconds)
        p.terminate()
        out, err = p.communicate(timeout=15)
        return out, err

    connects.clear()
    out, err = bridge({"HTTPS_PROXY": proxy_url})
    check("with HTTPS_PROXY set the bridge asks the proxy for a tunnel to api.telegram.org:443", any(c.startswith("CONNECT api.telegram.org:443") for c in connects), str(connects))
    check("the token is not in the tunnel request or in the bridge's output", all(TOKEN.split(":")[1] not in c for c in connects) and TOKEN.split(":")[1] not in out + err, str(connects) + err[:200])
    connects.clear()
    out, err = bridge({"HTTPS_PROXY": proxy_url, "NO_PROXY": "api.telegram.org"})
    check("NO_PROXY for the Telegram host sends nothing to the proxy", not connects, str(connects))
    connects.clear()
    out, err = bridge({"HTTPS_PROXY": proxy_url, "ALL_PROXY": proxy_url}, seconds=3)
    check("the bridge never sends the local service's traffic through the proxy", not any("127.0.0.1" in c for c in connects), str(connects))

    # an authenticated proxy: the credentials go to the proxy as Proxy-Authorization, and appear nowhere else
    import base64
    PASSWORD = "S3cretProxyPass"
    connects.clear()
    heads.clear()
    out, err = bridge({"HTTPS_PROXY": f"http://alice:{PASSWORD}@127.0.0.1:{srv.getsockname()[1]}"})
    want = "Proxy-Authorization: Basic " + base64.b64encode(f"alice:{PASSWORD}".encode()).decode()
    check("an authenticated proxy gets the credentials as a Basic Proxy-Authorization header", any(want in h for h in heads), str(heads)[:200])
    leaked = [str(f) for f in data.glob("telegram*") if f.is_file() and PASSWORD in f.read_text(errors="ignore")]
    check("the proxy password is in no output and in none of the bridge's files", PASSWORD not in out + err and not leaked, f"{leaked} {(out + err)[-120:]}")
    check("and the token is not in the tunnel request", all(TOKEN.split(":")[1] not in h for h in heads))

    # a SOCKS5 proxy is used, and the bridge does not go round it
    socks_seen.clear()
    out, err = bridge({"HTTPS_PROXY": f"socks5://127.0.0.1:{socks_srv.getsockname()[1]}"})
    check("a SOCKS5 proxy is asked for a connection to port 443 (the address of api.telegram.org, resolved here as socks5:// means)", any(k in ("name", "address") and port == 443 for k, _h, port in socks_seen if k != "unexpected request"), str(socks_seen))
    check("and the bridge did not reach Telegram directly (no 'token rejected' answer came back)", "rejected the bot token" not in out + err, (out + err)[-200:])
    socks_seen.clear()
    out, err = bridge({"HTTPS_PROXY": f"socks5h://127.0.0.1:{socks_srv.getsockname()[1]}"})
    check("socks5h:// hands the proxy the name api.telegram.org to resolve, so the name is not looked up on this computer", ("name", "api.telegram.org", 443) in socks_seen, str(socks_seen))
    socks_seen.clear()
    out, err = bridge({"HTTPS_PROXY": f"socks5://alice:{PASSWORD}@127.0.0.1:{socks_srv.getsockname()[1]}"})
    check("a SOCKS5 proxy with a password is used too, and the password is not printed", socks_seen and PASSWORD not in out + err, str(socks_seen) + (out + err)[-100:])

    # a proxy setting the bridge cannot use: it stops, and connects to nothing
    fake_api = f"http://127.0.0.1:{fake_tg.getsockname()[1]}"
    direct.clear()
    for bad in ["ftp://alice:" + PASSWORD + "@proxy.example:21", "not a proxy at all"]:
        p = subprocess.Popen([kaku, "telegram", "--data-dir", str(data), "--api-base", fake_api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(base_env, HTTPS_PROXY=bad))
        try:
            out, err = p.communicate(timeout=15)
        except subprocess.TimeoutExpired:
            p.kill()
            out, err = p.communicate()
            err += " (it kept running)"
        check(f"an unusable proxy setting stops the bridge with an explanation and a failing exit code ({bad[:8]}...)", p.returncode not in (0, None) and "HTTPS_PROXY" in err and "will not connect around a proxy" in err, f"rc={p.returncode} {err[-160:]}")
        check("and the message does not repeat the setting", PASSWORD not in out + err and "proxy.example" not in out + err)
    check("and nothing was sent to Telegram", not direct, str(direct))
    # asked to skip the proxy for Telegram, the same setting is not in the way and the connection is direct
    direct.clear()
    p = subprocess.Popen([kaku, "telegram", "--data-dir", str(data), "--api-base", fake_api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(base_env, HTTPS_PROXY="ftp://proxy.example:21", NO_PROXY="127.0.0.1"))
    time.sleep(4)
    p.terminate()
    out, err = p.communicate(timeout=15)
    check("with NO_PROXY naming the host the unusable setting is not in the way and the connection is direct", len(direct) >= 1 and "will not connect around a proxy" not in err, f"{direct} {err[-100:]}")
finally:
    service.kill()
    subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
    srv.close()
    socks_srv.close()
    fake_tg.close()

bad = results.count(False)
print(f"\nall {len(results)} proxy checks passed" if not bad else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
