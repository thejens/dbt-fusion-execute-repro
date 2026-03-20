# dbt-fusion `execute` global scoping bug — repro

Minimal reproduction for [dbt-fusion#1289](https://github.com/dbt-labs/dbt-fusion/issues/1289).

> **Note:** This bug only affects **library callers** of `configure_compile_and_run_jinja_environment`.
> The `dbt` CLI uses a separate internal execution pipeline and is not affected.
> `configure_compile_and_run_jinja_environment` is not called anywhere inside
> the dbt-fusion codebase itself — it is a public API for external library callers only.

## The bug

`build_compile_and_run_base_context` (in `dbt-jinja-utils`) inserts `execute` into
the render *context* map:

```rust
// dbt-jinja-utils/src/phases/compile_and_run_context.rs
// inside build_compile_and_run_base_context
ctx.insert("execute".to_string(), MinijinjaValue::from(true));
```

`configure_compile_and_run_jinja_environment` — the public API library callers use
to configure the run-phase environment — never adds `execute` as a Jinja **global**:

```rust
pub fn configure_compile_and_run_jinja_environment(
    env: &mut JinjaEnv,
    adapter: Arc<dyn BaseAdapter>,
) {
    env.set_adapter(adapter);
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    // execute is never added as a global here
}
```

When a library caller invokes a materialization via
`template.eval_to_state(context)` → `func.call(&state)`, and the materialization
body calls `statement()` — a macro defined in a separate template
(`dbt-adapters/macros/etc/statement.sql`) — dbt-fusion re-evaluates that template
in a fresh context. Whether `execute` propagates into that fresh context is not
guaranteed by the API contract. When it does not, `statement()` checks `if execute`,
sees it as undefined, and silently skips all SQL.

## Minimal repro — [`rust-repro/`](rust-repro/)

The Rust program in `rust-repro/` demonstrates why render-context variables cannot
be relied upon across template boundaries, whereas Jinja globals always are:

```sh
cd rust-repro
cargo run
```

**Output:**

```
BUG  — execute in caller context only : [SQL SILENTLY SKIPPED — execute was falsy/undefined]
FIXED — execute as Jinja global       : [SQL SENT TO WAREHOUSE]
```

The program renders `statement.sql` (which checks `if execute`) in a fresh context,
mirroring what happens when dbt-fusion's template dispatch evaluates it independently
of the materialization's render context. With `execute` only in the caller's context
it is undefined; as a global it is always visible.

## Call chain (dbt project reference)

The `macros/` and `models/` directories contain a minimal dbt project showing the
macro call chain that triggers the issue at runtime:

```
models/repro.sql
  → config(materialized='my_materialization')
    → macros/my_materialization.sql  [materialization_my_materialization_default]
      → {% call statement('main') %}
          → dbt-adapters/macros/etc/statement.sql  [cross-template dispatch]
            → {%- if execute -%}   ← undefined when execute is not a Jinja global
```

The seed (`seeds/sample.csv`) exercises the same path through the built-in
`materialization_seed_default`.

## Affected code

**`dbt-jinja-utils/src/phases/compile_and_run_context.rs`** — `build_compile_and_run_base_context`

```rust
ctx.insert("execute".to_string(), MinijinjaValue::from(true));  // render context only
```

**`dbt-jinja-utils/src/phases/compile_and_run_context.rs`** — `configure_compile_and_run_jinja_environment`

```rust
pub fn configure_compile_and_run_jinja_environment(
    env: &mut JinjaEnv,
    adapter: Arc<dyn BaseAdapter>,
) {
    env.set_adapter(adapter);
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    // ← execute is never added as a global here
}
```

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
