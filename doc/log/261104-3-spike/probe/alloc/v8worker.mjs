// V215 の選択肢の確かめ: Worker の resourceLimits で V8 のヒープを絞ると、
// ヒープが尽きたときに Worker だけが止まり、主スレッドは ERR_WORKER_OUT_OF_MEMORY を受け取って走り続けるか。
// 使い方: node v8worker.mjs
import { Worker } from 'node:worker_threads';
const src = `
  const { parentPort } = require('node:worker_threads');
  parentPort.postMessage('started');
  let l = null;
  for (let i = 0; ; i++) { l = { h: i, t: l }; }
`;
const w = new Worker(src, { eval: true, resourceLimits: { maxOldGenerationSizeMb: 64 } });
w.on('message', (m) => console.log('worker:', m));
w.on('error', (e) => { console.log('main: error', e.code); });
w.on('exit', (c) => { console.log('main: worker exit', c); process.exitCode = 3; });
