#!/bin/bash
# V230: RLIMIT_AS(KiB)を変えながら同じ KIR を各段で走らせ、終了状態と標準出力の行数を数える。
# 使い方: sweep_as.sh FILE.kir FROM TO STEP [native-stack]
set -u
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
f=$1; from=$2; to=$3; step=$4; ns=${5:-64m}
for ((as = from; as <= to; as += step)); do
  line="as=$as"
  for t in "--backend interp" "--tier jit-only" "--tier threshold=1" "--tier alternating=1"; do
    r=$(bash -c "ulimit -v $as; $K run $t --native-stack $ns $f 2>/dev/null | wc -l; echo \${PIPESTATUS[0]}" 2>/dev/null | tr '\n' ' ')
    set -- $r
    line="$line | ${t#--} exit=$2 out=$1"
  done
  echo "$line"
done
