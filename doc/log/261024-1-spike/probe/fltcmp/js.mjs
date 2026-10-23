import fs from 'node:fs';
const lines = fs.readFileSync(process.argv[2], 'utf8').trim().split('\n');
const dv = new DataView(new ArrayBuffer(8));
const norm = s => { let [m, e] = s.toLowerCase().split('e'); let ex = e ? parseInt(e) : 0; let neg = m.startsWith('-'); if (neg) m = m.slice(1);
  let [ip, fp = ''] = m.split('.'); let digits = (ip + fp).replace(/^0+/, ''); let lead = (ip + fp).length - (ip + fp).replace(/^0+/, '').length;
  let exp10 = ex + ip.length - 1 - lead; return (neg?'-':'') + digits.replace(/0+$/, '') + '@' + exp10; };
let diff = 0, diffShort = 0, shown = 0;
for (const l of lines) {
  const [hex, c] = l.split(' ');
  dv.setBigUint64(0, BigInt('0x' + hex)); const f = dv.getFloat64(0);
  let r = null; for (let p = 1; p <= 17; p++) { const s = f.toPrecision(p); if (Number(s) === f) { r = s; break; } } if (r === null) r = f.toPrecision(17);
  if (norm(r) !== norm(c)) { diff++; if (shown++ < 5) console.log('toPrecision diff', hex, c, r); }
  if (norm(String(f)) !== norm(c)) diffShort++;
}
console.log(`n=${lines.length} toPrecision_loop_diff=${diff} String(f)_diff=${diffShort}`);
