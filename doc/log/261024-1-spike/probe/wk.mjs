import { Worker } from "node:worker_threads";
const url = new URL("../wasm/depth.mjs", import.meta.url);
const w = new Worker(url, { resourceLimits: { stackSizeMb: 1024 } });
w.on("exit", c => console.log("worker exit", c));
w.on("error", e => console.log("worker error", e.message));
