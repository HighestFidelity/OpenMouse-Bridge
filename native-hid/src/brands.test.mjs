import assert from "node:assert/strict";
import test from "node:test";

import { BRAND_DRIVERS, GWOLVES_PRODUCT_IDS, SCYROX_PRODUCT_IDS, classesFor } from "./brands.mjs";

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

test("brands without product ids keep probing every class", () => {
  for (const [brand, entry] of Object.entries(BRAND_DRIVERS)) {
    if (entry.classes.some((candidate) => candidate.productIds)) continue;
    assert.deepEqual(classesFor(entry, 0x1234), entry.classes, brand);
  }
});

test("the copied product ids match the installed @openmouse/protocol catalogs", async (t) => {
  const gwolves = await protocolModule("@openmouse/protocol/drivers/gwolves/products");
  const scyrox = await protocolModule("@openmouse/protocol/scyrox");
  if (!gwolves || !scyrox) {
    t.skip("@openmouse/protocol is not installed");
    return;
  }
  const byProtocol = {};
  for (const [productId, product] of gwolves.GWOLVES_PRODUCTS) {
    (byProtocol[product.protocol] ??= []).push(productId);
  }
  assert.deepEqual(byProtocol, GWOLVES_PRODUCT_IDS);
  assert.deepEqual([...scyrox.SCYROX_PRODUCT_IDS], SCYROX_PRODUCT_IDS);
});
