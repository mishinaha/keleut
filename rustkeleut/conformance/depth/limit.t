Diktor の KIR の評価器が上限に達する前に終了する場合と、再帰が既定の上限の近くまで達する場合のテストである(IRSpec 11.5 節)。
期待出力は keleut run の結果である。

既定の上限 L = 2^20 のとき、L − 10 段の非末尾の再帰は終了コード 0 で終了する。

  $ cat > deep.kel <<'EOF'
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > echoln(show(f(1048566)))
  > EOF
  $ keleut run deep.kel
  1048566

Diktor の KIR の評価器は、非末尾呼び出しが 4096 段進むたびに OCaml のファイバーを分け、OCaml のスタックの上限はファイバーごとに適用される。
各段の呼び出しを 40 重のブロックで囲み、OCAMLRUNPARAM で上限を 1M ワードに下げると、4096 段分のスタックが上限を超える。
そのためペア dk-ir は、論理的な深さが上限に達する前に、スタックオーバーフローの実行時エラー(終了コード 3)で終了する。
keleut run は、上限 20000 のとき最後まで実行する。
上限の大きさは処理系に任されている(§15.5 の I)ので、この差分は known-diffs.toml に台帳 V213 で登録してある(理由は `限度は I` で始まる)。

  $ cat > nest40.kel <<'EOF'
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + { let v39 = { let v38 = { let v37 = { let v36 = { let v35 = { let v34 = { let v33 = { let v32 = { let v31 = { let v30 = { let v29 = { let v28 = { let v27 = { let v26 = { let v25 = { let v24 = { let v23 = { let v22 = { let v21 = { let v20 = { let v19 = { let v18 = { let v17 = { let v16 = { let v15 = { let v14 = { let v13 = { let v12 = { let v11 = { let v10 = { let v9 = { let v8 = { let v7 = { let v6 = { let v5 = { let v4 = { let v3 = { let v2 = { let v1 = { let v0 = f(n - 1); v0 }; v1 }; v2 }; v3 }; v4 }; v5 }; v6 }; v7 }; v8 }; v9 }; v10 }; v11 }; v12 }; v13 }; v14 }; v15 }; v16 }; v17 }; v18 }; v19 }; v20 }; v21 }; v22 }; v23 }; v24 }; v25 }; v26 }; v27 }; v28 }; v29 }; v30 }; v31 }; v32 }; v33 }; v34 }; v35 }; v36 }; v37 }; v38 }; v39 }
  > echoln(show(f(10000)))
  > EOF
  $ OCAMLRUNPARAM=l=1M keleut run --max-depth 20000 nest40.kel
  10000
