use rowl::ast::*;
use crate::scope::{NodeKey, ScopeCtx, type_assertion_lines, expr_to_sparql, op_to_sparql};

/// Base IRI for dolfin quantity terms (`dq:coefficient`, `dq:dimension`).
const DQ_NS: &str = "https://dolfin.dev/quantity#";

/// If `value` is a resolvable physical-quantity literal, return its
/// `(SI magnitude, canonical dimension string)` so a comparison against it can
/// be "unfolded" into a unit-aware SPARQL FILTER.
fn quantity_target(value: &Expr) -> Option<(f64, String)> {
    if let Expr::Literal { value: lit, .. } = value
        && matches!(lit, Literal::Quantity { .. })
            && let Some(Ok(q)) = lit.resolve_quantity() {
                return Some((q.si_value(), q.dimensions.canonical_string()));
            }
    None
}

/// Emit the unit-aware unfold for `?var <op> quantity(...)`: resolve the stored
/// value's unit via its datatype (joined to the `dq:` definition block emitted
/// alongside the data), guard on matching dimension, and compare SI magnitudes
/// (`value × coefficient`). Standard SPARQL 1.1; `dq:` predicates as full IRIs
/// so no PREFIX prologue is required.
fn quantity_unfold_lines(
    var: &str,
    op: &ComparisonOp,
    target_si: f64,
    target_dim: &str,
    ctx: &mut ScopeCtx,
) -> Vec<String> {
    let u = ctx.gensym("u");
    let c = ctx.gensym("c");
    let d = ctx.gensym("d");
    let op_s = op_to_sparql(op);
    vec![
        format!("BIND(datatype({}) AS {})", var, u),
        format!("{} <{}coefficient> {} .", u, DQ_NS, c),
        format!("{} <{}dimension> {} .", u, DQ_NS, d),
        format!(
            "FILTER({} = \"{}\" && (xsd:double(str({})) * {}) {} {})",
            d, target_dim, var, c, op_s, target_si
        ),
    ]
}

/// Translate all query body clauses to WHERE-body SPARQL lines.
pub fn translate_clauses(clauses: &[QueryClause], ctx: &mut ScopeCtx) -> String {
    let mut parts = Vec::new();
    for clause in clauses {
        let s = translate_clause(clause, ctx);
        if !s.is_empty() {
            parts.push(s);
        }
    }
    parts.join("\n")
}

pub fn translate_clause(clause: &QueryClause, ctx: &mut ScopeCtx) -> String {
    match clause {
        QueryClause::SubjectPattern(sp) => translate_subject_pattern(sp, ctx),
        QueryClause::ExistenceBlock(eb) => crate::existence::translate_existence(eb, ctx),
        QueryClause::InverseTriple(it) => translate_inverse_triple_top(it, ctx),
        QueryClause::BooleanFilter(be) => translate_bool_filter(be),
        QueryClause::AggregationQuery(aq) => crate::group::translate_agg_query(aq, ctx),
        QueryClause::Composition(_qc) => {
            // Composition is handled at the lib.rs level where all_queries is available.
            // Return empty here as a fallback.
            String::new()
        }
    }
}

/// Translate a top-level InverseTriple: `is prop of ?obj`
/// In SPARQL: ?obj prop ?primary_subject .
fn translate_inverse_triple_top(it: &InverseTriple, ctx: &mut ScopeCtx) -> String {
    let subject = ctx.primary_subject.clone().unwrap_or_else(|| ctx.gensym("s"));
    let obj = translate_object(&it.object, ctx);
    let prop = ctx.prop(&it.property, &obj);
    format!("{} {} {} .", obj, prop, subject)
}

/// Translate a boolean filter.
pub fn translate_bool_filter(be: &BoolExpr) -> String {
    let left = translate_bool_operand(&be.left);
    let right = translate_bool_operand(&be.right);
    let op = op_to_sparql(&be.op);
    format!("FILTER ({} {} {})", left, op, right)
}

fn translate_bool_operand(op: &BoolOperand) -> String {
    match op {
        BoolOperand::Variable(v) => v.clone(),
        BoolOperand::Literal(lit) => crate::scope::literal_to_sparql(lit),
    }
}

/// Translate one subject pattern to triples (and FILTERs, OPTIONALs).
pub fn translate_subject_pattern(sp: &SubjectPattern, ctx: &mut ScopeCtx) -> String {
    // Determine the subject variable
    let subject = match &sp.subject {
        Some(s) => {
            ctx.see_var(s);
            ctx.project(s);
            // Track the first non-silent subject as the primary subject
            if ctx.primary_subject.is_none() && !crate::scope::is_silent(s) {
                ctx.primary_subject = Some(s.clone());
            }
            s.clone()
        }
        None => {
            let g = ctx.gensym_for("s", NodeKey::at(sp.span.as_ref()));
            ctx.see_var(&g);
            g
        }
    };

    let mut lines = Vec::new();

    // Type assertion
    if let Some(tr) = &sp.type_ref {
        lines.push(type_assertion_lines(&subject, tr, ctx.namer));
    }

    // Property patterns
    for prop in &sp.properties {
        let translated = translate_property_pattern(prop, &subject, ctx);
        if !translated.is_empty() {
            lines.push(translated);
        }
    }

    lines.join("\n")
}

