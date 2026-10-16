#!/bin/bash
H=$1; P=$2; shift 2
for f in "$@"; do for m in "--type-check" ""; do a=$($H $m $f 2>&1; echo "[$?]"); b=$($P $m $f 2>&1; echo "[$?]"); if [ "$a" = "$b" ]; then echo "== $f $m: $(echo "$a"|tr '\n' ' ')"; else echo "!! $f $m"; echo " H: $(echo "$a"|tr '\n' ' ')"; echo " P: $(echo "$b"|tr '\n' ' ')"; fi; done; done
