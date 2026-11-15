import test from "node:test";
import assert from "node:assert/strict";
import { setupCommand } from "../src/setup.ts";

test("su01 the agent setup command names this installation's profile or data folder, so it connects to this Kakurega", () => {
  assert.equal(setupCommand(null), "kaku mcp install --client claude-code --print --leased");
  assert.equal(setupCommand({ profile: "work" }), "kaku --profile work mcp install --client claude-code --print --leased");
  assert.equal(setupCommand({ data_dir: "/Users/a/kk data" }), "kaku mcp install --client claude-code --print --leased --data-dir '/Users/a/kk data'");
  assert.equal(setupCommand({ data_dir: "/tmp/it's" }), "kaku mcp install --client claude-code --print --leased --data-dir '/tmp/it'\\''s'");
});
