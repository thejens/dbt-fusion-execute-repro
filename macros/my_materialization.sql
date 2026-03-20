{% materialization my_materialization, default %}

  {%- set target_relation = this.incorporate(type='table') -%}

  -- Log execute from the materialization's own scope.
  -- In dbt-core this prints True; in dbt-fusion it also prints True here
  -- because the render context is set up correctly at this level.
  {{ log("execute in materialization scope: " ~ execute, info=True) }}

  -- Now delegate to statement(), which is a macro defined in a *separate*
  -- template.  In minijinja, macros from other templates resolve variables
  -- from globals, not from the caller's render context.  So if `execute` was
  -- only injected into the render context (not added as a global), statement()
  -- will see it as undefined and silently skip the SQL.
  {% call statement('main', fetch_result=False) %}
    {{ log("execute inside statement() called from materialization: " ~ execute, info=True) }}

    create or replace table {{ target_relation }} as (
      {{ sql }}
    )
  {% endcall %}

  {{ return({'relations': [target_relation]}) }}

{% endmaterialization %}
