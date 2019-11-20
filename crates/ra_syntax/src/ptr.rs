//! FIXME: write short doc here

use std::{iter::successors, marker::PhantomData};

use crate::{AstNode, SyntaxKind, SyntaxNode, TextRange};

/// A pointer to a syntax node inside a file. It can be used to remember a
/// specific node across reparses of the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxNodePtr {
    pub(crate) range: TextRange,
    kind: SyntaxKind,
}

impl SyntaxNodePtr {
    pub fn new(node: &SyntaxNode) -> SyntaxNodePtr {
        SyntaxNodePtr { range: node.text_range(), kind: node.kind() }
    }

    pub fn to_node(self, root: &SyntaxNode) -> SyntaxNode {
        assert!(root.parent().is_none());
        successors(Some(root.clone()), |node| {
            node.children().find(|it| self.range.is_subrange(&it.text_range()))
        })
        .find(|it| it.text_range() == self.range && it.kind() == self.kind)
        .unwrap_or_else(|| panic!("can't resolve local ptr to SyntaxNode: {:?}", self))
    }

    pub fn range(self) -> TextRange {
        self.range
    }

    pub fn kind(self) -> SyntaxKind {
        self.kind
    }

    pub fn cast<N: AstNode>(self) -> Option<AstPtr<N>> {
        if !N::can_cast(self.kind()) {
            return None;
        }
        Some(AstPtr { raw: self, _ty: PhantomData })
    }
}

/// Like `SyntaxNodePtr`, but remembers the type of node
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct AstPtr<N: AstNode> {
    raw: SyntaxNodePtr,
    _ty: PhantomData<fn() -> N>,
}

impl<N: AstNode> Copy for AstPtr<N> {}
impl<N: AstNode> Clone for AstPtr<N> {
    fn clone(&self) -> AstPtr<N> {
        *self
    }
}

impl<N: AstNode> AstPtr<N> {
    pub fn new(node: &N) -> AstPtr<N> {
        AstPtr { raw: SyntaxNodePtr::new(node.syntax()), _ty: PhantomData }
    }

    pub fn to_node(self, root: &SyntaxNode) -> N {
        let syntax_node = self.raw.to_node(root);
        N::cast(syntax_node).unwrap()
    }

    pub fn syntax_node_ptr(self) -> SyntaxNodePtr {
        self.raw
    }

    pub fn cast<U: AstNode>(self) -> Option<AstPtr<U>> {
        if !U::can_cast(self.raw.kind()) {
            return None;
        }
        Some(AstPtr { raw: self.raw, _ty: PhantomData })
    }
}

impl<N: AstNode> From<AstPtr<N>> for SyntaxNodePtr {
    fn from(ptr: AstPtr<N>) -> SyntaxNodePtr {
        ptr.raw
    }
}

#[test]
fn test_local_syntax_ptr() {
    use crate::{ast, AstNode, SourceFile};

    let file = SourceFile::parse("struct Foo { f: u32, }").ok().unwrap();
    let field = file.syntax().descendants().find_map(ast::RecordFieldDef::cast).unwrap();
    let ptr = SyntaxNodePtr::new(field.syntax());
    let field_syntax = ptr.to_node(file.syntax());
    assert_eq!(field.syntax(), &field_syntax);
}
