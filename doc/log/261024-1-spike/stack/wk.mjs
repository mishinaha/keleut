// Runs depth.mjs in a Worker whose stack is set by resourceLimits.stackSizeMb (1024 MB).
// A Worker's stack is not bounded by RLIMIT_STACK, so no ulimit is needed (E12).
import { Worker } from "node:worker_threads";
const w = new Worker(new URL("./depth.mjs", import.meta.url), { resourceLimits: { stackSizeMb: 1024 } });
w.on("exit", (c) => console.log("worker exit", c));
w.on("error", (e) => console.log("worker error", e.message));
