// HidDeviceAdapter / candidateDevices against fake node-hid handles. The
// module imports node-hid at top level, so register.mjs resolves it to
// test-fakes/fake-node-hid.mjs before the dynamic import below.
import assert from "node:assert/strict";
import test from "node:test";

import "./test-fakes/register.mjs";
import { state } from "./test-fakes/state.mjs";

const { HidDeviceAdapter, candidateDevices } = await import("./hid-device-adapter.mjs");

/** A node-hid HID handle; `fail` lists the methods that throw on it. */
function fakeHandles({ fail = {}, info = {}, featureReply = {} } = {}) {
  const opened = [];
  class Handle {
    constructor(path, options) {
      if (fail[path]?.includes("open")) throw new Error(`open ${path} refused`);
      this.path = path;
      this.options = options;
      this.events = {};
      this.writes = [];
      this.features = [];
      this.closed = false;
      this.detached = false;
      opened.push(this);
    }
    getDeviceInfo() { return info[this.path]; }
    on(event, listener) { this.events[event] = listener; }
    removeAllListeners() { this.detached = true; this.events = {}; }
    write(frame) {
      if (fail[this.path]?.includes("write")) throw new Error(`write ${this.path} refused`);
      this.writes.push(frame);
    }
    sendFeatureReport(frame) {
      if (fail[this.path]?.includes("feature")) throw new Error(`feature ${this.path} refused`);
      this.features.push(frame);
    }
    getFeatureReport(reportId, length) {
      if (fail[this.path]?.includes("getFeature")) throw new Error(`getFeature ${this.path} refused`);
      this.requested = [reportId, length];
      return featureReply[this.path];
    }
    close() { this.closed = true; }
  }
  return { Handle, opened };
}

const info = (path, extra = {}) => ({ vendorId: 0x046d, productId: 0xc547, interface: 2, path, product: "Receiver", usagePage: 0xff00, usage: 1, ...extra });

test("candidateDevices groups split collections per interface, opens each path once, skips keyboard and boot-mouse collections", () => {
  state.devices = [
    info("a", { usage: 1 }),
    info("a", { usage: 2 }), // second top-level collection, same path
    info("b", { usagePage: 0xff43 }), // same interface, other path (macOS split)
    info("c", { interface: 1 }), // other interface
    info("d", { productId: 0xc548 }), // other product
    info("kbd", { usagePage: 1, usage: 6 }),
    info("mouse", { usagePage: 1, usage: 2 }),
    info("other-vendor", { vendorId: 0x1532 }),
  ];
  const adapters = candidateDevices(0x046d);
  assert.deepEqual(adapters.map((adapter) => adapter._infos.map((entry) => entry.path)), [["a", "b"], ["c"], ["d"]]);
  assert.deepEqual(adapters.map((adapter) => [adapter.vendorId, adapter.productId, adapter.productName]), [
    [0x046d, 0xc547, "Receiver"],
    [0x046d, 0xc547, "Receiver"],
    [0x046d, 0xc548, "Receiver"],
  ]);
  assert.equal(candidateDevices(0x9999).length, 0);
});

test("open keeps every split that opens, shared, and fails only when none does", async () => {
  const { Handle, opened } = fakeHandles({ fail: { b: ["open"], x: ["open"], y: ["open"] } });
  const adapter = new HidDeviceAdapter([info("a"), info("b"), info("c")], null, Handle);
  await adapter.open();
  assert.equal(adapter.opened, true);
  assert.deepEqual(opened.map((handle) => handle.path), ["a", "c"]);
  assert.deepEqual(opened[0].options, { nonExclusive: true });
  assert.equal(typeof opened[0].events.data, "function");

  const none = new HidDeviceAdapter([info("x"), info("y")], null, Handle);
  await assert.rejects(none.open(), /open y refused/);
  assert.equal(none.opened, false);
});

test("open with a captured identity accepts only the same live endpoint and closes a mismatch", async () => {
  const identity = { vendorId: 0x046d, productId: 0xc547, path: "a", serialNumber: "S1", interface: 2 };
  const same = fakeHandles({ info: { a: { ...identity } } });
  const verified = new HidDeviceAdapter([info("a")], identity, same.Handle);
  await verified.open();
  assert.equal(verified.opened, true);

  for (const live of [{ ...identity, serialNumber: "S2" }, { ...identity, interface: undefined }, undefined]) {
    const changed = fakeHandles({ info: { a: live } });
    const adapter = new HidDeviceAdapter([info("a")], identity, changed.Handle);
    await assert.rejects(adapter.open(), /native live handle identity verification failed: native live handle identity is missing or changed/);
    assert.equal(adapter.opened, false);
    assert.equal(changed.opened[0].closed, true);
    assert.equal(changed.opened[0].detached, true);
  }

  const two = new HidDeviceAdapter([info("a"), info("b")], identity, same.Handle);
  await assert.rejects(two.open(), /requires one endpoint/);
});

