//! This module resolves `mod foo;` declaration to file.
use hir_expand::name::Name;
use ra_db::{FileId, RelativePathBuf};
use ra_syntax::SmolStr;

use crate::{db::DefDatabase2, HirFileId};

#[derive(Clone, Debug)]
pub(super) struct ModDir {
    /// `.` for `mod.rs`, `lib.rs`
    /// `./foo` for `foo.rs`
    /// `./foo/bar` for `mod bar { mod x; }` nested in `foo.rs`
    path: RelativePathBuf,
    /// inside `./foo.rs`, mods with `#[path]` should *not* be relative to `./foo/`
    root_non_dir_owner: bool,
}

impl ModDir {
    pub(super) fn root() -> ModDir {
        ModDir { path: RelativePathBuf::default(), root_non_dir_owner: false }
    }

    pub(super) fn descend_into_definition(
        &self,
        name: &Name,
        attr_path: Option<&SmolStr>,
    ) -> ModDir {
        let mut path = self.path.clone();
        match attr_to_path(attr_path) {
            None => path.push(&name.to_string()),
            Some(attr_path) => {
                if self.root_non_dir_owner {
                    assert!(path.pop());
                }
                path.push(attr_path);
            }
        }
        ModDir { path, root_non_dir_owner: false }
    }

    pub(super) fn resolve_declaration(
        &self,
        db: &impl DefDatabase2,
        file_id: HirFileId,
        name: &Name,
        attr_path: Option<&SmolStr>,
    ) -> Result<(FileId, ModDir), RelativePathBuf> {
        let file_id = file_id.original_file(db);

        let mut candidate_files = Vec::new();
        match attr_to_path(attr_path) {
            Some(attr_path) => {
                let base =
                    if self.root_non_dir_owner { self.path.parent().unwrap() } else { &self.path };
                candidate_files.push(base.join(attr_path))
            }
            None => {
                candidate_files.push(self.path.join(&format!("{}.rs", name)));
                candidate_files.push(self.path.join(&format!("{}/mod.rs", name)));
            }
        };

        for candidate in candidate_files.iter() {
            if let Some(file_id) = db.resolve_relative_path(file_id, candidate) {
                let mut root_non_dir_owner = false;
                let mut mod_path = RelativePathBuf::new();
                if !(candidate.ends_with("mod.rs") || attr_path.is_some()) {
                    root_non_dir_owner = true;
                    mod_path.push(&name.to_string());
                }
                return Ok((file_id, ModDir { path: mod_path, root_non_dir_owner }));
            }
        }
        Err(candidate_files.remove(0))
    }
}

fn attr_to_path(attr: Option<&SmolStr>) -> Option<RelativePathBuf> {
    attr.and_then(|it| RelativePathBuf::from_path(&it.replace("\\", "/")).ok())
}
