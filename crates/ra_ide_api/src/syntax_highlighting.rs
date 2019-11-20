//! FIXME: write short doc here

use rustc_hash::{FxHashMap, FxHashSet};

use hir::{Mutability, Name, Source};
use ra_db::SourceDatabase;
use ra_prof::profile;
use ra_syntax::{ast, AstNode, Direction, SyntaxElement, SyntaxKind, SyntaxKind::*, TextRange, T};

use crate::{
    db::RootDatabase,
    references::{
        classify_name, classify_name_ref,
        NameKind::{self, *},
    },
    FileId,
};

#[derive(Debug)]
pub struct HighlightedRange {
    pub range: TextRange,
    pub tag: &'static str,
    pub binding_hash: Option<u64>,
}

fn is_control_keyword(kind: SyntaxKind) -> bool {
    match kind {
        T![for]
        | T![loop]
        | T![while]
        | T![continue]
        | T![break]
        | T![if]
        | T![else]
        | T![match]
        | T![return] => true,
        _ => false,
    }
}

pub(crate) fn highlight(db: &RootDatabase, file_id: FileId) -> Vec<HighlightedRange> {
    let _p = profile("highlight");
    let parse = db.parse(file_id);
    let root = parse.tree().syntax().clone();

    fn calc_binding_hash(file_id: FileId, name: &Name, shadow_count: u32) -> u64 {
        fn hash<T: std::hash::Hash + std::fmt::Debug>(x: T) -> u64 {
            use std::{collections::hash_map::DefaultHasher, hash::Hasher};

            let mut hasher = DefaultHasher::new();
            x.hash(&mut hasher);
            hasher.finish()
        }

        hash((file_id, name, shadow_count))
    }

    // Visited nodes to handle highlighting priorities
    // FIXME: retain only ranges here
    let mut highlighted: FxHashSet<SyntaxElement> = FxHashSet::default();
    let mut bindings_shadow_count: FxHashMap<Name, u32> = FxHashMap::default();

    let mut res = Vec::new();
    for node in root.descendants_with_tokens() {
        if highlighted.contains(&node) {
            continue;
        }
        let mut binding_hash = None;
        let tag = match node.kind() {
            FN_DEF => {
                bindings_shadow_count.clear();
                continue;
            }
            COMMENT => "comment",
            STRING | RAW_STRING | RAW_BYTE_STRING | BYTE_STRING => "string",
            ATTR => "attribute",
            NAME_REF => {
                if node.ancestors().any(|it| it.kind() == ATTR) {
                    continue;
                }

                let name_ref = node.as_node().cloned().and_then(ast::NameRef::cast).unwrap();
                let name_kind =
                    classify_name_ref(db, Source::new(file_id.into(), &name_ref)).map(|d| d.kind);

                if let Some(Local(local)) = &name_kind {
                    if let Some(name) = local.name(db) {
                        let shadow_count = bindings_shadow_count.entry(name.clone()).or_default();
                        binding_hash = Some(calc_binding_hash(file_id, &name, *shadow_count))
                    }
                };

                name_kind.map_or("text", |it| highlight_name(db, it))
            }
            NAME => {
                let name = node.as_node().cloned().and_then(ast::Name::cast).unwrap();
                let name_kind =
                    classify_name(db, Source::new(file_id.into(), &name)).map(|d| d.kind);

                if let Some(Local(local)) = &name_kind {
                    if let Some(name) = local.name(db) {
                        let shadow_count = bindings_shadow_count.entry(name.clone()).or_default();
                        *shadow_count += 1;
                        binding_hash = Some(calc_binding_hash(file_id, &name, *shadow_count))
                    }
                };

                match name_kind {
                    Some(name_kind) => highlight_name(db, name_kind),
                    None => name.syntax().parent().map_or("function", |x| match x.kind() {
                        TYPE_PARAM | STRUCT_DEF | ENUM_DEF | TRAIT_DEF | TYPE_ALIAS_DEF => "type",
                        RECORD_FIELD_DEF => "field",
                        _ => "function",
                    }),
                }
            }
            INT_NUMBER | FLOAT_NUMBER | CHAR | BYTE => "literal",
            LIFETIME => "parameter",
            T![unsafe] => "keyword.unsafe",
            k if is_control_keyword(k) => "keyword.control",
            k if k.is_keyword() => "keyword",
            _ => {
                if let Some(macro_call) = node.as_node().cloned().and_then(ast::MacroCall::cast) {
                    if let Some(path) = macro_call.path() {
                        if let Some(segment) = path.segment() {
                            if let Some(name_ref) = segment.name_ref() {
                                highlighted.insert(name_ref.syntax().clone().into());
                                let range_start = name_ref.syntax().text_range().start();
                                let mut range_end = name_ref.syntax().text_range().end();
                                for sibling in path.syntax().siblings_with_tokens(Direction::Next) {
                                    match sibling.kind() {
                                        T![!] | IDENT => range_end = sibling.text_range().end(),
                                        _ => (),
                                    }
                                }
                                res.push(HighlightedRange {
                                    range: TextRange::from_to(range_start, range_end),
                                    tag: "macro",
                                    binding_hash: None,
                                })
                            }
                        }
                    }
                }
                continue;
            }
        };
        res.push(HighlightedRange { range: node.text_range(), tag, binding_hash })
    }
    res
}