test("reports go out id-prefixed on every split, survive one refusing, and fail when all refuse", async () => {
  const { Handle, opened } = fakeHandles({ fail: { a: ["write", "feature"], b: [] } });
  const adapter = new HidDeviceAdapter([info("a"), info("b")], null, Handle);
  await assert.rejects(adapter.sendReport(0x10, new Uint8Array([1])), /not open/);
  await adapter.open();

  const payload = new Uint8Array([9, 1, 2, 3, 4]).subarray(1); // a view with an offset
  await adapter.sendReport(0x10, payload);
  await adapter.sendFeatureReport(0x11, payload.buffer);
  assert.deepEqual(opened[1].writes, [[0x10, 1, 2, 3, 4]]);
  assert.deepEqual(opened[1].features, [[0x11, 9, 1, 2, 3, 4]]);
  await assert.rejects(adapter.sendReport(256, payload), /Report id 256 is out of range/);

  const allRefuse = fakeHandles({ fail: { a: ["write", "feature"] } });
  const refused = new HidDeviceAdapter([info("a")], null, allRefuse.Handle);
  await refused.open();
  await assert.rejects(refused.sendReport(0x10, payload), /write a refused/);
  await assert.rejects(refused.sendFeatureReport(0x10, payload), /feature a refused/);
});

test("receiveFeatureReport strips the echoed id byte, keeps id-less replies, and falls through to the split that answers", async () => {
  const { Handle, opened } = fakeHandles({
    fail: { a: ["getFeature"] },
    featureReply: { b: [0x05, 0xaa, 0xbb], c: [0x07, 0x08] },
  });
  const adapter = new HidDeviceAdapter([info("a"), info("b")], null, Handle);
  await adapter.open();
  const reply = await adapter.receiveFeatureReport(0x05);
  assert.deepEqual([...new Uint8Array(reply.buffer, reply.byteOffset, reply.byteLength)], [0xaa, 0xbb]);
  assert.deepEqual(opened[1].requested, [0x05, 91]);

  const noEcho = new HidDeviceAdapter([info("c")], null, Handle);
  await noEcho.open();
  const raw = await noEcho.receiveFeatureReport(0x05);
  assert.deepEqual([...new Uint8Array(raw.buffer, raw.byteOffset, raw.byteLength)], [0x07, 0x08]);

  const broken = new HidDeviceAdapter([info("a")], null, Handle);
  await broken.open();
  await assert.rejects(broken.receiveFeatureReport(0x05), /getFeature a refused/);
});

test("input reports reach inputreport listeners until removed, and close detaches and closes every handle", async () => {
  const { Handle, opened } = fakeHandles();
  const adapter = new HidDeviceAdapter([info("a"), info("b")], null, Handle);
  await adapter.open();
  const seen = [];
  const listener = (event) => seen.push([event.reportId, [...new Uint8Array(event.data.buffer)]]);
  adapter.addEventListener("inputreport", listener);
  adapter.addEventListener("connect", () => seen.push("wrong type"));
  opened[1].events.data(Buffer.from([0x11, 0xff, 0x01]));
  opened[0].events.data(Buffer.alloc(0));
  adapter.removeEventListener("inputreport", listener);
  opened[0].events.data(Buffer.from([0x10, 0x01]));
  assert.deepEqual(seen, [[0x11, [0xff, 0x01]], [0, []]]);

  await adapter.close();
  assert.equal(adapter.opened, false);
  assert.ok(opened.every((handle) => handle.closed && handle.detached));
  await assert.rejects(adapter.receiveFeatureReport(0x10), /not open/);
});

// A device whose firmware stalls a GET_REPORT longer than its 64-byte feature
// report, as the wired G-Wolves HTS Ultra (0x33e4:0x5308) does.
function strictDevice(reportLength, { refuseSend = false } = {}) {
  const asked = [];
  class FakeHID {
    on() {}
    removeAllListeners() {}
    close() {}
    sendFeatureReport() {
      if (refuseSend) throw new Error("Send Feature Report DeviceIoControl: (0x0000001F)");
    }
    getFeatureReport(id, length) {
      asked.push(length);
      if (length > reportLength) throw new Error("Get Input/Feature Report DeviceIoControl: (0x0000001F)");
      return [id, ...new Uint8Array(reportLength - 1)];
    }
  }
  const info = { vendorId: 0x33e4, productId: 0x5308, product: "HTS Ultra", path: "strict", interface: 2 };
  return { device: new HidDeviceAdapter([info], null, FakeHID), asked };
}

test("a report the firmware refuses at full size is asked again at its sent length", async () => {
  const { device, asked } = strictDevice(65);
  await device.open();
  await device.sendFeatureReport(0, new Uint8Array(64));
  const reply = await device.receiveFeatureReport(0);
  assert.equal(reply.byteLength, 64);
  assert.deepEqual(asked, [91, 65]);
});

test("a short request still reads the whole reply when the firmware allows it", async () => {
  const { device, asked } = strictDevice(91);
  await device.open();
  await device.sendFeatureReport(0, new Uint8Array(8));
  const reply = await device.receiveFeatureReport(0);
  assert.equal(reply.byteLength, 90);
  assert.deepEqual(asked, [91]);
});

test("a report never sent is asked once, at full size", async () => {
  const { device, asked } = strictDevice(65);
  await device.open();
  await assert.rejects(device.receiveFeatureReport(0), /0x0000001F/);
  assert.deepEqual(asked, [91]);
});

test("a report no handle took to send is asked once, at full size", async () => {
  const { device, asked } = strictDevice(65, { refuseSend: true });
  await device.open();
  await assert.rejects(device.sendFeatureReport(0, new Uint8Array(64)), /0x0000001F/);
  await assert.rejects(device.receiveFeatureReport(0), /0x0000001F/);
  assert.deepEqual(asked, [91]);
});

test("a length sent before the handle closed is forgotten", async () => {
  const { device, asked } = strictDevice(65);
  await device.open();
  await device.sendFeatureReport(0, new Uint8Array(64));
  await device.close();
  await device.open();
  await assert.rejects(device.receiveFeatureReport(0), /0x0000001F/);
  assert.deepEqual(asked, [91]);
});