fn translate_property_pattern(pp: &PropertyPattern, subject: &str, ctx: &mut ScopeCtx) -> String {
    match pp {
        PropertyPattern::Value { property, object, .. } => {
            let prop = ctx.prop(property, subject);
            let obj = translate_object_and_project(object, ctx);
            format!("{} {} {} .", subject, prop, obj)
        }
        PropertyPattern::Constrained { property, block, .. } => {
            translate_constrained_property(property, block, subject, ctx)
        }
        PropertyPattern::Optional { property, object, .. } => {
            let prop = ctx.prop(property, subject);
            let obj = translate_object_and_project(object, ctx);
            format!("OPTIONAL {{ {} {} {} . }}", subject, prop, obj)
        }
        PropertyPattern::Inverse { property, outer_var, .. } => {
            // ?outer_var prop ?subject .
            let prop = ctx.prop(property, outer_var);
            ctx.see_var(outer_var);
            ctx.project(outer_var);
            format!("{} {} {} .", outer_var, prop, subject)
        }
        PropertyPattern::InverseNested { property, block, .. } => {
            // gensym inner node; inner ex:prop subject; constraints on inner
            let inner = ctx.gensym_for("n", NodeKey::at(block.span.as_ref()));
            let prop = ctx.prop(property, &inner);
            let mut lines = vec![format!("{} {} {} .", inner, prop, subject)];
            for c in &block.constraints {
                let s = translate_constraint(c, &inner, ctx);
                if !s.is_empty() {
                    lines.push(s);
                }
            }
            lines.join("\n")
        }
        PropertyPattern::Nested { property, block, .. } => {
            // gensym inner node; subject prop inner; inner properties
            let inner = ctx.gensym_for("n", NodeKey::at(block.span.as_ref()));
            let prop = ctx.prop(property, subject);
            let mut lines = vec![format!("{} {} {} .", subject, prop, inner)];
            for nested_pp in &block.properties {
                let s = translate_property_pattern(nested_pp, &inner, ctx);
                if !s.is_empty() {
                    lines.push(s);
                }
            }
            lines.join("\n")
        }
        PropertyPattern::Disjunction { either_branch, or_branches, .. } => {
            // { ?s prop1 val1 . } UNION { ?s prop2 val2 . }
            let mut branches = Vec::new();
            branches.push(translate_disj_branch(either_branch, subject, ctx));
            for b in or_branches {
                branches.push(translate_disj_branch(b, subject, ctx));
            }
            branches
                .iter()
                .map(|b| format!("{{ {} }}", b))
                .collect::<Vec<_>>()
                .join(" UNION ")
        }
    }
}

fn translate_disj_branch(branch: &DisjBranch, subject: &str, ctx: &mut ScopeCtx) -> String {
    translate_constrained_property(&branch.property, &branch.block, subject, ctx)
}

