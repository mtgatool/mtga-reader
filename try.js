const { readData } = require("./index.js");

const data = readData("MTGA", [
  "PAPA", "_instance", "_accountClient", "<AccountInformation>k__BackingField"
]);

console.log(data);