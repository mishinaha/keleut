# 簡易な cram の再生器。"  $ " と "  > " の行だけを 1 つの sh で順に実行し、
# 各コマンドの出力を、期待値の行を除いた形で元の文書に差し込んで印字する。
import subprocess, sys, os, tempfile
src, bindir = sys.argv[1], sys.argv[2]
lines = open(src, encoding="utf-8").read().split("\n")
cmds = []  # (index_of_command_line, command_text)
i = 0
out_doc = []
blocks = []
while i < len(lines):
    l = lines[i]
    if l.startswith("  $ "):
        cmd = l[4:]
        j = i + 1
        while j < len(lines) and lines[j].startswith("  > "):
            cmd += "\n" + lines[j][4:]
            j += 1
        # 期待値の行(2 空白で始まり $ でも > でもない)を飛ばす
        k = j
        while k < len(lines) and lines[k].startswith("  ") and not lines[k].startswith("  $ "):
            k += 1
        blocks.append(("cmd", lines[i:j], cmd))
        i = k
    else:
        blocks.append(("text", [l], None))
        i += 1
work = tempfile.mkdtemp(dir=os.environ.get("TMPDIR_BASE", "/tmp"))
script = []
marker = "@@MINICRAM@@"
for b in blocks:
    if b[0] == "cmd":
        script.append(b[2])
        script.append('__s=$?; [ $__s -ne 0 ] && echo "[$__s]"; echo ' + marker)
env = dict(os.environ)
env["PATH"] = bindir + ":" + env["PATH"]
env["TESTDIR"] = work
p = subprocess.run(["sh", "-c", "exec 2>&1\n" + "\n".join(script)], cwd=work, env=env, capture_output=True, text=True)
outs = p.stdout.split(marker + "\n")
n = 0
res = []
for b in blocks:
    if b[0] == "text":
        res.extend(b[1])
    else:
        res.extend(b[1])
        o = outs[n] if n < len(outs) else ""
        n += 1
        for ol in o.split("\n"):
            if ol != "":
                res.append("  " + ol)
print("\n".join(res))
