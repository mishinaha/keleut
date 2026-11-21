//! 宣言表を定義する(Diktor の `decls.ml`、第 6 章に対応する)。
//!
//! 宣言表は、型の名前空間、型エイリアス、データ型、エフェクト、型クラス、インスタンス、単位と module の別名、可視性の表を持つ。
//! 組み込みの型、エフェクト、型クラス、操作の登録もここで行う。

use super::fx::{HashMap, JMap, JSet};
use super::{GRef, Kind, Oid, R, Tc, Ty, TyNode, UnitId, VarInfo, display, terr};
use crate::ast::{NodeId, TypeExp, TypeParam};
use std::rc::Rc;

/// 型エイリアス 1 つの宣言の内容を持つ(Diktor の `alias_info` に対応する)。
#[derive(Clone, Debug)]
pub struct AliasInfo {
    pub al_name: Oid,
    pub al_params: Vec<TypeParam>,
    pub al_kinds: Vec<Kind>,
    pub al_kind: Option<String>,
    pub al_body: TypeExp,
    pub al_module: Option<String>,
    pub al_unit: UnitId,
}

/// コンストラクタのフィールド 1 つの、ラベルと型を持つ。
#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub fi_label: Option<Oid>,
    pub fi_ty: Ty,
}

/// コンストラクタ 1 つの、名前とフィールドの並びを持つ。
#[derive(Clone, Debug)]
pub struct CtorInfo {
    pub ct_name: Oid,
    pub ct_fields: Vec<FieldInfo>,
}

/// データ型(`newtype` で宣言する型と、組み込みの `Ref`、`Array`、`MutableArray`、`Never`)1 つの宣言の内容を持つ(Diktor の `data_info` に対応する)。
#[derive(Clone, Debug)]
pub struct DataInfo {
    pub dd_name: Oid,
    pub dd_params: Vec<VarInfo>,
    pub dd_ctors: Vec<CtorInfo>,
    pub dd_opaque: bool,
}

/// エフェクト 1 つの、名前と、操作の名前と型の並びを持つ。
#[derive(Clone, Debug)]
pub struct EffectInfo {
    pub ef_name: Oid,
    pub ef_ops: Vec<(Oid, Ty)>,
}

/// 型クラス 1 つの宣言の内容を持つ。
#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub ci_name: Oid,
    pub ci_param: VarInfo,
    pub ci_param_kind: Kind,
    pub ci_derive_structural: bool,
    pub ci_builtin: bool,
    pub ci_methods: Vec<(String, Ty)>,
}

/// インスタンス 1 つの、前提の並びと、組み込みかどうかを持つ。
#[derive(Clone, Debug)]
pub struct InstanceInfo {
    pub ii_premises: Vec<(usize, Oid)>,
    pub ii_builtin: bool,
}

/// 名前の可視性。宣言した module の名前(`vis_module`)と、`pub` を付けたかどうか(`vis_pub`)を持つ。
#[derive(Clone, Debug)]
pub struct Visibility {
    pub vis_module: String,
    pub vis_pub: bool,
}

/// 起点の単位の番号(Diktor の `root_unit` に対応する)。
pub const ROOT_UNIT: UnitId = 0;
/// 標準環境の単位の番号(Diktor の `std_unit` に対応する)。
pub const STD_UNIT: UnitId = -1;

