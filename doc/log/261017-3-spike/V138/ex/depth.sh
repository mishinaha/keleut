#!/bin/sh
# usage: depth.sh BIN TEMPLATE  (TEMPLATE has NNN placeholder) -> max n that prints ok under l=100k
bin=$1; t=$2; lo=1; hi=${HI:-200000}
while [ $((hi-lo)) -gt 1 ]; do
  mid=$(( (lo+hi)/2 ))
  sed "s/NNN/$mid/" $t > <一時領域>/items/V138/ex/_probe.kel
  if OCAMLRUNPARAM=l=${L:-100k} $bin <一時領域>/items/V138/ex/_probe.kel 2>&1 | grep -q '^ok$'; then lo=$mid; else hi=$mid; fi
done
echo $lo
