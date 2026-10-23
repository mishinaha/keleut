declare void @f()
define void @g() gc "erlang" {
  call void @f()
  ret void
}
