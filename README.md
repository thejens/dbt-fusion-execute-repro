# dbt-fusion `execute` repro

Minimal reproduction for [dbt-fusion#1289](https://github.com/dbt-labs/dbt-fusion/issues/1289).

## The bug

When a materialization macro calls `statement()`, the `statement()` macro checks
`if execute` but sees it as **undefined** in dbt-fusion.

**Root cause:** `configure_compile_and_run_jinja_environment` injects `execute=True`
into the *render context* of the model being compiled. However, `statement()` is
defined in a *separate* template (from dbt's macro library). In minijinja, macros
from other templates resolve variables from **globals**, not from the caller's render
context. So `execute` is invisible to `statement()`, the `if execute` guard is
falsy, and the SQL is silently skipped.

This affects any materialization that calls `statement()` internally — including the
built-in `materialization_seed_default`.

## Reproduce

```sh
# Install dbt-fusion (adjust to your installation method)
pip install dbt-fusion   # or however you have it

# Run the model with a custom materialization that calls statement()
dbt-fusion run --profiles-dir . --project-dir .

# Run the seed (uses built-in materialization_seed_default, same code path)
dbt-fusion seed --profiles-dir . --project-dir .
```

## Expected output

Both commands should succeed and create the table/seed in the warehouse.
The logs should show:

```
execute in materialization scope: True
execute inside statement() called from materialization: True
```

## Actual output (broken)

The `execute` value inside `statement()` is **undefined** (falsy), so the
`create table` SQL is never sent to the warehouse:

```
execute in materialization scope: True
execute inside statement() called from materialization:
```

The `run` command exits with success but no table is created. With seeds,
`dbt-fusion seed` silently no-ops.

## Workaround

After calling `configure_compile_and_run_jinja_environment`, also set `execute` as a
Jinja **global** (not just a render-context variable):

```rust
jinja_env
    .env
    .add_global("execute", minijinja::Value::from(true));
```

This makes `execute` visible to macros from any template, matching dbt-core behaviour.
