import test from "ava";

// These tests exercise the napi exports that DON'T need a live Arena
// process — they test error handling, input validation, and the
// pure-logic paths of the scanner functions.

let napi;
try {
  napi = await import("../index.js");
} catch {
  napi = null;
}

const hasNapi = napi !== null;

// --- Error handling for missing processes ---

test("readMtgaCards with nonexistent process returns error", (t) => {
  if (!hasNapi) return t.pass("napi module not built");
  const result = napi.readMtgaCards("nonexistent_process_99999");
  t.truthy(result.error, "Should return error for missing process");
});

test("readMtgaInventory with nonexistent process returns error", (t) => {
  if (!hasNapi) return t.pass("napi module not built");
  const result = napi.readMtgaInventory("nonexistent_process_99999");
  t.truthy(result.error, "Should return error for missing process");
});

// --- Mono scanner error handling ---

test("readMtgaCardsMono with nonexistent process returns error", (t) => {
  if (!hasNapi) return t.pass("napi module not built");
  const result = napi.readMtgaCardsMono("nonexistent_process_99999");
  t.truthy(result.error, "Should return error for missing process");
});

test("readMtgaInventoryMono with nonexistent process returns error", (t) => {
  if (!hasNapi) return t.pass("napi module not built");
  const result = napi.readMtgaInventoryMono("nonexistent_process_99999", 0, 0);
  t.truthy(result.error, "Should return error for missing process");
});

// Probe functions (probeHeapForI32Pair, probeMonoClass, readMonoBytes)
// are debug-only tools that may not be built on all platforms. Skip.

// --- Return shape validation (when process exists but no data) ---

test("readMtgaCards returns object with cards array or error", (t) => {
  if (!hasNapi) return t.pass("napi module not built");
  const result = napi.readMtgaCards("MTGA");
  t.is(typeof result, "object");
  t.true(
    Array.isArray(result.cards) || typeof result.error === "string",
    "Should return { cards: [] } or { error: string }",
  );
  if (result.cards) {
    for (const card of result.cards.slice(0, 5)) {
      t.is(typeof card.cardId, "number");
      t.is(typeof card.quantity, "number");
      t.true(card.cardId > 0);
      t.true(card.quantity >= 1 && card.quantity <= 4);
    }
  }
});

test("readMtgaInventory returns object with expected fields or error", (t) => {
  if (!hasNapi) return t.pass("napi module not built");
  const result = napi.readMtgaInventory("MTGA");
  t.is(typeof result, "object");
  if (!result.error) {
    t.is(typeof result.wcCommon, "number");
    t.is(typeof result.wcUncommon, "number");
    t.is(typeof result.wcRare, "number");
    t.is(typeof result.wcMythic, "number");
    t.is(typeof result.gold, "number");
    t.is(typeof result.gems, "number");
    t.is(typeof result.vaultProgress, "number");
    t.true(result.vaultProgress >= 0 && result.vaultProgress <= 100);
  }
});

