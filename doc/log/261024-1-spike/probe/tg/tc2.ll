declare ptr addrspace(1) @alloc(i64)
define tailcc ptr addrspace(1) @g(ptr addrspace(1) %x) gc "statepoint-example" {
  %y = call ptr addrspace(1) @alloc(i64 16)
  store ptr addrspace(1) %x, ptr addrspace(1) %y
  %r = musttail call tailcc ptr addrspace(1) @g(ptr addrspace(1) %y) #0
  ret ptr addrspace(1) %r
}
attributes #0 = { "gc-leaf-function" }
