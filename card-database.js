// card-database.js — Read Arena's card database from the on-disk SQLite file.
//
// The card database is static per Arena build (same for all players).
// It ships as a SQLite file named Raw_CardDatabase_*.mtga in the
// Arena install's Downloads/Raw/ directory.
//
// Usage:
//   const { readCardDatabase, findCardDatabasePath } = require('./card-database');
//   const db = await readCardDatabase();  // auto-finds the .mtga file
//   console.log(db.cards.length);         // ~24K cards
//   console.log(db.cards[0]);             // { grpId, name, set, collectorNumber, ... }
//
// Dependencies: sql.js (pure JS SQLite — no native deps, works everywhere)

const fs = require("fs");
const path = require("path");

// Arena's downloaded data, relative to a Windows-style install root.
const RAW_SUBPATH = path.join("MTGA_Data", "Downloads", "Raw");

// Arena's Steam app id, from appmanifest_2141910.acf.
const STEAM_APP_ID = "2141910";

// Where Arena lands inside a Wine/Proton prefix's drive_c, by installer.
const WINE_INSTALL_DIRS = [
  path.join("Program Files", "Wizards of the Coast", "MTGA"),
  path.join("Program Files (x86)", "Wizards of the Coast", "MTGA"),
  path.join("Program Files (x86)", "Steam", "steamapps", "common", "MTGA"),
  path.join("Program Files", "Epic Games", "MagicTheGathering"),
];

/**
 * Immediate subdirectories of `parent`, or [] if it isn't readable.
 * Used to enumerate Wine bottles and Proton prefixes, which have
 * user-chosen (CrossOver, Whisky) or per-app-id (Proton) names.
 */
function subdirs(parent) {
  try {
    return fs
      .readdirSync(parent, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => path.join(parent, entry.name));
  } catch {
    return [];
  }
}

/** Every plausible Raw dir inside the given Wine prefixes. */
function winePrefixCandidates(prefixes) {
  const dirs = [];
  for (const prefix of prefixes) {
    for (const install of WINE_INSTALL_DIRS) {
      dirs.push(path.join(prefix, "drive_c", install, RAW_SUBPATH));
    }
  }
  return dirs;
}

/**
 * Find the Raw_CardDatabase_*.mtga file in Arena's install directory.
 * Searches platform-specific paths automatically.
 *
 * @param {string} [arenaPath] - Optional explicit Arena install path.
 * @returns {string|null} Path to the largest CardDatabase .mtga file, or null.
 */
function findCardDatabasePath(arenaPath) {
  const candidates = [];
  const home = process.env.HOME || process.env.USERPROFILE || "";

  if (arenaPath) {
    // Explicit path: only check there, no platform fallbacks
    candidates.push(path.join(arenaPath, "MTGA_Data", "Downloads", "Raw"));
    candidates.push(path.join(arenaPath, "Downloads", "Raw"));
  } else if (process.platform === "darwin") {
    const appSupport = path.join(home, "Library", "Application Support");
    // Native builds. Steam is the common case; the standalone client keeps its
    // downloads next to the Unity player data instead of inside the bundle.
    candidates.push(
      path.join(appSupport, "Steam", "steamapps", "common", "MTGA", RAW_SUBPATH),
    );
    candidates.push(path.join(appSupport, "com.wizards.mtga", "Downloads", "Raw"));
    candidates.push(
      "/Users/Shared/Epic Games/MagicTheGathering/MTGA.app/Contents/Resources/Data/Downloads/Raw",
    );
    candidates.push(
      path.join("/Applications", "MTGA.app", "Contents", "Resources", "Data", "Downloads", "Raw"),
    );
    // Arena run under Wine on macOS (CrossOver, Whisky) uses the same drive_c
    // layout as Linux, so these need checking here too rather than only in the
    // non-darwin branch.
    candidates.push(
      ...winePrefixCandidates([
        ...subdirs(path.join(appSupport, "CrossOver", "Bottles")),
        ...subdirs(
          path.join(home, "Library", "Containers", "com.isaacmarovitz.Whisky", "Bottles"),
        ),
      ]),
    );
  } else if (process.platform === "win32") {
    // Prefer the environment over hardcoded drive letters, since Windows can be
    // installed somewhere other than C:.
    const programFiles = process.env.ProgramFiles || "C:\\Program Files";
    const programFilesX86 = process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)";
    candidates.push(path.join(programFiles, "Wizards of the Coast", "MTGA", RAW_SUBPATH));
    candidates.push(path.join(programFilesX86, "Wizards of the Coast", "MTGA", RAW_SUBPATH));
    candidates.push(
      path.join(programFilesX86, "Steam", "steamapps", "common", "MTGA", RAW_SUBPATH),
    );
    candidates.push(path.join(programFiles, "Epic Games", "MagicTheGathering", RAW_SUBPATH));
  } else {
    // Linux: Arena only runs under Wine/Proton, so everything is inside a prefix.
    const dataHome = process.env.XDG_DATA_HOME || path.join(home, ".local", "share");
    const steamRoots = [
      path.join(home, ".steam", "steam"),
      path.join(dataHome, "Steam"),
      // Flatpak Steam
      path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"),
    ];
    const prefixes = [
      path.join(home, ".wine"),
      ...subdirs(path.join(dataHome, "wineprefixes")),
    ];
    for (const root of steamRoots) {
      const compatdata = path.join(root, "steamapps", "compatdata");
      prefixes.push(path.join(compatdata, STEAM_APP_ID, "pfx"));
      // Fall back to scanning every prefix, in case Arena was added as a
      // non-Steam game (which gets an arbitrary generated app id).
      prefixes.push(...subdirs(compatdata).map((dir) => path.join(dir, "pfx")));
    }
    candidates.push(...winePrefixCandidates(prefixes));
  }

  for (const dir of candidates) {
    try {
      const files = fs.readdirSync(dir);
      const dbFiles = files
        .filter((f) => f.startsWith("Raw_CardDatabase_") && f.endsWith(".mtga"))
        .map((f) => ({
          name: f,
          path: path.join(dir, f),
          size: fs.statSync(path.join(dir, f)).size,
        }))
        .sort((a, b) => b.size - a.size); // largest first (newest version)

      if (dbFiles.length > 0) {
        return dbFiles[0].path;
      }
    } catch {
      // Directory doesn't exist, try next
    }
  }

  return null;
}

