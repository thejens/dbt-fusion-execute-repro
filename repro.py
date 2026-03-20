"""
Minimal repro for https://github.com/dbt-labs/dbt-fusion/issues/1289

Root cause
----------
configure_compile_and_run_jinja_environment puts `execute` into the render
*context* (ctx.insert), but NOT into Jinja *globals* (env.add_global).

In both Jinja2 and minijinja, macros imported from a separate template run in
an isolated scope: they can only see their own arguments and Jinja globals.
They do NOT inherit the caller template's render context.

So when statement() (from dbt-adapters/macros/etc/statement.sql) is called
inside a materialization macro and checks `if execute`, it sees `execute` as
undefined and silently skips all SQL.

Fix
---
In configure_compile_and_run_jinja_environment, also call:
    env.add_global("execute", Value::from(true))

Run
---
    pip install jinja2
    python repro.py
"""

from jinja2 import Environment, DictLoader

# statement.sql — simplified from dbt-adapters/macros/etc/statement.sql.
# The real macro guards all SQL execution behind `if execute`.
STATEMENT_SQL = """\
{%- macro statement() -%}
  {%- if execute -%}
    [SQL EXECUTED — execute was True]
  {%- else -%}
    [SQL SKIPPED — execute was falsy/undefined (BUG)]
  {%- endif -%}
{%- endmacro -%}
"""

# materialization.sql — mirrors what materialization_seed_default and all
# built-in materializations do: import statement() from another template and
# call it with {% call statement(...) %} to run the actual SQL.
MATERIALIZATION_SQL = """\
{%- from "statement.sql" import statement -%}
{{- statement() -}}
"""

# ---------------------------------------------------------------------------
# BUG: execute only in render context, not as a Jinja global.
#
# This matches what configure_compile_and_run_jinja_environment does today:
# the library puts execute=True into the BTreeMap context passed to template
# rendering, but never calls env.add_global("execute", ...).
#
# Result: the imported statement() macro runs in an isolated scope and does
# not inherit the caller's render context — so `execute` is undefined.
# ---------------------------------------------------------------------------
env_bug = Environment(loader=DictLoader({
    "statement.sql": STATEMENT_SQL,
    "materialization.sql": MATERIALIZATION_SQL,
}))

# execute=True is in the render context of materialization.sql,
# but statement() is from a different template and cannot see it.
result = env_bug.get_template("materialization.sql").render(execute=True)
print(f"BUG  — execute in render context only : {result}")
# Expected output: [SQL SKIPPED — execute was falsy/undefined (BUG)]

# ---------------------------------------------------------------------------
# FIX: add execute as a Jinja global.
#
# Globals are visible to every macro in every template, regardless of which
# template the macro was defined in or how it was called.
#
# One-line fix in configure_compile_and_run_jinja_environment (Rust):
#   env.add_global("execute", Value::from(true))
# ---------------------------------------------------------------------------
env_fix = Environment(loader=DictLoader({
    "statement.sql": STATEMENT_SQL,
    "materialization.sql": MATERIALIZATION_SQL,
}))
env_fix.globals["execute"] = True   # <-- the fix

result = env_fix.get_template("materialization.sql").render()
print(f"FIXED — execute as Jinja global       : {result}")
# Expected output: [SQL EXECUTED — execute was True]
