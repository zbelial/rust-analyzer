//! This is the actual "grammar" of the Rust language.
//!
//! Each function in this module and its children corresponds
//! to a production of the format grammar. Submodules roughly
//! correspond to different *areas* of the grammar. By convention,
//! each submodule starts with `use super::*` import and exports
//! "public" productions via `pub(super)`.
//!
//! See docs for `Parser` to learn about API, available to the grammar,
//! and see docs for `Event` to learn how this actually manages to
//! produce parse trees.
//!
//! Code in this module also contains inline tests, which start with
//! `// test name-of-the-test` comment and look like this:
//!
//! ```
//! // test function_with_zero_parameters
//! // fn foo() {}
//! ```
//!
//! After adding a new inline-test, run `cargo collect-tests` to extract
//! it as a standalone text-fixture into `tests/data/parser/inline`, and
//! run `cargo test` once to create the "gold" value.
//!
//! Coding convention: rules like `where_clause` always produce either a
//! node or an error, rules like `opt_where_clause` may produce nothing.
//! Non-opt rules typically start with `assert!(p.at(FIRST_TOKEN))`, the
//! caller is responsible for branching on the first token.
mod attributes;
mod expressions;
mod items;
mod params;
mod paths;
mod patterns;
mod type_args;
mod type_params;
mod types;

use crate::{
    parser::{CompletedMarker, Marker, Parser},
    SyntaxKind::{self, *},
    TokenSet,
};

pub(crate) fn root(p: &mut Parser) {
    let m = p.start(); // ZC start返回的marker记录了此event在event stream（一个Vec）中的位置。
    p.eat(SHEBANG);
    items::mod_contents(p, false); // ZC 将token转换为flat stream of events of the form "start expression, consume number literal, finish expression". 见parser.rs.
    m.complete(p, SOURCE_FILE); // ZC start和complete之间的所有token/node都属于同一个node
}

pub(crate) fn macro_items(p: &mut Parser) {
    let m = p.start();
    items::mod_contents(p, false);
    m.complete(p, MACRO_ITEMS);
}

pub(crate) fn macro_stmts(p: &mut Parser) {
    let m = p.start();

    while !p.at(EOF) {
        if p.current() == T![;] {
            p.bump();
            continue;
        }

        expressions::stmt(p, expressions::StmtWithSemi::Optional);
    }

    m.complete(p, MACRO_STMTS);
}

pub(crate) fn path(p: &mut Parser) {
    paths::type_path(p);
}

pub(crate) fn expr(p: &mut Parser) {
    expressions::expr(p);
}

pub(crate) fn type_(p: &mut Parser) {
    types::type_(p)
}

pub(crate) fn pattern(p: &mut Parser) {
    patterns::pattern(p)
}

pub(crate) fn stmt(p: &mut Parser, with_semi: bool) {
    let with_semi =
        if with_semi { expressions::StmtWithSemi::Yes } else { expressions::StmtWithSemi::No };

    expressions::stmt(p, with_semi)
}

pub(crate) fn block(p: &mut Parser) {
    expressions::block(p);
}

// Parse a meta item , which excluded [], e.g : #[ MetaItem ]
pub(crate) fn meta_item(p: &mut Parser) {
    fn is_delimiter(p: &mut Parser) -> bool {
        match p.current() {
            T!['{'] | T!['('] | T!['['] => true,
            _ => false,
        }
    }

    if is_delimiter(p) {
        items::token_tree(p);
        return;
    }

    let m = p.start();
    while !p.at(EOF) {
        if is_delimiter(p) {
            items::token_tree(p);
            break;
        } else {
            // https://doc.rust-lang.org/reference/attributes.html
            // https://doc.rust-lang.org/reference/paths.html#simple-paths
            // The start of an meta must be a simple path
            match p.current() {
                IDENT | T![::] | T![super] | T![self] | T![crate] => p.bump(),
                T![=] => {
                    p.bump();
                    match p.current() {
                        c if c.is_literal() => p.bump(),
                        T![true] | T![false] => p.bump(),
                        _ => {}
                    }
                    break;
                }
                _ => break,
            }
        }
    }

    m.complete(p, TOKEN_TREE);
}

pub(crate) fn item(p: &mut Parser) {
    items::item_or_macro(p, true, items::ItemFlavor::Mod)
}