/// 宣言表の本体。型、エフェクト、型クラス、インスタンスなどの宣言を、種類ごとの表に持つ。
#[derive(Clone, Default)]
pub struct Decls {
    pub in_prelude: bool,
    pub prelude_keys: JSet<(&'static str, Oid)>,
    pub type_namespace: JMap<Oid, &'static str>,
    pub externs: JSet<Oid>,
    pub con_kinds: JMap<Oid, Kind>,
    pub reserved_type_names: JSet<Oid>,
    pub current_module: Option<String>,
    pub current_unit: UnitId,
    pub unit_paths: JMap<UnitId, String>,
    pub decl_unit: JMap<NodeId, UnitId>,
    pub unit_synonyms: JMap<(UnitId, Oid), Oid>,
    pub unit_ctors: JMap<(UnitId, Oid), Oid>,
    pub unit_classes: JMap<(UnitId, Oid), Oid>,
    pub unit_qualifiers: JMap<(UnitId, String), String>,
    pub con_synonyms: JMap<Oid, Vec<Oid>>,
    pub module_con_synonyms: JMap<(String, Oid), Oid>,
    pub module_val_synonyms: JMap<(String, Oid), Oid>,
    pub con_hints: JMap<Oid, Vec<Oid>>,
    pub val_synonyms: JMap<Oid, Vec<Oid>>,
    pub value_visibility: JMap<Oid, Visibility>,
    pub con_visibility: JMap<Oid, Visibility>,
    pub decl_module: JMap<NodeId, String>,
    pub aliases: JMap<Oid, Rc<AliasInfo>>,
    pub datas: JMap<Oid, Rc<DataInfo>>,
    pub ctor_owner: JMap<Oid, Oid>,
    pub effects: JMap<Oid, Rc<EffectInfo>>,
    pub op_index: JMap<Oid, Vec<Oid>>,
    pub classes: JMap<Oid, Rc<ClassInfo>>,
    /// 型クラスを登録した順に並べたもの。
    /// Diktor は `Hashtbl` を走査するが、ここでは登録した順に走査する。
    pub class_order: Vec<Oid>,
    pub class_ranks: JMap<Oid, i64>,
    pub class_count: i64,
    pub instances: JMap<(Oid, Oid), InstanceInfo>,
    pub reserved_predicates: JSet<Oid>,
    pub builtin_ops: Vec<(String, Ty)>,
}

/// [`Decls::begin`] が返す値で、マークを置いた時点の、表でないフィールドの値を持つ。
/// [`Decls::rollback`] がこの値でフィールドを元に戻す。
pub struct DeclsMark {
    in_prelude: bool,
    current_module: Option<String>,
    current_unit: UnitId,
    class_order: usize,
    class_count: i64,
    builtin_ops: Vec<(String, Ty)>,
}

/// 宣言表のすべての表([`JMap`] と [`JSet`])に対して、マークの操作 `$op`(`begin`、`commit`、`rollback` のどれか)を呼ぶ。
macro_rules! each_table {
    ($self:ident, $op:ident) => {
        $self.prelude_keys.$op();
        $self.type_namespace.$op();
        $self.externs.$op();
        $self.con_kinds.$op();
        $self.reserved_type_names.$op();
        $self.unit_paths.$op();
        $self.decl_unit.$op();
        $self.unit_synonyms.$op();
        $self.unit_ctors.$op();
        $self.unit_classes.$op();
        $self.unit_qualifiers.$op();
        $self.con_synonyms.$op();
        $self.module_con_synonyms.$op();
        $self.module_val_synonyms.$op();
        $self.con_hints.$op();
        $self.val_synonyms.$op();
        $self.value_visibility.$op();
        $self.con_visibility.$op();
        $self.decl_module.$op();
        $self.aliases.$op();
        $self.datas.$op();
        $self.ctor_owner.$op();
        $self.effects.$op();
        $self.op_index.$op();
        $self.classes.$op();
        $self.class_ranks.$op();
        $self.instances.$op();
        $self.reserved_predicates.$op();
    };
}

impl Decls {
    /// 書き換えの記録を始めるマークを置く。
    ///
    /// 対話的な実行で入力を取り消すために、[`Tc::snapshot`] が使う。
    /// 表は以後の書き換えを記録し、表でないフィールドは値をコピーする。
    /// `class_order` は追加しかしないので、長さだけを記録する。
    /// そのため、処理時間は表のサイズによらない。
    pub fn begin(&mut self) -> DeclsMark {
        each_table!(self, begin);
        DeclsMark {
            in_prelude: self.in_prelude,
            current_module: self.current_module.clone(),
            current_unit: self.current_unit,
            class_order: self.class_order.len(),
            class_count: self.class_count,
            builtin_ops: self.builtin_ops.clone(),
        }
    }

    /// 最も内側のマークを外し、マークの後の書き換えを残す。
    pub fn commit(&mut self) {
        each_table!(self, commit);
    }

    /// マーク `m` の時点の状態に戻す。
    pub fn rollback(&mut self, m: DeclsMark) {
        each_table!(self, rollback);
        self.in_prelude = m.in_prelude;
        self.current_module = m.current_module;
        self.current_unit = m.current_unit;
        self.class_order.truncate(m.class_order);
        self.class_count = m.class_count;
        self.builtin_ops = m.builtin_ops;
    }
}

/// 内部名の接頭辞 `%N.` から単位の番号を求める(Diktor の `unit_of_spelling` に対応する)。
/// 接頭辞のない名前には `None` を返す。
pub fn unit_of_spelling(name: &str) -> Option<UnitId> {
    if name.len() > 1 && name.starts_with('%') {
        let i = name.find('.')?;
        ocaml_int_opt(&name[1..i])
    } else {
        None
    }
}

/// OCaml の `int_of_string_opt` に相当する。
/// 符号付きの 10 進表記だけを読む。
fn ocaml_int_opt(s: &str) -> Option<i32> {
    s.parse::<i32>().ok()
}

/// 単位 `u` で宣言した名前に接頭辞 `%N.` を付けるなら真を返す。
/// 接頭辞を付けるのは、番号が起点の単位より大きい単位である。
pub fn marks_unit(u: UnitId) -> bool {
    u > ROOT_UNIT
}

/// 単位 `u` で宣言した名前 `name` の内部名を返す。
/// 接頭辞を付けない単位では `name` をそのまま返す。
pub fn mark_spelling(u: UnitId, name: &str) -> String {
    if marks_unit(u) {
        format!("%{u}.{name}")
    } else {
        name.to_string()
    }
}

impl Tc {
    fn mark(&mut self, kind: &'static str, name: Oid) {
        if self.d.in_prelude {
            self.d.prelude_keys.insert((kind, name));
        }
    }

    /// 種類 `kind` の名前 `name` を、組み込みかプレリュードが登録していれば真を返す(Diktor の `prelude_owned` に対応する)。
    pub(crate) fn prelude_owned(&self, kind: &'static str, name: Oid) -> bool {
        self.d.prelude_keys.contains(&(kind, name))
    }

    /// 型の名前空間に、種類 `kind` の名前 `name` を登録する(Diktor の `claim_type_name` に対応する)。
    /// 同じ名前が別の種類で登録済みなら型エラーにする。
    pub(crate) fn claim_type_name(&mut self, kind: &'static str, name: Oid) -> R<()> {
        if let Some(prev) = self.d.type_namespace.get(&name)
            && *prev != kind
        {
            return terr(format!(
                "{} は既に {} として宣言されています({} では再宣言できません)",
                self.display_of(name),
                prev,
                kind
            ));
        }
        self.d.type_namespace.insert(name, kind);
        Ok(())
    }

