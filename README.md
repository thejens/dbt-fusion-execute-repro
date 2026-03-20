# dbt-fusion `execute` global scoping bug — repro

Minimal reproduction for [dbt-fusion#1289](https://github.com/dbt-labs/dbt-fusion/issues/1289).

## The bug

`configure_compile_and_run_jinja_environment` (in `dbt-jinja-utils`) does not add
`execute` as a Jinja **global**. It is only added to the render *context* by
`build_compile_and_run_base_context` (via `ctx.insert("execute", ...)` in
`compile_and_run_context.rs:102`).

When callers of the library API (e.g. [dbt-temporal](https://github.com/thejens/dbt-temporal))
invoke a materialization macro by calling `template.eval_to_state(context)` and then
`func.call(&state, ...)` manually, the materialization runs in a new execution scope.
Inside that scope, when the materialization calls `statement()` — a macro defined in a
*separate* template (`dbt-adapters/macros/etc/statement.sql`) — `statement()` executes
in its own frame and checks `if execute`. Because `execute` is not in the Jinja
*globals*, only in the render context that was threaded through, it can be seen as
undefined depending on how the context is propagated across the frame boundary.

**Affected code path** (`statement.sql`):

```jinja
{%- macro statement(name=None, fetch_result=False, ...) -%}
  {%- if execute: -%}         {# <-- this is the guard that may be undefined #}
    {%- set compiled_code = caller() -%}
    ...
    {%- set res, table = adapter.execute(compiled_code, ...) -%}
    ...
  {%- endif -%}
{%- endmacro %}
```

When `execute` is undefined (falsy), `statement()` silently no-ops — the SQL is
never sent to the warehouse, and no error is raised.

## Affected versions

Confirmed on `dbt-fusion 2.0.0-preview.148` (library crates at git rev
`24789794c453e44a0372bae2a6b9145b4ea5f5af`). The `ctx.insert` vs `add_global`
discrepancy is still present in `preview.158`.

## Why the `dbt` CLI does not reproduce this

Running `dbt run` or `dbt seed` directly does **not** trigger the bug. The CLI uses
its own internal rendering pipeline (`render_str` / `eval_to_state_with_outer_stack_depth`
in a chain that passes the base context through every frame transition). In that path,
`execute` propagates through `clone_base()` and is visible everywhere.

The bug only manifests when the *library API* is used directly and the caller follows
the `eval_to_state` → `state.lookup(macro)` → `func.call(&state)` pattern to invoke
materializations — as dbt-temporal does.

## This repo: macro structure involved

The macros in this repo show the call chain that triggers the issue. A custom
materialization calls `{% call statement('main') %}`, mirroring what
`materialization_seed_default` and all built-in materializations do:

```
models/repro.sql
  → config(materialized='my_materialization')
    → macros/my_materialization.sql  [materialization_my_materialization_default]
      → {% call statement('main') %}
          → dbt-adapters/macros/etc/statement.sql  [cross-template]
            → if execute:   <-- BUG: undefined when execute is not a Jinja global
```

The seed (`seeds/sample.csv`) exercises the same code path through
`materialization_seed_default`.

## How to run this project against the dbt-fusion CLI

> **Note:** The CLI itself does not reproduce the bug — it produces correct output
> (both logs print `True`). This run is shown for completeness and to confirm the
> macro structure is valid.

```sh
# Install dbt-fusion (version 2.0.0-preview.158 used here)
curl -fsSL https://public.cdn.getdbt.com/fs/install/install.sh | sh -s -- --update

# Run the model (expects DuckDB — no external services needed)
dbt run --profiles-dir . --project-dir .

# Expected CLI output (bug NOT triggered via CLI):
# execute in materialization scope: True
# execute inside statement() called from materialization: True
# Succeeded model main.repro

# Run the seed
dbt seed --profiles-dir . --project-dir .
# Expected: Succeeded seed main.sample
```

## The fix

In `configure_compile_and_run_jinja_environment`, also add `execute` as a Jinja
**global** (not just via the context map):

```rust
// In dbt-jinja-utils/src/phases/compile_and_run_context.rs:
pub fn configure_compile_and_run_jinja_environment(
    env: &mut JinjaEnv,
    adapter: Arc<dyn BaseAdapter>,
) {
    env.set_adapter(adapter);
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    env.add_global("execute", MinijinjaValue::from(true)); // <-- add this
}
```

This makes `execute` visible to any macro from any template, regardless of how the
caller threads the render context through.

## Workaround (applied in dbt-temporal)

```rust
dbt_jinja_utils::phases::configure_compile_and_run_jinja_environment(&mut jinja_env, adapter);

// Workaround: add execute as a global so cross-template macros (e.g. statement.sql)
// can see it. configure_compile_and_run_jinja_environment only calls ctx.insert,
// which puts execute in the render context but not in Jinja globals.
jinja_env
    .env
    .add_global("execute", minijinja::Value::from(true));
```
