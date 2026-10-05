use rowl::ast::*;
use std::collections::HashSet;
use crate::scope::{ScopeCtx, WithoutSubject};
use crate::error::QueryError;

/// Translate a QueryComposition into a SPARQL sub-SELECT string.
pub fn translate_composition(
    qc: &QueryComposition,
    all_queries: &[&QueryDef],
    ctx: &mut ScopeCtx,
    visiting: &mut HashSet<String>,
) -> Result<String, QueryError> {
    let query_name = qc.query_name.full();

    // Check for circular dependency
    if visiting.contains(&query_name) {
        let mut cycle: Vec<String> = visiting.iter().cloned().collect();
        cycle.sort();
        cycle.push(query_name.clone());
        return Err(QueryError::CircularDependency(cycle));
    }

    // Find the referenced query
    let target = all_queries
        .iter()
        .find(|q| q.name == query_name)
        .ok_or_else(|| QueryError::UnknownQuery(query_name.clone()))?;

    // Mark as visiting for cycle detection
    visiting.insert(query_name.clone());

    // Translate the inner query's WHERE body
    let namer = WithoutSubject(ctx.namer);
    let mut inner_ctx = ScopeCtx::with_namer(&namer);
    let where_body = translate_clauses_with_compose(&target.body.clauses, &mut inner_ctx, all_queries, visiting)?;

    visiting.remove(&query_name);

    // Build the sub-SELECT based on binding style
    let sparql = match &qc.binding {
        CompositionBinding::Named(bindings) => {
            // SELECT (?projectedVar AS ?localVar) ...
            let select_terms: Vec<String> = bindings
                .iter()
                .map(|(field, local_var)| {
                    // find the projected var from the inner query that matches the field name
                    let inner_var = inner_ctx
                        .projected
                        .iter()
                        .find(|v| {
                            // match by stripping leading ?
                            let vname = v.trim_start_matches('?');
                            vname == field.as_str()
                        })
                        .cloned()
                        .unwrap_or_else(|| format!("?{}", field));
                    format!("({} AS {})", inner_var, local_var)
                })
                .collect();
            let select_clause = format!("SELECT {}", select_terms.join(" "));
            build_sub_select(&select_clause, &where_body)
        }
        CompositionBinding::Scalar(var) => {
            // Scalar: project the first inner variable
            let inner_var = inner_ctx
                .projected
                .first()
                .cloned()
                .unwrap_or_else(|| "?_result".to_string());
            let select_clause = format!("SELECT ({} AS {})", inner_var, var);
            build_sub_select(&select_clause, &where_body)
        }
    };

    Ok(sparql)
}

fn build_sub_select(select_clause: &str, where_body: &str) -> String {
    if where_body.is_empty() {
        format!("{{\n  {}\n  WHERE {{ }}\n}}", select_clause)
    } else {
        let indented = where_body
            .lines()
            .map(|l| format!("    {}", l))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{{\n  {}\n  WHERE {{\n{}\n  }}\n}}", select_clause, indented)
    }
}

fn translate_clauses_with_compose(
    clauses: &[QueryClause],
    ctx: &mut ScopeCtx,
    all_queries: &[&QueryDef],
    visiting: &mut HashSet<String>,
) -> Result<String, QueryError> {
    let mut parts = Vec::new();
    for clause in clauses {
        let s = match clause {
            QueryClause::Composition(qc) => {
                translate_composition(qc, all_queries, ctx, visiting)?
            }
            other => crate::pattern::translate_clause(other, ctx),
        };
        if !s.is_empty() {
            parts.push(s);
        }
    }
    Ok(parts.join("\n"))
}

/// Entry point: translate composition without requiring a visiting set.
pub fn translate_composition_entry(
    qc: &QueryComposition,
    all_queries: &[&QueryDef],
    ctx: &mut ScopeCtx,
) -> Result<String, QueryError> {
    let mut visiting = HashSet::new();
    // Add the current query context (we don't know its name here, so just start fresh)
    translate_composition(qc, all_queries, ctx, &mut visiting)
}
