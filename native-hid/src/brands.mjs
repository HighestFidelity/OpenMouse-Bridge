// Maps a MouseStatus `brand` string (mouse-protocol/src/drivers/mouse-types.ts)
// to the HID vendor id(s) to enumerate and the candidate driver classes to
// try constructing against each interface found, in the same order
// mouse-protocol/src/drivers/registry.ts's DEVICE_DRIVERS lists them for
// that brand (first-match-wins there; here, first class whose open() +
// readStatus() probe succeeds wins — see apply.mjs).
//
// Deliberately excluded:
// - "Pulsar": Bridge has a dependency-free native Rust driver for it
//   (src/drivers/pulsar.rs) that claims the brand before this helper is ever
//   invoked — see src/drivers/mod.rs.
// - Redragon M719RGB-PRO (0x25a7:0xfa7e / 0xfab0): its client's open()
//   requires the command/notify collections in `device.collections`, which
//   HidDeviceAdapter leaves empty, so it can never open here.
// - Redragon M916 Pro 4K (OpenMouse-Bridge#79): no client and no known
//   product id in mouse-protocol v0.58.0.
//
// Vendor ids are copied from mouse-protocol/src/drivers/vendors.ts and each
// driver's own `isSupported()`; class import paths use the package's
// "./drivers/*" subpath export (mouse-protocol/package.json), which maps
// straight onto its compiled dist/drivers/ layout.
//
// An entry may narrow what candidateDevices() opens for it: `productIds`
// keeps only those products of its vendor ids, and `usagePage` keeps only an
// interface's collections on that page, for receivers whose other
// collections must stay closed (see the Scyrox entry).

// `productIds` limits a class to the products it speaks for, when one vendor
// id carries several wire protocols that a probe could confuse; `vendorId`
// further limits it to one of its entry's vendor ids.
const client = (module, exportName, productIds, vendorId) => ({ module, exportName, productIds, vendorId });

// G-Wolves ships three protocols under one vendor id; each driver class only
// checks its product's protocol in the collection-based isSupported(), which
// this adapter cannot run, so the catalog picks the class instead. Copied per
// `protocol` from mouse-protocol's GWOLVES_PRODUCTS
// (src/drivers/gwolves/products.ts, v0.58.0) rather than imported, so this
// file stays plain data that loads without the package (test-fakes/
// register.mjs imports it first); brands.test.mjs checks the copy against the
// installed package when there is one.
export const GWOLVES_PRODUCT_IDS = {
  xvi: [
    0x3908, 0x3917, 0x7904, 0x7913, 0x3708, 0x3717, 0x5308, 0x5317, 0x7704, 0x7713, 0x3508, 0x3517, 0x7908, 0x7917,
    0x5808, 0x5817, 0x2708, 0x2717, 0x7204, 0x7203, 0x5807, 0x5407, 0x5707, 0x5907, 0x5408, 0x5417, 0x5708, 0x5717,
    0x5908, 0x5917, 0x7708, 0x7717, 0x5804, 0x5803, 0x5404, 0x5403, 0x5704, 0x5703, 0x5904, 0x5903, 0x001a, 0x00aa,
  ],
  "xvi-new": [0x3808, 0x3817, 0x6808, 0x6817, 0x5608, 0x5617, 0x3608, 0x3617],
  gwolves: [
    0x5618, 0x3854, 0x5219, 0x5418, 0x4718, 0x4719, 0x5619, 0x5419, 0x3619, 0x2719, 0x4219, 0x2619, 0x3519,
  ],
};

// mouse-protocol's SCYROX_PRODUCT_IDS (src/scyrox/index.ts, v0.58.0), copied
// for the same reason. Keep in step with SCYROX_PRODUCT_IDS in
// src/drivers/mod.rs.
export const SCYROX_PRODUCT_IDS = [0xf515, 0xf517, 0xf5f7];

