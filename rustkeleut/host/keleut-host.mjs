// RustKeleut の WasmGC バックエンドが Node の側で使うランタイムである。以下ではホストと呼ぶ。
//
// `keleut build` と `keleut run --backend wasm` は、このファイルの後ろに `await start(META)` の 1 行を追加して
// `main.mjs` を作り、`program.wasm` と `rt.wasm` と同じディレクトリに置く。
// META は crate keleut-wasm の meta.rs が作る表で、診断メッセージのテンプレート、値の表示に必要な名前、
// 深さの上限(maxDepth)、1 段に見積もるバイト数(bProg)、プログラムのシェイプ(shapes)を持つ。
//
// 閉じた標準出力の扱い: Node は起動時に、閉じているファイル記述子 1 を /dev/null で開き直す。
// そのため、このファイルの中では、標準出力が閉じている実行と /dev/null に向けた実行を区別できない。
// そこで、Node を起動する側が先に調べ、閉じていれば環境変数 KELEUT_STDOUT_CLOSED を 1 にして渡す。
// このときホストは、ファイル記述子 1 への書き込みをすべて EBADF の失敗として扱う。
// プログラムが出力すると出力エラーになり、ホストは `keleut: 標準出力に書き出せません: Bad file descriptor` を
// 標準エラーに書いて、終了コード 74 で終了する。
// この変数を渡すのは、`keleut run --backend wasm` と、`keleut build` が書く起動スクリプト DIR/run である。
// `node DIR/main.mjs` を直接実行すると、閉じた標準出力を区別できないので、プログラムが出力しても終了コード 0 で終了する。
//
// ホストは次の処理を行う。
//
// - 起動: `rt.wasm` を先にインスタンス化し、その線形メモリと関数を `program.wasm` に渡す。
//   環境変数 KELEUT_WASM_DIRECT が 1 なら、この Node 自身のスタックで実行する。
//   このスタックは、`keleut run` が RLIMIT_STACK と `--stack-size` を引き上げて用意したものである。
//   1 でなければ、`Worker` の `resourceLimits.stackSizeMb` でスタックを確保して実行する。
//   `node main.mjs` だけで実行するときは、この経路になる。
//   Worker のスタックサイズは META.bProg(プログラムの 1 段に見積もるバイト数)から求め、
//   STACK_CAP_KIB を超えたら STACK_CAP_KIB にする。
//   ただし、1 段を B_WASM バイトとして求めたサイズより小さくはしない。
//   環境変数 KELEUT_WASM_STATS があれば、選んだ経路と Worker のスタックサイズを標準エラーに 1 行書く。
//   環境変数 KELEUT_WASM_RMDIR がこのファイルのディレクトリを指すときは、2 つの `.wasm` を読んだ後で
//   そのディレクトリを削除する。
//   `keleut run` が、自分の作った一時ディレクトリをこの変数で渡す。
//   モジュールの読み込みかインスタンス化に失敗したとき、Node のバージョンが古いとき、Worker を作れないとき、
//   例外が捕捉されなかったときは、内部エラーとして終了コード 70 で終了する。
//   ただし、起動の途中で線形メモリを拡張できないときは、メモリ不足の実行時エラーとして終了コード 3 で終了する。
//   環境変数 KELEUT_IGNORED_SIGNALS に名前のあるシグナルのうち、SIGINT、SIGTERM、SIGHUP には、
//   何もしないシグナルハンドラを登録する。
//   `keleut` は、自分の起動時に無視する設定(SIG_IGN)だったシグナルの名前を、この変数で渡す。
//   Node は引き継いだ SIG_IGN を既定の動作に戻すので、登録しなければシグナルを無視したままにできない。
//   V8 のヒープそのものを使い切ったときは Node が SIGABRT で終了し、`keleut run` は終了コード 70 で終了する。
//   このとき cancel 節は実行されず、バッファに残っていた標準出力は失われる。
//   これは LangSpec §15.5 との既知の相違である。
// - 標準出力: crate keleut-rt の out.rs と同じ規約に従う。
//   出力は 64 KiB のバッファに追記し、バッファがいっぱいになったとき、標準エラーに書く直前、終了の前に flush する。
//   1 回の書き込みで一部しか書けなかったときは、残りを書き終えるまで書き込みを繰り返す。
//   EINTR ならすぐに、EAGAIN なら少し待ってから再試行する。
//   EBADF、EPIPE、ECONNRESET で失敗したらバッファを捨て、それ以外のエラーなら書けなかったバイトをバッファに残す。
// - 診断: 実行時エラーのメッセージ(値の表示を含む)を作り、終了コードを決める。
//   メッセージは、インタプリタ(crate keleut-interp)とバイト単位で一致させる。
// - シェイプ表: rtcore のシェイプ表を線形メモリに置き、いっぱいになったら拡張する。

