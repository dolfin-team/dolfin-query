use rowl::ast::*;
use crate::scope::{ScopeCtx, agg_to_sparql, op_to_sparql};
use crate::pattern::translate_clauses;

/// Translate a group-by block.
/// Returns (agg_select_terms, group_by_clause, having_clause).
pub fn translate_group_by(gb: &GroupByBlock) -> (Vec<String>, String, String) {
    // Aggregation SELECT terms: (AGG(?input) AS ?result)
    let agg_terms: Vec<String> = gb
        .specs
        .iter()
        .map(|spec| {
            let func = agg_to_sparql(&spec.kind);
            format!("({func}({}) AS {})", spec.input_var, spec.result_var)
        })
        .collect();

    // GROUP BY clause
    let group_by = format!("GROUP BY {}", gb.var);

    // HAVING clause
    let having = if gb.having.is_empty() {
        String::new()
    } else {
        let conditions: Vec<String> = gb
            .having
            .iter()
            .map(|be| {
                let left = translate_bool_operand_for_having(&be.left);
                let right = translate_bool_operand_for_having(&be.right);
                let op = op_to_sparql(&be.op);
                format!("({} {} {})", left, op, right)
            })
            .collect();
        format!("HAVING ({})", conditions.join(" && "))
    };

    (agg_terms, group_by, having)
}

fn translate_bool_operand_for_having(op: &BoolOperand) -> String {
    match op {
        BoolOperand::Variable(v) => v.clone(),
        BoolOperand::Literal(lit) => crate::scope::literal_to_sparql(lit),
    }
}

/// Translate a body-level AggregationQuery into an inner sub-SELECT.
/// `average ?rating as ?directorAvg` with sub-clauses →
/// { SELECT ?groupVar (AVG(?rating) AS ?directorAvg) WHERE { ... } }
pub fn translate_agg_query(aq: &AggregationQuery, ctx: &mut ScopeCtx) -> String {
    // Translate the sub-clauses to get the WHERE body
    let where_body = translate_clauses(&aq.sub_clauses, ctx);

    let func = agg_to_sparql(&aq.kind);
    let agg_term = format!("({func}({}) AS {})", aq.input_var, aq.result_var);

    // The GROUP BY variable: find the first non-silent projected variable from sub-clauses
    // that also appears in the outer scope. As a heuristic, use the first non-silent
    // projected variable from ctx that isn't the result var.
    let group_var = ctx
        .projected
        .iter()
        .find(|v| *v != &aq.result_var && !crate::scope::is_silent(v))
        .cloned()
        .unwrap_or_else(|| ctx.gensym("g"));

    let select_clause = format!("SELECT {} {}", group_var, agg_term);

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