    /// extern の宣言を登録する(Diktor の `add_extern` に対応する)。
    pub(crate) fn add_extern(&mut self, unit: UnitId, prim: &str, name: &str) -> R<()> {
        let spelled = if unit > 0 {
            format!("%{unit}.{name}")
        } else {
            name.to_string()
        };
        let o = self.intern(&spelled);
        let po = self.intern(prim);
        if self.prelude_owned("extern", po) {
            terr(format!("プレリュードの extern {prim} は再宣言できません"))
        } else if self.d.externs.contains(&o) {
            terr(format!("extern {} が二重に宣言されています", display(name)))
        } else {
            self.d.externs.insert(o);
            self.mark("extern", po);
            Ok(())
        }
    }

    /// `extern "C"` で宣言できる既知の関数 `name` のシグネチャを返す(Diktor の `c_known_signature` に対応する)。
    /// 返すのは、引数の型、返り値の型、表示用の文字列の組である。
    pub(crate) fn c_known_signature(
        &mut self,
        name: &str,
    ) -> Option<(Vec<Oid>, Oid, &'static str)> {
        let f64 = self.intern("Float64");
        match name {
            "sin" | "cos" | "sqrt" | "exp" | "log" => {
                Some((vec![f64], f64, "(Float64) => Float64"))
            }
            _ => None,
        }
    }

    /// 型構成子 `c` のカインドを返す(Diktor の `con_kind` に対応する)。
    /// カインドの登録がなければ、引数が `nargs` 個の型構成子のカインドを返す。
    pub(crate) fn con_kind(&mut self, c: Oid, nargs: usize) -> Kind {
        match self.d.con_kinds.get(&c) {
            Some(k) => *k,
            None => self.k_arrow(nargs as u32),
        }
    }

    fn check_std_type_twin(&mut self, kind: &'static str, name: Oid) -> R<()> {
        let s = self.name_of(name);
        let dn = display(&s).to_string();
        if dn != *s {
            let d = self.intern(&dn);
            let sp = if kind == "型エイリアス" {
                ""
            } else {
                " "
            };
            if self.d.reserved_type_names.contains(&d) {
                return terr(format!(
                    "組み込み型 {dn} は{sp}{kind}{sp}で再宣言できません"
                ));
            }
            match self.d.type_namespace.get(&d) {
                Some(prev) if *prev != kind => {
                    return terr(format!(
                        "{dn} は既に {prev} として宣言されています({kind} では再宣言できません)"
                    ));
                }
                Some(_) => return terr(format!("標準環境の{sp}{kind} {dn} は再宣言できません")),
                None => {}
            }
        }
        Ok(())
    }

    // ---- 単位 ----

    fn resolve_in(&self, tbl: &HashMap<(UnitId, Oid), Oid>, c: Oid) -> Oid {
        *tbl.get(&(self.d.current_unit, c)).unwrap_or(&c)
    }

    /// 型の名前空間の名前 `c` を、現在の単位の別名の表で内部名に解決する(Diktor の `resolve_unit` に対応する)。
    /// 表になければ `c` をそのまま返す。
    pub(crate) fn resolve_unit(&self, c: Oid) -> Oid {
        self.resolve_in(&self.d.unit_synonyms, c)
    }

    /// コンストラクタの名前 `c` を、現在の単位の別名の表で内部名に解決する(Diktor の `resolve_ctor` に対応する)。
    pub(crate) fn resolve_ctor(&self, c: Oid) -> Oid {
        self.resolve_in(&self.d.unit_ctors, c)
    }

    /// 型クラスの名前 `c` を、現在の単位の別名の表で内部名に解決する(Diktor の `resolve_class` に対応する)。
    pub(crate) fn resolve_class(&self, c: Oid) -> Oid {
        self.resolve_in(&self.d.unit_classes, c)
    }

    /// 修飾名の先頭の要素を、現在の単位の表で内部名に置き換える(Diktor の `qualify` に対応する)。
    /// `%` で始まる名前と、先頭の要素が表にない名前は、そのまま返す。
    pub(crate) fn qualify(&self, name: &str) -> String {
        if name.starts_with('%') {
            return name.to_string();
        }
        match name.find('.') {
            Some(i) => match self
                .d
                .unit_qualifiers
                .get(&(self.d.current_unit, name[..i].to_string()))
            {
                Some(h) => format!("{h}{}", &name[i..]),
                None => name.to_string(),
            },
            None => name.to_string(),
        }
    }

    /// 名前 `oid` が現在の単位から見えるなら真を返す(Diktor の `visible_in_unit` に対応する)。
    /// 接頭辞のない名前は、どの単位からも見える。
    /// 接頭辞のある名前は、現在の単位の別名の表(`which` で選ぶ)がその名前を指しているときだけ見える。
    pub(crate) fn visible_in_unit(&mut self, which: UnitTable, oid: Oid) -> bool {
        let name = self.name_of(oid);
        match unit_of_spelling(&name) {
            None => true,
            Some(_) => {
                let d = self.intern(display(&name));
                let tbl = match which {
                    UnitTable::Synonyms => &self.d.unit_synonyms,
                    UnitTable::Classes => &self.d.unit_classes,
                };
                tbl.get(&(self.d.current_unit, d)) == Some(&oid)
            }
        }
    }

    /// 型の名前 `short` のグローバルな別名の表に、解決先 `qual` を追加する(Diktor の `add_con_synonym` に対応する)。
    /// 解決先が 1 つだけのとき、`short` はその解決先に解決される。
    pub(crate) fn add_con_synonym(&mut self, short: Oid, qual: Oid) {
        let prev = self.d.con_synonyms.get_or_insert_with(short, Vec::new);
        if !prev.contains(&qual) {
            prev.push(qual);
        }
    }