/**
 * Find the Raw_ClientLocalization_*.mtga file (same directory as card DB).
 */
function findLocalizationPath(rawDir) {
  try {
    const files = fs.readdirSync(rawDir);
    const locFiles = files
      .filter((f) => f.startsWith("Raw_ClientLocalization_") && f.endsWith(".mtga"))
      .map((f) => ({
        name: f,
        path: path.join(rawDir, f),
        size: fs.statSync(path.join(rawDir, f)).size,
      }))
      .sort((a, b) => b.size - a.size);
    return locFiles.length > 0 ? locFiles[0].path : null;
  } catch {
    return null;
  }
}

/**
 * Read the card database from Arena's on-disk SQLite file.
 *
 * @param {object} [options]
 * @param {string} [options.arenaPath] - Explicit Arena install path.
 * @param {string} [options.locale] - Locale for card names (default: "enUS").
 * @returns {Promise<{cards: Array, localization: Object, dbPath: string}>}
 */
async function readCardDatabase(options = {}) {
  const initSqlJs = require("sql.js");
  const locale = options.locale || "enUS";

  // Find the database file
  const dbPath = findCardDatabasePath(options.arenaPath);
  if (!dbPath) {
    throw new Error(
      "Could not find Raw_CardDatabase_*.mtga. Is Arena installed? " +
      "Pass { arenaPath: '...' } to specify the install directory.",
    );
  }

  const SQL = await initSqlJs();
  const buf = fs.readFileSync(dbPath);
  const db = new SQL.Database(buf);

  // Check if the localization table for the requested locale exists
  const locTable = `Localizations_${locale}`;
  const tableCheck = db.exec(
    `SELECT name FROM sqlite_master WHERE type='table' AND name='${locTable}'`,
  );
  const hasLocalization = tableCheck.length > 0 && tableCheck[0].values.length > 0;

  // Read all cards with localized names
  let query;
  if (hasLocalization) {
    query = `
      SELECT c.GrpId, c.ExpansionCode, c.CollectorNumber, c.TitleId,
             c.Rarity, c.IsToken, c.IsPrimaryCard, c.IsDigitalOnly,
             c.IsRebalanced, c.Types, c.Subtypes, c.Colors, c.ColorIdentity,
             c.Power, c.Toughness,
             l.Loc as Name
      FROM Cards c
      LEFT JOIN ${locTable} l ON l.LocId = c.TitleId
      ORDER BY c.GrpId
    `;
  } else {
    query = `
      SELECT c.GrpId, c.ExpansionCode, c.CollectorNumber, c.TitleId,
             c.Rarity, c.IsToken, c.IsPrimaryCard, c.IsDigitalOnly,
             c.IsRebalanced, c.Types, c.Subtypes, c.Colors, c.ColorIdentity,
             c.Power, c.Toughness,
             NULL as Name
      FROM Cards c
      ORDER BY c.GrpId
    `;
  }

  const result = db.exec(query);
  if (!result.length) {
    db.close();
    throw new Error("Cards table is empty");
  }

  const cards = result[0].values.map((row) => ({
    grpId: row[0],
    set: row[1] || "",
    collectorNumber: row[2] || "",
    titleId: row[3],
    rarity: row[4],
    isToken: row[5] === 1,
    isPrimaryCard: row[6] === 1,
    isDigitalOnly: row[7] === 1,
    isRebalanced: row[8] === 1,
    types: row[9] || "",
    subtypes: row[10] || "",
    colors: row[11] || "",
    colorIdentity: row[12] || "",
    power: row[13] || "",
    toughness: row[14] || "",
    name: row[15] || "",
  }));

  // Build grpId → card lookup for convenience
  const byGrpId = new Map(cards.map((c) => [c.grpId, c]));

  // Get DB version
  const versionResult = db.exec("SELECT Version FROM Versions WHERE Type='Data'");
  const version = versionResult.length ? versionResult[0].values[0][0] : "unknown";

  db.close();

  return {
    cards,
    byGrpId,
    version,
    dbPath,
    locale,
    totalCards: cards.length,
  };
}

module.exports = { readCardDatabase, findCardDatabasePath };
