import { readFile } from "node:fs/promises";
import { join } from "node:path";

export async function readAdmin(dataDir) {
  const raw = JSON.parse(await readFile(join(dataDir, "admin.json"), "utf8"));
  if (!Number.isInteger(raw.port) || typeof raw.token !== "string" || !raw.token) throw new Error("damaged admin.json");
  return { base: `http://127.0.0.1:${raw.port}`, origin: `http://127.0.0.1:${raw.port}`, token: raw.token };
}

export function adminClient(admin, fetchImpl = fetch) {
  const call = async (method, path, body) => {
    const r = await fetchImpl(admin.base + path, {
      method,
      headers: { authorization: `Bearer ${admin.token}`, ...(body ? { "content-type": "application/json" } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    let json = null;
    try {
      json = await r.json();
    } catch {}
    return { status: r.status, json };
  };
  return { get: (p) => call("GET", p), post: (p, b = {}) => call("POST", p, b) };
}