import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { Worker, isMainThread, workerData } from 'node:worker_threads';

const NODE_MIN = [26, 9, 0];
// B_WASM は 1 段に見積もるバイト数の最小の値で、crate keleut-wasm の b_prog が k = 0 かつ r = 0 のプログラムに返す値である。
// STACK_CAP_KIB はスタックサイズの上限(KiB)である。
// どちらも crate keleut-wasm の同じ名前の定数と同じ値にする。
const B_WASM = 1536;
const STACK_CAP_KIB = 16777216;
const BUFFER_BYTES = 65536;

// テンプレートの種類(meta.rs の kind)。
const K_RT = 0, K_DEPTH = 1, K_OOM = 2, K_IO = 3, K_UNHANDLED = 4, K_UNIMPL = 5, K_INTERNAL = 6;
// テンプレートの後ろにつなぐ引数の種類(meta.rs の arg)。
const A_NONE = 0, A_SHOW = 1, A_F64 = 2, A_STR = 3, A_ARGS = 4;
// 状態ワードの種類(lib.rs の pk)。
const PK_ERROR = 4;
// 値の種類(rt.rs の vk)。
const VK = {
  I31: 1, BOX_I32: 2, I64: 3, F64: 4, STR: 5, UNIT: 6, BOOL: 7, DATA: 8, VARIANT: 9, RECORD: 10,
  CLOSURE: 11, METHOD: 12, EXTERNU: 13, REF: 14, ARRAY: 15, MARRAY: 16, DICT: 17,
};

const enc = new TextEncoder();

