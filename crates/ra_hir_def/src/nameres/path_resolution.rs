//! This modules implements a function to resolve a path `foo::bar::baz` to a
//! def, which is used within the name resolution.
//!
//! When name resolution is finished, the result of resolving a path is either
//! `Some(def)` or `None`. However, when we are in process of resolving imports
//! or macros, there's a third possibility:
//!
//!   I can't resolve this path right now, but I might be resolve this path
//!   later, when more macros are expanded.
//!
//! `ReachedFixedPoint` signals about this.

use hir_expand::name::Name;
use ra_db::Edition;
use test_utils::tested_by;

use crate::{
    db::DefDatabase2,
    nameres::{per_ns::PerNs, CrateDefMap},
    path::{Path, PathKind},
    AdtId, CrateModuleId, EnumVariantId, ModuleDefId, ModuleId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResolveMode {
    Import,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReachedFixedPoint {
    Yes,
    No,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvePathResult {
    pub(super) resolved_def: PerNs,
    pub(super) segment_index: Option<usize>,
    pub(super) reached_fixedpoint: ReachedFixedPoint,
}

impl ResolvePathResult {
    fn empty(reached_fixedpoint: ReachedFixedPoint) -> ResolvePathResult {
        ResolvePathResult::with(PerNs::none(), reached_fixedpoint, None)
    }

    fn with(
        resolved_def: PerNs,
        reached_fixedpoint: ReachedFixedPoint,
        segment_index: Option<usize>,
    ) -> ResolvePathResult {
        ResolvePathResult { resolved_def, reached_fixedpoint, segment_index }
    }
}

impl CrateDefMap {
    pub(super) fn resolve_name_in_extern_prelude(&self, name: &Name) -> PerNs {
        self.extern_prelude.get(name).map_or(PerNs::none(), |&it| PerNs::types(it))
    }

    // Returns Yes if we are sure that additions to `ItemMap` wouldn't change
    // the result.
    pub(super) fn resolve_path_fp_with_macro(
        &self,
        db: &impl DefDatabase2,
        mode: ResolveMode,
        original_module: CrateModuleId,
        path: &Path,
    ) -> ResolvePathResult {
        let mut segments = path.segments.iter().enumerate();
        let mut curr_per_ns: PerNs = match path.kind {
            PathKind::DollarCrate(krate) => {
                if krate == self.krate {
                    tested_by!(macro_dollar_crate_self);
                    PerNs::types(ModuleId { krate: self.krate, module_id: self.root }.into())
                } else {
                    let def_map = db.crate_def_map(krate);
                    let module = ModuleId { krate, module_id: def_map.root };
                    tested_by!(macro_dollar_crate_other);
                    PerNs::types(module.into())
                }
            }
            PathKind::Crate => {
                PerNs::types(ModuleId { krate: self.krate, module_id: self.root }.into())
            }
            PathKind::Self_ => {
                PerNs::types(ModuleId { krate: self.krate, module_id: original_module }.into())
            }
            // plain import or absolute path in 2015: crate-relative with
            // fallback to extern prelude (with the simplification in
            // rust-lang/rust#57745)
            // FIXME there must be a nicer way to write this condition
            PathKind::Plain | PathKind::Abs
                if self.edition == Edition::Edition2015
                    && (path.kind == PathKind::Abs || mode == ResolveMode::Import) =>
            {
                let segment = match segments.next() {
                    Some((_, segment)) => segment,
                    None => return ResolvePathResult::empty(ReachedFixedPoint::Yes),
                };
                log::debug!("resolving {:?} in crate root (+ extern prelude)", segment);
                self.resolve_name_in_crate_root_or_extern_prelude(&segment.name)
            }
            PathKind::Plain => {
                let segment = match segments.next() {
                    Some((_, segment)) => segment,
                    None => return ResolvePathResult::empty(ReachedFixedPoint::Yes),
                };
                log::debug!("resolving {:?} in module", segment);
                self.resolve_name_in_module(db, original_module, &segment.name)
            }
            PathKind::Super => {
                if let Some(p) = self.modules[original_module].parent {
                    PerNs::types(ModuleId { krate: self.krate, module_id: p }.into())
                } else {
                    log::debug!("super path in root module");
                    return ResolvePathResult::empty(ReachedFixedPoint::Yes);
                }
            }
            PathKind::Abs => {
                // 2018-style absolute path -- only extern prelude
                let segment = match segments.next() {
                    Some((_, segment)) => segment,
                    None => return ResolvePathResult::empty(ReachedFixedPoint::Yes),
                };
                if let Some(def) = self.extern_prelude.get(&segment.name) {
                    log::debug!("absolute path {:?} resolved to crate {:?}", path, def);
                    PerNs::types(*def)
                } else {
                    return ResolvePathResult::empty(ReachedFixedPoint::No); // extern crate declarations can add to the extern prelude
                }
            }
            PathKind::Type(_) => {
                // This is handled in `infer::infer_path_expr`
                // The result returned here does not matter
                return ResolvePathResult::empty(ReachedFixedPoint::Yes);
            }
        };

        for (i, segment) in segments {
            let curr = match curr_per_ns.take_types() {
                Some(r) => r,
                None => {
                    // we still have path segments left, but the path so far
                    // didn't resolve in the types namespace => no resolution
                    // (don't break here because `curr_per_ns` might contain
                    // something in the value namespace, and it would be wrong
                    // to return that)
                    return ResolvePathResult::empty(ReachedFixedPoint::No);
                }
            };
            // resolve segment in curr

            curr_per_ns = match curr {
                ModuleDefId::ModuleId(module) => {
                    if module.krate != self.krate {
                        let path =
                            Path { segments: path.segments[i..].to_vec(), kind: PathKind::Self_ };
                        log::debug!("resolving {:?} in other crate", path);
                        let defp_map = db.crate_def_map(module.krate);
                        let (def, s) = defp_map.resolve_path(db, module.module_id, &path);
                        return ResolvePathResult::with(
                            def,
                            ReachedFixedPoint::Yes,
                            s.map(|s| s + i),
                        );
                    }

                    // Since it is a qualified path here, it should not contains legacy macros
                    match self[module.module_id].scope.get(&segment.name) {
                        Some(res) => res.def,
                        _ => {
                            log::debug!("path segment {:?} not found", segment.name);
                            return ResolvePathResult::empty(ReachedFixedPoint::No);
                        }
                    }
                }
                ModuleDefId::AdtId(AdtId::EnumId(e)) => {
                    // enum variant
                    tested_by!(can_import_enum_variant);
                    let enum_data = db.enum_data(e);
                    match enum_data.variant(&segment.name) {
                        Some(local_id) => {
                            let variant = EnumVariantId { parent: e, local_id };
                            PerNs::both(variant.into(), variant.into())
                        }
                        None => {
                            return ResolvePathResult::with(
                                PerNs::types(e.into()),
                                ReachedFixedPoint::Yes,
                                Some(i),
                            );
                        }
                    }
                }
                s => {
                    // could be an inherent method call in UFCS form
                    // (`Struct::method`), or some other kind of associated item
                    log::debug!(
                        "path segment {:?} resolved to non-module {:?}, but is not last",
                        segment.name,
                        curr,
                    );

                    return ResolvePathResult::with(
                        PerNs::types(s),
                        ReachedFixedPoint::Yes,
                        Some(i),
                    );
                }
            };
        }
        ResolvePathResult::with(curr_per_ns, ReachedFixedPoint::Yes, None)
    }

    fn resolve_name_in_module(
        &self,
        db: &impl DefDatabase2,
        module: CrateModuleId,
        name: &Name,
    ) -> PerNs {
        // Resolve in:
        //  - legacy scope of macro
        //  - current module / scope
        //  - extern prelude
        //  - std prelude
        let from_legacy_macro =
            self[module].scope.get_legacy_macro(name).map_or_else(PerNs::none, PerNs::macros);
        let from_scope = self[module].scope.get(name).map_or_else(PerNs::none, |res| res.def);
        let from_extern_prelude =
            self.extern_prelude.get(name).map_or(PerNs::none(), |&it| PerNs::types(it));
        let from_prelude = self.resolve_in_prelude(db, name);

        from_legacy_macro.or(from_scope).or(from_extern_prelude).or(from_prelude)
    }

    fn resolve_name_in_crate_root_or_extern_prelude(&self, name: &Name) -> PerNs {
        let from_crate_root =
            self[self.root].scope.get(name).map_or_else(PerNs::none, |res| res.def);
        let from_extern_prelude = self.resolve_name_in_extern_prelude(name);

        from_crate_root.or(from_extern_prelude)
    }

    fn resolve_in_prelude(&self, db: &impl DefDatabase2, name: &Name) -> PerNs {
        if let Some(prelude) = self.prelude {
            let keep;
            let def_map = if prelude.krate == self.krate {
                self
            } else {
                // Extend lifetime
                keep = db.crate_def_map(prelude.krate);
                &keep
            };
            def_map[prelude.module_id].scope.get(name).map_or_else(PerNs::none, |res| res.def)
        } else {
            PerNs::none()
        }
    }
}
