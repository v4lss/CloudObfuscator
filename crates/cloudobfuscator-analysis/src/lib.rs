mod collect;
mod properties;
mod purity;

pub use collect::{
    collect_assign_target_pat_idents, collect_pat_idents, contains_unsupported_stmt, count_branches,
    is_dynamic_access, is_flattenable, is_hoistable_lexical, is_string_concat, BindingInfo,
    BindingKind, FunctionInfo, NumberSite, ProgramAnalysis, PropertyIndex, StringSite,
};
pub use properties::{plan_property_mangling, PropertyMode, PropertyPlan};
pub use purity::{
    body_stmts, expr_contains_ident, has_side_effect, is_pure_expr, is_pure_pat, is_simple_target,
    pat_contains_ident,
};
