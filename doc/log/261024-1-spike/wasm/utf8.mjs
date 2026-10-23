import { readFileSync } from "node:fs";
const bytes = readFileSync(new URL("./out/utf8.wasm", import.meta.url));
let got;
try {
  const { instance } = await WebAssembly.instantiate(bytes, { env: { log: (s) => { got = s; } } }, { builtins: ["text-decoder"] });
  instance.exports.go();
  console.log(`PASS builtin text-decoder: ${JSON.stringify(got)} (builtin used: ${typeof got === "string"})`);
} catch (e) { console.log(`FAIL ${e.constructor.name}: ${e.message}`); }
