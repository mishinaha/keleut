declare void @f()
define void @g() gc "statepoint-example" {
  call void @f()
  ret void
}
