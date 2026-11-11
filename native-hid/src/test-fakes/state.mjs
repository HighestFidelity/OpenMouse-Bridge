// Shared state for the fake `node-hid` and fake driver classes used by the
// native-hid tests. Never loaded by production code.
//
// In a test process the state is filled in directly; in a child process
// (apply.mjs / button-listener.mjs run as a CLI) it comes from the
// FAKE_HID_SCENARIO env var, and every recorded call is also appended as a
// JSON line to FAKE_HID_LOG so the parent test can read it back.
import { appendFileSync } from "node:fs";

const scenario = process.env.FAKE_HID_SCENARIO ? JSON.parse(process.env.FAKE_HID_SCENARIO) : {};

export const state = (globalThis.__fakeHidState ??= {
  // node-hid `devices()` entries.
  devices: scenario.devices ?? [],
  // Paths whose `new HID(path)` throws.
  openFail: scenario.openFail ?? [],
  // Paths whose sendFeatureReport throws.
  featureFail: scenario.featureFail ?? [],
  // { [exportName]: { [productId | "*"]: spec } }, see fake-driver.mjs.
  drivers: scenario.drivers ?? {},
  log: [],
});

export function record(...event) {
  state.log.push(event);
  if (process.env.FAKE_HID_LOG) appendFileSync(process.env.FAKE_HID_LOG, `${JSON.stringify(event)}\n`);
}
