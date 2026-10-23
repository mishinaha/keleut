#include <stdint.h>
#include <stddef.h>
typedef struct { int64_t args[2]; void *jit_f; int64_t interp_steps; } Ctx;
typedef struct { int64_t val; void *target; } Ret;
/* "interpreted" g(n, acc): if n == 0 then acc else f(n - 1, acc + 1)  (tail call into JIT code f) */
Ret interp_run(Ctx *ctx, int64_t fid, int64_t n, int64_t acc) {
  (void)fid;
  ctx->interp_steps++;
  if (n == 0) { Ret r = { acc, NULL }; return r; }
  ctx->args[0] = n - 1; ctx->args[1] = acc + 1;
  Ret r = { 0, ctx->jit_f };  /* tail-call request: the native stub performs it */
  return r;
}
