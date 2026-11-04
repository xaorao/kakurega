#!/usr/bin/env python3
"""The same boundary attacks as tests/e2e/desktop_attack.mjs, run inside the Tauri candidate (test-only build feature
`attack-test`, which evaluates the attack script in the real webview and reports what happened on stdout)."""
import json, os, shutil, subprocess, sys, time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/tauri-attack-tmp"
shutil.rmtree(work, ignore_errors=True); (work / "root").mkdir(parents=True); (work / "data").mkdir()
kaku = repo / "target/debug/kaku"
tauri = repo / "target/tauri-spike/release/kakurega-tauri-shell"
results = []
def check(n, ok, d=""):
    results.append(ok); print(("PASS  " if ok else "FAIL  ") + n + (("  " + d) if d else ""))
srv = subprocess.Popen([str(kaku), "serve", "--data-dir", str(work / "data"), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
app = None
try:
    for _ in range(100):
        if (work / "data/admin.json").exists(): break
        time.sleep(0.1)
    app = subprocess.Popen([str(tauri)], env=dict(os.environ, KAKUREGA_DATA_DIR=str(work / "data")), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    lines, t0 = [], time.time()
    for line in app.stdout:
        lines.append(line.rstrip())
        if line.startswith("ATTACKS_DONE") or time.time() - t0 > 120: break
    res = next((json.loads(l.split(" ", 1)[1]) for l in lines if l.startswith("ATTACK_RESULT") and not l.endswith("null")), None)
    check("the attack script ran inside the Tauri webview and reported", res is not None)
    if res:
        check("XSS cannot read the session: document.cookie empty, storage empty", res["cookie"] == "" and res["storage"] == [0, 0], json.dumps([res["cookie"], res["storage"]]))
        adm = [res["adminList"], res["adminJobs"], res["approverEnroll"], res["logExport"]]
        check("XSS cannot reach any admin endpoint", all(c in (401, 403) for c in adm), json.dumps(adm))
        check("CSP blocks inline script, eval and Function", (res["inlineScript"], res["evalWorks"], res["fn"]) == ("blocked", "blocked", "blocked"), json.dumps([res["inlineScript"], res["evalWorks"], res["fn"]]))
        check("CSP blocks a remote script, an image beacon and a no-cors request to another site", (res["remoteScript"], res["exfil"], res["cross"]) == ("blocked", "blocked", "blocked"), json.dumps([res["remoteScript"], res["exfil"], res["cross"]]))
        ipc = [res["ipcDialog"], res["ipcWindow"], res["ipcApp"], res["ipcFs"], res["ipcCustom"]]
        check("a compromised page cannot call any native command (dialog, window, app, fs, custom): none is ALLOWED", "ALLOWED" not in ipc, json.dumps({"global": res["tauriGlobal"], "internals": res["internals"], "calls": ipc}))
        check("window.open creates no window", all(res["opens"]) and any(l.strip() == "WINDOWS 1" for l in lines), json.dumps(res["opens"]))
    navs = [l.split(" ", 3) for l in lines if l.startswith("NAV ")]
    check("every navigation target was attempted", len(navs) == 7, str(len(navs)))
    left = [n[1] for n in navs if n[2] == "LEFT"]
    check("navigating to file, http(s) elsewhere, javascript, data, about:blank, blob and view-source is refused or the app is put back", not left, "LEFT: " + ", ".join(left))
finally:
    if app and app.poll() is None: app.kill()
    srv.kill()
    subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work / 'data'}"], capture_output=True)
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} tauri attack checks passed" if bad == 0 else f"\n{bad} FAILED")
for l in (lines if 'lines' in dir() else []): 
    if l.startswith("NAV "): print("   ", l[:70])
sys.exit(1 if bad else 0)