    fn resolve_con_global_only(&self, c: Oid) -> Oid {
        match self.d.con_synonyms.get(&c) {
            Some(v) if v.len() == 1 => v[0],
            _ => c,
        }
    }

    /// 型の名前を、単位の別名、グローバルな別名の順に引いて解決する(Diktor の `resolve_con_global` に対応する)。
    pub(crate) fn resolve_con_global(&mut self, c: Oid) -> Oid {
        let q = self.qualify(&self.name_of(c));
        let c = self.intern(&q);
        match self.d.unit_synonyms.get(&(self.d.current_unit, c)) {
            Some(q) => *q,
            None => self.resolve_con_global_only(c),
        }
    }

    /// 型の名前を、module スコープの別名、単位の別名、グローバルな別名の順に引いて解決する(Diktor の `resolve_con` に対応する)。
    pub(crate) fn resolve_con(&mut self, c: Oid) -> Oid {
        let q = self.qualify(&self.name_of(c));
        let c = self.intern(&q);
        match &self.d.current_module {
            Some(m) => match self.d.module_con_synonyms.get(&(m.clone(), c)) {
                Some(q) => *q,
                None => self.resolve_con_global(c),
            },
            None => self.resolve_con_global(c),
        }
    }

    /// 型の名前 `short` について、未知の型と未知のエフェクトの診断で示す修飾名の候補に `qual` を追加する(Diktor の `add_con_hint` に対応する)。
    /// この表は診断だけに使い、名前の解決には使わない。
    pub(crate) fn add_con_hint(&mut self, short: Oid, qual: Oid) {
        let prev = self.d.con_hints.get_or_insert_with(short, Vec::new);
        if !prev.contains(&qual) {
            prev.push(qual);
        }
    }

    fn here(&self, q: Oid) -> bool {
        match unit_of_spelling(&self.name_of(q)) {
            None => true,
            Some(u) => u == self.d.current_unit,
        }
    }

    /// 型の名前 `c` について、診断で示す修飾名の候補を返す(Diktor の `con_synonym_candidates` に対応する)。
    /// 接頭辞 `%N.` が現在の単位と違う単位を指す名前は、候補から取り除く。
    /// 接頭辞のない名前は残す。
    pub(crate) fn con_synonym_candidates(&self, c: Oid) -> Vec<Oid> {
        self.d
            .con_hints
            .get(&c)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|q| self.here(*q))
            .collect()
    }

    /// 値の名前 `short` について、未束縛の変数の診断で示す修飾名の候補に `qual` を追加する(Diktor の `add_val_synonym` に対応する)。
    pub(crate) fn add_val_synonym(&mut self, short: Oid, qual: Oid) {
        let prev = self.d.val_synonyms.get_or_insert_with(short, Vec::new);
        if !prev.contains(&qual) {
            prev.push(qual);
        }
    }

