#!/usr/bin/env python3
"""Checks that what the public documents tell a reader to do is true, by doing it.

Every claim below is something a stranger would follow: a command, a script, a test name, a link, a flag, a tool list, a
configuration snippet. Each one is tested against the repository and the built `kaku` binary.

    python3 scripts/docs_check.py [--kaku target/debug/kaku]

Exits 1 and names each broken claim; exits 0 when every claim holds.
"""
import glob
import json
import re
import subprocess
import sys
from pathlib import Path

repo = Path(__file__).resolve().parent.parent
args = sys.argv[1:]
kaku = repo / (args[args.index("--kaku") + 1] if "--kaku" in args else "target/debug/kaku")
DOCS = [repo / "README.md", repo / "docs/mcp.md", repo / "docs/LIMITS.md"]
problems = []
checked = 0


def bad(where, what):
    problems.append(f"{where}: {what}")


def text_of(p):
    return p.read_text()


def rel(p):
    return str(p.relative_to(repo))


# 1. links and paths: every relative markdown link and every backticked repository path exists
for doc in DOCS:
    body = text_of(doc)
    for m in re.finditer(r"\]\(([^)#\s]+)(?:#[^)]*)?\)", body):
        target = m.group(1)
        if re.match(r"^[a-z]+:", target):
            continue
        checked += 1
        if not (doc.parent / target).exists() and not (repo / target).exists():
            bad(rel(doc), f"the link {target} does not exist")
    for m in re.finditer(r"`((?:docs|scripts|program|crates|apps|packages|fixtures|tests)/[A-Za-z0-9_./-]+)`", body):
        target = m.group(1).rstrip("/.")
        if "*" in target or "{" in target:
            continue
        checked += 1
        if not (repo / target).exists():
            bad(rel(doc), f"the path {target} does not exist")

# 2. commands in code blocks and inline: scripts exist, python files exist, cargo test targets exist
for doc in DOCS:
    body = text_of(doc)
    lines = [l.strip() for block in re.findall(r"```[a-z]*\n(.*?)```", body, re.S) for l in block.split("\n")]
    lines += re.findall(r"`([^`\n]+)`", body)
    for line in lines:
        line = re.sub(r"\s+#.*$", "", line)
        m = re.match(r"^(?:python3 )?((?:scripts|tests)/[A-Za-z0-9_./-]+)", line)
        if m:
            checked += 1
            if not (repo / m.group(1)).exists():
                bad(rel(doc), f"`{line}` names {m.group(1)}, which does not exist")
        m = re.match(r"^cargo test -p ([a-z0-9-]+)(?: --test ([A-Za-z0-9_]+))?", line)
        if m:
            checked += 1
            crate, test = m.groups()
            cdir = repo / "crates" / crate
            if not cdir.exists():
                bad(rel(doc), f"`{line}` names the crate {crate}, which does not exist")
            elif test and not (cdir / "tests" / f"{test}.rs").exists():
                bad(rel(doc), f"`{line}` names the test target {test}, but crates/{crate}/tests/{test}.rs does not exist")

# 3. `kaku ...` commands and flags are real
usage = subprocess.run([str(kaku), "--help"], capture_output=True, text=True).stdout
if not usage:
    bad("kaku", f"{kaku} did not print its usage: build it first (cargo build -p kk-cli)")
subcommands = set(re.findall(r"^\s+kaku ([a-z]+)", usage, re.M))
sources = "\n".join(Path(p).read_text() for p in glob.glob(str(repo / "crates/*/src/**/*.rs"), recursive=True))
for doc in DOCS:
    body = text_of(doc)
    for line in re.findall(r"`(kaku[^`\n]*)`", body) + re.findall(r"^\s*(?:\$ )?(kaku [^\n`]+)$", body, re.M):
        words = line.split()
        if len(words) > 1 and re.match(r"^[a-z]+$", words[1]):
            checked += 1
            if words[1] not in subcommands:
                bad(rel(doc), f"`{line}`: `kaku {words[1]}` is not a command (the commands are {sorted(subcommands)})")
        for flag in re.findall(r"(--[a-z][a-z-]+)", line):
            checked += 1
            if f'"{flag}"' not in sources:
                bad(rel(doc), f"`{line}`: the flag {flag} is not accepted by any command")

