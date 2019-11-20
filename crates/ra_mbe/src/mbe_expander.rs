//! This module takes a (parsed) definition of `macro_rules` invocation, a
//! `tt::TokenTree` representing an argument of macro invocation, and produces a
//! `tt::TokenTree` for the result of the expansion.

mod matcher;
mod transcriber;

use ra_syntax::SmolStr;
use rustc_hash::FxHashMap;

use crate::ExpandError;

pub(crate) fn expand(
    rules: &crate::MacroRules,
    input: &tt::Subtree,
) -> Result<tt::Subtree, ExpandError> {
    rules.rules.iter().find_map(|it| expand_rule(it, input).ok()).ok_or(ExpandError::NoMatchingRule)
}

fn expand_rule(rule: &crate::Rule, input: &tt::Subtree) -> Result<tt::Subtree, ExpandError> {
    let bindings = matcher::match_(&rule.lhs, input)?;
    let res = transcriber::transcribe(&rule.rhs, &bindings)?;
    Ok(res)
}

/// The actual algorithm for expansion is not too hard, but is pretty tricky.
/// `Bindings` structure is the key to understanding what we are doing here.
///
/// On the high level, it stores mapping from meta variables to the bits of
/// syntax it should be substituted with. For example, if `$e:expr` is matched
/// with `1 + 1` by macro_rules, the `Binding` will store `$e -> 1 + 1`.
///
/// The tricky bit is dealing with repetitions (`$()*`). Consider this example:
///
/// ```not_rust
/// macro_rules! foo {
///     ($($ i:ident $($ e:expr),*);*) => {
///         $(fn $ i() { $($ e);*; })*
///     }
/// }
/// foo! { foo 1,2,3; bar 4,5,6 }
/// ```
///
/// Here, the `$i` meta variable is matched first with `foo` and then with
/// `bar`, and `$e` is matched in turn with `1`, `2`, `3`, `4`, `5`, `6`.
///
/// To represent such "multi-mappings", we use a recursive structures: we map
/// variables not to values, but to *lists* of values or other lists (that is,
/// to the trees).
///
/// For the above example, the bindings would store
///
/// ```not_rust
/// i -> [foo, bar]
/// e -> [[1, 2, 3], [4, 5, 6]]
/// ```
///
/// We construct `Bindings` in the `match_lhs`. The interesting case is
/// `TokenTree::Repeat`, where we use `push_nested` to create the desired
/// nesting structure.
///
/// The other side of the puzzle is `expand_subtree`, where we use the bindings
/// to substitute meta variables in the output template. When expanding, we
/// maintain a `nesting` stack of indices which tells us which occurrence from
/// the `Bindings` we should take. We push to the stack when we enter a
/// repetition.
///
/// In other words, `Bindings` is a *multi* mapping from `SmolStr` to
/// `tt::TokenTree`, where the index to select a particular `TokenTree` among
/// many is not a plain `usize`, but an `&[usize]`.
#[derive(Debug, Default)]
struct Bindings {
    inner: FxHashMap<SmolStr, Binding>,
}

#[derive(Debug)]
enum Binding {
    Fragment(Fragment),
    Nested(Vec<Binding>),
    Empty,
}

#[derive(Debug, Clone)]
enum Fragment {
    /// token fragments are just copy-pasted into the output
    Tokens(tt::TokenTree),
    /// Ast fragments are inserted with fake delimiters, so as to make things
    /// like `$i * 2` where `$i = 1 + 1` work as expectd.
    Ast(tt::TokenTree),
}

#[cfg(test)]
mod tests {
    use ra_syntax::{ast, AstNode};

    use super::*;
    use crate::ast_to_token_tree;

    #[test]
    fn test_expand_rule() {
        assert_err(
            "($($i:ident);*) => ($i)",
            "foo!{a}",
            ExpandError::BindingError(String::from(
                "expected simple binding, found nested binding `i`",
            )),
        );

        // FIXME:
        // Add an err test case for ($($i:ident)) => ($())
    }

    fn assert_err(macro_body: &str, invocation: &str, err: ExpandError) {
        assert_eq!(expand_first(&create_rules(&format_macro(macro_body)), invocation), Err(err));
    }

    fn format_macro(macro_body: &str) -> String {
        format!(
            "
        macro_rules! foo {{
            {}
        }}
",
            macro_body
        )
    }

    fn create_rules(macro_definition: &str) -> crate::MacroRules {
        let source_file = ast::SourceFile::parse(macro_definition).ok().unwrap();
        let macro_definition =
            source_file.syntax().descendants().find_map(ast::MacroCall::cast).unwrap();

        let (definition_tt, _) =
            ast_to_token_tree(&macro_definition.token_tree().unwrap()).unwrap();
        crate::MacroRules::parse(&definition_tt).unwrap()
    }

    fn expand_first(
        rules: &crate::MacroRules,
        invocation: &str,
    ) -> Result<tt::Subtree, ExpandError> {
        let source_file = ast::SourceFile::parse(invocation).ok().unwrap();
        let macro_invocation =
            source_file.syntax().descendants().find_map(ast::MacroCall::cast).unwrap();

        let (invocation_tt, _) =
            ast_to_token_tree(&macro_invocation.token_tree().unwrap()).unwrap();

        expand_rule(&rules.rules[0], &invocation_tt)
    }
}
