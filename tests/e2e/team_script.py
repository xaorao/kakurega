#!/usr/bin/env python3
"""Stands in for a local model server (the Ollama chat API) for the team screen's browser test: it answers as a lead that
writes a plan, as experts doing tasks (one of which asks the owner a question first), and as the lead closing.

  python3 tests/e2e/team_script.py PORT
"""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

asked_owner = {"done": False}


def answer(messages):
    system = "\n".join(m.get("content", "") for m in messages if m.get("role") == "system")
    last = messages[-1].get("content", "") if messages else ""
    if "You are the lead of a small team" in system:
        if "refuse-this-plan" in last:
            return "I think the boss should do it all."
        return (
            "Here is the plan.\n"
            "1 | security-reviewer | after: - | List the ways the payment key could leak.\n"
            "2 | software-architect | after: - | Say where the key should live and what that costs.\n"
            "3 | plain-writer | after: 1, 2 | Write the decision in five plain lines for the board.\n"
        )
    if "The team's tasks have ended" in system:
        return "**Keep the payment key in the vault.** The reviewer and the architect agree.\n\nNot done: Nothing."
    if "Your task (task 2 of 3)" in last:
        if "You asked the owner" not in last and not asked_owner["done"]:
            asked_owner["done"] = True
            return "NEEDS INPUT: Which cloud do you deploy to?"
        return "Put it in the cloud's own vault.\n\n- cost: one more service\n- gain: the key is never in the code"
    if "Your task (task 1 of 3)" in last:
        time.sleep(1.5)
        return "1. It leaks through logs.\n2. It leaks through the repository history.\n<script>window.pwned = 1</script>"
    if "Your task (task 3 of 3)" in last:
        return "The key moves to the vault this week."
    return "ok"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _json(self, body, status=200):
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path.startswith("/api/tags"):
            return self._json({"models": [{"name": "team-script:latest", "model": "team-script:latest", "digest": "0" * 64, "details": {}}]})
        if self.path.startswith("/api/version"):
            return self._json({"version": "0.0.0-test"})
        self._json({}, 404)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", "0"))) or b"{}")
        if self.path.startswith("/api/show"):
            return self._json({"model_info": {"general.architecture": "test", "test.context_length": 8192}, "details": {}, "capabilities": ["completion"]})
        if self.path.startswith("/api/chat"):
            text = answer(body.get("messages", []))
            if body.get("stream"):
                self.send_response(200)
                self.send_header("content-type", "application/x-ndjson")
                self.end_headers()
                for word in text.split(" "):
                    self.wfile.write((json.dumps({"message": {"role": "assistant", "content": word + " "}, "done": False}) + "\n").encode())
                    self.wfile.flush()
                self.wfile.write((json.dumps({"message": {"role": "assistant", "content": ""}, "done": True, "prompt_eval_count": 10, "eval_count": 10}) + "\n").encode())
                return
            return self._json({"message": {"role": "assistant", "content": text}, "done": True, "prompt_eval_count": 10, "eval_count": 10})
        self._json({}, 404)


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
