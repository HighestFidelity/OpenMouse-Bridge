// The one place for the Node.js runtime bundled into native-hid/node/, so
// Bridge can push DPI/polling-rate settings to non-Pulsar mice without end
// users installing Node.js. ci.yml, gpui-build.yml, release.yml and
// update-mouse-protocol.yml all read it, so the helper is tested, built and
// shipped on the same Node.js. Bump by hand when a newer LTS ships: the
// version and the SHA-256 of each archive together, from
// https://nodejs.org/dist/<version>/SHASUMS256.txt.
//
// Usage (in a workflow step, with the runner's preinstalled node):
//   node .github/scripts/native-hid-node.mjs
// Appends NATIVE_HID_NODE_VERSION and NATIVE_HID_NODE_SHA256_* to $GITHUB_ENV
// for the job's later steps, and `version` (no leading v, for
// actions/setup-node) to $GITHUB_OUTPUT. Outside Actions it prints both.

import { appendFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const NODE_VERSION = "v24.18.1";

/** SHA-256 of each archive the workflows download, keyed by env-var suffix. */
export const NODE_SHA256 = {
  DARWIN_ARM64: "eb02f7fab96d3d67de40c5ec8566096fcb4c2026728787683ae5a97eb612b941",
  DARWIN_X64: "6fb20fceacbb157c2f95825b80df4a454a0f6d81cdcd7bb81eeae9147e0e76ec",
  LINUX_X64: "d6c664df3f3f61458e8c277585571328522d705166723a7c7823a9253a4d15a0",
  WIN_X64: "ec56b84a7551893ab2324ebdfdc4ab974a63b4781162600b68a1293cc3e53765",
};

/** The env vars the workflows' download steps verify against, or an error. */
export function pinnedEnv(version = NODE_VERSION, hashes = NODE_SHA256) {
  if (!/^v\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`Node.js version ${version} is not like v24.18.1`);
  }
  const expected = ["DARWIN_ARM64", "DARWIN_X64", "LINUX_X64", "WIN_X64"];
  const keys = Object.keys(hashes).sort();
  if (keys.join() !== expected.join()) {
    throw new Error(`Node.js archive hashes must be exactly ${expected.join(", ")}; got ${keys.join(", ")}`);
  }
  const env = { NATIVE_HID_NODE_VERSION: version };
  for (const key of expected) {
    if (!/^[0-9a-f]{64}$/.test(hashes[key])) {
      throw new Error(`NATIVE_HID_NODE_SHA256_${key} is not a lowercase SHA-256`);
    }
    env[`NATIVE_HID_NODE_SHA256_${key}`] = hashes[key];
  }
  return env;
}

/** The version as actions/setup-node takes it: `24.18.1`. */
export function setupNodeVersion(version = NODE_VERSION) {
  return pinnedEnv(version).NATIVE_HID_NODE_VERSION.slice(1);
}

function lines(object) {
  return Object.entries(object)
    .map(([key, value]) => `${key}=${value}\n`)
    .join("");
}

function main() {
  const env = lines(pinnedEnv());
  const output = lines({ version: setupNodeVersion() });
  if (process.env.GITHUB_ENV) appendFileSync(process.env.GITHUB_ENV, env);
  else process.stdout.write(env);
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, output);
  else process.stdout.write(output);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main();
}
