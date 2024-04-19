const { readData } = require("./index.js");

const data = readData("MTGA", [
  "PAPA",
  "_instance",
  "_inventoryManager",
  "_inventoryServiceWrapper",
  "m_inventory",
]);

console.log(data);