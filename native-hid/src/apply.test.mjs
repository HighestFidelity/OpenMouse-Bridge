// apply.mjs run as Bridge runs it (a child process, request on stdin), with
// node-hid and the driver classes replaced by test-fakes/. Exit codes are the
// contract src/drivers/native_hid.rs depends on: 0 applied, 1 failed, 3 no driver.
import assert from "node:assert/strict";
import test from "node:test";

import { calls, runCli } from "./test-fakes/run-cli.mjs";

const EGG = 0x3367;
const vendorInterface = { usagePage: 0xff00, usage: 1, interface: 1 };
const egg = (productId, path) => ({ vendorId: EGG, productId, path, product: `EGG ${productId}`, ...vendorInterface });

test("unknown brand exits 3 without opening anything", async () => {
  const result = await runCli("apply.mjs", { devices: [egg(1, "egg-a")] }, { brand: "NoSuchBrand", dpi: 800 });
  assert.equal(result.code, 3);
  assert.match(result.stderr, /no driver registered for brand "NoSuchBrand"/);
  assert.deepEqual(result.log, []);
});

test("probe falls through classes, then devices, past a hung open, and applies to the first that answers", async () => {
  const scenario = {
    devices: [egg(1, "egg-a"), egg(2, "egg-b")],
    drivers: {
      EggOp1HidClient: { 1: { open: "hang" }, 2: { readStatus: "throw" } },
      EggWeHidClient: { 1: { readStatus: "throw" }, 2: { status: { name: "OP1we" } } },
    },
  };
  const result = await runCli("apply.mjs", scenario, { brand: "Endgame Gear", dpi: 800, pollingRateHz: 1000 });
  assert.equal(result.code, 0, result.stderr);
  // BRAND_DRIVERS order within a device, devices in enumeration order.
  assert.deepEqual(calls(result.log, "open").map(([, name, pid]) => `${name}@${pid}`), [
    "EggOp1HidClient@1",
    "EggWeHidClient@1",
    "EggOp1HidClient@2",
    "EggWeHidClient@2",
  ]);
  // Every failed probe closed the HID handle before the next class tried it.
  assert.deepEqual(calls(result.log, "hid.close").map(([, path]) => path).slice(0, 3), ["egg-a", "egg-a", "egg-b"]);
  // DPI first, then polling rate, both on the winning client, then closed.
  const writes = result.log.filter(([step]) => ["setDpi", "setPollingRate", "close"].includes(step));
  assert.deepEqual(writes, [
    ["setDpi", "EggWeHidClient", 2, 800],
    ["setPollingRate", "EggWeHidClient", 2, 1000],
    ["close", "EggWeHidClient", 2],
  ]);
});

test("no device answering exits 1 and reports every attempt, timeouts included", async () => {
  const scenario = {
    devices: [egg(1, "egg-a")],
    drivers: { EggOp1HidClient: { "*": { open: "hang" } }, EggWeHidClient: { "*": { readStatus: "hang" } } },
  };
  const result = await runCli("apply.mjs", scenario, { brand: "Endgame Gear", dpi: 800 });
  assert.equal(result.code, 1);
  assert.match(result.stderr, /no Endgame Gear device answered\. Tried:/);
  assert.match(result.stderr, /EggOp1HidClient on EGG 1: EggOp1HidClient\.open\(\) timed out after 3000ms/);
  assert.match(result.stderr, /EggWeHidClient on EGG 1: EggWeHidClient\.readStatus\(\) timed out after 3000ms/);
  assert.deepEqual(calls(result.log, "setDpi"), []);

  // Only a boot-mouse collection of the vendor: filtered out, nothing to try.
  const mouseOnly = { devices: [{ ...egg(1, "egg-a"), usagePage: 1, usage: 2 }], drivers: scenario.drivers };
  const empty = await runCli("apply.mjs", mouseOnly, { brand: "Endgame Gear", dpi: 800 });
  assert.equal(empty.code, 1);
  assert.match(empty.stderr, /No matching HID interface was found\./);
  assert.deepEqual(calls(empty.log, "open"), []);
});