pub(crate) fn highlight_as_html(db: &RootDatabase, file_id: FileId, rainbow: bool) -> String {
    let parse = db.parse(file_id);

    fn rainbowify(seed: u64) -> String {
        use rand::prelude::*;
        let mut rng = SmallRng::seed_from_u64(seed);
        format!(
            "hsl({h},{s}%,{l}%)",
            h = rng.gen_range::<u16, _, _>(0, 361),
            s = rng.gen_range::<u16, _, _>(42, 99),
            l = rng.gen_range::<u16, _, _>(40, 91),
        )
    }

    let mut ranges = highlight(db, file_id);
    ranges.sort_by_key(|it| it.range.start());
    // quick non-optimal heuristic to intersect token ranges and highlighted ranges
    let mut frontier = 0;
    let mut could_intersect: Vec<&HighlightedRange> = Vec::new();

    let mut buf = String::new();
    buf.push_str(&STYLE);
    buf.push_str("<pre><code>");
    let tokens = parse.tree().syntax().descendants_with_tokens().filter_map(|it| it.into_token());
    for token in tokens {
        could_intersect.retain(|it| token.text_range().start() <= it.range.end());
        while let Some(r) = ranges.get(frontier) {
            if r.range.start() <= token.text_range().end() {
                could_intersect.push(r);
                frontier += 1;
            } else {
                break;
            }
        }
        let text = html_escape(&token.text());
        let ranges = could_intersect
            .iter()
            .filter(|it| token.text_range().is_subrange(&it.range))
            .collect::<Vec<_>>();
        if ranges.is_empty() {
            buf.push_str(&text);
        } else {
            let classes = ranges.iter().map(|x| x.tag).collect::<Vec<_>>().join(" ");
            let binding_hash = ranges.first().and_then(|x| x.binding_hash);
            let color = match (rainbow, binding_hash) {
                (true, Some(hash)) => format!(
                    " data-binding-hash=\"{}\" style=\"color: {};\"",
                    hash,
                    rainbowify(hash)
                ),
                _ => "".into(),
            };
            buf.push_str(&format!("<span class=\"{}\"{}>{}</span>", classes, color, text));
        }
    }
    buf.push_str("</code></pre>");
    buf
}

