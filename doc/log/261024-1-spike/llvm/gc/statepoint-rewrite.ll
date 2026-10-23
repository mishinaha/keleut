declare void @f()
define ptr addrspace(1) @g(ptr addrspace(1) %obj) gc "statepoint-example" {
  call void @f()
  ret ptr addrspace(1) %obj
}
