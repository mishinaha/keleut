; ModuleID = 'tc2.ll'
source_filename = "tc2.ll"

declare ptr addrspace(1) @alloc(i64)

define tailcc ptr addrspace(1) @g(ptr addrspace(1) %x) gc "statepoint-example" {
  %statepoint_token = call token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 2882400000, i32 0, ptr elementtype(ptr addrspace(1) (i64)) @alloc, i32 1, i32 0, i64 16, i32 0, i32 0) [ "gc-live"(ptr addrspace(1) %x) ]
  %y1 = call ptr addrspace(1) @llvm.experimental.gc.result.p1(token %statepoint_token)
  %x.relocated = call coldcc ptr addrspace(1) @llvm.experimental.gc.relocate.p1(token %statepoint_token, i32 0, i32 0) ; (%x, %x)
  store ptr addrspace(1) %x.relocated, ptr addrspace(1) %y1, align 8
  %r = musttail call tailcc ptr addrspace(1) @g(ptr addrspace(1) %y1) #1
  ret ptr addrspace(1) %r
}

declare void @__tmp_use(...)

declare token @llvm.experimental.gc.statepoint.p0(i64 immarg, i32 immarg, ptr, i32 immarg, i32 immarg, ...)

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(none)
declare ptr addrspace(1) @llvm.experimental.gc.result.p1(token) #0

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(none)
declare ptr addrspace(1) @llvm.experimental.gc.relocate.p1(token, i32 immarg, i32 immarg) #0

attributes #0 = { nocallback nofree nosync nounwind willreturn memory(none) }
attributes #1 = { "gc-leaf-function" }
