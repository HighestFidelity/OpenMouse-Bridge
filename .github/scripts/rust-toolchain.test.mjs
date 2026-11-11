// Run with: node --test .github/scripts/
//
// rust-toolchain.toml pins the Rust release; Cargo.toml's rust-version and
// every dtolnay/rust-toolchain step must name the same one, or CI would
// install one toolchain and rustup would switch to (and download) another.

import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

function pinnedChannel() {
  const channel = read("rust-toolchain.toml").match(/^channel = "([^"]+)"$/m);
  assert.ok(channel, "rust-toolchain.toml has no channel");
  assert.match(channel[1], /^\d+\.\d+\.\d+$/, "rust-toolchain.toml must pin an exact release");
  return channel[1];
}

test("rust-toolchain.toml carries clippy and rustfmt", () => {
  assert.match(read("rust-toolchain.toml"), /^components = \["clippy", "rustfmt"\]$/m);
});

test("Cargo.toml rust-version matches rust-toolchain.toml", () => {
  const rustVersion = read("Cargo.toml").match(/^rust-version = "([^"]+)"$/m);
  assert.ok(rustVersion, "Cargo.toml has no rust-version");
  assert.equal(rustVersion[1], pinnedChannel());
});

test("every rust-toolchain step installs the pinned release", () => {
  const channel = pinnedChannel();
  const workflowsDir = path.join(root, ".github", "workflows");
  let steps = 0;
  for (const file of readdirSync(workflowsDir).filter((name) => name.endsWith(".yml"))) {
    const text = readFileSync(path.join(workflowsDir, file), "utf8");
    for (const step of text.split(/^(?=\s+- )/m).filter((s) => s.includes("dtolnay/rust-toolchain@"))) {
      steps += 1;
      const toolchain = step.match(/^\s+toolchain: (\S+)/m);
      assert.ok(toolchain, `${file}: a rust-toolchain step names no toolchain`);
      assert.equal(toolchain[1], channel, `${file}: rust-toolchain step installs ${toolchain[1]}`);
    }
  }
  assert.equal(steps, 7);
});
