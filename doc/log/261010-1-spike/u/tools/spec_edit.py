import sys
src, dst = sys.argv[1], sys.argv[2]
L = open(src, encoding='utf-8').read().split('\n')
def ln(n): return L[n-1]
# sanity anchors
assert ln(965) == '### 13.2. 注釈の位置'
assert ln(1030) == '### 13.3. `perform`'
assert ln(508).startswith('ただし、値束縛の型注釈の最外の矢印に閉じた行')
assert ln(526).startswith('同じ再帰群の中で、最外の矢印に閉じた行を注釈した関数を')
assert ln(529).startswith('行の違う関数どうしを')
assert ln(1293).startswith('ただし、`pub let rec f')
assert ln(1294).startswith('`pub let rec f(...): T = ...`')
assert ln(1535).startswith('最外の `@ {}` は使用時に尾部を開くので')
assert ln(1536).startswith('`extern "prim"` の省略は行多相のままである')
assert ln(963).startswith('異なるラベルの順序は型の一致に影響せず')
assert ln(176).startswith('2 つの型が一致するのは')
assert ln(538).startswith('`pub` の束縛で最外の `@` を省略した場合の意味は')
sec132 = open(sys.argv[3], encoding='utf-8').read().rstrip('\n').split('\n')
edits = []  # (start, end_inclusive, new_lines) applied bottom-up
edits.append((1535, 1536, [
 '`@ {}` の外部関数は、使用時の開き（§13.2）によって、どの行の文脈からも呼べる。',
 '`extern "prim"` で `@` を省略すると、使うたびに新しい行変数になる（§13.2）。']))
edits.append((1293, 1294, []))
edits.append((965, 1029, sec132 + ['']))
edits.append((963, 963, [ln(963), '',
 '処理系が関数型を表示するときは、閉じた行を `@ {}` や `@ {Print}` のように書き、行変数だけからなる行は `@` ごと省く。',
 '行変数で終わる行は、`@ {Print extends R1}` のように書く。',
 '表示で `@` を省いた矢印の行は行変数であり、`pub let` の注釈で `@` を省略したとき（`@ {}`）とは意味が違う。']))
edits.append((538, 538, ['`pub` の束縛で最外の `@` を省略した場合の意味は、§13.2 の省略の表に従う。']))
edits.append((526, 529, [
 '同じ再帰群の中の呼び出しにも、使用時の開き（§13.2）を適用する。',
 '処理系は、最外の矢印に書いた行、`pub` の関数の省略した行（`@ {}`）、値束縛の型注釈を、群のどの本体を検査するよりも前に置く。',
 'そのため、注釈した閉じた行のラベルが呼ぶ側の行に含まれていれば、関数を書いた順序によらず呼べる。',
 '各関数がそれぞれ導入した行変数は、綴りが同じというだけでは同じ変数にならない。',
 '行を注釈しない関数の行は本体から推論し、群の中の呼び出しは単相なので、互いを呼ぶ関数の推論した行は一致する。']))
edits.append((508, 508, [
 '閉じたエフェクト行を持つ関数を使うときに行の尾部を開く規則（§13.2）は、一般化とは別の規則であり、初期化式が構文上の値でなくても働く。']))
edits.append((176, 176, [ln(176),
 'ただし、関数を呼び出すときと、関数型を要求する位置に関数を置くときは、閉じたエフェクト行の尾部を開いてから一致させる（§13.2）。']))
for s, e, new in sorted(edits, key=lambda x: -x[0]):
    L[s-1:e] = new
open(dst, 'w', encoding='utf-8').write('\n'.join(L))
