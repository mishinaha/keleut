declare ptr addrspace(1) @alloc(i64)
declare void @safepoint_poll()
define ptr addrspace(1) @mk(ptr addrspace(1) %x) gc "statepoint-example" {
  %y = call ptr addrspace(1) @alloc(i64 16)
  store ptr addrspace(1) %x, ptr addrspace(1) %y
  %z = call ptr addrspace(1) @alloc(i64 16)
  store ptr addrspace(1) %y, ptr addrspace(1) %z
  ret ptr addrspace(1) %x
}
