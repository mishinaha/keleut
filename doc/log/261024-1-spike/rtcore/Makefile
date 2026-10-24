# rtcore の試作: musl の sin cos exp log を x86_64 と wasm32 に組み、結果のビットを比べる。
# 使い方: make check(出力は out/ に置く。make clean で消す)

CLANG   ?= clang
WASMLD  ?= wasm-ld
NODE    ?= node
NM      ?= nm
LD      ?= ld
OUT     ?= out

MUSL_SRC = sin.c cos.c __sin.c __cos.c __rem_pio2.c __rem_pio2_large.c \
           exp.c exp_data.c log.c log_data.c floor.c scalbn.c \
           __math_oflow.c __math_uflow.c __math_xflow.c __math_divzero.c __math_invalid.c
EXPORTS  = krt_sin krt_cos krt_exp krt_log

# libc のヘッダを読まない(-nostdlibinc)。stdint.h と float.h は clang の組み込みのものを使い、
# math.h、endian.h、features.h、fp_arch.h は shim/ のものを使う。
CFLAGS_COMMON = -std=c99 -O2 -ffreestanding -fno-builtin -ffp-contract=off -fno-math-errno \
                -nostdlibinc -Ishim -Imusl/src/internal -include shim/krt_names.h \
                -Wall -Wno-unused-function -Wno-parentheses -Wno-dangling-else -Wno-unused-but-set-variable
CFLAGS_X86  = --target=x86_64-unknown-linux-gnu -fno-stack-protector $(CFLAGS_COMMON)
CFLAGS_WASM = --target=wasm32 -nostdlib $(CFLAGS_COMMON)

X86_OBJ  = $(MUSL_SRC:%.c=$(OUT)/x86_64/%.o)
WASM_OBJ = $(MUSL_SRC:%.c=$(OUT)/wasm32/%.o)

.PHONY: all check clean
all: $(OUT)/krt_musl.o $(OUT)/rt.wasm $(OUT)/native

$(OUT)/x86_64/%.o: musl/src/math/%.c $(wildcard shim/*.h) | $(OUT)/x86_64
	$(CLANG) $(CFLAGS_X86) -c $< -o $@
$(OUT)/wasm32/%.o: musl/src/math/%.c $(wildcard shim/*.h) | $(OUT)/wasm32
	$(CLANG) $(CFLAGS_WASM) -c $< -o $@
$(OUT)/x86_64 $(OUT)/wasm32:
	mkdir -p $@

# x86_64 版は 1 つの再配置可能なオブジェクトにまとめ、未定義の記号が無いことを確かめる。
$(OUT)/krt_musl.o: $(X86_OBJ)
	$(LD) -r -o $@ $^

$(OUT)/rt.wasm: $(WASM_OBJ)
	$(WASMLD) --no-entry $(EXPORTS:%=--export=%) -o $@ $^

# 試験の駆動部は通常の(hosted な)C で、比較のために glibc の libm を結ぶ。
$(OUT)/native: test/native.c $(OUT)/krt_musl.o
	$(CLANG) -std=c99 -O2 -ffp-contract=off -Wall -o $@ $^ -lm

check: all
	@echo "== nm -u (x86_64 object; must be empty)"
	@u=$$($(NM) -u $(OUT)/krt_musl.o); echo "$${u:-(none)}"; test -z "$$u"
	@echo "== global symbols defined by the x86_64 object (must all start with krt_)"
	@g=$$($(NM) -g --defined-only $(OUT)/krt_musl.o | awk '{print $$3}'); echo $$g; \
	  ! echo "$$g" | grep -v '^krt_'
	@echo "== x86_64 (krt_* vs glibc)"
	$(OUT)/native $(OUT)
	@echo "== wasm32 under node (vs x86_64)"
	$(NODE) test/check.mjs $(OUT)

clean:
	rm -rf $(OUT)
