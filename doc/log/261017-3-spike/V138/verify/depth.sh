#!/bin/sh
bin=$1; t=$2; lo=${LO:-1}; hi=${HI:-200000}; p=$3
while [ $((hi-lo)) -gt 1 ]; do
  mid=$(( (lo+hi)/2 ))
  sed "s/NNN/$mid/" $t > $p
  if OCAMLRUNPARAM=l=${L:-100k} $bin $p 2>&1 | grep -q '^ok$'; then lo=$mid; else hi=$mid; fi
done
echo $lo
