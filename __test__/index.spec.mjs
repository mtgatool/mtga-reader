import test from "ava";

import { readData, isAdmin, findProcess } from "../index.js";

test("readData returns a Promise and resolves to an error object", async (t) => {
  const pending = readData("MTGA", []);
  t.is(typeof pending.then, "function", "readData should return a Promise");
  // On macOS IL2CPP backend, empty path returns "No path specified"
  // On Windows/Linux Mono backend with no process, returns "Process not found"
  const result = await pending;
  t.true(result.error !== undefined, "Should resolve to an error object");
});

test("isAdmin returns boolean (sync)", (t) => {
  let result = isAdmin();
  t.is(typeof result, "boolean");
});

test("findProcess resolves to false for a missing process", async (t) => {
  const pending = findProcess("nonexistent_process_12345");
  t.is(typeof pending.then, "function", "findProcess should return a Promise");
  t.is(await pending, false);
});
