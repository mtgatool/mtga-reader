// Smoke test for the macOS/IL2CPP backend through the .node addon.
//   npm run build:debug && node scripts/macos-smoke.js   (root not needed if the
//   host binary carries the com.apple.security.cs.debugger entitlement)
const mtga = require('../index.js');

const NAME = 'MTGA';

function brief(v, n = 300) {
  const s = JSON.stringify(v);
  return s.length > n ? s.slice(0, n) + ' ...' : s;
}

(async () => {
  console.log('isAdmin        :', mtga.isAdmin());
  console.log('findProcess    :', await mtga.findProcess(NAME));

  const t0 = Date.now();
  console.log('init           :', await mtga.init(NAME), `(${Date.now() - t0}ms)`);
  console.log('isInitialized  :', mtga.isInitialized());

  for (const fn of ['readAccount', 'readInventory', 'readRanks']) {
    const t = Date.now();
    console.log(`\n${fn} (${Date.now() - t}ms):`, brief(await mtga[fn](NAME)));
  }

  let t = Date.now();
  const coll = await mtga.readCollection(NAME);
  console.log(
    `\nreadCollection (${Date.now() - t}ms): count=${coll.count}`,
    'first:', brief(coll.cards && coll.cards.slice(0, 3))
  );

  t = Date.now();
  const decks = await mtga.readDecks(NAME);
  console.log(`readDecks (${Date.now() - t}ms): count=${decks.count}`);
  const d = (decks.decks || []).find((x) => x.piles && x.piles.length);
  if (d) {
    console.log('  sample deck:', d.name, '|', d.attributes && d.attributes.Format, '|', d.deckId);
    console.log('  piles:', brief(d.piles.map((p) => ({ pile: p.pileName, total: p.total }))));
    console.log('  first cards:', brief(d.piles[0].cards.slice(0, 4)));
  }

  // The low-level path API the tracker uses for live-match info.
  console.log('\n--- readData paths (as mtgatool-desktop calls them) ---');
  const paths = {
    matchManager: ['MatchSceneManager', 'Instance', '_matchManager'],
    localPlayer: ['MatchSceneManager', 'Instance', '_matchManager', '<LocalPlayerInfo>k__BackingField'],
    opponent: ['MatchSceneManager', 'Instance', '_matchManager', '<OpponentInfo>k__BackingField'],
    inventoryViaWrapper: ['WrapperController', '<Instance>k__BackingField', '<InventoryManager>k__BackingField'],
  };
  for (const [label, p] of Object.entries(paths)) {
    const t2 = Date.now();
    const r = await mtga.readData(NAME, p);
    console.log(`  ${label} (${Date.now() - t2}ms):`, brief(r, 220));
  }

  // Debug-explorer APIs used by the settings/reader status screens.
  console.log('\n--- explorer APIs ---');
  console.log('  getAssemblies:', brief(mtga.getAssemblies()));
  const det = mtga.getClassDetails('GameAssembly', 'WrapperController');
  console.log('  getClassDetails(WrapperController): fields=%d statics=%s',
    det.fields.length, brief(det.staticInstances));

  // Poll cadence, as the tracker would.
  console.log('\npoll timings:');
  for (let i = 0; i < 3; i++) {
    const s = Date.now();
    await mtga.readInventory(NAME);
    console.log(`  readInventory #${i}: ${Date.now() - s}ms`);
  }
})().catch((e) => {
  console.error('FAILED:', e);
  process.exit(1);
});
