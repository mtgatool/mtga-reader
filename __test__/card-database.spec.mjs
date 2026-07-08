import test from "ava";
import { readCardDatabase, findCardDatabasePath } from "../card-database.js";
import fs from "fs";

test("findCardDatabasePath returns a path or null", (t) => {
  const result = findCardDatabasePath();
  if (result) {
    t.true(fs.existsSync(result), "Returned path should exist on disk");
    t.true(result.includes("Raw_CardDatabase_"), "Path should contain Raw_CardDatabase_");
    t.true(result.endsWith(".mtga"), "Path should end with .mtga");
  } else {
    // No Arena installed — that's OK for CI
    t.pass("No Arena install found (expected in CI)");
  }
});

test("findCardDatabasePath with nonexistent explicit arenaPath returns null", (t) => {
  // Non-existent path that can't match any fallback
  const result = findCardDatabasePath("/tmp/definitely_not_arena_" + Date.now());
  t.is(result, null);
});

// Only run DB content tests if Arena is installed
const dbPath = findCardDatabasePath();
const hasArena = dbPath !== null;

test("readCardDatabase loads cards with names", async (t) => {
  if (!hasArena) return t.pass("No Arena install — skipping");

  const db = await readCardDatabase();

  t.true(db.totalCards > 20000, `Should have >20K cards, got ${db.totalCards}`);
  t.truthy(db.version, "Should have a version string");
  t.is(db.locale, "enUS");
  t.truthy(db.dbPath);

  // Every card should have a grpId and set code
  const sample = db.cards.slice(0, 100);
  for (const card of sample) {
    t.is(typeof card.grpId, "number");
    t.true(card.grpId > 0, `grpId should be positive: ${card.grpId}`);
    t.is(typeof card.set, "string");
    t.is(typeof card.name, "string");
  }
});

test("readCardDatabase byGrpId lookup works", async (t) => {
  if (!hasArena) return t.pass("No Arena install — skipping");

  const db = await readCardDatabase();

  // Known card from the ANB set (confirmed in earlier testing)
  const card = db.byGrpId.get(75452);
  if (card) {
    t.is(card.set, "ANB");
    t.is(card.collectorNumber, "11");
    t.is(card.name, "Inspiring Commander");
  } else {
    // Card might not exist in a different Arena version
    t.pass("Card 75452 not in this DB version");
  }
});

test("readCardDatabase handles locale parameter", async (t) => {
  if (!hasArena) return t.pass("No Arena install — skipping");

  // Japanese locale
  const db = await readCardDatabase({ locale: "jaJP" });
  t.is(db.locale, "jaJP");
  t.true(db.totalCards > 20000);

  // Check that names are in Japanese for a known card
  const card = db.byGrpId.get(75452);
  if (card && card.name) {
    // Japanese name should be different from English
    t.true(card.name.length > 0, "Should have a Japanese name");
  }
});

test("readCardDatabase handles nonexistent locale gracefully", async (t) => {
  if (!hasArena) return t.pass("No Arena install — skipping");

  const db = await readCardDatabase({ locale: "xxXX" });
  t.true(db.totalCards > 20000);
  // Cards should still load, just with empty names
  const card = db.byGrpId.get(75452);
  if (card) {
    t.is(card.name, "", "Unknown locale should give empty names");
  }
});

test("readCardDatabase with invalid path throws", async (t) => {
  // Use a path that can't match any platform fallback either
  await t.throwsAsync(
    () => readCardDatabase({ arenaPath: "/tmp/definitely_not_arena_" + Date.now() }),
    { message: /Could not find Raw_CardDatabase/ },
  );
});

test("readCardDatabase card fields have correct types", async (t) => {
  if (!hasArena) return t.pass("No Arena install — skipping");

  const db = await readCardDatabase();
  const card = db.cards.find((c) => c.grpId > 70000 && c.name);
  if (!card) return t.pass("No suitable card found");

  t.is(typeof card.grpId, "number");
  t.is(typeof card.set, "string");
  t.is(typeof card.collectorNumber, "string");
  t.is(typeof card.titleId, "number");
  t.is(typeof card.rarity, "number");
  t.is(typeof card.isToken, "boolean");
  t.is(typeof card.isPrimaryCard, "boolean");
  t.is(typeof card.isDigitalOnly, "boolean");
  t.is(typeof card.isRebalanced, "boolean");
  t.is(typeof card.name, "string");
  t.true(card.name.length > 0, "Named card should have a non-empty name");
});

test("readCardDatabase includes tokens and non-tokens", async (t) => {
  if (!hasArena) return t.pass("No Arena install — skipping");

  const db = await readCardDatabase();
  const tokens = db.cards.filter((c) => c.isToken);
  const nonTokens = db.cards.filter((c) => !c.isToken);

  t.true(tokens.length > 100, `Should have >100 tokens, got ${tokens.length}`);
  t.true(nonTokens.length > 15000, `Should have >15K non-tokens, got ${nonTokens.length}`);
});
