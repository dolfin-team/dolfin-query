pub mod error;
pub mod scope;
pub mod pattern;
pub mod existence;
pub mod group;
pub mod compose;
pub mod return_;

use rowl::ast::{QueryDef, OntologyFile, QueryClause};
use error::QueryError;
use scope::ScopeCtx;
pub use scope::{AsWritten, NodeKey, PropNamer};
use std::collections::HashSet;

/// Translate a single query to a SPARQL 1.1 SELECT string.
///
/// `all_queries` provides the full set of named queries available for
/// composition (sub-query inlining). The query being translated must be
/// in `all_queries`.
pub fn to_sparql(query: &QueryDef, all_queries: &[&QueryDef]) -> Result<String, QueryError> {
    to_sparql_with(query, all_queries, &AsWritten)
}

/// [`to_sparql`], rendering every property name through `namer`.
pub fn to_sparql_with(query: &QueryDef, all_queries: &[&QueryDef], namer: &dyn PropNamer) -> Result<String, QueryError> {
    let mut ctx = ScopeCtx::with_namer(namer);

    // Translate WHERE body clauses, handling Composition specially
    let where_body = translate_clauses_with_compose(
        &query.body.clauses,
        &mut ctx,
        all_queries,
        &mut HashSet::new(),
        &query.name,
    )?;

    // Group by / aggregation
    let (group_agg_terms, group_by_clause, having_clause) = match &query.body.group_by {
        Some(gb) => {
            let (agg_terms, gb_clause, having) = group::translate_group_by(gb);
            (agg_terms, gb_clause, having)
        }
        None => (Vec::new(), String::new(), String::new()),
    };

    // Build SELECT clause
    let ret = return_::translate_return(query.body.return_block.as_ref(), &ctx.projected);

    // If we have a group-by, merge agg_terms into the SELECT clause
    let select_clause = if !group_agg_terms.is_empty() {
        // Replace the auto-projected SELECT with group-by SELECT
        // Extract the base select part (SELECT [DISTINCT] vars...)
        let base = &ret.select_clause;
        let prefix = if base.starts_with("SELECT DISTINCT ") {
            "SELECT DISTINCT "
        } else {
            "SELECT "
        };
        let vars_part = &base[prefix.len()..];
        // Combine: SELECT ?groupVar (AGG(?x) AS ?y) ?otherReturnVars
        let agg_str = group_agg_terms.join(" ");
        if vars_part == "*" {
            format!("{}{}",
                prefix,
                if !group_by_clause.is_empty() {
                    // group var from group_by_clause
                    let gb_var = group_by_clause.trim_start_matches("GROUP BY ").trim();
                    format!("{} {}", gb_var, agg_str)
                } else {
                    agg_str
                }
            )
        } else {
            // Keep the vars from the return block, append agg terms.
            // A returned aggregate result var is already bound by its
            // `(AGG(..) AS ?var)` term; projecting it bare too is invalid SPARQL.
            let result_vars: Vec<&str> = query.body.group_by.iter()
                .flat_map(|gb| gb.specs.iter().map(|s| s.result_var.as_str()))
                .collect();
            let vars: Vec<&str> = vars_part.split_whitespace()
                .filter(|t| !result_vars.contains(t))
                .collect();
            if vars.is_empty() {
                format!("{}{}", prefix, agg_str)
            } else {
                format!("{}{} {}", prefix, vars.join(" "), agg_str)
            }
        }
    } else {
        ret.select_clause.clone()
    };

    // Assemble the full SPARQL query
    let mut parts = Vec::new();
    parts.push(select_clause);
    parts.push("WHERE {".to_string());
    if !where_body.is_empty() {
        for line in where_body.lines() {
            parts.push(format!("  {}", line));
        }
    }
    parts.push("}".to_string());

    if !group_by_clause.is_empty() {
        parts.push(group_by_clause);
    }
    if !having_clause.is_empty() {
        parts.push(having_clause);
    }
    if !ret.order_clause.is_empty() {
        parts.push(ret.order_clause);
    }
    if !ret.limit_clause.is_empty() {
        parts.push(ret.limit_clause);
    }

    Ok(parts.join("\n"))
}

fn translate_clauses_with_compose(
    clauses: &[QueryClause],
    ctx: &mut ScopeCtx,
    all_queries: &[&QueryDef],
    visiting: &mut HashSet<String>,
    current_query_name: &str,
) -> Result<String, QueryError> {
    // Mark the current query as being visited for cycle detection
    visiting.insert(current_query_name.to_string());

    let mut parts = Vec::new();
    for clause in clauses {
        let s = match clause {
            QueryClause::Composition(qc) => {
                compose::translate_composition(qc, all_queries, ctx, visiting)?
            }
            other => pattern::translate_clause(other, ctx),
        };
        if !s.is_empty() {
            parts.push(s);
        }
    }

    visiting.remove(current_query_name);
    Ok(parts.join("\n"))
}

/// Translate all queries in a file, returning one SPARQL string per query.
pub fn file_to_sparql(file: &OntologyFile) -> Vec<(String, Result<String, QueryError>)> {
    let queries: Vec<QueryDef> = file.queries();
    let refs: Vec<&QueryDef> = queries.iter().collect();
    queries
        .iter()
        .map(|q| (q.name.clone(), to_sparql(q, &refs)))
        .collect()
}
