//! FIXME: write short doc here

use hir::ModuleSource;
use ra_db::{RelativePath, RelativePathBuf, SourceDatabase, SourceDatabaseExt};
use ra_syntax::{algo::find_node_at_offset, ast, AstNode, SyntaxNode};
use ra_text_edit::TextEdit;

use crate::{
    db::RootDatabase, FileId, FilePosition, FileSystemEdit, RangeInfo, SourceChange,
    SourceFileEdit, TextRange,
};

use super::find_all_refs;

pub(crate) fn rename(
    db: &RootDatabase,
    position: FilePosition,
    new_name: &str,
) -> Option<RangeInfo<SourceChange>> {
    let parse = db.parse(position.file_id);
    if let Some((ast_name, ast_module)) =
        find_name_and_module_at_offset(parse.tree().syntax(), position)
    {
        let range = ast_name.syntax().text_range();
        rename_mod(db, &ast_name, &ast_module, position, new_name)
            .map(|info| RangeInfo::new(range, info))
    } else {
        rename_reference(db, position, new_name)
    }
}

fn find_name_and_module_at_offset(
    syntax: &SyntaxNode,
    position: FilePosition,
) -> Option<(ast::Name, ast::Module)> {
    let ast_name = find_node_at_offset::<ast::Name>(syntax, position.offset)?;
    let ast_module = ast::Module::cast(ast_name.syntax().parent()?)?;
    Some((ast_name, ast_module))
}

fn source_edit_from_file_id_range(
    file_id: FileId,
    range: TextRange,
    new_name: &str,
) -> SourceFileEdit {
    SourceFileEdit { file_id, edit: TextEdit::replace(range, new_name.into()) }
}

fn rename_mod(
    db: &RootDatabase,
    ast_name: &ast::Name,
    ast_module: &ast::Module,
    position: FilePosition,
    new_name: &str,
) -> Option<SourceChange> {
    let mut source_file_edits = Vec::new();
    let mut file_system_edits = Vec::new();
    let module_src = hir::Source { file_id: position.file_id.into(), ast: ast_module.clone() };
    if let Some(module) = hir::Module::from_declaration(db, module_src) {
        let src = module.definition_source(db);
        let file_id = src.file_id.original_file(db);
        match src.ast {
            ModuleSource::SourceFile(..) => {
                let mod_path: RelativePathBuf = db.file_relative_path(file_id);
                // mod is defined in path/to/dir/mod.rs
                let dst_path = if mod_path.file_stem() == Some("mod") {
                    mod_path
                        .parent()
                        .and_then(|p| p.parent())
                        .or_else(|| Some(RelativePath::new("")))
                        .map(|p| p.join(new_name).join("mod.rs"))
                } else {
                    Some(mod_path.with_file_name(new_name).with_extension("rs"))
                };
                if let Some(path) = dst_path {
                    let move_file = FileSystemEdit::MoveFile {
                        src: file_id,
                        dst_source_root: db.file_source_root(position.file_id),
                        dst_path: path,
                    };
                    file_system_edits.push(move_file);
                }
            }
            ModuleSource::Module(..) => {}
        }
    }

    let edit = SourceFileEdit {
        file_id: position.file_id,
        edit: TextEdit::replace(ast_name.syntax().text_range(), new_name.into()),
    };
    source_file_edits.push(edit);

    Some(SourceChange::from_edits("rename", source_file_edits, file_system_edits))
}

fn rename_reference(
    db: &RootDatabase,
    position: FilePosition,
    new_name: &str,
) -> Option<RangeInfo<SourceChange>> {
    let RangeInfo { range, info: refs } = find_all_refs(db, position, None)?;

    let edit = refs
        .into_iter()
        .map(|range| source_edit_from_file_id_range(range.file_id, range.range, new_name))
        .collect::<Vec<_>>();

    if edit.is_empty() {
        return None;
    }

    Some(RangeInfo::new(range, SourceChange::source_file_edits("rename", edit)))
}

#[cfg(test)]
mod tests {
    use insta::assert_debug_snapshot;
    use ra_text_edit::TextEditBuilder;
    use test_utils::assert_eq_text;

    use crate::{
        mock_analysis::analysis_and_position, mock_analysis::single_file_with_position, FileId,
        ReferenceSearchResult,
    };

    #[test]
    fn test_find_all_refs_for_local() {
        let code = r#"
    fn main() {
        let mut i = 1;
        let j = 1;
        i = i<|> + j;

        {
            i = 0;
        }

        i = 5;
    }"#;

        let refs = get_all_refs(code);
        assert_eq!(refs.len(), 5);
    }

