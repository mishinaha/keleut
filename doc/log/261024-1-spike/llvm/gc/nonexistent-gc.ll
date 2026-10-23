declare void @f()
define void @g() gc "nonexistent-gc" {
  call void @f()
  ret void
}
