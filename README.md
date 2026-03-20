# dbt-fusion `execute` global scoping bug — repro

Minimal reproduction for [dbt-fusion#1289](https://github.com/dbt-labs/dbt-fusion/issues/1289).

## The bug

`build_compile_and_run_base_context` (in `dbt-jinja-utils`) inserts `execute` into
the render *context* map:

```rust
// dbt-jinja-utils/src/phases/compile_and_run_context.rs
// (inside build_compile_and_run_base_context)
ctx.insert("execute".to_string(), MinijinjaValue::from(true));
```

`configure_compile_and_run_jinja_environment` — the function callers invoke to set up
the run-phase environment — never adds `execute` as a Jinja **global**:

```rust
pub fn configure_compile_and_run_jinja_environment(
    env: &mut JinjaEnv,
    adapter: Arc<dyn BaseAdapter>,
) {
    env.set_adapter(adapter);
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    // execute is never registered as a global here
}
```

When library callers invoke a materialization via
`template.eval_to_state(context)` → `func.call(&state)`, and the materialization body
calls `statement()` — a macro defined in a separate template
(`dbt-adapters/macros/etc/statement.sql`) — the `statement()` macro checks
`if execute` and sees it as undefined. The SQL is silently skipped.

## Call chain

```
models/repro.sql
  → config(materialized='my_materialization')
    → macros/my_materialization.sql  [materialization_my_materialization_default]
      → {% call statement('main') %}
          → dbt-adapters/macros/etc/statement.sql  [cross-template dispatch]
            → {%- if execute -%}   ← undefined when execute is not a Jinja global
```

The `macros/` and `models/` directories in this repo contain the minimal dbt project
that exercises this call chain. The seed (`seeds/sample.csv`) exercises the same path
through the built-in `materialization_seed_default`.

## Why the `dbt` CLI does not reproduce this

Running `dbt run` / `dbt seed` directly does **not** trigger the bug. The CLI's own
execution pipeline sets up its Jinja environment and rendering context differently from
the library API. The bug only manifests when external callers invoke materializations
via the `configure_compile_and_run_jinja_environment` + `eval_to_state` →
`func.call(&state)` pattern.

## The fix

Add one line to `configure_compile_and_run_jinja_environment`:

```rust
pub fn configure_compile_and_run_jinja_environment(
    env: &mut JinjaEnv,
    adapter: Arc<dyn BaseAdapter>,
) {
    env.set_adapter(adapter);
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    env.add_global("execute", MinijinjaValue::from(true));  // ← add this
}
```

Adding `execute` as a Jinja global guarantees it is visible to every macro in every
template, regardless of how the macro was dispatched or which template it belongs to.

## Affected version

Confirmed on `dbt-fusion 2.0.0-preview.148` (library crates at git rev
`24789794c453e44a0372bae2a6b9145b4ea5f5af`). Both `build_compile_and_run_base_context`
and `configure_compile_and_run_jinja_environment` are unchanged in `preview.158`.

## Workaround (for library callers)

Until fixed upstream, call `add_global` immediately after
`configure_compile_and_run_jinja_environment`:

```rust
dbt_jinja_utils::phases::configure_compile_and_run_jinja_environment(&mut jinja_env, adapter);

// Workaround for https://github.com/dbt-labs/dbt-fusion/issues/1289:
// execute must be a Jinja global so that statement() and other macros
// dispatched from separate templates can see it.
jinja_env
    .env
    .add_global("execute", minijinja::Value::from(true));
```
