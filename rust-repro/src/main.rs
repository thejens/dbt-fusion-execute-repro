/// Demonstrates why `execute` must be a Jinja *global*, not just a render
/// context variable, for it to be reliably visible across template boundaries.
///
/// Background
/// ----------
/// dbt-fusion's `configure_compile_and_run_jinja_environment` (dbt-jinja-utils)
/// sets up the run-phase environment for library callers. It does NOT add
/// `execute` as a Jinja global. The variable only appears in the render context
/// map that callers pass when they render a template.
///
/// When that template dispatches to a macro from a *different* template (e.g.
/// the materialization calling `statement()` from dbt-adapters), dbt-fusion
/// re-evaluates that second template in a fresh context. Whether `execute`
/// propagates into that fresh context depends on internal details of the
/// dispatch machinery — it is not guaranteed.
///
/// By contrast, Jinja globals are attached to the `Environment` itself and are
/// visible in every template render, every macro call, and every cross-template
/// dispatch, without exception.
///
/// This program demonstrates the root of that difference.
use minijinja::{Environment, Value, context};

fn main() {
    // statement.sql — simplified from dbt-adapters/macros/etc/statement.sql.
    // Checks `execute` and runs SQL only if it is truthy.
    let statement_sql = r#"
{%- if execute -%}
  [SQL SENT TO WAREHOUSE]
{%- else -%}
  [SQL SILENTLY SKIPPED — execute was falsy/undefined]
{%- endif -%}
"#;

    // -----------------------------------------------------------------------
    // CASE 1: execute in render context — NOT visible when statement.sql is
    //         rendered independently (as dbt-fusion's template dispatch does).
    //
    // The caller (materialization template) has execute=True in its render
    // context. But when statement.sql is dispatched as a fresh template
    // evaluation, it starts with an empty context and execute is undefined.
    // -----------------------------------------------------------------------
    {
        let mut env = Environment::new();
        env.add_template("statement.sql", statement_sql).unwrap();

        // Caller has execute=True in its render context — but that context
        // does not carry into the fresh evaluation of statement.sql.
        let result = env
            .get_template("statement.sql").unwrap()
            .render(context! {})   // fresh render: no execute
            .unwrap();
        println!("BUG  — execute in caller context only : {}", result.trim());
        // [SQL SILENTLY SKIPPED — execute was falsy/undefined]
    }

    // -----------------------------------------------------------------------
    // CASE 2: execute as a Jinja global — always visible.
    //
    // env.add_global attaches execute to the Environment itself. Every
    // template render and every cross-template macro dispatch in this
    // environment can see it, regardless of how the invocation is structured.
    //
    // This is the one-line fix for configure_compile_and_run_jinja_environment:
    //   env.add_global("execute", MinijinjaValue::from(true))
    // -----------------------------------------------------------------------
    {
        let mut env = Environment::new();
        env.add_template("statement.sql", statement_sql).unwrap();
        env.add_global("execute", Value::from(true));  // ← the fix

        let result = env
            .get_template("statement.sql").unwrap()
            .render(context! {})   // fresh render: execute visible via global
            .unwrap();
        println!("FIXED — execute as Jinja global       : {}", result.trim());
        // [SQL SENT TO WAREHOUSE]
    }
}
