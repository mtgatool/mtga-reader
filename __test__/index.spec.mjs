import test from "ava";

import { readData } from "../index.js";

test("readData from native", (t) => {
  let result = readData("MTGA", []);
  t.deepEqual(result, {
    error: "Process not found",
  });
});

