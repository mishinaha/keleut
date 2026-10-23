declare void @f()
define void @g() gc "coreclr" {
  call void @f()
  ret void
}
