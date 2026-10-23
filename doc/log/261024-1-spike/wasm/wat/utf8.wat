(module
  ;; imported-strings UTF-8 builtins: i8 byte array -> JS string
  (type $bytes (array (mut i8)))
  (import "wasm:text-decoder" "decodeStringFromUTF8Array"
    (func $dec (param (ref null $bytes) i32 i32) (result (ref extern))))
  (import "env" "log" (func $log (param externref)))
  (func (export "go")
    (call $log (call $dec (array.new_fixed $bytes 4 (i32.const 0x4b) (i32.const 0xe3) (i32.const 0x81) (i32.const 0x82)) (i32.const 0) (i32.const 4))))
)
