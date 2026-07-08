// Test the SQLite-based card database reader
const { readCardDatabase, findCardDatabasePath } = require("./card-database");

async function main() {
  console.log("DB path:", findCardDatabasePath());

  const t0 = Date.now();
  const db = await readCardDatabase();
  console.log(`Loaded ${db.totalCards} cards in ${Date.now() - t0}ms`);
  console.log(`DB version: ${db.version}`);
  console.log(`Locale: ${db.locale}`);
  console.log(`DB file: ${db.dbPath}`);

  // Sample cards
  console.log("\nSample cards:");
  for (const id of [75452, 75450, 79412, 6873, 101328]) {
    const card = db.byGrpId.get(id);
    if (card) {
      console.log(
        `  ${card.grpId} | ${card.set} #${card.collectorNumber} | ${card.name} | ${card.rarity} | ${card.colors}`,
      );
    } else {
      console.log(`  ${id} | NOT FOUND`);
    }
  }

  // If we can also read the collection, combine them
  try {
    const r = require("./index.js");
    // Try macOS IL2CPP first, then Mono
    let coll = r.readMtgaCards("MTGA");
    if (coll.error) coll = r.readMtgaCardsMono("MTGA.exe");
    if (!coll.error && coll.cards) {
      console.log(`\nCollection: ${coll.cards.length} unique cards`);
      console.log("First 5 with names:");
      for (const entry of coll.cards.slice(0, 5)) {
        const card = db.byGrpId.get(entry.cardId);
        console.log(
          `  ${entry.cardId} x${entry.quantity} → ${card ? card.name + " (" + card.set + " #" + card.collectorNumber + ")" : "UNKNOWN"}`,
        );
      }
    }
  } catch {
    console.log("\n(readMtgaCards not available — skipping collection test)");
  }
}

main().catch(console.error);
