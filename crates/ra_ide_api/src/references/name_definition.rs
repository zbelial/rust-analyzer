//! `NameDefinition` keeps information about the element we want to search references for.
//! The element is represented by `NameKind`. It's located inside some `container` and
//! has a `visibility`, which defines a search scope.
//! Note that the reference search is possible for not all of the classified items.

use hir::{
    Adt, AssocItem, GenericParam, HasSource, Local, MacroDef, Module, ModuleDef, StructField, Ty,
    VariantDef,
};
use ra_syntax::{ast, ast::VisibilityOwner};

use crate::db::RootDatabase;

#[derive(Debug, PartialEq, Eq)]
pub enum NameKind {
    Macro(MacroDef),
    Field(StructField),
    AssocItem(AssocItem),
    Def(ModuleDef),
    SelfType(Ty),
    Local(Local),
    GenericParam(GenericParam),
}

#[derive(PartialEq, Eq)]
pub(crate) struct NameDefinition {
    pub visibility: Option<ast::Visibility>,
    pub container: Module,
    pub kind: NameKind,
}

pub(super) fn from_assoc_item(db: &RootDatabase, item: AssocItem) -> NameDefinition {
    let container = item.module(db);
    let visibility = match item {
        AssocItem::Function(f) => f.source(db).ast.visibility(),
        AssocItem::Const(c) => c.source(db).ast.visibility(),
        AssocItem::TypeAlias(a) => a.source(db).ast.visibility(),
    };
    let kind = NameKind::AssocItem(item);
    NameDefinition { kind, container, visibility }
}

pub(super) fn from_struct_field(db: &RootDatabase, field: StructField) -> NameDefinition {
    let kind = NameKind::Field(field);
    let parent = field.parent_def(db);
    let container = parent.module(db);
    let visibility = match parent {
        VariantDef::Struct(s) => s.source(db).ast.visibility(),
        VariantDef::EnumVariant(e) => e.source(db).ast.parent_enum().visibility(),
    };
    NameDefinition { kind, container, visibility }
}

pub(super) fn from_module_def(
    db: &RootDatabase,
    def: ModuleDef,
    module: Option<Module>,
) -> NameDefinition {
    let kind = NameKind::Def(def);
    let (container, visibility) = match def {
        ModuleDef::Module(it) => {
            let container = it.parent(db).or_else(|| Some(it)).unwrap();
            let visibility = it.declaration_source(db).and_then(|s| s.ast.visibility());
            (container, visibility)
        }
        ModuleDef::EnumVariant(it) => {
            let container = it.module(db);
            let visibility = it.source(db).ast.parent_enum().visibility();
            (container, visibility)
        }
        ModuleDef::Function(it) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::Const(it) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::Static(it) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::Trait(it) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::TypeAlias(it) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::Adt(Adt::Struct(it)) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::Adt(Adt::Union(it)) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::Adt(Adt::Enum(it)) => (it.module(db), it.source(db).ast.visibility()),
        ModuleDef::BuiltinType(..) => (module.unwrap(), None),
    };
    NameDefinition { kind, container, visibility }
}