# 4. docs/mcp.md: the tool table is the real tool list, and its configuration snippet really starts a working server
mcp = text_of(repo / "docs/mcp.md")
documented = set(re.findall(r"`([a-z_]+)`", "\n".join(l for l in mcp.split("\n") if l.startswith("| `"))))
proc = subprocess.run(
    [str(kaku), "mcp", "--data-dir", str(repo / "target/docs-check-empty")],
    input='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"docs","version":"0"}}}\n'
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}\n',
    capture_output=True,
    text=True,
    timeout=30,
)
real = set()
for out in proc.stdout.strip().split("\n"):
    try:
        v = json.loads(out)
    except ValueError:
        continue
    if v.get("id") == 2:
        real = {t["name"] for t in v["result"]["tools"]}
checked += 1
if not real:
    bad("docs/mcp.md", "`kaku mcp` did not answer a tools/list")
else:
    if documented != real:
        bad("docs/mcp.md", f"the tool table lists {sorted(documented)} but the server offers {sorted(real)}")
for snippet in re.findall(r"```json\n(.*?)```", mcp, re.S):
    checked += 1
    try:
        cfg = json.loads(snippet)
    except ValueError as e:
        bad("docs/mcp.md", f"a json example does not parse: {e}")
        continue
    for name, server in cfg.get("mcpServers", {}).items():
        if server.get("command") != "kaku":
            bad("docs/mcp.md", f"the example for {name} does not start `kaku`")
        r = subprocess.run([str(kaku), *server.get("args", []), "--data-dir", str(repo / "target/docs-check-empty")], input='{"jsonrpc":"2.0","id":1,"method":"ping"}\n', capture_output=True, text=True, timeout=30)
        if '"id":1' not in r.stdout.replace(" ", "") or r.returncode != 0:
            bad("docs/mcp.md", f"starting `kaku {' '.join(server.get('args', []))}` as the example says does not answer: {r.stderr[:120]}")

# resources and prompts the document names are really offered, and the ones offered are named
def ask(method):
    r = subprocess.run(
        [str(kaku), "mcp", "--data-dir", str(repo / "target/docs-check-empty")],
        input='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"docs","version":"0"}}}\n'
        + json.dumps({"jsonrpc": "2.0", "id": 2, "method": method}) + "\n",
        capture_output=True,
        text=True,
        timeout=30,
    )
    for out in r.stdout.strip().split("\n"):
        try:
            v = json.loads(out)
        except ValueError:
            continue
        if v.get("id") == 2:
            return v.get("result", {})
    return {}


offered = {x["uri"] for x in ask("resources/list").get("resources", [])}
offered |= {x["uriTemplate"] for x in ask("resources/templates/list").get("resourceTemplates", [])}
offered_prompts = {x["name"] for x in ask("prompts/list").get("prompts", [])}
named = set(re.findall(r"`(kakurega://[^`\s]+)`", mcp))
# report links are made per result (`kakurega://verification/` and an id); they are not in any list, and the test mc43 proves them
named = {u for u in named if not u.startswith("kakurega://verification/")}
for uri in named:
    checked += 1
    template_form = re.sub(r"/job-\d+/", "/{id}/", uri)
    if uri not in offered and template_form not in offered:
        bad("docs/mcp.md", f"names {uri}, which the server does not offer")
for uri in offered:
    checked += 1
    if uri not in named:
        bad("docs/mcp.md", f"the server offers {uri}, which the document does not name")
for name in offered_prompts:
    checked += 1
    if f"`{name}`" not in mcp:
        bad("docs/mcp.md", f"the server offers the prompt {name}, which the document does not name")

# 5. modes the MCP document must explain, because an agent owner needs them
for needed in ("--read-only", "--allow", "--leased", "kaku agent"):
    checked += 1
    if needed not in mcp:
        bad("docs/mcp.md", f"does not explain {needed}")

if problems:
    print("\n".join("FAIL  " + p for p in problems))
    print(f"\n{len(problems)} documentation claim(s) are broken ({checked} checked)")
    sys.exit(1)
print(f"{checked} documentation claims checked, all true")
