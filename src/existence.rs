use rowl::ast::*;
use crate::scope::ScopeCtx;
use crate::pattern::translate_clause;

/// Translate an existence block to FILTER EXISTS / FILTER NOT EXISTS.
pub fn translate_existence(eb: &ExistenceBlock, ctx: &mut ScopeCtx) -> String {
    let keyword = if eb.negated { "FILTER NOT EXISTS" } else { "FILTER EXISTS" };

    // Translate clauses inside the existence block
    let inner_parts: Vec<String> = eb
        .clauses
        .iter()
        .map(|c| translate_clause(c, ctx))
        .filter(|s| !s.is_empty())
        .collect();

    let inner = inner_parts.join("\n  ");

    if inner.is_empty() {
        format!("{} {{ }}", keyword)
    } else {
        format!("{} {{\n  {}\n}}", keyword, inner)
    }
}
