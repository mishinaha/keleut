(module
  ;; JS String Builtins (wasm:js-string), i16 array -> JS string
  (type $u16 (array (mut i16)))
  (import "wasm:js-string" "fromCharCodeArray" (func $from (param (ref null $u16) i32 i32) (result (ref extern))))
  (import "wasm:js-string" "length" (func $len (param externref) (result i32)))
  (import "env" "log" (func $log (param externref)))
  (func (export "go") (result i32)
    (local $s (ref extern))
    (local.set $s (call $from (array.new_fixed $u16 3 (i32.const 0x4b) (i32.const 0x65) (i32.const 0x6c)) (i32.const 0) (i32.const 3)))
    (call $log (local.get $s))
    (call $len (local.get $s)))
)
