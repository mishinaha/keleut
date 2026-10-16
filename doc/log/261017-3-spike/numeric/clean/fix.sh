#!/bin/bash
# usage: fix.sh in out
S=<一時領域>
sed -E \
  -e "s#^diff -ruN -x _build (${S}/|\.\./\.\./)snap/diktor/([^ ]+) diktor/([^ ]+)\$#diff -ruN a/\2 b/\3#" \
  -e "s#^--- (${S}/|\.\./\.\./)snap/diktor/#--- a/#" \
  -e "s#^\+\+\+ diktor/#+++ b/#" \
  -e "s#^--- ${S}/clusters/numeric/xp/unify.before-fix.ml#--- a/lib/unify.ml#" \
  -e "s#^\+\+\+ lib/unify.ml#+++ b/lib/unify.ml#" \
  "$1" > "$2"
