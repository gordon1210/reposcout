// Sample JavaScript file for reposcout fixtures.
import { readFile } from "node:fs/promises";
const path = require("node:path");

// HACK: temporary shim
export async function loadJson(file) {
  const raw = await readFile(file, "utf8");
  return JSON.parse(raw);
}

export function pick(obj, keys) {
  const out = {};
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(obj, key)) {
      out[key] = obj[key];
    }
  }
  return out;
}
