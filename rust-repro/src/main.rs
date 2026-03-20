/// Demonstrates why `execute` must be a Jinja *global*, not just a render
/// context variable, for it to be reliably visible across template boundaries.
///
/// Background
/// ----------
/// dbt-fusion's `configure_compile_and_run_jinja_environment` does NOT add
/// `execute` as a Jinja global. The variable only appears in the render context
/// map that callers pass when they invoke a template.
///
/// When that template dispatches to a macro from a *different* template (e.g.
/// the materialization calling `statement()` from dbt-adapters), the macro can
/// be invoked in a context where the outer render context is not present.
/// Globals are attached to the Environment itself and are always visible
/// regardless of how the invocation is structured.
///
/// NOTE: standard minijinja's `{% from ... import ... %}` happens to propagate
/// the render context into imported macros, so the scoping issue cannot be
/// demonstrated with that mechanism alone. The failure mode in dbt-fusion
/// requires its internal template registry dispatch (not reproduced here without
/// that library). This program demonstrates the underlying principle: render
/// context is local to one render call; globals are universal.
use minijinja::{Environment, Value, context};

fn main() {
    // statement.sql — simplified from dbt-adapters/macros/etc/statement.sql.
    // Checks `execute` and runs SQL only if truthy.
    let statement_sql = r#"
{%- if execute -%}
  [SQL SENT TO WAREHOUSE]
{%- else -%}
  [SQL SILENTLY SKIPPED — execute was falsy/undefined]
{%- endif -%}
"#;

    // -----------------------------------------------------------------------
    // For reference: standard minijinja {% from ... import ... %} actually
    // propagates the render context into imported macros, so execute IS
    // visible here. This is NOT the failure mode in dbt-fusion.
    // -----------------------------------------------------------------------
    {
        let mut env = Environment::new();
        env.add_template("statement.sql", r#"{%- macro statement() -%}{%- if execute -%}[EXECUTED]{%- else -%}[SKIPPED]{%- endif -%}{%- endmacro -%}"#).unwrap();
        env.add_template("mat.sql", r#"{%- from "statement.sql" import statement -%}{{- statement() -}}"#).unwrap();

        let r = env.get_template("mat.sql").unwrap().render(context! { execute => true }).unwrap();
        println!("minijinja import (context propagates) : {r}");
        // Prints [EXECUTED] — minijinja passes context through; no bug this way
    }

    // -----------------------------------------------------------------------
    // The actual failure mode: dbt-fusion's template registry dispatch
    // re-evaluates the statement template in a *fresh* context. When execute
    // is only in the caller's render context and not a global, the fresh
    // evaluation of statement.sql cannot see it.
    //
    // This is what execute_node.rs works around by calling
    //   jinja_env.env.add_global("execute", minijinja::Value::from(true))
    // after configure_compile_and_run_jinja_environment.
    // -----------------------------------------------------------------------
    println!();
    {
        let mut env = Environment::new();
        env.add_template("statement.sql", statement_sql).unwrap();

        // The materialization renders with execute=True in its context.
        // But dbt-fusion's template dispatch re-evaluates statement.sql
        // independently — execute is not carried into that fresh evaluation.
        let _caller_context_has_execute = true; // (present in the materialization's render)
        let result = env
            .get_template("statement.sql").unwrap()
            .render(context! {})   // statement.sql evaluated fresh, no execute
            .unwrap();
        println!("BUG  — fresh template eval, no global  : {}", result.trim());
    }

    // -----------------------------------------------------------------------
    // Fix: add execute as a Jinja global. Globals are attached to the
    // Environment and visible in every render — including fresh template
    // evaluations triggered by cross-template dispatch.
    //
    // One-line fix in configure_compile_and_run_jinja_environment:
    //   env.add_global("execute", MinijinjaValue::from(true))
    // -----------------------------------------------------------------------
    {
        let mut env = Environment::new();
        env.add_template("statement.sql", statement_sql).unwrap();
        env.add_global("execute", Value::from(true));  // ← the fix

        let result = env
            .get_template("statement.sql").unwrap()
            .render(context! {})   // fresh eval, but global is always present
            .unwrap();
        println!("FIXED — execute as Jinja global        : {}", result.trim());
    }
}