fn translate_constrained_property(
    property: &QualifiedName,
    block: &ConstraintBlock,
    subject: &str,
    ctx: &mut ScopeCtx,
) -> String {
    let prop = ctx.prop(property, subject);
    // Every constraint below gets its own variable for the constrained
    // object, all standing for this block's node.
    let key = NodeKey::at(block.span.as_ref());
    let mut lines = Vec::new();

    for constraint in &block.constraints {
        match constraint {
            Constraint::Comparison { binding, operator, value, .. } => {
                let var = match binding {
                    Some(b) => {
                        ctx.see_var(b);
                        ctx.project(b);
                        b.clone()
                    }
                    None => ctx.gensym_for("v", key.clone()),
                };
                lines.push(format!("{} {} {} .", subject, prop, var));
                if let Some((target_si, target_dim)) = quantity_target(value) {
                    lines.extend(quantity_unfold_lines(&var, operator, target_si, &target_dim, ctx));
                } else {
                    let val_str = expr_to_sparql(value);
                    let op_str = op_to_sparql(operator);
                    lines.push(format!("FILTER ({} {} {})", var, op_str, val_str));
                }
            }
            Constraint::TypeIs { type_ref, .. } => {
                let var = ctx.gensym_for("v", key.clone());
                lines.push(format!("{} {} {} .", subject, prop, var));
                lines.push(type_assertion_lines(&var, type_ref, ctx.namer));
            }
            Constraint::PropertyValue { property: nested_prop, value, .. } => {
                // Need a var for the constrained object
                let var = ctx.gensym_for("v", key.clone());
                lines.push(format!("{} {} {} .", subject, prop, var));
                let nprop = ctx.prop(nested_prop, &var);
                let val_str = translate_object_and_project(value, ctx);
                lines.push(format!("{} {} {} .", var, nprop, val_str));
            }
            Constraint::PropertyConstraint { property: nested_prop, block: nested_block, .. } => {
                let var = ctx.gensym_for("v", key.clone());
                lines.push(format!("{} {} {} .", subject, prop, var));
                let s = translate_constrained_property(nested_prop, nested_block, &var, ctx);
                if !s.is_empty() {
                    lines.push(s);
                }
            }
            // `subject prop [ is inv_prop of value ]` → the constrained object `var`
            // is the inverse-subject: `value inv_prop var`.
            Constraint::Inverse { property: inv_prop, value, .. } => {
                let var = ctx.gensym_for("v", key.clone());
                lines.push(format!("{} {} {} .", subject, prop, var));
                let val_str = translate_object_and_project(value, ctx);
                let iprop = ctx.prop(inv_prop, &val_str);
                lines.push(format!("{} {} {} .", val_str, iprop, var));
            }
            // `subject prop [ is inv_prop of [ ... ] ]` → `inner inv_prop var`,
            // constraints applied to the gensym `inner`.
            Constraint::InverseNested { property: inv_prop, block: inner_block, .. } => {
                let var = ctx.gensym_for("v", key.clone());
                lines.push(format!("{} {} {} .", subject, prop, var));
                let inner = ctx.gensym_for("n", NodeKey::at(inner_block.span.as_ref()));
                let iprop = ctx.prop(inv_prop, &inner);
                lines.push(format!("{} {} {} .", inner, iprop, var));
                for c in &inner_block.constraints {
                    let s = translate_constraint(c, &inner, ctx);
                    if !s.is_empty() {
                        lines.push(s);
                    }
                }
            }
        }
    }

    lines.join("\n")
}

fn translate_constraint(c: &Constraint, subject: &str, ctx: &mut ScopeCtx) -> String {
    match c {
        Constraint::TypeIs { type_ref, .. } => type_assertion_lines(subject, type_ref, ctx.namer),
        Constraint::Comparison { binding, operator, value, .. } => {
            let var = match binding {
                Some(b) => {
                    ctx.see_var(b);
                    ctx.project(b);
                    b.clone()
                }
                None => subject.to_string(),
            };
            if let Some((target_si, target_dim)) = quantity_target(value) {
                quantity_unfold_lines(&var, operator, target_si, &target_dim, ctx).join("\n")
            } else {
                let val_str = expr_to_sparql(value);
                let op_str = op_to_sparql(operator);
                format!("FILTER ({} {} {})", var, op_str, val_str)
            }
        }
        Constraint::PropertyValue { property, value, .. } => {
            let prop = ctx.prop(property, subject);
            let val_str = translate_object_and_project(value, ctx);
            format!("{} {} {} .", subject, prop, val_str)
        }
        Constraint::PropertyConstraint { property, block, .. } => {
            translate_constrained_property(property, block, subject, ctx)
        }
        // `is prop of value` on `subject` → `value prop subject`.
        Constraint::Inverse { property, value, .. } => {
            let val_str = translate_object_and_project(value, ctx);
            let prop = ctx.prop(property, &val_str);
            format!("{} {} {} .", val_str, prop, subject)
        }
        // `is prop of [ ... ]` on `subject` → `inner prop subject`, constraints on `inner`.
        Constraint::InverseNested { property, block, .. } => {
            let inner = ctx.gensym_for("n", NodeKey::at(block.span.as_ref()));
            let prop = ctx.prop(property, &inner);
            let mut lines = vec![format!("{} {} {} .", inner, prop, subject)];
            for c in &block.constraints {
                let s = translate_constraint(c, &inner, ctx);
                if !s.is_empty() {
                    lines.push(s);
                }
            }
            lines.join("\n")
        }
    }
}

/// Translate an Object, projecting the variable if it's a named variable.
pub fn translate_object_and_project(obj: &Object, ctx: &mut ScopeCtx) -> String {
    match obj {
        Object::Variable { name, .. } => {
            ctx.see_var(name);
            ctx.project(name);
            name.clone()
        }
        Object::Literal { value, .. } => expr_to_sparql(value),
        Object::Constant { value, .. } => ctx.namer.name(value),
        Object::Constraint { block } => ctx.gensym_for("v", NodeKey::at(block.span.as_ref())),
    }
}

/// Translate an Object without projecting (for existence/inverse contexts).
pub fn translate_object(obj: &Object, ctx: &mut ScopeCtx) -> String {
    match obj {
        Object::Variable { name, .. } => name.clone(),
        Object::Literal { value, .. } => expr_to_sparql(value),
        Object::Constant { value, .. } => ctx.namer.name(value),
        Object::Constraint { block } => ctx.gensym_for("v", NodeKey::at(block.span.as_ref())),
    }
}
