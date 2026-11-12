import assert from "node:assert/strict";
import test from "node:test";

import {
  BRAND_DRIVERS,
  GWOLVES_PRODUCT_IDS,
  REDRAGON_HOLTEK_PRODUCT_IDS,
  REDRAGON_HOLTEK_VENDOR_ID,
  REDRAGON_M690_PRO_PRODUCT_IDS,
  REDRAGON_M690_PRO_VENDOR_ID,
  SCYROX_PRODUCT_IDS,
  classesFor,
} from "./brands.mjs";

const names = (classes) => classes.map((candidate) => candidate.exportName);

/** The installed @openmouse/protocol module, or null when it isn't installed. */
async function protocolModule(specifier) {
  try {
    return await import(specifier);
  } catch (error) {
    if (error?.code === "ERR_MODULE_NOT_FOUND") return null;
    throw error;
  }
}

test("each G-Wolves product is probed with the class of its own protocol only", () => {
  const gwolves = BRAND_DRIVERS["g-wolves"];
  assert.deepEqual(names(classesFor(gwolves, 0x5317)), ["GWolvesXviHidClient"]);
  assert.deepEqual(names(classesFor(gwolves, 0x3817)), ["GWolvesXviNewHidClient"]);
  assert.deepEqual(names(classesFor(gwolves, 0x5618)), ["GWolvesHidClient"]);
  for (const productId of Object.values(GWOLVES_PRODUCT_IDS).flat()) {
    assert.equal(classesFor(gwolves, productId).length, 1, `product 0x${productId.toString(16)}`);
  }
  assert.deepEqual(classesFor(gwolves, 0xffff), []);
});

test("each Redragon product is probed only with its own client, on its own vendor id", () => {
  const redragon = BRAND_DRIVERS.redragon;
  assert.deepEqual(redragon.productIds, [...REDRAGON_HOLTEK_PRODUCT_IDS, ...REDRAGON_M690_PRO_PRODUCT_IDS]);
  for (const productId of REDRAGON_HOLTEK_PRODUCT_IDS) {
    assert.deepEqual(names(classesFor(redragon, productId, REDRAGON_HOLTEK_VENDOR_ID)), ["RedragonHidClient"]);
    assert.deepEqual(classesFor(redragon, productId, REDRAGON_M690_PRO_VENDOR_ID), []);
  }
  for (const productId of REDRAGON_M690_PRO_PRODUCT_IDS) {
    assert.deepEqual(names(classesFor(redragon, productId, REDRAGON_M690_PRO_VENDOR_ID)), ["RedragonM690ProHidClient"]);
    assert.deepEqual(classesFor(redragon, productId, REDRAGON_HOLTEK_VENDOR_ID), []);
  }
  // Glorious's classic Model O on SinoWealth's id is not Redragon's.
  assert.deepEqual(classesFor(redragon, 0x0036, REDRAGON_M690_PRO_VENDOR_ID), []);
});

test("brands without product ids keep probing every class", () => {
  for (const [brand, entry] of Object.entries(BRAND_DRIVERS)) {
    if (entry.classes.some((candidate) => candidate.productIds)) continue;
    assert.deepEqual(classesFor(entry, 0x1234), entry.classes, brand);
  }
});

test("the copied product ids match the installed @openmouse/protocol catalogs", async (t) => {
  const gwolves = await protocolModule("@openmouse/protocol/drivers/gwolves/products");
  const scyrox = await protocolModule("@openmouse/protocol/scyrox");
  const redragon = await protocolModule("@openmouse/protocol/redragon");
  if (!gwolves || !scyrox || !redragon) {
    t.skip("@openmouse/protocol is not installed");
    return;
  }
  const byProtocol = {};
  for (const [productId, product] of gwolves.GWOLVES_PRODUCTS) {
    (byProtocol[product.protocol] ??= []).push(productId);
  }
  assert.deepEqual(byProtocol, GWOLVES_PRODUCT_IDS);
  assert.deepEqual([...scyrox.SCYROX_PRODUCT_IDS], SCYROX_PRODUCT_IDS);
  assert.equal(redragon.REDRAGON_VENDOR_ID, REDRAGON_HOLTEK_VENDOR_ID);
  assert.deepEqual([...redragon.REDRAGON_PRODUCT_IDS], REDRAGON_HOLTEK_PRODUCT_IDS);
  assert.equal(redragon.REDRAGON_M690_PRO_VENDOR_ID, REDRAGON_M690_PRO_VENDOR_ID);
  assert.deepEqual([...redragon.REDRAGON_M690_PRO_PRODUCT_IDS], REDRAGON_M690_PRO_PRODUCT_IDS);
});
