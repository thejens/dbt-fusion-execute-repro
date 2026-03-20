# dbt-fusion `execute` global scoping bug — repro

Minimal reproduction for [dbt-fusion#1289](https://github.com/dbt-labs/dbt-fusion/issues/1289).

## The bug

`configure_compile_and_run_jinja_environment` (in `dbt-jinja-utils`) puts `execute`
into the render *context* via `ctx.insert(...)`, but never adds it as a Jinja
**global** via `env.add_global(...)`.

In both Jinja2 and minijinja, macros imported from a **separate template** run in an
isolated scope: they can only see their own arguments and Jinja globals. They do **not**
inherit the caller template's render context.

So when `statement()` (defined in `dbt-adapters/macros/etc/statement.sql`) is called
from inside a materialization macro and checks `if execute`, it sees `execute` as
undefined and silently skips all SQL.

## Minimal repro — [`repro.py`](repro.py)

```
pip install jinja2
python repro.py
```

**Output:**

```
BUG  — execute in render context only : [SQL SKIPPED — execute was falsy/undefined (BUG)]
FIXED — execute as Jinja global       : [SQL EXECUTED — execute was True]
```

The script uses two templates that mirror the dbt macro structure:

```
materialization.sql
  └── imports and calls statement() from statement.sql
        └── {% if execute %}  ← undefined unless execute is a Jinja global
```

`execute=True` is passed as a render context variable to `materialization.sql`. The
imported `statement()` macro cannot see it because it runs in an isolated scope. When
`execute` is added as a Jinja **global** instead, it is visible to all macros in all
templates.

## Affected code

**`dbt-jinja-utils/src/phases/compile_and_run_context.rs:102`**

```rust
// compile_and_run_context.rs — build_compile_and_run_base_context
ctx.insert("execute".to_string(), MinijinjaValue::from(true));  // render context only
```

**`dbt-jinja-utils/src/phases/compile_and_run_context.rs:30`**

```rust
// configure_compile_and_run_jinja_environment — missing the global
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
`24789794c453e44a0372bae2a6b9145b4ea5f5af`). The `ctx.insert` vs `add_global`
discrepancy is still present in `preview.158`.

## Why the `dbt` CLI does not reproduce this

Running `dbt run` / `dbt seed` directly does **not** trigger the bug. The CLI's
internal rendering pipeline happens to propagate the base context through every
frame transition, so `execute` is visible. The bug only manifests when library
callers (e.g. [dbt-temporal](https://github.com/thejens/dbt-temporal)) use the
`eval_to_state` → `state.lookup(macro)` → `func.call(&state)` pattern to invoke
materializations directly.

## Workaround (applied in dbt-temporal)

```rust
dbt_jinja_utils::phases::configure_compile_and_run_jinja_environment(&mut jinja_env, adapter);

// Workaround for https://github.com/dbt-labs/dbt-fusion/issues/1289:
// execute must be a Jinja global, not just a render context variable,
// so that statement() and other cross-template macros can see it.
jinja_env
    .env
    .add_global("execute", minijinja::Value::from(true));
```

## dbt project (for macro structure reference)

The [`macros/`](macros/) and [`models/`](models/) directories contain a minimal dbt
project showing the call chain that triggers the issue:

```
models/repro.sql
  → config(materialized='my_materialization')
    → macros/my_materialization.sql  [materialization_my_materialization_default]
      → {% call statement('main') %}
          → dbt-adapters/macros/etc/statement.sql  [cross-template]
            → if execute:   ← undefined when execute is not a Jinja global
```
