(module
  ;; stack switching (WasmFX): generator yielding 1..n
  (type $ft (func))
  (type $ct (cont $ft))
  (tag $yield (param i32))
  (elem declare func $gen)
  (global $n (mut i32) (i32.const 0))
  (func $gen
    (local $i i32)
    (loop $l
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (suspend $yield (local.get $i))
      (br_if $l (i32.lt_u (local.get $i) (global.get $n)))))
  (func (export "sum") (param $n i32) (result i32)
    (local $k (ref null $ct)) (local $acc i32)
    (global.set $n (local.get $n))
    (local.set $k (cont.new $ct (ref.func $gen)))
    (loop $drive
      (block $on_yield (result i32 (ref $ct))
        (resume $ct (on $yield $on_yield) (local.get $k))
        (return (local.get $acc)))
      (local.set $k)
      (local.set $acc (i32.add (local.get $acc)))
      (br $drive))
    (unreachable))
)