fn highlight_name(db: &RootDatabase, name_kind: NameKind) -> &'static str {
    match name_kind {
        Macro(_) => "macro",
        Field(_) => "field",
        AssocItem(hir::AssocItem::Function(_)) => "function",
        AssocItem(hir::AssocItem::Const(_)) => "constant",
        AssocItem(hir::AssocItem::TypeAlias(_)) => "type",
        Def(hir::ModuleDef::Module(_)) => "module",
        Def(hir::ModuleDef::Function(_)) => "function",
        Def(hir::ModuleDef::Adt(_)) => "type",
        Def(hir::ModuleDef::EnumVariant(_)) => "constant",
        Def(hir::ModuleDef::Const(_)) => "constant",
        Def(hir::ModuleDef::Static(_)) => "constant",
        Def(hir::ModuleDef::Trait(_)) => "type",
        Def(hir::ModuleDef::TypeAlias(_)) => "type",
        Def(hir::ModuleDef::BuiltinType(_)) => "type",
        SelfType(_) => "type",
        GenericParam(_) => "type",
        Local(local) => {
            if local.is_mut(db) {
                "variable.mut"
            } else {
                match local.ty(db).as_reference() {
                    Some((_, Mutability::Mut)) => "variable.mut",
                    _ => "variable",
                }
            }
        }
    }
}

//FIXME: like, real html escaping
fn html_escape(text: &str) -> String {
    text.replace("<", "&lt;").replace(">", "&gt;")
}

const STYLE: &str = "
<style>
body                { margin: 0; }
pre                 { color: #DCDCCC; background: #3F3F3F; font-size: 22px; padding: 0.4em; }

.comment            { color: #7F9F7F; }
.string             { color: #CC9393; }
.function           { color: #93E0E3; }
.parameter          { color: #94BFF3; }
.builtin            { color: #DD6718; }
.text               { color: #DCDCCC; }
.attribute          { color: #94BFF3; }
.literal            { color: #BFEBBF; }
.macro              { color: #94BFF3; }
.variable           { color: #DCDCCC; }
.variable\\.mut     { color: #DCDCCC; text-decoration: underline; }

.keyword            { color: #F0DFAF; }
.keyword\\.unsafe   { color: #DFAF8F; }
.keyword\\.control  { color: #F0DFAF; font-weight: bold; }
</style>
";

#[cfg(test)]
mod tests {
    use crate::mock_analysis::single_file;
    use test_utils::{assert_eq_text, project_dir, read_text};

    #[test]
    fn test_highlighting() {
        let (analysis, file_id) = single_file(
            r#"
#[derive(Clone, Debug)]
struct Foo {
    pub x: i32,
    pub y: i32,
}

fn foo<T>() -> T {
    unimplemented!();
    foo::<i32>();
}

// comment
fn main() {
    println!("Hello, {}!", 92);

    let mut vec = Vec::new();
    if true {
        vec.push(Foo { x: 0, y: 1 });
    }
    unsafe { vec.set_len(0); }

    let mut x = 42;
    let y = &mut x;
    let z = &y;

    y;
}
"#
            .trim(),
        );
        let dst_file = project_dir().join("crates/ra_ide_api/src/snapshots/highlighting.html");
        let actual_html = &analysis.highlight_as_html(file_id, false).unwrap();
        let expected_html = &read_text(&dst_file);
        std::fs::write(dst_file, &actual_html).unwrap();
        assert_eq_text!(expected_html, actual_html);
    }

    #[test]
    fn test_rainbow_highlighting() {
        let (analysis, file_id) = single_file(
            r#"
fn main() {
    let hello = "hello";
    let x = hello.to_string();
    let y = hello.to_string();

    let x = "other color please!";
    let y = x.to_string();
}

fn bar() {
    let mut hello = "hello";
}
"#
            .trim(),
        );
        let dst_file =
            project_dir().join("crates/ra_ide_api/src/snapshots/rainbow_highlighting.html");
        let actual_html = &analysis.highlight_as_html(file_id, true).unwrap();
        let expected_html = &read_text(&dst_file);
        std::fs::write(dst_file, &actual_html).unwrap();
        assert_eq_text!(expected_html, actual_html);
    }
}
