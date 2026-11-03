  $ printf 'let e = {c = 3 extends {a = 1, b = 2}}\nlet g(x: {c: Int32, a: Int32, b: Int32}): Int32 = x match { case {c = 0, a, b} => a }\ng(e)\nlet k: {m: Int32, n: Int32} = {n = 1, m = 2}\n' | diktor --repl
  e : {c: Int32, a: Int32, b: Int32} = {c = 3, a = 1, b = 2}
  ⚠ match が非網羅的です。例えば {c = 1, a = _, b = _} が漏れています
  g : ({c: Int32, a: Int32, b: Int32}) => Int32 = <fn>
  実行時エラー: match のどの節にも一致しません: {c = 3, a = 1, b = 2}
  k : {m: Int32, n: Int32} = {n = 1, m = 2}