// Redragon's mice ride other makers' vendor ids, so only these exact products
// are claimed (same reason for copying). Holtek 0x04d9: mouse-protocol's
// REDRAGON_PRODUCT_IDS (M724 K1NG 1K, M612 Predator; src/redragon/index.ts).
// SinoWealth 0x258a: REDRAGON_M690_PRO_PRODUCT_IDS (cable, receiver;
// src/redragon/m690-pro.ts). Keep in step with KNOWN_PRODUCTS in
// src/drivers/mod.rs.
export const REDRAGON_HOLTEK_VENDOR_ID = 0x04d9;
export const REDRAGON_HOLTEK_PRODUCT_IDS = [0xfc7a, 0xfc61];
export const REDRAGON_M690_PRO_VENDOR_ID = 0x258a;
export const REDRAGON_M690_PRO_PRODUCT_IDS = [0x002e, 0x002f];

export const BRAND_DRIVERS = {
  zaunkoenig: {
    vendorIds: [0x0483],
    classes: [client("@openmouse/protocol/drivers/zaunkoenig/hid", "ZaunkoenigHidClient")],
  },
  finalmouse: {
    vendorIds: [0x361d],
    classes: [client("@openmouse/protocol/drivers/finalmouse/hid", "FinalmouseHidClient")],
  },
  "endgame gear": {
    vendorIds: [0x3367],
    classes: [
      client("@openmouse/protocol/drivers/endgame/egg-op1-hid", "EggOp1HidClient"),
      client("@openmouse/protocol/drivers/endgame/egg-we-hid", "EggWeHidClient"),
    ],
  },
  teevolution: {
    vendorIds: [0x3554],
    classes: [client("@openmouse/protocol/drivers/teevolution/hid", "TeevolutionHidClient")],
  },
  vgn: {
    vendorIds: [0x3554],
    classes: [client("@openmouse/protocol/drivers/vgn/hid", "VgnF2HidClient")],
  },
  // CompX's vendor id again. The Scyrox dongles answer report 8 only on their
  // 0xff02 collection, and on Windows the V8's "SCYROX 8K Dongle" stops
  // answering it while Bridge also listens on the interface's other vendor
  // collections, so only that collection is opened.
  scyrox: {
    vendorIds: [0x3554],
    productIds: SCYROX_PRODUCT_IDS,
    usagePage: 0xff02,
    classes: [client("@openmouse/protocol/drivers/scyrox/hid", "ScyroxHidClient")],
  },
  logitech: {
    vendorIds: [0x046d],
    classes: [client("@openmouse/protocol/drivers/logitech/hidpp", "LogitechHidppClient")],
  },
  wlmouse: {
    vendorIds: [0x36a7],
    classes: [
      client("@openmouse/protocol/drivers/wlmouse/beast-x-4k-hid", "WLMouseBeastX4kHidClient"),
      client("@openmouse/protocol/drivers/wlmouse/hid", "WLMouseHidClient"),
    ],
  },
  lamzu: {
    vendorIds: [0x373e],
    classes: [client("@openmouse/protocol/drivers/lamzu/hid", "LamzuHidClient")],
  },
  // Same CompX ODM hardware/class as Lamzu; readStatus() reports the brand
  // that matches the product id.
  crdrako: {
    vendorIds: [0x373e],
    classes: [client("@openmouse/protocol/drivers/lamzu/hid", "LamzuHidClient")],
  },
  moddomouse: {
    vendorIds: [0x2fe3],
    classes: [client("@openmouse/protocol/drivers/moddo/hid", "ModdoHidClient")],
  },
  ninjutso: {
    // NINJUTSO_VENDOR_ID (current) and NINJUTSO_LEGACY_VENDOR_ID (shared
    // with Orbital) — see mouse-protocol/src/ninjutso/index.ts.
    vendorIds: [0x093a, 0x1915],
    classes: [client("@openmouse/protocol/drivers/ninjutso/hid", "NinjutsoHidClient")],
  },
  orbital: {
    vendorIds: [0x1915],
    classes: [client("@openmouse/protocol/drivers/orbital/hid", "OrbitalHidClient")],
  },
  razer: {
    vendorIds: [0x1532],
    classes: [
      client("@openmouse/protocol/drivers/razer/hid", "RazerHidClient"),
      client("@openmouse/protocol/drivers/razer/cobra-hid", "RazerCobraHidClient"),
      client("@openmouse/protocol/drivers/razer/viper-mini-hid", "RazerViperMiniHidClient"),
      client("@openmouse/protocol/drivers/razer/viper-hid", "RazerViperHidClient"),
      client("@openmouse/protocol/drivers/razer/viper-v4-pro-hid", "RazerViperV4ProHidClient"),
    ],
  },
  atk: {
    vendorIds: [0x373b],
    classes: [client("@openmouse/protocol/drivers/atk/hid", "AtkHidClient")],
  },
  "attack shark": {
    vendorIds: [0x1d57, 0x25a7, 0x373e],
    classes: [client("@openmouse/protocol/drivers/attackshark/hid", "AttackSharkHidClient")],
  },
  keychron: {
    vendorIds: [0x3434],
    classes: [client("@openmouse/protocol/drivers/keychron/hid", "KeychronHidClient")],
  },
  fantech: {
    vendorIds: [0x3151],
    classes: [client("@openmouse/protocol/drivers/fantech/hid", "FantechHidClient")],
  },
  wooting: {
    vendorIds: [0x31e3],
    classes: [client("@openmouse/keyboard-protocol/drivers/wooting/hid", "WootingHidClient")],
  },
  microsoft: {
    vendorIds: [0x045e],
    classes: [client("@openmouse/protocol/drivers/microsoft/hid", "MicrosoftHidClient")],
  },
  wallhack: {
    vendorIds: [0x3879, 0x1caa],
    classes: [
      client("@openmouse/protocol/drivers/wallhack/mouse-hid", "WallhackMouseHidClient"),
      client("@openmouse/protocol/drivers/wallhack/keyboard-hid", "WallhackKeyboardHidClient"),
    ],
  },
  "g-wolves": {
    vendorIds: [0x33e4],
    classes: [
      client("@openmouse/protocol/drivers/gwolves/xvi-hid", "GWolvesXviHidClient", GWOLVES_PRODUCT_IDS.xvi),
      client("@openmouse/protocol/drivers/gwolves/xvi-new-hid", "GWolvesXviNewHidClient", GWOLVES_PRODUCT_IDS["xvi-new"]),
      client("@openmouse/protocol/drivers/gwolves/hid", "GWolvesHidClient", GWOLVES_PRODUCT_IDS.gwolves),
    ],
  },
  // The M690 PRO's 154-byte config block is longer than the adapter's 91-byte
  // feature read (hid-device-adapter.mjs), so its status may come back
  // identity-only with settings off [Inference: not tried on hardware].
  redragon: {
    vendorIds: [0x04d9, 0x258a],
    productIds: [...REDRAGON_HOLTEK_PRODUCT_IDS, ...REDRAGON_M690_PRO_PRODUCT_IDS],
    classes: [
      client(
        "@openmouse/protocol/drivers/redragon/hid",
        "RedragonHidClient",
        REDRAGON_HOLTEK_PRODUCT_IDS,
        REDRAGON_HOLTEK_VENDOR_ID,
      ),
      client(
        "@openmouse/protocol/drivers/redragon/m690-pro-hid",
        "RedragonM690ProHidClient",
        REDRAGON_M690_PRO_PRODUCT_IDS,
        REDRAGON_M690_PRO_VENDOR_ID,
      ),
    ],
  },
  delux: {
    vendorIds: [0x1d57, 0x248a, 0x373e],
    classes: [
      client("@openmouse/protocol/drivers/delux/hid", "DeluxHidClient"),
      client("@openmouse/protocol/drivers/attackshark/hid", "AttackSharkHidClient"),
      client("@openmouse/protocol/drivers/lamzu/hid", "LamzuHidClient"),
    ],
  },
};

/** Normalizes a MouseStatus `brand` string / Bridge `device.id` prefix into
 * a BRAND_DRIVERS key. */
export function brandKey(brand) {
  return brand.trim().toLowerCase();
}

/** The classes of a BRAND_DRIVERS entry worth probing on a device with this
 * product id (and vendor id): every class, minus those whose `productIds` or
 * `vendorId` leave it out. */
export function classesFor(entry, productId, vendorId) {
  return entry.classes.filter(
    (candidate) =>
      (!candidate.productIds || candidate.productIds.includes(productId)) &&
      (candidate.vendorId === undefined || candidate.vendorId === vendorId),
  );
}
