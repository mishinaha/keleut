declare i64 @g(i64, i64)
define i64 @f(i64 %x) {
  %r = musttail call i64 @g(i64 %x, i64 %x)
  ret i64 %r
}
