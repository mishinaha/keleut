declare void @f()
define void @g() gc "shadow-stack" {
  call void @f()
  ret void
}
