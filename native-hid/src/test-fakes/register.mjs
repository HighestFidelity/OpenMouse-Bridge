// Test-only module hooks: resolve `node-hid` to fake-node-hid.mjs and every
// @openmouse driver module to a generated module of fake classes, so the
// production files load unchanged without node-hid or the protocol packages
// installed. Import it before the code under test, or preload it in a child
// with `node --import <this file>`.
//
// FAKE_HID_TIME_SCALE=<n> divides every setTimeout delay by n, so the 3000 ms
// step timeouts in apply.mjs / button-listener.mjs fire in milliseconds.
import { registerHooks } from "node:module";

import { BRAND_DRIVERS } from "../brands.mjs";

const NODE_HID = new URL("./fake-node-hid.mjs", import.meta.url).href;
const DRIVER = new URL("./fake-driver.mjs", import.meta.url).href;
const DRIVER_MODULE = `${DRIVER}?module=`;
const CLASS_NAMES = [...new Set(Object.values(BRAND_DRIVERS).flatMap((entry) => entry.classes.map((c) => c.exportName)))];

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "node-hid") return { url: NODE_HID, shortCircuit: true };
    if (/^@openmouse\/(keyboard-)?protocol\/drivers\//.test(specifier)) {
      return { url: DRIVER_MODULE + encodeURIComponent(specifier), shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (!url.startsWith(DRIVER_MODULE)) return nextLoad(url, context);
    const source = [
      `import { makeClient } from ${JSON.stringify(DRIVER)};`,
      ...CLASS_NAMES.map((name) => `export const ${name} = makeClient(${JSON.stringify(name)});`),
    ].join("\n");
    return { format: "module", source, shortCircuit: true };
  },
});

const scale = Number(process.env.FAKE_HID_TIME_SCALE);
if (scale > 1) {
  const realSetTimeout = globalThis.setTimeout;
  globalThis.setTimeout = (callback, ms = 0, ...args) => realSetTimeout(callback, ms / scale, ...args);
}
