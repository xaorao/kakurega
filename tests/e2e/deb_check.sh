#!/usr/bin/env bash
# Installs the .deb on this Linux machine, starts the installed launcher on a virtual display, and checks what a user gets:
# files, ownership, the sandbox helper, the closed fuses, a planted loose app folder being ignored, the service answering
# with the packaged web interface, the service stopping with the window, and a clean removal. Needs sudo and xvfb.
set -uo pipefail
deb="$1"; version="$2"
pass=0; fail=0
check() { if [ "$2" = "0" ]; then echo "PASS  $1"; pass=$((pass+1)); else echo "FAIL  $1  $3"; fail=$((fail+1)); fi; }
sudo dpkg -r kakurega >/dev/null 2>&1
sudo dpkg -i "$deb" >/tmp/deb-install.log 2>&1; check "the package installs" $? "$(tail -3 /tmp/deb-install.log)"
dpkg -s kakurega 2>/dev/null | grep -q "Version: $version"; check "dpkg reports version $version" $?
[ "$(stat -c '%U:%G %a' /opt/kakurega/chrome-sandbox)" = "root:root 4755" ]; check "the sandbox helper is root-owned and setuid" $? "$(stat -c '%U:%G %a' /opt/kakurega/chrome-sandbox)"
bad=$(sudo find /opt/kakurega -not -user root | head -3); [ -z "$bad" ]; check "every installed file is owned by root" $? "$bad"
bad=$(find /opt/kakurega -perm -0002 -not -type l | head -3); [ -z "$bad" ]; check "nothing installed is writable by everyone" $? "$bad"
[ ! -e /opt/kakurega/resources/default_app.asar ] && [ -f /opt/kakurega/resources/app.asar ]; check "the app is one asar and the stock demo app is gone" $?
[ -x /usr/bin/kakurega ] && [ -x /opt/kakurega/kaku ] && [ -x /opt/kakurega/kk-worker ] && [ -f /opt/kakurega/web/index.html ]; check "launcher, service, worker and web interface are installed" $?
FUSES=$(cd apps/desktop && node --input-type=module -e 'import {getCurrentFuseWire} from "@electron/fuses"; console.log(JSON.stringify(await getCurrentFuseWire("/opt/kakurega/kakurega")))' 2>&1)
echo "$FUSES" | grep -q '"0":48,"1":49,"2":48,"3":48'; check "RunAsNode is off, cookie encryption on, NODE_OPTIONS and --inspect off" $? "$FUSES"
echo "$FUSES" | grep -q '"5":49'; check "only the asar is loaded" $? "$FUSES"
echo "$FUSES" | grep -q '"7":48'; check "the file protocol has no extra privileges" $? "$FUSES"
# the runtime cannot be turned into a Node interpreter
OUT=$(ELECTRON_RUN_AS_NODE=1 timeout 15 /opt/kakurega/kakurega -e 'console.log("NODE-MODE")' 2>&1 </dev/null | head -2)
echo "$OUT" | grep -q "NODE-MODE"; [ $? -ne 0 ]; check "ELECTRON_RUN_AS_NODE does not give a Node interpreter" $? "$OUT"
# start the installed launcher on a virtual display
export HOME_BEFORE="$HOME"; TMPHOME=$(mktemp -d); export HOME="$TMPHOME"
MARK="$TMPHOME/loose-app-ran"; sudo mkdir -p /opt/kakurega/resources/app && echo "require('fs').writeFileSync('$MARK','ran')" | sudo tee /opt/kakurega/resources/app/main.js >/dev/null; echo '{"name":"loose","main":"main.js"}' | sudo tee /opt/kakurega/resources/app/package.json >/dev/null
xvfb-run -a /usr/bin/kakurega >/tmp/deb-run.log 2>&1 &
LPID=$!
for i in $(seq 1 60); do [ -f "$HOME/.kakurega/admin.json" ] && break; sleep 0.5; done
[ -f "$HOME/.kakurega/admin.json" ]; check "the installed launcher starts the service" $? "$(tail -5 /tmp/deb-run.log)"
PORT=$(python3 -c "import json,os;print(json.load(open(os.path.expanduser('~/.kakurega/admin.json')))['port'])" 2>/dev/null)
sleep 3
CODE=$(curl -s -o /tmp/deb-index.html -w '%{http_code}' "http://127.0.0.1:$PORT/"); grep -qi "<!doctype html" /tmp/deb-index.html; check "the service serves the packaged web interface ($CODE)" $?
[ ! -e "$MARK" ]; check "a loose app folder planted beside the asar is ignored and never runs" $?
sudo rm -rf /opt/kakurega/resources/app
pkill -f "/opt/kakurega/kakurega" >/dev/null 2>&1; kill $LPID >/dev/null 2>&1
for i in $(seq 1 30); do pgrep -f "/opt/kakurega/kaku serve" >/dev/null || break; sleep 0.5; done
pgrep -f "/opt/kakurega/kaku serve" >/dev/null; [ $? -ne 0 ]; check "the service stops when the window is closed" $?
export HOME="$HOME_BEFORE"; rm -rf "$TMPHOME"
sudo dpkg -r kakurega >/tmp/deb-remove.log 2>&1; check "the package removes" $?
[ ! -e /opt/kakurega ] && [ ! -e /usr/bin/kakurega ] && [ ! -e /usr/share/applications/kakurega.desktop ]; check "removal leaves none of its files behind" $?
echo; if [ $fail -eq 0 ]; then echo "all $pass package checks passed"; else echo "$fail FAILED"; exit 1; fi
