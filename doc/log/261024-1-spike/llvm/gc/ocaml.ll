declare void @f()
define void @g() gc "ocaml" {
  call void @f()
  ret void
}
