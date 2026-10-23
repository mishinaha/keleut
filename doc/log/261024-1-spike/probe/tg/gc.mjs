const gc = new Uint8Array([0,0x61,0x73,0x6d,1,0,0,0,
 1,0x10,3, 0x5f,1,0x7f,0, 0x60,0,1,0x64,0, 0x60,1,0x64,0,1,0x7f,
 3,3,2,1,2,
 7,9,2, 1,0x6e,0,0, 1,0x67,0,1,
 10,0x12,2, 7,0,0x41,7,0xfb,0,0,0x0b, 8,0,0x20,0,0xfb,2,0,0,0x0b]);
const i = new WebAssembly.Instance(new WebAssembly.Module(gc));
const s = i.exports.n();
console.log("struct.new ->", typeof s, Object.prototype.toString.call(s), "struct.get ->", i.exports.g(s));
let keep = 0; for (let k = 0; k < 1e7; k++) keep += i.exports.g(i.exports.n());
console.log("1e7 allocs ok", keep, "heapUsed MB", (process.memoryUsage().heapUsed/1e6).toFixed(1));