    /// 値の名前 `short` について、診断で示す修飾名の候補を返す(Diktor の `val_synonym_candidates` に対応する)。
    /// 接頭辞 `%N.` が現在の単位と違う単位を指す名前は、候補から取り除く。
    /// 接頭辞のない名前は残す。
    pub(crate) fn val_synonym_candidates(&self, short: Oid) -> Vec<Oid> {
        self.d
            .val_synonyms
            .get(&short)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|q| self.here(*q))
            .collect()
    }

    /// 宣言 `id` を書いた module と単位を現在のものにして `f` を実行し、終わったら元に戻す(Diktor の `with_decl_scope` に対応する)。
    pub(crate) fn with_decl_scope<T>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut Tc) -> R<T>,
    ) -> R<T> {
        let cm = self.d.current_module.clone();
        let cu = self.d.current_unit;
        self.d.current_module = self.d.decl_module.get(&id).cloned();
        self.d.current_unit = *self.d.decl_unit.get(&id).unwrap_or(&ROOT_UNIT);
        let r = f(self);
        self.d.current_module = cm;
        self.d.current_unit = cu;
        r
    }

    fn visible_here(&self, v: &Visibility) -> bool {
        v.vis_pub || self.d.current_module.as_deref() == Some(v.vis_module.as_str())
    }

    /// 型 `oid` が現在の module から見えるなら真を返す。
    /// 可視性の登録がない型には `None` を返す。
    /// Diktor の `Decls.visible_here` を、型の可視性の表に適用したものに相当する。
    pub(crate) fn con_visible_here(&self, oid: Oid) -> Option<bool> {
        self.d
            .con_visibility
            .get(&oid)
            .map(|v| self.visible_here(v))
    }

    /// 型 `oid` を現在の module から参照できなければ、型エラーにする(Diktor の `check_con_visible` に対応する)。
    pub(crate) fn check_con_visible(&self, oid: Oid) -> R<()> {
        match self.d.con_visibility.get(&oid) {
            Some(v) if !self.visible_here(v) => terr(format!(
                "型 {} は module {} の外からは参照できません(pub を付けてください)",
                self.display_of(oid),
                display(&v.vis_module)
            )),
            _ => Ok(()),
        }
    }

    /// 値 `oid` を現在の module から参照できなければ、型エラーにする(Diktor の `check_value_visible` に対応する)。
    pub(crate) fn check_value_visible(&self, oid: Oid) -> R<()> {
        match self.d.value_visibility.get(&oid) {
            Some(v) if !self.visible_here(v) => terr(format!(
                "{} は module {} の外からは参照できません(pub を付けてください)",
                self.display_of(oid),
                display(&v.vis_module)
            )),
            _ => Ok(()),
        }
    }

    /// コンストラクタ `ctor` を持つ型 `owner` を現在の module から参照できなければ、型エラーにする(Diktor の `check_ctor_visible` に対応する)。
    pub(crate) fn check_ctor_visible(&self, ctor: Oid, owner: Oid) -> R<()> {
        match self.d.con_visibility.get(&owner) {
            Some(v) if !self.visible_here(v) => terr(format!(
                "コンストラクタ {} は module {} の外からは参照できません(newtype {} に pub を付けてください)",
                self.display_of(ctor),
                display(&v.vis_module),
                self.display_of(owner)
            )),
            _ => Ok(()),
        }
    }

    // ---- 型エイリアス、データ型、エフェクト ----

    /// 型エイリアスを登録する(Diktor の `add_alias` に対応する)。
    #[allow(clippy::map_entry)]
    pub(crate) fn add_alias(&mut self, info: AliasInfo) -> R<()> {
        let name = info.al_name;
        if self.d.reserved_type_names.contains(&name) && !self.d.in_prelude {
            return terr(format!(
                "組み込み型 {} は型エイリアスで再宣言できません",
                self.display_of(name)
            ));
        } else {
            self.check_std_type_twin("型エイリアス", name)?;
        }
        self.claim_type_name("型エイリアス", name)?;
        if self.d.aliases.contains_key(&name) {
            if self.prelude_owned("alias", name) {
                terr(format!(
                    "標準環境の型エイリアス {} は再宣言できません",
                    self.display_of(name)
                ))
            } else {
                terr(format!(
                    "型エイリアス {} が二重に宣言されています",
                    self.display_of(name)
                ))
            }
        } else {
            self.d.aliases.insert(name, Rc::new(info));
            self.mark("alias", name);
            Ok(())
        }
    }

    /// データ型を登録する(Diktor の `add_data` に対応する)。
    pub(crate) fn add_data(&mut self, info: DataInfo) -> R<()> {
        let name = info.dd_name;
        if self.d.reserved_type_names.contains(&name) && !self.d.in_prelude {
            return terr(format!(
                "組み込み型 {} は newtype で再宣言できません",
                self.display_of(name)
            ));
        } else {
            self.check_std_type_twin("newtype", name)?;
        }
        self.claim_type_name("newtype", name)?;
        if self.d.datas.contains_key(&name) {
            if self.prelude_owned("data", name) {
                return terr(format!(
                    "標準環境の newtype {} は再宣言できません",
                    self.display_of(name)
                ));
            }
            return terr(format!(
                "newtype {} が二重に宣言されています",
                self.display_of(name)
            ));
        }
        let ctors: Vec<Oid> = info.dd_ctors.iter().map(|c| c.ct_name).collect();
        self.d.datas.insert(name, Rc::new(info));
        self.mark("data", name);
        for ct in ctors {
            let s = self.name_of(ct);
            let ds = display(&s).to_string();
            let dso = self.intern(&ds);
            if self.d.ctor_owner.contains_key(&ct) || self.d.ctor_owner.contains_key(&dso) {
                return terr(format!(
                    "コンストラクタ {ds} が二重に宣言されています(コンストラクタ名はファイルの中で一意)"
                ));
            }
            self.d.ctor_owner.insert(ct, name);
        }
        Ok(())
    }

    /// エフェクトの名前を登録する(Diktor の `declare_effect` に対応する)。
    #[allow(clippy::map_entry)]
    pub(crate) fn declare_effect(&mut self, name: Oid) -> R<()> {
        if self.d.reserved_type_names.contains(&name) && !self.d.in_prelude {
            return terr(format!(
                "組み込み型 {} は effect で再宣言できません",
                self.display_of(name)
            ));
        } else {
            self.check_std_type_twin("effect", name)?;
        }
        self.claim_type_name("effect", name)?;
        if self.d.effects.contains_key(&name) {
            if self.prelude_owned("effect", name) {
                terr(format!(
                    "標準環境の effect {} は再宣言できません",
                    self.display_of(name)
                ))
            } else {
                terr(format!(
                    "effect {} が二重に宣言されています",
                    self.display_of(name)
                ))
            }
        } else {
            self.d.effects.insert(
                name,
                Rc::new(EffectInfo {
                    ef_name: name,
                    ef_ops: Vec::new(),
                }),
            );
            self.mark("effect", name);
            Ok(())
        }
    }

    /// エフェクトの操作を登録する(Diktor の `set_effect_ops` に対応する)。
    pub(crate) fn set_effect_ops(&mut self, info: EffectInfo) {
        let name = info.ef_name;
        let ops: Vec<Oid> = info.ef_ops.iter().map(|(o, _)| *o).collect();
        self.d.effects.insert(name, Rc::new(info));
        for op in ops {
            self.d.op_index.get_or_insert_with(op, Vec::new).push(name);
        }
    }

    pub(crate) fn add_effect(&mut self, info: EffectInfo) -> R<()> {
        self.declare_effect(info.ef_name)?;
        self.set_effect_ops(info);
        Ok(())
    }

    pub(crate) fn find_effect(&self, e: Oid) -> Option<Rc<EffectInfo>> {
        self.d.effects.get(&e).cloned()
    }

    /// 操作 `op` を持つエフェクトのうち、現在の単位から見えるものを返す(Diktor の `op_candidates` に対応する)。
    pub(crate) fn op_candidates(&mut self, op: Oid) -> Vec<Oid> {
        let cands = self.d.op_index.get(&op).cloned().unwrap_or_default();
        cands
            .into_iter()
            .filter(|e| self.visible_in_unit(UnitTable::Synonyms, *e))
            .collect()
    }

    // ---- 型クラスとインスタンス ----

    pub(crate) fn find_class(&self, c: Oid) -> Option<Rc<ClassInfo>> {
        self.d.classes.get(&c).cloned()
    }

    fn note_class_rank(&mut self, c: Oid) {
        self.d.class_count += 1;
        self.d.class_ranks.insert(c, self.d.class_count);
    }

    pub(crate) fn class_rank(&self, c: Oid) -> i64 {
        *self.d.class_ranks.get(&c).unwrap_or(&i64::MAX)
    }

    fn insert_class(&mut self, info: ClassInfo) {
        let name = info.ci_name;
        if !self.d.classes.contains_key(&name) {
            self.d.class_order.push(name);
        }
        self.d.classes.insert(name, Rc::new(info));
    }

    fn std_type(&self, c: Oid) -> bool {
        self.d.reserved_type_names.contains(&c)
            || self.prelude_owned("data", c)
            || self.prelude_owned("alias", c)
    }

    fn std_class(&self, c: Oid) -> bool {
        match self.d.classes.get(&c) {
            Some(ci) => ci.ci_builtin || self.prelude_owned("class", c),
            None => false,
        }
    }

    /// 名前 `oid` を宣言した単位を返す(Diktor の `entity_unit` に対応する)。
    /// 接頭辞のない名前には、組み込みかプレリュードの名前なら標準環境の単位を、それ以外なら起点の単位を返す。
    pub(crate) fn entity_unit(&self, oid: Oid) -> UnitId {
        match unit_of_spelling(&self.name_of(oid)) {
            Some(u) => u,
            None => {
                if self.std_type(oid) || self.std_class(oid) || self.prelude_owned("effect", oid) {
                    STD_UNIT
                } else {
                    ROOT_UNIT
                }
            }
        }
    }

    /// 単位 `u` の表示用のパスを返す(Diktor の `unit_path` に対応する)。
    /// 標準環境には `標準環境` を、パスの登録がない単位には `?` を返す。
    pub(crate) fn unit_path(&self, u: UnitId) -> String {
        if u == STD_UNIT {
            "標準環境".to_string()
        } else {
            self.d
                .unit_paths
                .get(&u)
                .cloned()
                .unwrap_or_else(|| "?".to_string())
        }
    }

    /// ユーザが宣言した型クラスを登録する(Diktor の `add_class_decl` に対応する)。
    pub(crate) fn add_class_decl(&mut self, info: ClassInfo) -> R<()> {
        let s = self.name_of(info.ci_name);
        let ds = display(&s).to_string();
        let dso = self.intern(&ds);
        if self.d.classes.contains_key(&dso) && ds != *s {
            return terr(format!("標準環境の type class {ds} は再宣言できません"));
        }
        match self.d.classes.get(&info.ci_name) {
            Some(prev) if prev.ci_builtin => terr(format!(
                "標準環境の type class {} は再宣言できません",
                self.display_of(info.ci_name)
            )),
            Some(_) => terr(format!(
                "type class {} が二重に宣言されています",
                self.display_of(info.ci_name)
            )),
            None => {
                let name = info.ci_name;
                self.insert_class(info);
                self.note_class_rank(name);
                self.mark("class", name);
                Ok(())
            }
        }
    }

    pub(crate) fn reserved_predicate(&self, c: Oid) -> bool {
        self.d.reserved_predicates.contains(&c)
    }

    /// インスタンスを登録する(Diktor の `add_instance` に対応する)。
    pub(crate) fn add_instance(
        &mut self,
        builtin: bool,
        cls: Oid,
        con: Oid,
        premises: Vec<(usize, Oid)>,
    ) -> R<()> {
        if self.reserved_predicate(cls) && !builtin {
            return terr(format!(
                "{} は予約されたリテラル述語です(インスタンスは宣言できません、D8)",
                self.display_of(cls)
            ));
        }
        match self.d.instances.get(&(cls, con)) {
            Some(prev) if prev.ii_builtin && !builtin => terr(format!(
                "標準環境のインスタンス {}[{}] は再宣言できません",
                self.display_of(cls),
                self.display_of(con)
            )),
            Some(_) => terr(format!(
                "インスタンス {}[{}] が二重に宣言されています(コヒーレンス違反)",
                self.display_of(cls),
                self.display_of(con)
            )),
            None => {
                self.d.instances.insert(
                    (cls, con),
                    InstanceInfo {
                        ii_premises: premises,
                        ii_builtin: builtin,
                    },
                );
                Ok(())
            }
        }
    }

    pub(crate) fn find_instance(&self, cls: Oid, con: Oid) -> Option<InstanceInfo> {
        self.d.instances.get(&(cls, con)).cloned()
    }

    // ---- 組み込み ----

    fn arrow_args(&mut self, args: &[Ty], ret: Ty, eff: Ty) -> Ty {
        let row = self.closed_item_row(args);
        let p = self.record(row);
        self.arrow(p, ret, eff)
    }

    fn register_ref_array(&mut self) {
        let ref_oid = self.intern("Ref");
        let arr_oid = self.intern("Array");
        let marr_oid = self.intern("MutableArray");
        let k2 = self.k_arrow(2);
        self.d.con_kinds.insert(ref_oid, k2);
        let k1 = self.k_arrow(1);
        self.d.con_kinds.insert(arr_oid, k1);
        let k2 = self.k_arrow(2);
        self.d.con_kinds.insert(marr_oid, k2);
        let ginfo = |tc: &mut Tc| VarInfo {
            vid: tc.new_oid(),
            vlevel: 0,
            vkind: tc.k_star,
            vcls: Vec::new(),
        };
        let p = vec![ginfo(self), ginfo(self)];
        let _ = self.add_data(DataInfo {
            dd_name: ref_oid,
            dd_params: p,
            dd_ctors: Vec::new(),
            dd_opaque: true,
        });
        let p = vec![ginfo(self)];
        let _ = self.add_data(DataInfo {
            dd_name: arr_oid,
            dd_params: p,
            dd_ctors: Vec::new(),
            dd_opaque: true,
        });
        let p = vec![ginfo(self), ginfo(self)];
        let _ = self.add_data(DataInfo {
            dd_name: marr_oid,
            dd_params: p,
            dd_ctors: Vec::new(),
            dd_opaque: true,
        });
        let heap = self.eff_heap;
        let _ = self.add_effect(EffectInfo {
            ef_name: heap,
            ef_ops: Vec::new(),
        });
        let blk = self.eff_blocking;
        let _ = self.add_effect(EffectInfo {
            ef_name: blk,
            ef_ops: Vec::new(),
        });
        let star = self.k_star;
        let row = self.k_row;
        let unit = self.t_unit;
        let i32t = self.t_int32;
        let empty = self.row_empty;
        let heap_row = |tc: &mut Tc, h: Ty| tc.extend(heap, h, empty);
        // OCaml の `let h = generic () and a = generic ()` は右から評価するので、`a` を先に作る。
        macro_rules! ha {
            () => {{
                let a = self.generic(star, Vec::new());
                let h = self.generic(star, Vec::new());
                (h, a)
            }};
        }
        let (h, a) = ha!();
        let r = self.con(ref_oid, vec![h, a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[a], r, e);
        self.d.builtin_ops.push(("Ref.new".into(), t));
        let (h, a) = ha!();
        let r = self.con(ref_oid, vec![h, a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[r], a, e);
        self.d.builtin_ops.push(("Ref.get".into(), t));
        let (h, a) = ha!();
        let r = self.con(ref_oid, vec![h, a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[r, a], unit, e);
        self.d.builtin_ops.push(("Ref.set".into(), t));
        let a = self.generic(star, Vec::new());
        let r = self.con(arr_oid, vec![a]);
        let t = self.arrow_args(&[r], i32t, empty);
        self.d.builtin_ops.push(("Array.length".into(), t));
        let a = self.generic(star, Vec::new());
        let r = self.con(arr_oid, vec![a]);
        let t = self.arrow_args(&[r, i32t], a, empty);
        self.d.builtin_ops.push(("Array.get".into(), t));
        let e = self.generic(row, Vec::new());
        let a = self.generic(star, Vec::new());
        let r = self.con(arr_oid, vec![a]);
        let f = self.arrow_args(&[a], unit, e);
        let t = self.arrow_args(&[r, f], unit, e);
        self.d.builtin_ops.push(("Array.each".into(), t));
        let (h, a) = ha!();
        let r = self.con(marr_oid, vec![h, a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[i32t, a], r, e);
        self.d.builtin_ops.push(("MutableArray.new".into(), t));
        let (h, a) = ha!();
        let r = self.con(marr_oid, vec![h, a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[r], i32t, e);
        self.d.builtin_ops.push(("MutableArray.length".into(), t));
        let (h, a) = ha!();
        let r = self.con(marr_oid, vec![h, a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[r, i32t], a, e);
        self.d.builtin_ops.push(("MutableArray.get".into(), t));
        let (h, a) = ha!();
        let r = self.con(marr_oid, vec![h, a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[r, i32t, a], unit, e);
        self.d.builtin_ops.push(("MutableArray.set".into(), t));
        let (h, a) = ha!();
        let r = self.con(marr_oid, vec![h, a]);
        let ra = self.con(arr_oid, vec![a]);
        let e = heap_row(self, h);
        let t = self.arrow_args(&[r], ra, e);
        self.d.builtin_ops.push(("MutableArray.freeze".into(), t));
    }

    fn register_pinned(&mut self) {
        let star = self.k_star;
        let row = self.k_row;
        let e = self.generic(row, Vec::new());
        let a = self.generic(star, Vec::new());
        let blk = self.eff_blocking;
        let unit = self.t_unit;
        let inner_eff = self.extend(blk, unit, e);
        let f = self.arrow_args(&[], a, inner_eff);
        let t = self.arrow_args(&[f], a, e);
        self.d.builtin_ops.push(("pinned".into(), t));
    }

    /// 組み込みの型、エフェクト、型クラス、インスタンス、操作を登録する(Diktor の `register_builtins` に対応する)。
    pub(crate) fn register_builtins(&mut self) {
        let saved = self.d.in_prelude;
        self.d.in_prelude = true;
        for n in ["Boolean", "Int32", "Int64", "Float64", "String", "Never"] {
            let o = self.intern(n);
            let s = self.k_star;
            self.d.con_kinds.insert(o, s);
        }
        for n in ["Boolean", "Int32", "Int64", "Float64", "String"] {
            let o = self.intern(n);
            self.d.reserved_type_names.insert(o);
        }
        let numerics = ["Int32", "Int64", "Float64"];
        type Methods<'a> = &'a dyn Fn(&mut Tc, Ty) -> Vec<(String, Ty)>;
        let def_class =
            |tc: &mut Tc, name: &str, derive: bool, methods: Methods, insts: &[&str]| {
                let cls = tc.intern(name);
                let pinfo = VarInfo {
                    vid: tc.new_oid(),
                    vlevel: 0,
                    vkind: tc.k_star,
                    vcls: vec![cls],
                };
                let c = tc.new_cell(super::TVar::Generic(pinfo.clone()));
                let a = tc.tvar(c);
                let ms = methods(tc, a);
                let star = tc.k_star;
                tc.insert_class(ClassInfo {
                    ci_name: cls,
                    ci_param: pinfo,
                    ci_param_kind: star,
                    ci_derive_structural: derive,
                    ci_builtin: true,
                    ci_methods: ms,
                });
                tc.note_class_rank(cls);
                for con in insts {
                    let co = tc.intern(con);
                    let _ = tc.add_instance(true, cls, co, Vec::new());
                }
            };
        let arrow2 = |tc: &mut Tc, a: Ty, ret: Ty| {
            let e = tc.row_empty;
            tc.arrow_args(&[a, a], ret, e)
        };
        let arrow1 = |tc: &mut Tc, a: Ty, ret: Ty| {
            let e = tc.row_empty;
            tc.arrow_args(&[a], ret, e)
        };
        let with_string = ["String", "Int32", "Int64", "Float64"];
        def_class(
            self,
            "Add",
            false,
            &|tc, a| vec![("add".into(), arrow2(tc, a, a))],
            &with_string,
        );
        def_class(
            self,
            "Sub",
            false,
            &|tc, a| vec![("sub".into(), arrow2(tc, a, a))],
            &numerics,
        );
        def_class(
            self,
            "Mul",
            false,
            &|tc, a| vec![("mul".into(), arrow2(tc, a, a))],
            &numerics,
        );
        def_class(
            self,
            "Div",
            false,
            &|tc, a| vec![("div".into(), arrow2(tc, a, a))],
            &numerics,
        );
        def_class(
            self,
            "Rem",
            false,
            &|tc, a| vec![("rem".into(), arrow2(tc, a, a))],
            &["Int32", "Int64"],
        );
        def_class(
            self,
            "Neg",
            false,
            &|tc, a| vec![("neg".into(), arrow1(tc, a, a))],
            &numerics,
        );
        def_class(
            self,
            "Eq",
            true,
            &|tc, a| {
                let b = tc.t_boolean;
                vec![("eq".into(), arrow2(tc, a, b))]
            },
            &["String", "Boolean", "Int32", "Int64", "Float64"],
        );
        def_class(
            self,
            "Ord",
            false,
            &|tc, a| {
                let b = tc.t_boolean;
                ["lt", "le", "gt", "ge"]
                    .iter()
                    .map(|m| (m.to_string(), arrow2(tc, a, b)))
                    .collect()
            },
            &with_string,
        );
        def_class(
            self,
            "Show",
            false,
            &|tc, a| {
                let s = tc.t_string;
                vec![("show".into(), arrow1(tc, a, s))]
            },
            &["String", "Boolean", "Int32", "Int64", "Float64"],
        );
        for n in ["Integral", "Fractional"] {
            let o = self.intern(n);
            self.d.reserved_predicates.insert(o);
        }
        def_class(
            self,
            "Integral",
            false,
            &|_, _| Vec::new(),
            &["Int32", "Int64"],
        );
        def_class(self, "Fractional", false, &|_, _| Vec::new(), &["Float64"]);
        let never = self.intern("Never");
        let _ = self.add_data(DataInfo {
            dd_name: never,
            dd_params: Vec::new(),
            dd_ctors: Vec::new(),
            dd_opaque: false,
        });
        self.d.builtin_ops.clear();
        self.register_ref_array();
        self.register_pinned();
        self.d.in_prelude = saved;
    }

    /// 組み込みの値の一覧を返す(Diktor の `builtin_value_refs` に対応する)。
    /// 各項目は、名前、型、参照先の組である。
    pub(crate) fn builtin_value_refs(&self) -> Vec<(String, Ty, GRef)> {
        let mut acc: Vec<(String, Ty, GRef)> = Vec::new();
        for c in &self.d.class_order {
            let ci = &self.d.classes[c];
            for (m, ty) in &ci.ci_methods {
                let q = format!("{}.{}", self.name_of(ci.ci_name), m);
                acc.push((m.clone(), *ty, GRef::Method(q.clone())));
                acc.push((q.clone(), *ty, GRef::Method(q)));
            }
        }
        for (n, ty) in &self.d.builtin_ops {
            acc.push((n.clone(), *ty, GRef::Builtin(n.clone())));
        }
        acc
    }

    /// `name` が、組み込みが値環境に置く名前なら真を返す(Diktor の `is_builtin_value` に対応する)。
    pub(crate) fn is_builtin_value(&self, name: &str) -> bool {
        self.builtin_value_refs().iter().any(|(n, _, _)| n == name)
    }

    /// 型 `t` が、引数のない型構成子 `c`(Diktor の `TCon (c, [])`)なら真を返す。
    pub(crate) fn is_con0(&mut self, t: Ty, c: Oid) -> bool {
        let t = self.repr(t);
        matches!(self.node_ref(t), TyNode::Con(c2, args) if *c2 == c && args.is_empty())
    }
}

/// [`Tc::visible_in_unit`] が調べる単位の別名の表を選ぶ。
/// `Synonyms` は型の名前空間の表、`Classes` は型クラスの表である。
#[derive(Clone, Copy)]
pub enum UnitTable {
    Synonyms,
    Classes,
}
