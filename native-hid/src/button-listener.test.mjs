// button-listener.mjs run as Bridge runs it (a long-lived child: one JSON line
// on stdin starts it, closing stdin stops it), with test-fakes/ in place of
// node-hid and the Logitech driver. Exit codes: 0 clean stop, 1 failure, 3 no driver.
import assert from "node:assert/strict";
import test from "node:test";

import { calls, runCli, startCli } from "./test-fakes/run-cli.mjs";

const mouse = (productId, path) => ({ vendorId: 0x046d, productId, path, product: `Mouse ${productId}`, usagePage: 0xff00, usage: 1, interface: 2 });
const logitech = (specs) => ({ devices: [mouse(1, "lg-1")], drivers: { LogitechHidppClient: specs } });
const MODE_STEPS = ["setOnboardMode", "startButtonListener", "stopButtonListener", "close"];
// Starts the helper and leaves stdin open, as Bridge does, until it exits by itself.
const runOpen = (scenario, input) => {
  const cli = startCli("button-listener.mjs", scenario);
  cli.stdin.write(typeof input === "string" ? input : `${JSON.stringify(input)}\n`);
  return cli.done;
};
const modeSteps = (log) => log.filter(([step]) => MODE_STEPS.includes(step)).map(([step, , , arg]) => (arg ? `${step}:${arg}` : step));

test("an Onboard mouse is switched to Host, reports buttons, and is put back when stdin closes", async () => {
  const cli = startCli("button-listener.mjs", logitech({ "*": { status: { name: "PRO X", deviceMode: "Onboard" } } }));
  cli.stdin.write(`${JSON.stringify({ brand: "Logitech", device: "PRO X" })}\n`);
  assert.deepEqual(await cli.next("ready"), { event: "ready", priorMode: "Onboard", buttons: [1, 2, 3, 4, 5] });
  assert.deepEqual(await cli.next("buttons"), { event: "buttons", mask: 16 });
  cli.stdin.end();
  const result = await cli.done;
  assert.equal(result.code, 0, result.stderr);
  assert.match(result.stdout, /"event":"stopped"/);
  assert.deepEqual(modeSteps(result.log), [
    "setOnboardMode:Host",
    "startButtonListener",
    "stopButtonListener",
    "setOnboardMode:Onboard",
    "close",
  ]);
});

test("a failed listener start exits 1 and still puts the mouse back", async () => {
  for (const startButtonListener of ["throw", "hang"]) {
    const scenario = logitech({ "*": { status: { deviceMode: "Onboard" }, startButtonListener } });
    const result = await runOpen(scenario, { brand: "Logitech" });
    assert.equal(result.code, 1, startButtonListener);
    assert.match(result.stdout, startButtonListener === "throw" ? /"message":"startButtonListener failed"/ : /startButtonListener timed out after 3000ms/);
    assert.doesNotMatch(result.stdout, /"event":"ready"/);
    assert.deepEqual(modeSteps(result.log).slice(-3), ["stopButtonListener", "setOnboardMode:Onboard", "close"]);
  }
});

test("requireHost leaves a mouse on an onboard profile alone", async () => {
  const scenario = logitech({ "*": { status: { deviceMode: "Onboard" } } });
  const result = await runOpen(scenario, { brand: "Logitech", requireHost: true });
  assert.equal(result.code, 1);
  assert.match(result.stdout, /the mouse is not on the Host profile/);
  assert.deepEqual(modeSteps(result.log), ["close"]);
});

test("a stop during discovery exits 0 without changing the mode", async () => {
  // readStatus takes 2 s (scaled, under the 3 s timeout); runCli closes stdin
  // right after the start line, so the stop lands while discovery is still probing.
  const scenario = logitech({ "*": { status: { deviceMode: "Onboard" }, readStatus: 2000 } });
  const result = await runCli("button-listener.mjs", scenario, `${JSON.stringify({ brand: "Logitech" })}\n`);
  assert.equal(result.code, 0, result.stderr);
  assert.match(result.stdout, /"event":"stopped"/);
  assert.doesNotMatch(result.stdout, /"event":"ready"/);
  assert.deepEqual(modeSteps(result.log), ["close"]);
});

test("with several mice only the named one is used and the others are closed", async () => {
  const scenario = {
    devices: [mouse(1, "lg-1"), mouse(2, "lg-2")],
    drivers: { LogitechHidppClient: { 1: { status: { name: "G502", deviceMode: "Host" } }, 2: { status: { name: "PRO X", deviceMode: "Host" } } } },
  };
  const cli = startCli("button-listener.mjs", scenario);
  cli.stdin.write(`${JSON.stringify({ brand: "Logitech", device: "pro x" })}\n`);
  await cli.next("ready");
  cli.stdin.end();
  const result = await cli.done;
  assert.equal(result.code, 0, result.stderr);
  const steps = result.log.filter(([step]) => ["close", "startButtonListener", "setOnboardMode"].includes(step)).map(([step, , pid]) => `${step}@${pid}`);
  // Host already: no mode switch; the other mouse is let go before listening.
  assert.deepEqual(steps, ["close@1", "startButtonListener@2", "close@2"]);

  const wrongName = await runOpen(scenario, { brand: "Logitech", device: "Viper" });
  assert.equal(wrongName.code, 1);
  assert.match(wrongName.stdout, /none of the 2 connected mice is named \\"Viper\\"/);
  assert.equal(calls(wrongName.log, "close").length, 2);
  assert.deepEqual(calls(wrongName.log, "startButtonListener"), []);
});

test("start-up failures: no driver exits 3, bad JSON and a protocol without the listener exit 1", async () => {
  const noDriver = await runOpen({ devices: [] }, { brand: "Nope" });
  assert.equal(noDriver.code, 3);
  assert.match(noDriver.stdout, /no driver registered for brand \\"Nope\\"/);

  const badJson = await runOpen({ devices: [] }, "not json\n");
  assert.equal(badJson.code, 1);
  assert.match(badJson.stdout, /the first line on stdin must be JSON/);

  const old = await runOpen(logitech({ "*": { noListener: true } }), { brand: "Logitech" });
  assert.equal(old.code, 1);
  assert.match(old.stdout, /no mouse answered\. LogitechHidppClient: this protocol version has no button listener/);
  assert.equal(calls(old.log, "close").length, 1);
});