// OS のエラーメッセージの表。glibc の strerror と同じ文字列にして、Rust の io::Error の表示に合わせる。
// x86_64 の Linux の glibc の表を、エラー名(Node の err.code)ごとに書き写した。
const STRERROR = {
  EPERM: 'Operation not permitted',
  ENOENT: 'No such file or directory',
  ESRCH: 'No such process',
  EINTR: 'Interrupted system call',
  EIO: 'Input/output error',
  ENXIO: 'No such device or address',
  E2BIG: 'Argument list too long',
  ENOEXEC: 'Exec format error',
  EBADF: 'Bad file descriptor',
  ECHILD: 'No child processes',
  EAGAIN: 'Resource temporarily unavailable',
  EWOULDBLOCK: 'Resource temporarily unavailable',
  ENOMEM: 'Cannot allocate memory',
  EACCES: 'Permission denied',
  EFAULT: 'Bad address',
  ENOTBLK: 'Block device required',
  EBUSY: 'Device or resource busy',
  EEXIST: 'File exists',
  EXDEV: 'Invalid cross-device link',
  ENODEV: 'No such device',
  ENOTDIR: 'Not a directory',
  EISDIR: 'Is a directory',
  EINVAL: 'Invalid argument',
  ENFILE: 'Too many open files in system',
  EMFILE: 'Too many open files',
  ENOTTY: 'Inappropriate ioctl for device',
  ETXTBSY: 'Text file busy',
  EFBIG: 'File too large',
  ENOSPC: 'No space left on device',
  ESPIPE: 'Illegal seek',
  EROFS: 'Read-only file system',
  EMLINK: 'Too many links',
  EPIPE: 'Broken pipe',
  EDOM: 'Numerical argument out of domain',
  ERANGE: 'Numerical result out of range',
  EDEADLK: 'Resource deadlock avoided',
  EDEADLOCK: 'Resource deadlock avoided',
  ENAMETOOLONG: 'File name too long',
  ENOLCK: 'No locks available',
  ENOSYS: 'Function not implemented',
  ENOTEMPTY: 'Directory not empty',
  ELOOP: 'Too many levels of symbolic links',
  ENOMSG: 'No message of desired type',
  EIDRM: 'Identifier removed',
  ECHRNG: 'Channel number out of range',
  EL2NSYNC: 'Level 2 not synchronized',
  EL3HLT: 'Level 3 halted',
  EL3RST: 'Level 3 reset',
  ELNRNG: 'Link number out of range',
  EUNATCH: 'Protocol driver not attached',
  ENOCSI: 'No CSI structure available',
  EL2HLT: 'Level 2 halted',
  EBADE: 'Invalid exchange',
  EBADR: 'Invalid request descriptor',
  EXFULL: 'Exchange full',
  ENOANO: 'No anode',
  EBADRQC: 'Invalid request code',
  EBADSLT: 'Invalid slot',
  EBFONT: 'Bad font file format',
  ENOSTR: 'Device not a stream',
  ENODATA: 'No data available',
  ETIME: 'Timer expired',
  ENOSR: 'Out of streams resources',
  ENONET: 'Machine is not on the network',
  ENOPKG: 'Package not installed',
  EREMOTE: 'Object is remote',
  ENOLINK: 'Link has been severed',
  EADV: 'Advertise error',
  ESRMNT: 'Srmount error',
  ECOMM: 'Communication error on send',
  EPROTO: 'Protocol error',
  EMULTIHOP: 'Multihop attempted',
  EDOTDOT: 'RFS specific error',
  EBADMSG: 'Bad message',
  EOVERFLOW: 'Value too large for defined data type',
  ENOTUNIQ: 'Name not unique on network',
  EBADFD: 'File descriptor in bad state',
  EREMCHG: 'Remote address changed',
  ELIBACC: 'Can not access a needed shared library',
  ELIBBAD: 'Accessing a corrupted shared library',
  ELIBSCN: '.lib section in a.out corrupted',
  ELIBMAX: 'Attempting to link in too many shared libraries',
  ELIBEXEC: 'Cannot exec a shared library directly',
  EILSEQ: 'Invalid or incomplete multibyte or wide character',
  ERESTART: 'Interrupted system call should be restarted',
  ESTRPIPE: 'Streams pipe error',
  EUSERS: 'Too many users',
  ENOTSOCK: 'Socket operation on non-socket',
  EDESTADDRREQ: 'Destination address required',
  EMSGSIZE: 'Message too long',
  EPROTOTYPE: 'Protocol wrong type for socket',
  ENOPROTOOPT: 'Protocol not available',
  EPROTONOSUPPORT: 'Protocol not supported',
  ESOCKTNOSUPPORT: 'Socket type not supported',
  ENOTSUP: 'Operation not supported',
  EOPNOTSUPP: 'Operation not supported',
  EPFNOSUPPORT: 'Protocol family not supported',
  EAFNOSUPPORT: 'Address family not supported by protocol',
  EADDRINUSE: 'Address already in use',
  EADDRNOTAVAIL: 'Cannot assign requested address',
  ENETDOWN: 'Network is down',
  ENETUNREACH: 'Network is unreachable',
  ENETRESET: 'Network dropped connection on reset',
  ECONNABORTED: 'Software caused connection abort',
  ECONNRESET: 'Connection reset by peer',
  ENOBUFS: 'No buffer space available',
  EISCONN: 'Transport endpoint is already connected',
  ENOTCONN: 'Transport endpoint is not connected',
  ESHUTDOWN: 'Cannot send after transport endpoint shutdown',
  ETOOMANYREFS: 'Too many references: cannot splice',
  ETIMEDOUT: 'Connection timed out',
  ECONNREFUSED: 'Connection refused',
  EHOSTDOWN: 'Host is down',
  EHOSTUNREACH: 'No route to host',
  EALREADY: 'Operation already in progress',
  EINPROGRESS: 'Operation now in progress',
  ESTALE: 'Stale file handle',
  EUCLEAN: 'Structure needs cleaning',
  ENOTNAM: 'Not a XENIX named type file',
  ENAVAIL: 'No XENIX semaphores available',
  EISNAM: 'Is a named type file',
  EREMOTEIO: 'Remote I/O error',
  EDQUOT: 'Disk quota exceeded',
  ENOMEDIUM: 'No medium found',
  EMEDIUMTYPE: 'Wrong medium type',
  ECANCELED: 'Operation canceled',
  ENOKEY: 'Required key not available',
  EKEYEXPIRED: 'Key has expired',
  EKEYREVOKED: 'Key has been revoked',
  EKEYREJECTED: 'Key was rejected by service',
  EOWNERDEAD: 'Owner died',
  ENOTRECOVERABLE: 'State not recoverable',
  ERFKILL: 'Operation not possible due to RF-kill',
  EHWPOISON: 'Memory page has hardware error',
};

// 表にないエラーは、glibc が未知の番号に返すメッセージと同じにする。libuv の errno は負の数なので、絶対値を使う。
function strerror(e) {
  const s = STRERROR[e.code];
  if (s !== undefined) return s;
  if (typeof e.errno === 'number') return `Unknown error ${Math.abs(e.errno)}`;
  return String(e.message ?? e);
}

