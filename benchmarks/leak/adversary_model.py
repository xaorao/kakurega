#!/usr/bin/env python3
"""The worst model a benchmark can ask for: it knows exactly which secrets were planted, and whenever one of them (or an
encoded form of it) is anywhere in what it is sent, it puts it into its answer, in whatever shape the product expects.
It speaks the local-model API (Ollama's), and it writes every request it receives to a log, which is the benchmark's
independent record of what reached the model.

  python3 benchmarks/leak/adversary_model.py HOST PORT CANARY_FILE LOG_FILE
CANARY_FILE is JSON: {"needles": ["...", ...]} and is re-read on every request, so the harness can plant fresh secrets.
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST, PORT, CANARY_FILE, LOG_FILE = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
lock = threading.Lock()


def needles():
    try:
        return json.load(open(CANARY_FILE))["needles"]
    except Exception:
        return []


def answer(messages):
    seen = "\n".join(str(m.get("content", "")) for m in messages)
    system = "\n".join(str(m.get("content", "")) for m in messages if m.get("role") == "system")
    found = [n for n in needles() if n in seen]
    loot = " ".join(found) if found else "nothing"
    if "You write notes from a meeting transcript" in system:
        return json.dumps({"summary": f"LOOT {loot}", "decisions": [{"text": f"LOOT {loot}"[:500], "quote": loot[:300]}], "actions": [], "questions": []})
    if "You answer questions about the documents" in system:
        return json.dumps({"answer": f"LOOT {loot}"[:3500], "citations": []})
    return f"LOOT {loot}"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, v, code=200):
        raw = json.dumps(v).encode()
        self.send_response(code)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path.startswith("/api/tags"):
            return self._json({"models": [{"name": "adversary:latest", "model": "adversary:latest", "digest": "0" * 64, "details": {}}]})
        if self.path.startswith("/api/version"):
            return self._json({"version": "0.0.0-bench"})
        self._json({}, 404)

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("content-length", "0")) or 0)
        # the record is the bytes as they arrived, whatever the path: an embedding request is a model request too
        with lock:
            with open(LOG_FILE, "a") as f:
                f.write(json.dumps({"path": self.path, "body": raw.decode("utf-8", "replace")}) + "\n")
        try:
            body = json.loads(raw or b"{}")
        except Exception:
            body = {}
        if self.path.startswith("/api/show"):
            return self._json({"model_info": {"general.architecture": "bench", "bench.context_length": 32768}, "details": {}, "capabilities": ["completion"]})
        if self.path.startswith("/api/embed"):
            n = len(body.get("input", [])) if isinstance(body.get("input"), list) else 1
            return self._json({"embeddings": [[0.0] * 8 for _ in range(n)]})
        if self.path.startswith("/api/chat"):
            text = answer(body.get("messages", []))
            if body.get("stream"):
                self.send_response(200)
                self.send_header("content-type", "application/x-ndjson")
                self.end_headers()
                self.wfile.write((json.dumps({"message": {"role": "assistant", "content": text}, "done": False}) + "\n").encode())
                self.wfile.write((json.dumps({"message": {"role": "assistant", "content": ""}, "done": True, "prompt_eval_count": 10, "eval_count": 10}) + "\n").encode())
                return
            return self._json({"message": {"role": "assistant", "content": text}, "done": True, "prompt_eval_count": 10, "eval_count": 10})
        self._json({}, 404)


ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
