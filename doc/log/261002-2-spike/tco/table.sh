#!/bin/sh
# usage: table.sh exe   — prints l=10k N=100000 and default N=1000000 for every program
cd "$(dirname "$0")/m"
for f in t*.kel p2*.kel p3*.kel p4*.kel p5*.kel p6*.kel p7*.kel p9*.kel n*.kel c_*.kel; do
  python3 ../run.py "$1" $f 100000 l=10k
  python3 ../run.py "$1" $f 1000000
done
