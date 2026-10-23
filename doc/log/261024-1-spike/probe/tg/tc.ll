define tailcc ptr addrspace(1) @g(ptr addrspace(1) %x) gc "statepoint-example" {
  %r = musttail call tailcc ptr addrspace(1) @g(ptr addrspace(1) %x)
  ret ptr addrspace(1) %r
}
