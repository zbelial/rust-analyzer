//! Trait solving using Chalk.
use std::sync::{Arc, Mutex};

use chalk_ir::{cast::Cast, family::ChalkIr};
use log::debug;
use ra_db::salsa;
use ra_prof::profile;
use rustc_hash::FxHashSet;

use super::{Canonical, GenericPredicate, HirDisplay, ProjectionTy, TraitRef, Ty, TypeWalk};
use crate::{db::HirDatabase, expr::ExprId, Crate, DefWithBody, ImplBlock, Trait, TypeAlias};

use self::chalk::{from_chalk, ToChalk};

pub(crate) mod chalk;

#[derive(Debug, Clone)]
pub struct TraitSolver {
    krate: Crate,
    inner: Arc<Mutex<chalk_solve::Solver<ChalkIr>>>,
}

/// We need eq for salsa
impl PartialEq for TraitSolver {
    fn eq(&self, other: &TraitSolver) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for TraitSolver {}

impl TraitSolver {
    fn solve(
        &self,
        db: &impl HirDatabase,
        goal: &chalk_ir::UCanonical<chalk_ir::InEnvironment<chalk_ir::Goal<ChalkIr>>>,
    ) -> Option<chalk_solve::Solution<ChalkIr>> {
        let context = ChalkContext { db, krate: self.krate };
        debug!("solve goal: {:?}", goal);
        let mut solver = match self.inner.lock() {
            Ok(it) => it,
            // Our cancellation works via unwinding, but, as chalk is not
            // panic-safe, we need to make sure to propagate the cancellation.
            // Ideally, we should also make chalk panic-safe.
            Err(_) => ra_db::Canceled::throw(),
        };
        let solution = solver.solve(&context, goal);
        debug!("solve({:?}) => {:?}", goal, solution);
        solution
    }
}

/// This controls the maximum size of types Chalk considers. If we set this too
/// high, we can run into slow edge cases; if we set it too low, Chalk won't
/// find some solutions.
const CHALK_SOLVER_MAX_SIZE: usize = 4;

#[derive(Debug, Copy, Clone)]
struct ChalkContext<'a, DB> {
    db: &'a DB,
    krate: Crate,
}

pub(crate) fn trait_solver_query(
    db: &(impl HirDatabase + salsa::Database),
    krate: Crate,
) -> TraitSolver {
    db.salsa_runtime().report_untracked_read();
    // krate parameter is just so we cache a unique solver per crate
    let solver_choice = chalk_solve::SolverChoice::SLG { max_size: CHALK_SOLVER_MAX_SIZE };
    debug!("Creating new solver for crate {:?}", krate);
    TraitSolver { krate, inner: Arc::new(Mutex::new(solver_choice.into_solver())) }
}

/// Collects impls for the given trait in the whole dependency tree of `krate`.
pub(crate) fn impls_for_trait_query(
    db: &impl HirDatabase,
    krate: Crate,
    trait_: Trait,
) -> Arc<[ImplBlock]> {
    let mut impls = FxHashSet::default();
    // We call the query recursively here. On the one hand, this means we can
    // reuse results from queries for different crates; on the other hand, this
    // will only ever get called for a few crates near the root of the tree (the
    // ones the user is editing), so this may actually be a waste of memory. I'm
    // doing it like this mainly for simplicity for now.
    for dep in krate.dependencies(db) {
        impls.extend(db.impls_for_trait(dep.krate, trait_).iter());
    }
    let crate_impl_blocks = db.impls_in_crate(krate);
    impls.extend(crate_impl_blocks.lookup_impl_blocks_for_trait(trait_));
    impls.into_iter().collect()
}

/// A set of clauses that we assume to be true. E.g. if we are inside this function:
/// ```rust
/// fn foo<T: Default>(t: T) {}
/// ```
/// we assume that `T: Default`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TraitEnvironment {
    pub predicates: Vec<GenericPredicate>,
}

impl TraitEnvironment {
    /// Returns trait refs with the given self type which are supposed to hold
    /// in this trait env. E.g. if we are in `foo<T: SomeTrait>()`, this will
    /// find that `T: SomeTrait` if we call it for `T`.
    pub(crate) fn trait_predicates_for_self_ty<'a>(
        &'a self,
        ty: &'a Ty,
    ) -> impl Iterator<Item = &'a TraitRef> + 'a {
        self.predicates.iter().filter_map(move |pred| match pred {
            GenericPredicate::Implemented(tr) if tr.self_ty() == ty => Some(tr),
            _ => None,
        })
    }
}

/// Something (usually a goal), along with an environment.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InEnvironment<T> {
    pub environment: Arc<TraitEnvironment>,
    pub value: T,
}

impl<T> InEnvironment<T> {
    pub fn new(environment: Arc<TraitEnvironment>, value: T) -> InEnvironment<T> {
        InEnvironment { environment, value }
    }
}

/// Something that needs to be proven (by Chalk) during type checking, e.g. that
/// a certain type implements a certain trait. Proving the Obligation might
/// result in additional information about inference variables.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Obligation {
    /// Prove that a certain type implements a trait (the type is the `Self` type
    /// parameter to the `TraitRef`).
    Trait(TraitRef),
    Projection(ProjectionPredicate),
}

