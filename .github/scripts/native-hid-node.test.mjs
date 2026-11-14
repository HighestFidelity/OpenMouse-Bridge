// Run with: node --test .github/scripts/

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { NODE_SHA256, NODE_VERSION, pinnedEnv, setupNodeVersion } from "./native-hid-node.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, "native-hid-node.mjs");
const workflowsDir = path.join(here, "..", "workflows");

/** Each workflow's jobs as { file, job, text }, split on two-space job keys. */
function workflowJobs() {
  const jobs = [];
  for (const file of readdirSync(workflowsDir).filter((name) => name.endsWith(".yml"))) {
    const text = readFileSync(path.join(workflowsDir, file), "utf8");
    const body = text.split(/^jobs:\n/m)[1] ?? "";
    for (const chunk of body.split(/^(?=  [A-Za-z0-9_-]+:\n)/m)) {
      const name = chunk.match(/^  ([A-Za-z0-9_-]+):\n/);
      if (name) jobs.push({ file, job: name[1], text: chunk });
    }
  }
  return jobs;
}

test("the pins are well formed", () => {
  const env = pinnedEnv();
  assert.equal(env.NATIVE_HID_NODE_VERSION, NODE_VERSION);
  assert.equal(Object.keys(env).length, 5);
  for (const [key, hash] of Object.entries(NODE_SHA256)) {
    assert.equal(env[`NATIVE_HID_NODE_SHA256_${key}`], hash);
  }
  assert.equal(setupNodeVersion(), NODE_VERSION.slice(1));
});

test("bad pins are refused", () => {
  assert.throws(() => pinnedEnv("24.18.1"), /not like v24\.18\.1/);
  assert.throws(() => pinnedEnv("v24"), /not like/);
  const { WIN_X64: _dropped, ...missing } = NODE_SHA256;
  assert.throws(() => pinnedEnv(NODE_VERSION, missing), /must be exactly/);
  assert.throws(() => pinnedEnv(NODE_VERSION, { ...NODE_SHA256, EXTRA: NODE_SHA256.WIN_X64 }), /must be exactly/);
  assert.throws(() => pinnedEnv(NODE_VERSION, { ...NODE_SHA256, LINUX_X64: "ABC" }), /lowercase SHA-256/);
  assert.throws(
    () => pinnedEnv(NODE_VERSION, { ...NODE_SHA256, LINUX_X64: NODE_SHA256.LINUX_X64.toUpperCase() }),
    /lowercase SHA-256/,
  );
});

test("writes GITHUB_ENV and GITHUB_OUTPUT the way Actions reads them", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "native-hid-node-"));
  try {
    const envFile = path.join(dir, "env");
    const outputFile = path.join(dir, "output");
    writeFileSync(envFile, "EARLIER=1\n");
    writeFileSync(outputFile, "");
    execFileSync(process.execPath, [script], {
      env: { ...process.env, GITHUB_ENV: envFile, GITHUB_OUTPUT: outputFile },
    });
    const expectedEnv = Object.entries(pinnedEnv())
      .map(([key, value]) => `${key}=${value}\n`)
      .join("");
    assert.equal(readFileSync(envFile, "utf8"), `EARLIER=1\n${expectedEnv}`);
    assert.equal(readFileSync(outputFile, "utf8"), `version=${setupNodeVersion()}\n`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("no workflow carries its own copy of the pins", () => {
  for (const file of readdirSync(workflowsDir).filter((name) => name.endsWith(".yml"))) {
    const text = readFileSync(path.join(workflowsDir, file), "utf8");
    assert.doesNotMatch(text, /NATIVE_HID_NODE_[A-Z0-9_]*:/, `${file} defines a Node.js pin`);
    for (const hash of Object.values(NODE_SHA256)) {
      assert.ok(!text.includes(hash), `${file} hardcodes a Node.js archive hash`);
    }
    assert.ok(!text.includes(NODE_VERSION.slice(1)), `${file} hardcodes the Node.js version`);
  }
});

test("every job that uses the pins reads them first and runs that Node.js", () => {
  const readStep = "run: node .github/scripts/native-hid-node.mjs";
  const pinned = "node-version: ${{ steps.native-hid-node.outputs.version }}";
  let readers = 0;
  for (const { file, job, text } of workflowJobs()) {
    const where = `${file} job ${job}`;
    const readAt = text.indexOf(readStep);
    if (readAt === -1) {
      assert.ok(!text.includes("NATIVE_HID_NODE_"), `${where} uses the pins without reading them`);
      assert.ok(!text.includes("steps.native-hid-node"), `${where} has no native-hid-node step`);
      continue;
    }
    readers += 1;
    const setupAt = text.indexOf("actions/setup-node@");
    assert.ok(setupAt > readAt, `${where} must read the pins before actions/setup-node`);
    assert.ok(text.includes(pinned), `${where} must give setup-node the pinned version`);
    assert.doesNotMatch(text, /node-version: (?!\$\{\{ steps\.native-hid-node)/, `${where} sets another Node.js`);
    const firstUse = text.indexOf("NATIVE_HID_NODE_");
    assert.ok(firstUse === -1 || firstUse > readAt, `${where} uses the pins before reading them`);
  }
  // ci.yml, update-mouse-protocol.yml, update-keyboard-protocol.yml, and the
  // three build jobs each in gpui-build.yml and release.yml.
  assert.equal(readers, 9);
});
