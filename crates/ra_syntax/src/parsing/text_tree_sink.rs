//! FIXME: write short doc here

use std::mem;

use ra_parser::{ParseError, TreeSink};

use crate::{
    parsing::Token,
    syntax_node::GreenNode,
    SmolStr, SyntaxError,
    SyntaxKind::{self, *},
    SyntaxTreeBuilder, TextRange, TextUnit,
};

/// Bridges the parser with our specific syntax tree representation.
///
/// `TextTreeSink` also handles attachment of trivia (whitespace) to nodes.
pub(crate) struct TextTreeSink<'a> {
    text: &'a str,
    tokens: &'a [Token],
    text_pos: TextUnit,
    token_pos: usize,
    state: State,
    inner: SyntaxTreeBuilder,
}

enum State {
    PendingStart,
    Normal,
    PendingFinish,
}

impl<'a> TreeSink for TextTreeSink<'a> {
    fn token(&mut self, kind: SyntaxKind, n_tokens: u8) {
        match mem::replace(&mut self.state, State::Normal) {
            State::PendingStart => unreachable!(),
            State::PendingFinish => self.inner.finish_node(),
            State::Normal => (),
        }
        self.eat_trivias();
        let n_tokens = n_tokens as usize;
        let len = self.tokens[self.token_pos..self.token_pos + n_tokens]
            .iter()
            .map(|it| it.len)
            .sum::<TextUnit>();
        self.do_token(kind, len, n_tokens);
    }

    // ZC 开始一个node，同时会附加trivia到node上
    fn start_node(&mut self, kind: SyntaxKind) {
        match mem::replace(&mut self.state, State::Normal) {
            //ZC new的时候state为PendingStart，所以一开始replace的结果就是PendingStart
            State::PendingStart => {
                self.inner.start_node(kind);
                // No need to attach trivias to previous node: there is no
                // previous node.
                return;
            }
            State::PendingFinish => self.inner.finish_node(),
            State::Normal => (),
        }

        let n_trivias =
            self.tokens[self.token_pos..].iter().take_while(|it| it.kind.is_trivia()).count();
        let leading_trivias = &self.tokens[self.token_pos..self.token_pos + n_trivias];
        let mut trivia_end =
            self.text_pos + leading_trivias.iter().map(|it| it.len).sum::<TextUnit>();

        let n_attached_trivias = {
            let leading_trivias = leading_trivias.iter().rev().map(|it| {
                let next_end = trivia_end - it.len;
                let range = TextRange::from_to(next_end, trivia_end);
                trivia_end = next_end;
                (it.kind, &self.text[range])
            });
            n_attached_trivias(kind, leading_trivias)
        };
        self.eat_n_trivias(n_trivias - n_attached_trivias);
        self.inner.start_node(kind);
        self.eat_n_trivias(n_attached_trivias);
    }

    fn finish_node(&mut self) {
        match mem::replace(&mut self.state, State::PendingFinish) {
            State::PendingStart => unreachable!(),
            State::PendingFinish => self.inner.finish_node(),
            State::Normal => (),
        }
    }

    fn error(&mut self, error: ParseError) {
        self.inner.error(error, self.text_pos)
    }
}

impl<'a> TextTreeSink<'a> {
    //ZC 记录文件内容、token（所有的）、当前位置、当前token、解析状态，以及内部语法树builder
    pub(super) fn new(text: &'a str, tokens: &'a [Token]) -> TextTreeSink<'a> {
        TextTreeSink {
            text,
            tokens,
            text_pos: 0.into(),
            token_pos: 0,
            state: State::PendingStart,
            inner: SyntaxTreeBuilder::default(),
        }
    }

    pub(super) fn finish(mut self) -> (GreenNode, Vec<SyntaxError>) {
        match mem::replace(&mut self.state, State::Normal) {
            State::PendingFinish => {
                self.eat_trivias();
                self.inner.finish_node()
            }
            State::PendingStart | State::Normal => unreachable!(),
        }

        self.inner.finish_raw()
    }

    fn eat_trivias(&mut self) {
        while let Some(&token) = self.tokens.get(self.token_pos) {
            if !token.kind.is_trivia() {
                break;
            }
            self.do_token(token.kind, token.len, 1);
        }
    }

    fn eat_n_trivias(&mut self, n: usize) {
        for _ in 0..n {
            let token = self.tokens[self.token_pos];
            assert!(token.kind.is_trivia());
            self.do_token(token.kind, token.len, 1);
        }
    }

    fn do_token(&mut self, kind: SyntaxKind, len: TextUnit, n_tokens: usize) {
        let range = TextRange::offset_len(self.text_pos, len);
        let text: SmolStr = self.text[range].into();
        self.text_pos += len;
        self.token_pos += n_tokens;
        self.inner.token(kind, text);
    }
}

//ZC 判断有哪些trivia是当前token的trivia（从\n\n开始的都是）。trivias是从当前token往前，倒序存放的trivia token。
fn n_attached_trivias<'a>(
    kind: SyntaxKind,
    trivias: impl Iterator<Item = (SyntaxKind, &'a str)>,
) -> usize {
    match kind {
        MACRO_CALL | CONST_DEF | TYPE_ALIAS_DEF | STRUCT_DEF | ENUM_DEF | ENUM_VARIANT | FN_DEF
        | TRAIT_DEF | MODULE | RECORD_FIELD_DEF | STATIC_DEF => {
            let mut res = 0;
            for (i, (kind, text)) in trivias.enumerate() {
                match kind {
                    WHITESPACE => {
                        if text.contains("\n\n") {
                            break;
                        }
                    }
                    COMMENT => {
                        res = i + 1;
                    }
                    _ => (),
                }
            }
            res
        }
        _ => 0,
    }
}
