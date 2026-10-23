%Ctx = type { [2 x i64], ptr, i64 }
%Ret = type { i64, ptr }
declare %Ret @interp_run(ptr, i64, i64, i64)
declare i32 @printf(ptr, ...)
@fmt = private constant [32 x i8] c"result=%ld interp_steps=%ld\0A\00\00\00\00"

; JIT-compiled f(n, acc): if n == 0 then acc else g(n - 1, acc + 1), g is interpreted -> via stub
define tailcc i64 @f(ptr %ctx, i64 %n, i64 %acc) {
  %z = icmp eq i64 %n, 0
  br i1 %z, label %done, label %rec
done:
  ret i64 %acc
rec:
  %n1 = sub i64 %n, 1
  %a1 = add i64 %acc, 1
  %r = call tailcc i64 @stub_g(ptr %ctx, i64 %n1, i64 %a1)
  ret i64 %r
}

; interpreter entry stub for interpreted function g (id 1)
define tailcc i64 @stub_g(ptr %ctx, i64 %n, i64 %acc) {
  %r = call %Ret @interp_run(ptr %ctx, i64 1, i64 %n, i64 %acc)
  %t = extractvalue %Ret %r, 1
  %isv = icmp eq ptr %t, null
  br i1 %isv, label %val, label %tc
val:
  %v = extractvalue %Ret %r, 0
  ret i64 %v
tc:
  %p0 = getelementptr %Ctx, ptr %ctx, i32 0, i32 0, i32 0
  %p1 = getelementptr %Ctx, ptr %ctx, i32 0, i32 0, i32 1
  %x0 = load i64, ptr %p0
  %x1 = load i64, ptr %p1
  %rr = call tailcc i64 %t(ptr %ctx, i64 %x0, i64 %x1)
  ret i64 %rr
}

define i32 @main() {
  %ctx = alloca %Ctx
  %fp = getelementptr %Ctx, ptr %ctx, i32 0, i32 1
  store ptr @f, ptr %fp
  %sp = getelementptr %Ctx, ptr %ctx, i32 0, i32 2
  store i64 0, ptr %sp
  %r = call tailcc i64 @f(ptr %ctx, i64 100000000, i64 0)
  %s = load i64, ptr %sp
  call i32 (ptr, ...) @printf(ptr @fmt, i64 %r, i64 %s)
  ret i32 0
}
