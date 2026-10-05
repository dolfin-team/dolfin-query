use rowl::ast::*;

pub struct ReturnInfo {
    /// "SELECT ?x ?y" or "SELECT DISTINCT ..." or "SELECT *"
    pub select_clause: String,
    /// "ORDER BY DESC(?rating) ASC(?year)" or ""
    pub order_clause: String,
    /// "LIMIT 10" or ""
    pub limit_clause: String,
}

/// Translate the return block into SPARQL SELECT/ORDER BY/LIMIT clauses.
///
/// `projected` is the list of non-silent variables seen in the WHERE body,
/// used when there is no return block (SELECT *) or when we need to
/// auto-project.
pub fn translate_return(rb: Option<&ReturnBlock>, projected: &[String]) -> ReturnInfo {
    match rb {
        None => {
            // No return block: SELECT * (or project all non-silent vars)
            let select_clause = if projected.is_empty() {
                "SELECT *".to_string()
            } else {
                format!("SELECT {}", projected.join(" "))
            };
            ReturnInfo {
                select_clause,
                order_clause: String::new(),
                limit_clause: String::new(),
            }
        }
        Some(rb) => {
            // Check if any column has distinct
            let has_distinct = rb.columns.iter().any(|c| c.distinct);
            let distinct_kw = if has_distinct { " DISTINCT" } else { "" };

            // Build SELECT terms
            let select_terms: Vec<String> = rb
                .columns
                .iter()
                .map(|col| {
                    if let Some(alias) = &col.alias {
                        // alias ?var → (?var AS ?alias)
                        format!("({} AS ?{})", col.var, alias)
                    } else {
                        col.var.clone()
                    }
                })
                .collect();

            let select_clause = format!(
                "SELECT{} {}",
                distinct_kw,
                select_terms.join(" ")
            );

            // Build ORDER BY clause
            let order_terms: Vec<String> = rb
                .columns
                .iter()
                .filter_map(|col| {
                    col.order.map(|dir| match dir {
                        OrderDir::Asc => format!("ASC({})", col.var),
                        OrderDir::Desc => format!("DESC({})", col.var),
                    })
                })
                .collect();

            let order_clause = if order_terms.is_empty() {
                String::new()
            } else {
                format!("ORDER BY {}", order_terms.join(" "))
            };

            // Build LIMIT clause
            let limit_clause = rb
                .limit
                .map(|n| format!("LIMIT {}", n))
                .unwrap_or_default();

            ReturnInfo {
                select_clause,
                order_clause,
                limit_clause,
            }
        }
    }
}
