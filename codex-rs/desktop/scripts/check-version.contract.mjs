import assert from "node:assert/strict";
import { test } from "node:test";

import {
  validateWixUpgradeCode,
  wixVersionFor,
} from "./check-version.mjs";

test("keeps MSI versions ordered from RCs through stable and the next patch", () => {
  assert.deepEqual(
    [
      wixVersionFor("1.0.0-rc.1"),
      wixVersionFor("1.0.0-rc.98"),
      wixVersionFor("1.0.0"),
      wixVersionFor("1.0.1-rc.1"),
      wixVersionFor("1.0.1"),
    ],
    ["1.0.1", "1.0.98", "1.0.99", "1.0.101", "1.0.199"],
  );
});

test("rejects versions outside the MSI release contract", () => {
  for (const version of [
    "1.0.0-rc.0",
    "1.0.0-rc.99",
    "1.0.00",
    "1.0.655",
    "1.0.655-rc.1",
    "256.0.0",
  ]) {
    assert.throws(() => wixVersionFor(version));
  }
});

test("rejects a replacement WiX upgrade code", () => {
  assert.throws(
    () => validateWixUpgradeCode("00000000-0000-0000-0000-000000000000"),
    /WiX upgradeCode must remain pinned/,
  );
});
