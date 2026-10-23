declare tailcc i64 @g(i64, i64, i64, i64, i64, i64, i64, i64)
define tailcc i64 @f(i64 %x) {
  %r = musttail call tailcc i64 @g(i64 %x, i64 %x, i64 %x, i64 %x, i64 %x, i64 %x, i64 %x, i64 %x)
  ret i64 %r
}