test("status action prints the probed status (nulls for unknown fields) and exits 0", async () => {
  const status = { batteryPercent: 41, batteryState: "Discharging", name: "OP1we", dpi: 1600 };
  const scenario = { devices: [egg(1, "egg-a")], drivers: { EggOp1HidClient: { "*": { status } } } };
  const result = await runCli("apply.mjs", scenario, { brand: "Endgame Gear", action: "status" });
  assert.equal(result.code, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), {
    batteryPercent: 41,
    batteryState: "Discharging",
    deviceName: "OP1we",
    dpi: 1600,
    pollingRateHz: null,
    switchMode: null,
  });
  assert.deepEqual(calls(result.log, "setDpi"), []);
  assert.equal(calls(result.log, "close").length, 1);
});

test("a failed or hung write exits 1, skips the rest and still closes the client", async () => {
  for (const setDpi of ["throw", "hang"]) {
    const scenario = { devices: [egg(1, "egg-a")], drivers: { EggOp1HidClient: { "*": { setDpi } } } };
    const result = await runCli("apply.mjs", scenario, { brand: "Endgame Gear", dpi: 800, pollingRateHz: 1000 });
    assert.equal(result.code, 1, setDpi);
    assert.match(result.stderr, setDpi === "throw" ? /could not apply settings: setDpi failed/ : /could not apply settings: setDpi timed out after 3000ms/);
    assert.deepEqual(calls(result.log, "setPollingRate"), []);
    assert.equal(calls(result.log, "close").length, 1);
  }
});

test("flash saves are refused without confirmed: true and sent with it", async () => {
  const wooting = { vendorId: 0x31e3, productId: 0x1310, path: "wooting", product: "60HE+", ...vendorInterface };
  const scenario = { devices: [wooting], drivers: { WootingHidClient: { "*": {} } } };
  const refused = await runCli("apply.mjs", scenario, { brand: "Wooting", action: "save-rgb", slot: 1 });
  assert.equal(refused.code, 1);
  assert.match(refused.stderr, /overwrites onboard flash: pass confirmed: true/);
  assert.deepEqual(calls(refused.log, "saveProfile"), []);

  const saved = await runCli("apply.mjs", scenario, { brand: "Wooting", action: "save-rgb", slot: 1, confirmed: true });
  assert.equal(saved.code, 0, saved.stderr);
  assert.deepEqual(calls(saved.log, "saveProfile"), [["saveProfile", "WootingHidClient", 0x1310, 0x08, 1, true]]);
});

test("Attack Shark X11: polling-only takes the direct feature report, DPI goes through the driver", async () => {
  const x11 = { vendorId: 0x1d57, productId: 0xfa60, interface: 2, path: "x11", product: "X11", usagePage: 0xff02, usage: 2 };
  const scenario = { devices: [x11], drivers: { AttackSharkHidClient: { "*": {} } } };
  const direct = await runCli("apply.mjs", scenario, { brand: "Attack Shark", pollingRateHz: 1000 });
  assert.equal(direct.code, 0, direct.stderr);
  assert.deepEqual(calls(direct.log, "hid.sendFeatureReport"), [["hid.sendFeatureReport", "x11", [0x06, 0x09, 0x01, 0x01, 0xfe, 0, 0, 0, 0]]]);
  assert.deepEqual(calls(direct.log, "open"), []);

  const withDpi = await runCli("apply.mjs", scenario, { brand: "Attack Shark", dpi: 800, pollingRateHz: 1000 });
  assert.equal(withDpi.code, 0, withDpi.stderr);
  assert.deepEqual(calls(withDpi.log, "hid.sendFeatureReport"), []);
  assert.deepEqual(calls(withDpi.log, "setDpi"), [["setDpi", "AttackSharkHidClient", 0xfa60, 800]]);
});