pub(crate) fn reparser(
    node: SyntaxKind,
    first_child: Option<SyntaxKind>,
    parent: Option<SyntaxKind>,
) -> Option<fn(&mut Parser)> {
    let res = match node {
        BLOCK => expressions::block,
        NAMED_FIELD_DEF_LIST => items::named_field_def_list,
        NAMED_FIELD_LIST => items::named_field_list,
        ENUM_VARIANT_LIST => items::enum_variant_list,
        MATCH_ARM_LIST => items::match_arm_list,
        USE_TREE_LIST => items::use_tree_list,
        EXTERN_ITEM_LIST => items::extern_item_list,
        TOKEN_TREE if first_child? == T!['{'] => items::token_tree,
        ITEM_LIST => match parent? {
            IMPL_BLOCK => items::impl_item_list,
            TRAIT_DEF => items::trait_item_list,
            MODULE => items::mod_item_list,
            _ => return None,
        },
        _ => return None,
    };
    Some(res)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockLike {
    Block,
    NotBlock,
}

impl BlockLike {
    fn is_block(self) -> bool {
        self == BlockLike::Block
    }
}

pub(crate) fn opt_visibility(p: &mut Parser) -> bool {
    match p.current() {
        T![pub] => {
            let m = p.start();
            p.bump();
            if p.at(T!['(']) {
                match p.nth(1) {
                    // test crate_visibility
                    // pub(crate) struct S;
                    // pub(self) struct S;
                    // pub(self) struct S;
                    // pub(self) struct S;
                    T![crate] | T![self] | T![super] => {
                        p.bump();
                        p.bump();
                        p.expect(T![')']);
                    }
                    T![in] => {
                        p.bump();
                        p.bump();
                        paths::use_path(p);
                        p.expect(T![')']);
                    }
                    _ => (),
                }
            }
            m.complete(p, VISIBILITY);
        }
        // test crate_keyword_vis
        // crate fn main() { }
        // struct S { crate field: u32 }
        // struct T(crate u32);
        //
        // test crate_keyword_path
        // fn foo() { crate::foo(); }
        T![crate] if p.nth(1) != T![::] => {
            let m = p.start();
            p.bump();
            m.complete(p, VISIBILITY);
        }
        _ => return false,
    }
    true
}

fn opt_alias(p: &mut Parser) {
    if p.at(T![as]) {
        let m = p.start();
        p.bump();
        if !p.eat(T![_]) {
            name(p);
        }
        m.complete(p, ALIAS);
    }
}

fn abi(p: &mut Parser) {
    assert!(p.at(T![extern]));
    let abi = p.start();
    p.bump();
    match p.current() {
        STRING | RAW_STRING => p.bump(),
        _ => (),
    }
    abi.complete(p, ABI);
}

fn opt_fn_ret_type(p: &mut Parser) -> bool {
    if p.at(T![->]) {
        let m = p.start();
        p.bump();
        types::type_(p);
        m.complete(p, RET_TYPE);
        true
    } else {
        false
    }
}

fn name_r(p: &mut Parser, recovery: TokenSet) {
    if p.at(IDENT) {
        let m = p.start();
        p.bump();
        m.complete(p, NAME);
    } else {
        p.err_recover("expected a name", recovery);
    }
}

fn name(p: &mut Parser) {
    name_r(p, TokenSet::empty())
}

fn name_ref(p: &mut Parser) {
    if p.at(IDENT) {
        let m = p.start();
        p.bump();
        m.complete(p, NAME_REF);
    } else if p.at(T![self]) {
        let m = p.start();
        p.bump();
        m.complete(p, T![self]);
    } else {
        p.err_and_bump("expected identifier");
    }
}

fn name_ref_or_index(p: &mut Parser) {
    if p.at(IDENT) || p.at(INT_NUMBER) {
        let m = p.start();
        p.bump();
        m.complete(p, NAME_REF);
    } else {
        p.err_and_bump("expected identifier");
    }
}

fn error_block(p: &mut Parser, message: &str) {
    assert!(p.at(T!['{']));
    let m = p.start();
    p.error(message);
    p.bump();
    expressions::expr_block_contents(p);
    p.eat(T!['}']);
    m.complete(p, ERROR);
}
