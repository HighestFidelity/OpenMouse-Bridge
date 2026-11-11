// Stand-in for every @openmouse driver class (register.mjs generates a module
// per driver path exporting makeClient(name) under each class name).
//
// Behaviour comes from state.drivers[name][productId] (or ["*"]). No spec for
// a device means open() rejects, as a wrong class on a real interface would.
// Each step in a spec ("open", "readStatus", "setDpi", ...) may be "throw"
// (rejects with "<step> failed") or "hang" (never settles, so the caller's
// timeout has to fire) or a number of ms to wait first (scaled like every
// timer, see register.mjs); anything else resolves.
import { record, state } from "./state.mjs";

function act(spec, step) {
  const behaviour = spec?.[step];
  if (behaviour === "throw") return Promise.reject(new Error(`${step} failed`));
  if (behaviour === "hang") return new Promise(() => {});
  if (typeof behaviour === "number") return new Promise((resolve) => setTimeout(resolve, behaviour));
  return Promise.resolve();
}

export function makeClient(name) {
  return class FakeClient {
    constructor(device) {
      this.device = device;
      const specs = state.drivers[name] ?? {};
      this.spec = specs[device.productId] ?? specs["*"] ?? null;
      this.onButton = null;
      // A protocol release without the button listener.
      if (this.spec?.noListener) this.startButtonListener = undefined;
    }

    _record(step, ...args) {
      record(step, name, this.device.productId, ...args);
    }

    async open() {
      this._record("open");
      if (!this.spec) throw new Error(`${name} does not drive this device`);
      await this.device.open();
      return act(this.spec, "open");
    }

    async readStatus() {
      this._record("readStatus");
      await act(this.spec, "readStatus");
      return this.spec.status ?? {};
    }

    async setDpi(dpi) {
      this._record("setDpi", dpi);
      await act(this.spec, "setDpi");
      return dpi;
    }

    async setPollingRate(hz) {
      this._record("setPollingRate", hz);
      await act(this.spec, "setPollingRate");
      return hz;
    }

    async saveProfile(commandId, slot, confirmed) {
      this._record("saveProfile", commandId, slot, confirmed);
      await act(this.spec, "saveProfile");
    }

    async setOnboardMode(mode) {
      this._record("setOnboardMode", mode);
      await act(this.spec, "setOnboardMode");
    }

    onButtonChange(callback) {
      this.onButton = callback;
    }

    async startButtonListener() {
      this._record("startButtonListener");
      await act(this.spec, "startButtonListener");
      setImmediate(() => this.onButton?.(16));
      return [1, 2, 3, 4, 5];
    }

    async stopButtonListener() {
      this._record("stopButtonListener");
      await act(this.spec, "stopButtonListener");
    }

    async close() {
      this._record("close");
      await this.device.close();
    }
  };
}