    #[test]
    fn test_find_all_refs_for_param_inside() {
        let code = r#"
    fn foo(i : u32) -> u32 {
        i<|>
    }"#;

        let refs = get_all_refs(code);
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_find_all_refs_for_fn_param() {
        let code = r#"
    fn foo(i<|> : u32) -> u32 {
        i
    }"#;

        let refs = get_all_refs(code);
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_find_all_refs_field_name() {
        let code = r#"
            //- /lib.rs
            struct Foo {
                pub spam<|>: u32,
            }

            fn main(s: Foo) {
                let f = s.spam;
            }
        "#;

        let refs = get_all_refs(code);
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_find_all_refs_impl_item_name() {
        let code = r#"
            //- /lib.rs
            struct Foo;
            impl Foo {
                fn f<|>(&self) {  }
            }
        "#;

        let refs = get_all_refs(code);
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_find_all_refs_enum_var_name() {
        let code = r#"
            //- /lib.rs
            enum Foo {
                A,
                B<|>,
                C,
            }
        "#;

        let refs = get_all_refs(code);
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_find_all_refs_modules() {
        let code = r#"
            //- /lib.rs
            pub mod foo;
            pub mod bar;

            fn f() {
                let i = foo::Foo { n: 5 };
            }

            //- /foo.rs
            use crate::bar;

            pub struct Foo {
                pub n: u32,
            }

            fn f() {
                let i = bar::Bar { n: 5 };
            }

            //- /bar.rs
            use crate::foo;

            pub struct Bar {
                pub n: u32,
            }

            fn f() {
                let i = foo::Foo<|> { n: 5 };
            }
        "#;

        let (analysis, pos) = analysis_and_position(code);
        let refs = analysis.find_all_refs(pos, None).unwrap().unwrap();
        assert_eq!(refs.len(), 3);
    }

    fn get_all_refs(text: &str) -> ReferenceSearchResult {
        let (analysis, position) = single_file_with_position(text);
        analysis.find_all_refs(position, None).unwrap().unwrap()
    }

    #[test]
    fn test_rename_for_local() {
        test_rename(
            r#"
    fn main() {
        let mut i = 1;
        let j = 1;
        i = i<|> + j;

        {
            i = 0;
        }

        i = 5;
    }"#,
            "k",
            r#"
    fn main() {
        let mut k = 1;
        let j = 1;
        k = k + j;

        {
            k = 0;
        }

        k = 5;
    }"#,
        );
    }

    #[test]
    fn test_rename_for_param_inside() {
        test_rename(
            r#"
    fn foo(i : u32) -> u32 {
        i<|>
    }"#,
            "j",
            r#"
    fn foo(j : u32) -> u32 {
        j
    }"#,
        );
    }

    #[test]
    fn test_rename_refs_for_fn_param() {
        test_rename(
            r#"
    fn foo(i<|> : u32) -> u32 {
        i
    }"#,
            "new_name",
            r#"
    fn foo(new_name : u32) -> u32 {
        new_name
    }"#,
        );
    }

    #[test]
    fn test_rename_for_mut_param() {
        test_rename(
            r#"
    fn foo(mut i<|> : u32) -> u32 {
        i
    }"#,
            "new_name",
            r#"
    fn foo(mut new_name : u32) -> u32 {
        new_name
    }"#,
        );
    }

    #[test]
    fn test_rename_mod() {
        let (analysis, position) = analysis_and_position(
            "
            //- /lib.rs
            mod bar;

            //- /bar.rs
            mod foo<|>;

            //- /bar/foo.rs
            // emtpy
            ",
        );
        let new_name = "foo2";
        let source_change = analysis.rename(position, new_name).unwrap();
        assert_debug_snapshot!(&source_change,
@r###"
        Some(
            RangeInfo {
                range: [4; 7),
                info: SourceChange {
                    label: "rename",
                    source_file_edits: [
                        SourceFileEdit {
                            file_id: FileId(
                                2,
                            ),
                            edit: TextEdit {
                                atoms: [
                                    AtomTextEdit {
                                        delete: [4; 7),
                                        insert: "foo2",
                                    },
                                ],
                            },
                        },
                    ],
                    file_system_edits: [
                        MoveFile {
                            src: FileId(
                                3,
                            ),
                            dst_source_root: SourceRootId(
                                0,
                            ),
                            dst_path: "bar/foo2.rs",
                        },
                    ],
                    cursor_position: None,
                },
            },
        )
        "###);
    }

    #[test]
    fn test_rename_mod_in_dir() {
        let (analysis, position) = analysis_and_position(
            "
            //- /lib.rs
            mod fo<|>o;
            //- /foo/mod.rs
            // emtpy
            ",
        );
        let new_name = "foo2";
        let source_change = analysis.rename(position, new_name).unwrap();
        assert_debug_snapshot!(&source_change,
        @r###"
        Some(
            RangeInfo {
                range: [4; 7),
                info: SourceChange {
                    label: "rename",
                    source_file_edits: [
                        SourceFileEdit {
                            file_id: FileId(
                                1,
                            ),
                            edit: TextEdit {
                                atoms: [
                                    AtomTextEdit {
                                        delete: [4; 7),
                                        insert: "foo2",
                                    },
                                ],
                            },
                        },
                    ],
                    file_system_edits: [
                        MoveFile {
                            src: FileId(
                                2,
                            ),
                            dst_source_root: SourceRootId(
                                0,
                            ),
                            dst_path: "foo2/mod.rs",
                        },
                    ],
                    cursor_position: None,
                },
            },
        )
        "###
               );
    }

    fn test_rename(text: &str, new_name: &str, expected: &str) {
        let (analysis, position) = single_file_with_position(text);
        let source_change = analysis.rename(position, new_name).unwrap();
        let mut text_edit_builder = TextEditBuilder::default();
        let mut file_id: Option<FileId> = None;
        if let Some(change) = source_change {
            for edit in change.info.source_file_edits {
                file_id = Some(edit.file_id);
                for atom in edit.edit.as_atoms() {
                    text_edit_builder.replace(atom.delete, atom.insert.clone());
                }
            }
        }
        let result =
            text_edit_builder.finish().apply(&*analysis.file_text(file_id.unwrap()).unwrap());
        assert_eq_text!(expected, &*result);
    }
}
