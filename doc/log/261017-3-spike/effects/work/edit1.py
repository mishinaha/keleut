import re
W='<一時領域>/clusters/effects/work/'
def ed(f, pairs):
    p=W+f; s=open(p).read()
    for a,b in pairs:
        n=s.count(a)
        assert n==1, (f,a,n)
        s=s.replace(a,b)
    s=s.replace('must_fix ','必須の指摘 ')
    open(p,'w').write(s)

ed('V25.md',[
 ('V25 の試作の上に V58 の試作を当てると、ずれはあるが当たり、ビルドは通る(反証者の実測、`doc/log/261017-3-spike/V25/verify/both/`)。',
  'V25 の試作の上に V58 の試作を当てると、ずれはあるが当たり、ビルドは通る(反証者の実測。分野の合成 N.z でも同じ結果を実測した)。'),
 ('(いずれも diktor の写しの中)。','(いずれも diktor の `doc/log/` の中)。'),
 ('試作の文面。litcheck の lex / block / width と marker 0 件を写しで確認した。','試作の文面。litcheck の lex / block / width と marker 0 件を試作で確認した。'),
 ('- 後から入れる側のコミットで qual5 の位置を直す。V25 を先に入れるなら、V58 の期待値の更新のコミットで qual5 を 5:19 から 7:8 に改める。V58 が先なら、V25 の期待値の更新で qual5 を 7:8 にする。',
  '- 後から入れる側のコミットで qual5 の位置を直す。本計画は V25 を先に入れ、V58 の期待値の更新のコミットで qual5 を 5:19 から 7:8 に改める(N.2、N.z)。'),
])
ed('V58.md',[
 ('試作は `doc/log/261017-3-spike/V58/V58.patch`(`diktor/` の写しに対する差分)である。','試作は `doc/log/261017-3-spike/V58/V58.patch`(HEAD に対する差分)である。'),
 ('前書きは実測から起こした(試作のファイルを参照)。','前書きは実測から起こした(差分の中の `test/handle_clause_loc.t` を参照)。'),
 ('(litcheck は git の中でしか動かないので、写しを `git init` した `litgit/` で走らせた)','(litcheck は git の中でしか動かないので、試作を一時的な git の写しにして走らせた)'),
])
ed('V101.md',[
 ('反証者が一時的な git の写し(`doc/log/261017-3-spike/V101/verify/gitcheck`)で','反証者が一時的な git の写しで'),
])
ed('V133.md',[
 ('一時の git の写し(`litgit2/`)での','一時的な git の写しでの'),
 ('新しいファイル `test/resume_sites.t`(試作の `diktor/test/resume_sites.t`)。','新しいファイル `test/resume_sites.t`(`doc/log/261017-3-spike/V133/V133.patch` の中)。'),
])
ed('V128.md',[
 ('### N.7 V128：(V125 を統合): 再開する操作の節が `resume` の前に脱出したときの、ハンドラ自身の `cancel` 節','### N.7 V128：再開する操作の節が `resume` の前に脱出したときの、ハンドラ自身の `cancel` 節(V125 を統合)'),
 ('(`diktor-alt/`、`resume` 前の','(`resume` 前の'),
 ('(試作 `doc/log/261017-3-spike/V128/V128.patch`、`diktor/`)','(試作 `doc/log/261017-3-spike/V128/V128.patch`)'),
])
ed('V89.md',[])
ed('V127.md',[])
ed('X-perform.md',[
 ('試作は V101 の試作(`doc/log/261017-3-spike/V101/V101.patch`)を当てた写しの上に作った。','試作は V101 の試作(`doc/log/261017-3-spike/V101/V101.patch`)を当てた写しの上に作った。') if False else ('試作は V101 の試作(`../V101/V101.patch`)を当てた写しの上に作った。','試作は V101 の試作(`doc/log/261017-3-spike/V101/V101.patch`)を当てた写しの上に作った。'),
])
