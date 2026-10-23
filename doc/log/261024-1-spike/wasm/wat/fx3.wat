(module
  ;; the switch instruction itself (symmetric stack switching)
  (rec (type $fa (func (param (ref null $ca)) (result i32)))
       (type $ca (cont $fa)))
  (tag $sw (result i32))
  (elem declare func $b)
  ;; $b receives the continuation of main and returns 7 when started
  (func $b (type $fa) (i32.const 7))
  (func $main (export "main") (type $fa)
    (drop (switch $ca $sw (cont.new $ca (ref.func $b))))
    (i32.const 1))
  (func (export "run") (result i32)
    (resume $ca (on $sw switch) (ref.null $ca) (cont.new $ca (ref.func $main))))
)
