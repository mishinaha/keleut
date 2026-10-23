(module
  (import "env" "sleep_then" (func $sleep_then (param i32) (result i32)))
  (func (export "run") (param $x i32) (result i32)
    (i32.add (call $sleep_then (local.get $x)) (call $sleep_then (i32.const 100))))
)
