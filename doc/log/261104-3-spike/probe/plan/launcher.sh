#!/bin/sh
if ! { true 3>&1; } 2>/dev/null; then
  echo 'keleut: 標準出力に書き出せません: Bad file descriptor' >&2
  exit 74
fi
echo ok
