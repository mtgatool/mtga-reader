import test from "ava";

import { readData } from "../index.js";

test("sum from native", (t) => {
  let result = readData("MTGA", []);
  t.deepEqual(result, {
    error: "Process not found",
  });
});
