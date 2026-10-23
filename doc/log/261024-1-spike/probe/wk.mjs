import { Worker } from "node:worker_threads";
const url = new URL("file:///home/sumito3478/repo/keleut/doc/log/261024-1-spike/wasm/depth.mjs");
const w = new Worker(url, { resourceLimits: { stackSizeMb: 1024 } });
w.on("exit", c => console.log("worker exit", c));
w.on("error", e => console.log("worker error", e.message));
