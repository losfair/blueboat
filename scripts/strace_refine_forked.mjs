#!/usr/bin/env zx

const fs = require("fs");

const data = fs.readFileSync(argv.f, "utf-8")
  .split("\n")
  .map(x => x.trim())
  .map(x => x.substr(x.search(" ") + 1))
  .filter(x => x && x[0] != "+" && x[0] != "-" && x[0] != "<")
  .map(x => x.split("(")[0]);
const m = {};
for(const x of data) m[x] = true;
console.log(JSON.stringify(Object.keys(m).sort(), null, 2));