impl Obligation {
    pub fn from_predicate(predicate: GenericPredicate) -> Option<Obligation> {
        match predicate {
            GenericPredicate::Implemented(trait_ref) => Some(Obligation::Trait(trait_ref)),
            GenericPredicate::Projection(projection_pred) => {
                Some(Obligation::Projection(projection_pred))
            }
            GenericPredicate::Error => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectionPredicate {
    pub projection_ty: ProjectionTy,
    pub ty: Ty,
}

impl TypeWalk for ProjectionPredicate {
    fn walk(&self, f: &mut impl FnMut(&Ty)) {
        self.projection_ty.walk(f);
        self.ty.walk(f);
    }

    fn walk_mut_binders(&mut self, f: &mut impl FnMut(&mut Ty, usize), binders: usize) {
        self.projection_ty.walk_mut_binders(f, binders);
        self.ty.walk_mut_binders(f, binders);
    }
}

/// Solve a trait goal using Chalk.
pub(crate) fn trait_solve_query(
    db: &impl HirDatabase,
    krate: Crate,
    goal: Canonical<InEnvironment<Obligation>>,
) -> Option<Solution> {
    let _p = profile("trait_solve_query");
    debug!("trait_solve_query({})", goal.value.value.display(db));

    if let Obligation::Projection(pred) = &goal.value.value {
        if let Ty::Bound(_) = &pred.projection_ty.parameters[0] {
            // Hack: don't ask Chalk to normalize with an unknown self type, it'll say that's impossible
            return Some(Solution::Ambig(Guidance::Unknown));
        }
    }

    let canonical = goal.to_chalk(db).cast();

    // We currently don't deal with universes (I think / hope they're not yet
    // relevant for our use cases?)
    let u_canonical = chalk_ir::UCanonical { canonical, universes: 1 };
    let solution = db.trait_solver(krate).solve(db, &u_canonical);
    solution.map(|solution| solution_from_chalk(db, solution))
}

fn solution_from_chalk(
    db: &impl HirDatabase,
    solution: chalk_solve::Solution<ChalkIr>,
) -> Solution {
    let convert_subst = |subst: chalk_ir::Canonical<chalk_ir::Substitution<ChalkIr>>| {
        let value = subst
            .value
            .parameters
            .into_iter()
            .map(|p| {
                let ty = match p {
                    chalk_ir::Parameter(chalk_ir::ParameterKind::Ty(ty)) => from_chalk(db, ty),
                    chalk_ir::Parameter(chalk_ir::ParameterKind::Lifetime(_)) => unimplemented!(),
                };
                ty
            })
            .collect();
        let result = Canonical { value, num_vars: subst.binders.len() };
        SolutionVariables(result)
    };
    match solution {
        chalk_solve::Solution::Unique(constr_subst) => {
            let subst = chalk_ir::Canonical {
                value: constr_subst.value.subst,
                binders: constr_subst.binders,
            };
            Solution::Unique(convert_subst(subst))
        }
        chalk_solve::Solution::Ambig(chalk_solve::Guidance::Definite(subst)) => {
            Solution::Ambig(Guidance::Definite(convert_subst(subst)))
        }
        chalk_solve::Solution::Ambig(chalk_solve::Guidance::Suggested(subst)) => {
            Solution::Ambig(Guidance::Suggested(convert_subst(subst)))
        }
        chalk_solve::Solution::Ambig(chalk_solve::Guidance::Unknown) => {
            Solution::Ambig(Guidance::Unknown)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolutionVariables(pub Canonical<Vec<Ty>>);

#[derive(Clone, Debug, PartialEq, Eq)]
/// A (possible) solution for a proposed goal.
pub enum Solution {
    /// The goal indeed holds, and there is a unique value for all existential
    /// variables.
    Unique(SolutionVariables),

    /// The goal may be provable in multiple ways, but regardless we may have some guidance
    /// for type inference. In this case, we don't return any lifetime
    /// constraints, since we have not "committed" to any particular solution
    /// yet.
    Ambig(Guidance),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// When a goal holds ambiguously (e.g., because there are multiple possible
/// solutions), we issue a set of *guidance* back to type inference.
pub enum Guidance {
    /// The existential variables *must* have the given values if the goal is
    /// ever to hold, but that alone isn't enough to guarantee the goal will
    /// actually hold.
    Definite(SolutionVariables),

    /// There are multiple plausible values for the existentials, but the ones
    /// here are suggested as the preferred choice heuristically. These should
    /// be used for inference fallback only.
    Suggested(SolutionVariables),

    /// There's no useful information to feed back to type inference
    Unknown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum FnTrait {
    FnOnce,
    FnMut,
    Fn,
}

impl FnTrait {
    fn lang_item_name(self) -> &'static str {
        match self {
            FnTrait::FnOnce => "fn_once",
            FnTrait::FnMut => "fn_mut",
            FnTrait::Fn => "fn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClosureFnTraitImplData {
    def: DefWithBody,
    expr: ExprId,
    fn_trait: FnTrait,
}

/// An impl. Usually this comes from an impl block, but some built-in types get
/// synthetic impls.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Impl {
    /// A normal impl from an impl block.
    ImplBlock(ImplBlock),
    /// Closure types implement the Fn traits synthetically.
    ClosureFnTraitImpl(ClosureFnTraitImplData),
}

/// An associated type value. Usually this comes from a `type` declaration
/// inside an impl block, but for built-in impls we have to synthesize it.
/// (We only need this because Chalk wants a unique ID for each of these.)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AssocTyValue {
    /// A normal assoc type value from an impl block.
    TypeAlias(TypeAlias),
    /// The output type of the Fn trait implementation.
    ClosureFnTraitImplOutput(ClosureFnTraitImplData),
}
