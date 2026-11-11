// Stand-in for the `node-hid` package (resolved in its place by register.mjs).
// No real device is ever enumerated or opened.
import { record, state } from "./state.mjs";

export function devices() {
  return state.devices.map((info) => ({ ...info }));
}

export class HID {
  constructor(path, options) {
    if (state.openFail.includes(path)) throw new Error(`cannot open ${path}`);
    this.path = path;
    record("hid.open", path, options ?? null);
  }

  getDeviceInfo() {
    return state.devices.find((info) => info.path === this.path);
  }

  on() {}

  removeAllListeners() {}

  write(frame) {
    record("hid.write", this.path, [...frame]);
  }

  sendFeatureReport(frame) {
    if (state.featureFail.includes(this.path)) throw new Error(`feature report refused on ${this.path}`);
    record("hid.sendFeatureReport", this.path, [...frame]);
  }

  getFeatureReport(reportId) {
    return [reportId, 0];
  }

  close() {
    record("hid.close", this.path);
  }
}
