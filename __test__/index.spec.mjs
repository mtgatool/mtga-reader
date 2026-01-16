import test from "ava";

import { readData, isAdmin, findProcess } from "../index.js";

test("readData with empty path returns error", (t) => {
  let result = readData("MTGA", []);
  // On macOS IL2CPP backend, empty path returns "No path specified"
  // On Windows Mono backend with no process, returns "Process not found"
  t.true(
    result.error !== undefined,
    "Should return an error object"
  );
});

test("isAdmin returns boolean", (t) => {
  let result = isAdmin();
  t.is(typeof result, "boolean");
});

test("findProcess returns boolean", (t) => {
  let result = findProcess("nonexistent_process_12345");
  t.is(result, false);
});