function bytesOf(parts) {
  const chunks = parts.map((p) => (typeof p === 'string' ? enc.encode(p) : p));
  let n = 0;
  for (const c of chunks) n += c.length;
  const out = new Uint8Array(n);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

const sleeper = new Int32Array(new SharedArrayBuffer(4));

function sleepMs(ms) {
  Atomics.wait(sleeper, 0, 0, ms);
}

// ファイル記述子 fd に data を書く。標準エラーへの出力に使い、エラーは無視する。
function writeQuiet(fd, data) {
  let written = 0;
  while (written < data.length) {
    try {
      const n = fs.writeSync(fd, data, written, data.length - written);
      if (n <= 0) return;
      written += n;
    } catch (e) {
      if (e.code === 'EINTR') continue;
      if (e.code === 'EAGAIN') {
        sleepMs(1);
        continue;
      }
      return;
    }
  }
}

// 標準出力。out.rs の Out と同じ規約に従う。
class Out {
  constructor(closed) {
    this.closed = closed;
    this.buf = new Uint8Array(2 * BUFFER_BYTES);
    this.len = 0;
  }

  push(data) {
    if (this.len + data.length > this.buf.length) {
      const nb = new Uint8Array(Math.max(this.buf.length * 2, this.len + data.length));
      nb.set(this.buf.subarray(0, this.len));
      this.buf = nb;
    }
    this.buf.set(data, this.len);
    this.len += data.length;
  }

  // data をファイル記述子 1 に書く。すべて書けたら null を返し、失敗したら { error, written, discard } を返す。
  // error はエラーメッセージ、written は書けたバイト数である。
  // discard は、バッファを捨てるエラー(EBADF、EPIPE、ECONNRESET)のときに真になる。
  writeAll(data) {
    let written = 0;
    let waits = 0;
    while (written < data.length) {
      let n;
      try {
        if (this.closed) {
          const e = new Error('closed');
          e.code = 'EBADF';
          throw e;
        }
        n = fs.writeSync(1, data, written, data.length - written);
      } catch (e) {
        if (e.code === 'EINTR') continue;
        if (e.code === 'EAGAIN') {
          waits += 1;
          sleepMs(Math.min(waits, 50));
          continue;
        }
        const discard = e.code === 'EBADF' || e.code === 'EPIPE' || e.code === 'ECONNRESET';
        return { error: strerror(e), written, discard };
      }
      if (n === 0) return { error: 'write returned 0', written, discard: false };
      written += n;
      waits = 0;
    }
    return null;
  }

  // バッファを flush する。失敗したらエラーメッセージを返す。
  flush() {
    if (this.len === 0) return null;
    const data = this.buf.slice(0, this.len);
    const f = this.writeAll(data);
    if (f === null) {
      this.len = 0;
      return null;
    }
    if (f.discard) {
      this.len = 0;
    } else {
      const rest = data.subarray(f.written);
      this.len = 0;
      this.push(rest);
    }
    return f.error;
  }

  // Console.write の文字列をバッファに追記し、バッファがいっぱいになったら flush する。
  // 書き込みに失敗したらエラーメッセージを返す。
  write(data) {
    if (this.len + data.length < BUFFER_BYTES) {
      this.push(data);
      return null;
    }
    const room = BUFFER_BYTES - this.len;
    this.push(data.subarray(0, room));
    const e = this.flush();
    if (e !== null) return e;
    const rest = data.subarray(room);
    const big = rest.length - (rest.length % BUFFER_BYTES);
    if (big > 0) {
      const f = this.writeAll(rest.subarray(0, big));
      if (f !== null) {
        if (!f.discard) {
          const left = rest.subarray(f.written, big);
          this.push(left.subarray(0, Math.min(left.length, BUFFER_BYTES)));
        }
        return f.error;
      }
    }
    this.push(rest.subarray(big));
    if (this.len >= BUFFER_BYTES) {
      const e2 = this.flush();
      if (e2 !== null) return e2;
    }
    return null;
  }

  // 標準出力を flush してから、msg を標準エラーに書く。flush の失敗は無視する。
  eprint(msg) {
    this.flush();
    writeQuiet(2, msg);
  }
}

function versionOk() {
  const v = process.versions.node.split('.').map((x) => parseInt(x, 10));
  for (let i = 0; i < 3; i++) {
    if ((v[i] ?? 0) > NODE_MIN[i]) return true;
    if ((v[i] ?? 0) < NODE_MIN[i]) return false;
  }
  return true;
}

// OCaml の String.escaped と同じ規則でバイト列をエスケープした文字列を返す(crate keleut-rt の escape_into と同じ)。
function escapeBytes(b) {
  let s = '';
  for (const c of b) {
    if (c === 0x22) s += '\\"';
    else if (c === 0x5c) s += '\\\\';
    else if (c === 0x0a) s += '\\n';
    else if (c === 0x09) s += '\\t';
    else if (c === 0x0d) s += '\\r';
    else if (c === 0x08) s += '\\b';
    else if (c >= 0x20 && c <= 0x7e) s += String.fromCharCode(c);
    else s += '\\' + String(c).padStart(3, '0');
  }
  return s;
}

// dir が keleut run の作った一時ディレクトリなら削除する。
// 2 つの .wasm を読んだ後に呼ぶので、その後で keleut が強制終了されても一時ディレクトリは残らない。
function removeTempDir(dir) {
  const want = process.env.KELEUT_WASM_RMDIR;
  if (want === undefined || want === '') return;
  const here = fileURLToPath(dir).replace(/\/+$/, '');
  if (here !== want.replace(/\/+$/, '')) return;
  try {
    fs.rmSync(here, { recursive: true, force: true });
  } catch (e) {
    // 削除できなくても、keleut が後で削除する。
  }
}

function runProgram(dir, META, exit) {
  const out = new Out(process.env.KELEUT_STDOUT_CLOSED === '1');
  const L = BigInt(META.maxDepth);
  let rtMod, progMod, rt;
  try {
    const rtBytes = fs.readFileSync(new URL('./rt.wasm', dir));
    const progBytes = fs.readFileSync(new URL('./program.wasm', dir));
    removeTempDir(dir);
    rtMod = new WebAssembly.Module(rtBytes);
    progMod = new WebAssembly.Module(progBytes);
    rt = new WebAssembly.Instance(rtMod, {}).exports;
  } catch (e) {
    out.eprint(enc.encode(`keleut: 内部エラー: モジュールを読み込めません: ${e}\n`));
    exit(70);
    return;
  }
  const mem = rt.memory;
  let top = (rt.__heap_base.value + 15) & ~15;
  // テスト用: 環境変数 KELEUT_TEST_WASM_MEM_GROW(バイト数)があれば、線形メモリの拡張を、
  // 起動を終えた時点のサイズにそのバイト数を加えたところまでに制限する。それを超える拡張はメモリ不足として扱う。
  // 起動の途中では制限しない(memCap は起動を終えてから決める)。
  let memCap = null;

  // 線形メモリの上に n バイトの領域を確保する。解放はしない。
  // 線形メモリを拡張できなければ、メモリ不足として 0 を返す。
  // アドレス 0 は heap_base より下にあるので、確保した領域のアドレスが 0 になることはない。
  function alloc(n) {
    const p = top;
    const end = p + ((n + 15) & ~15);
    const need = end - mem.buffer.byteLength;
    if (need > 0) {
      if (memCap !== null && end > memCap) return 0;
      try {
        mem.grow(Math.ceil(need / 65536));
      } catch (e) {
        return 0;
      }
    }
    top = end;
    return p;
  }

  // 起動の途中で線形メモリを拡張できないときは、LangSpec §15.5 の「評価のための領域を用意できない場合」に相当する。
  // §15.5 に従い、初期化を始めるときに起きた実行時エラーとして `実行時エラー: メモリ不足です` を書き、終了コード 3 で終了する。
  function startAlloc(n) {
    const p = alloc(n);
    if (p === 0) {
      out.eprint(enc.encode('実行時エラー: メモリ不足です\n'));
      exit(3);
    }
    return p;
  }

  const scratch = rt.krt_scratch();
  // シェイプ表の容量である。capShapes は入れられるシェイプの個数、capLabels は入れられるラベルの個数を表す。
  let capShapes = 1024;
  let capLabels = 16384;
  // シェイプ表を確保する。線形メモリを拡張できなければ 0 を返す。
  function newTable(cs, cl) {
    const bytes = rt.krt_shapes_bytes(cs, cl);
    const p = alloc(bytes + 8);
    if (p === 0) return 0;
    const at = (p + 7) & ~7;
    const t = rt.krt_shapes_init(at, cs, cl);
    if (t === 0) {
      out.eprint(enc.encode('keleut: 内部エラー: 形の表を作れません\n'));
      exit(70);
    }
    return t;
  }
  {
    let total = 0;
    for (const s of META.shapes) total += s.length;
    while (total + 1 > capLabels || META.shapes.length + 1 > capShapes) {
      capLabels *= 2;
      capShapes *= 2;
    }
  }
  const table = newTable(capShapes, capLabels);
  if (table === 0) {
    out.eprint(enc.encode('実行時エラー: メモリ不足です\n'));
    exit(3);
  }
  for (let i = 1; i < META.shapes.length; i++) {
    const s = META.shapes[i];
    const p = startAlloc(4 * s.length + 4);
    new Uint32Array(mem.buffer, p, s.length).set(s);
    const id = rt.krt_shape_intern(table, p, s.length, 0);
    if (id !== i) {
      out.eprint(enc.encode(`keleut: 内部エラー: 形の表の番号が合いません(LIR の ${i} 番が実行時の ${id} 番)\n`));
      exit(70);
    }
  }
  const shapes = new WebAssembly.Global({ value: 'i32', mutable: true }, table);

  // 文字列の受け渡し用のバッファ。サイズは rt.rs の STAGE と同じである。
  // 起動時に 1 度だけ確保し、拡張しない。
  // 長い文字列は Wasm の側が断片ごとにコピーするので、シェイプ表が線形メモリを使い切った後でも出力できる。
  const STAGE = 65536;
  const stage = startAlloc(STAGE);
  function buf(n) {
    if (n > STAGE) {
      out.eprint(enc.encode('keleut: 内部エラー: 受け渡しの領域より大きい切れです\n'));
      exit(70);
    }
    return stage;
  }
  {
    const grow = process.env.KELEUT_TEST_WASM_MEM_GROW;
    if (grow !== undefined && grow !== '') memCap = mem.buffer.byteLength + Number(grow);
  }
  // Wasm の側が host.write に渡した断片のうち、最後の断片が来るまでためておくもの。
  let pending = [];

  // 番号から診断メッセージを引く表。
  const msgs = new Map();
  let nextMsg = 1;
  let X = null;

  function strBytes(v) {
    const n = X.v_n(v);
    const r = new Uint8Array(n);
    for (let off = 0; off < n; off += STAGE) {
      const p = X.s_out(v, off);
      r.set(new Uint8Array(mem.buffer, p, Math.min(STAGE, n - off)), off);
    }
    return r;
  }

  function f64show(bits) {
    const p = scratch + 1024;
    const n = rt.krt_f64_show_bits(bits, p);
    return new Uint8Array(mem.buffer, p, n).slice();
  }

  function shapeLabels(shape) {
    const n = rt.krt_shape_len(shapes.value, shape);
    const p = rt.krt_shape_labels(shapes.value, shape);
    return Array.from(new Uint32Array(mem.buffer, p, n));
  }

  // 値を表示用のバイト列にする(crate keleut-rt の Rt::show と同じ)。
  // 深い値でも JS のスタックを使わないように、再帰せずにワークリストでたどる。
  // レコードのフィールドは、Rt::show と同じく正準なレイアウトの順に書く。
  // IRSpec の付録 B は、ラベルの違うフィールドどうしの順序をリーダに任せている。
  // しかし RustKeleut のバックエンドどうしは出力が一致しなければならないので、この順序を Rt::show と変えてはならない。
  function show(v0) {
    const parts = [];
    const work = [{ v: v0 }];
    const seq = (open, xs, close) => {
      work.push({ t: close });
      for (let i = xs.length - 1; i >= 0; i--) {
        work.push({ v: xs[i] });
        if (i > 0) work.push({ t: ', ' });
      }
      work.push({ t: open });
    };
    while (work.length > 0) {
      const it = work.pop();
      if (it.t !== undefined) {
        parts.push(it.t);
        continue;
      }
      const w = it.v;
      if (typeof w === 'number') {
        parts.push(String(w));
        continue;
      }
      const k = X.v_kind(w);
      const elems = () => {
        const n = X.v_n(w);
        const xs = [];
        for (let i = 0; i < n; i++) xs.push(X.v_get(w, i));
        return xs;
      };
      switch (k) {
        case VK.BOX_I32:
          parts.push(String(X.v_a(w)));
          break;
        case VK.I64:
          parts.push(X.v_i64(w).toString());
          break;
        case VK.F64:
          parts.push(f64show(X.v_i64(w)));
          break;
        case VK.STR:
          parts.push('"' + escapeBytes(strBytes(w)) + '"');
          break;
        case VK.UNIT:
          parts.push('()');
          break;
        case VK.BOOL:
          parts.push(X.v_a(w) ? 'true' : 'false');
          break;
        case VK.DATA: {
          const name = META.ctors[X.v_a(w)][X.v_b(w)];
          const xs = elems();
          if (xs.length === 0) parts.push(name);
          else seq(name + '(', xs, ')');
          break;
        }
        case VK.VARIANT: {
          const l = META.labels[X.v_a(w)];
          const p = X.v_get(w, 0);
          if (typeof p !== 'number' && X.v_kind(p) === VK.UNIT) parts.push('#' + l);
          else seq('#' + l + '(', [p], ')');
          break;
        }
        case VK.RECORD: {
          const labels = shapeLabels(X.v_a(w));
          const xs = elems();
          const tuple = labels.every((l) => META.items[l] === 1);
          if (tuple) {
            seq('(', xs, xs.length === 1 ? ',)' : ')');
          } else {
            work.push({ t: '}' });
            for (let i = xs.length - 1; i >= 0; i--) {
              work.push({ v: xs[i] });
              work.push({ t: (i > 0 ? ', ' : '') + META.labels[labels[i]] + ' = ' });
            }
            work.push({ t: '{' });
          }
          break;
        }
        case VK.CLOSURE: {
          const p = META.prim[X.v_a(w)];
          parts.push(p === null || p === undefined ? '<fn>' : `<prim ${p}>`);
          break;
        }
        case VK.METHOD:
          parts.push(META.methods[X.v_a(w)][X.v_b(w)]);
          break;
        case VK.EXTERNU:
          parts.push(`<prim ${META.globals[X.v_a(w)]}>`);
          break;
        case VK.REF:
          parts.push('<ref>');
          break;
        case VK.ARRAY:
          seq('[', elems(), ']');
          break;
        case VK.MARRAY:
          seq('<mutable [', elems(), ']>');
          break;
        case VK.DICT:
          parts.push('<dict>');
          break;
        default:
          parts.push('<?>');
      }
    }
    return bytesOf(parts);
  }

  function argsText(arr) {
    const n = X.v_n(arr);
    const parts = ['('];
    for (let i = 0; i < n; i++) {
      if (i > 0) parts.push(', ');
      parts.push(show(X.v_get(arr, i)));
    }
    if (n === 1) parts.push(',');
    parts.push(')');
    return bytesOf(parts);
  }

  function message(m) {
    switch (m.kind) {
      case K_RT:
        return bytesOf(['実行時エラー: ', m.text, '\n']);
      case K_DEPTH:
        return bytesOf([`実行時エラー: 論理的な深さが限度 ${L} を超えました\n`]);
      case K_OOM:
        return bytesOf(['実行時エラー: メモリ不足です\n']);
      case K_IO:
        return bytesOf(['keleut: 標準出力に書き出せません: ', m.text, '\n']);
      case K_UNHANDLED:
        return bytesOf(['未処理のエフェクト操作: ', m.text, '\n']);
      default:
        return bytesOf(['keleut: 内部エラー: 知らない種類の誤り\n']);
    }
  }

  function exitCode(m) {
    return m.kind === K_IO ? 74 : 3;
  }

  function suppressedText(m) {
    switch (m.kind) {
      case K_RT:
        return m.text;
      case K_DEPTH:
        return bytesOf([`論理的な深さが限度 ${L} を超えました`]);
      case K_OOM:
        return bytesOf(['メモリ不足です']);
      case K_IO:
        return bytesOf(['標準出力に書き出せません: ', m.text]);
      case K_UNHANDLED:
        return bytesOf(['未処理のエフェクト操作: ', m.text]);
      default:
        return bytesOf(['']);
    }
  }

  function fatal(kind, text) {
    if (kind === K_UNIMPL) {
      out.eprint(bytesOf(['keleut: 未実装: ', text, '\n']));
      exit(4);
    }
    out.eprint(bytesOf(['keleut: 内部エラー: ', text, '\n']));
    exit(70);
  }

  const host = {
    // more が 0 でなければ、続きの断片がある。断片をためておき、最後の断片が来たらまとめて 1 度に書く。
    write(ptr, len, more) {
      const part = new Uint8Array(mem.buffer, ptr, len).slice();
      if (more !== 0) {
        pending.push(part);
        return 0;
      }
      let data = part;
      if (pending.length > 0) {
        pending.push(part);
        let total = 0;
        for (const q of pending) total += q.length;
        data = new Uint8Array(total);
        let at = 0;
        for (const q of pending) {
          data.set(q, at);
          at += q.length;
        }
        pending = [];
      }
      const e = out.write(data);
      if (e === null) return 0;
      const id = nextMsg++;
      msgs.set(id, { kind: K_IO, text: enc.encode(e) });
      return id;
    },
    buf,
    err(t, v, x) {
      const [kind, a, text] = META.templates[t];
      let tail = new Uint8Array(0);
      if (a === A_SHOW) tail = show(v);
      else if (a === A_F64) tail = f64show(x);
      else if (a === A_STR) tail = strBytes(v);
      else if (a === A_ARGS) tail = argsText(v);
      const id = nextMsg++;
      msgs.set(id, { kind, text: bytesOf([text, tail]) });
      return id;
    },
    fatal(t) {
      const [kind, , text] = META.templates[t];
      fatal(kind, text);
    },
    suppress(kind, msg) {
      let text;
      if (kind === PK_ERROR && msgs.has(msg)) {
        text = suppressedText(msgs.get(msg));
        msgs.delete(msg);
      } else {
        text = enc.encode('cancel 節から操作の巻き戻しで脱出しようとしました');
      }
      const e = out.flush();
      if (e !== null) {
        out.eprint(bytesOf(['keleut: 標準出力に書き出せません: ', e, '\n']));
        exit(74);
      }
      writeQuiet(2, bytesOf(['cancel 節で例外が抑制されました: ', text, '\n']));
    },
    // シェイプ表を 2 倍に拡張する。
    // 線形メモリを拡張できなければ、表も容量も元のまま 1 を返す。
    // 1 を受け取った program.wasm のランタイム関数が、メモリ不足の ERROR を設定する。
    // cancel 節の中で再び表がいっぱいになっても、同じサイズをもう一度要求して同じように失敗する。
    shapes_grow() {
      const t2 = newTable(capShapes * 2, capLabels * 2);
      if (t2 === 0) return 1;
      if (rt.krt_shapes_copy(t2, shapes.value) !== 0) {
        fatal(K_INTERNAL, '形の表を広げられません');
      }
      capShapes *= 2;
      capLabels *= 2;
      shapes.value = t2;
      return 0;
    },
    shapes,
    scratch: new WebAssembly.Global({ value: 'i32', mutable: false }, scratch),
  };

  try {
    X = new WebAssembly.Instance(progMod, { rt, host }).exports;
  } catch (e) {
    out.eprint(enc.encode(`keleut: 内部エラー: モジュールを読み込めません: ${e}\n`));
    exit(70);
    return;
  }
  let code;
  try {
    code = X.main(L);
  } catch (e) {
    out.eprint(enc.encode(`keleut: 内部エラー: ${e}\n`));
    exit(70);
    return;
  }
  if (code === 0) {
    const e = out.flush();
    if (e !== null) {
      out.eprint(bytesOf(['keleut: 標準出力に書き出せません: ', e, '\n']));
      exit(74);
    }
    exit(0);
  }
  const kind = X.p_kind.value;
  if (kind === PK_ERROR && msgs.has(X.p_msg.value)) {
    const m = msgs.get(X.p_msg.value);
    out.eprint(message(m));
    exit(exitCode(m));
  }
  out.eprint(enc.encode(`keleut: 内部エラー: 最上位に届いた状態語: ${kind}\n`));
  exit(70);
}

// プログラムを起動する。main.mjs の最後の行が呼ぶ。
export async function start(META) {
  const dir = new URL('./', import.meta.url);
  const exit = (c) => process.exit(c);
  // ホストの中で例外が捕捉されないと、Node は既定で終了コード 1 で終了するが、1 は型エラーの終了コードである。
  // そこで、例外を内部エラーとして標準エラーに書き、終了コード 70 で終了する。
  const fail = (e) => {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: ${e}\n`));
    exit(70);
  };
  process.on('uncaughtException', fail);
  process.on('unhandledRejection', fail);
  if (!isMainThread) {
    runProgram(dir, workerData.META, exit);
    return;
  }
  // keleut の起動時に無視する設定(SIG_IGN)だったシグナルは、ここでも無視する。
  for (const sig of (process.env.KELEUT_IGNORED_SIGNALS ?? '').split(',')) {
    if (sig === 'SIGINT' || sig === 'SIGTERM' || sig === 'SIGHUP') process.on(sig, () => {});
  }
  if (!versionOk()) {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: Node ${NODE_MIN.join('.')} 以上が要ります(この Node は ${process.versions.node})\n`));
    exit(70);
  }
  // 環境変数 KELEUT_WASM_STATS があれば、選んだ経路と Worker のスタックサイズを標準エラーに 1 行書く。測定とテストに使う。
  const stats = process.env.KELEUT_WASM_STATS !== undefined;
  if (process.env.KELEUT_WASM_DIRECT === '1') {
    if (stats) writeQuiet(2, enc.encode('keleut-host: path=direct\n'));
    runProgram(dir, META, exit);
    return;
  }
  // KELEUT_WASM_DIRECT が 1 でないときは、Node を起動した側が RLIMIT_STACK と --stack-size を引き上げていないので、Worker のスタックで実行する。
  // スタックサイズは (L + R) × B_prog + 32 MiB で、keleut run が --stack-size に渡す値と同じ式で求める(crate keleut-wasm の stack_size_kib)。
  // B_prog はプログラムの 1 段に見積もるバイト数で、META.bProg に入っている(crate keleut-wasm の b_prog)。
  // STACK_CAP_KIB を超えたら STACK_CAP_KIB にするが、1 段を B_WASM バイトとして求めたサイズより小さくはしない。
  // resourceLimits.stackSizeMb には、MiB に切り上げてから 64 MiB を加えた値を渡す。
  const L = META.maxDepth;
  const R = Math.max(1024, Math.floor(L / 64));
  const need = (b) => Math.ceil(((L + R) * b + 32 * 1024 * 1024) / 1024);
  let stackKiB = need(META.bProg);
  if (stackKiB > STACK_CAP_KIB) stackKiB = Math.max(STACK_CAP_KIB, need(B_WASM));
  const stackSizeMb = Math.ceil(stackKiB / 1024) + 64;
  if (stats) writeQuiet(2, enc.encode(`keleut-host: path=worker stackSizeMb=${stackSizeMb}\n`));
  let w;
  try {
    w = new Worker(new URL(import.meta.url), {
      workerData: { META },
      resourceLimits: { stackSizeMb },
      stdout: false,
      stderr: false,
    });
  } catch (e) {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: 実行のスレッドを作れません(${stackSizeMb} MiB のスタック): ${e.message ?? e}\n`));
    exit(70);
    return;
  }
  w.on('error', (e) => {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: ${e}\n`));
    exit(70);
  });
  w.on('exit', (c) => exit(c));
}
