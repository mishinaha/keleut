import subprocess,time,sys
head,new,big,tiny=sys.argv[1:5]
def t(cmd,n=7):
    best=1e9
    for _ in range(n):
        a=time.perf_counter(); subprocess.run(cmd,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); best=min(best,time.perf_counter()-a)
    return best*1000
for label,args in [("--dump-ast big.kel",["--dump-ast",big]),("--type-check big.kel",["--type-check",big]),("tiny.kel の実行",[tiny])]:
    print(f"{label}: HEAD {t([head]+args):.1f} ms / 合成 {t([new]+args):.1f} ms (7 回の最小)")
