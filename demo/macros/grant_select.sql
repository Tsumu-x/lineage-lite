{#
  post-hook macro: 自動 GRANT SELECT 給指定的 roles
  在 dbt_project.yml 裡這樣用:
    post_hook: "{{ grant_select_to_roles(['bi_readonly', 'analyst']) }}"
#}
{% macro grant_select_to_roles(roles) %}
    {% for role in roles %}
        GRANT SELECT ON {{ this }} TO ROLE {{ role }};
    {% endfor %}
{% endmacro %}

{#
  有時候 on-run-end 也要跑，確保新 table 都有權限
  這種 macro 散落在各處，沒人整理過，每次 CI 都有機會爆
#}
{% macro grant_usage_on_schema(schema_name, role) %}
    GRANT USAGE ON SCHEMA {{ schema_name }} TO ROLE {{ role }};
    GRANT SELECT ON ALL TABLES IN SCHEMA {{ schema_name }} TO ROLE {{ role }};
    GRANT SELECT ON ALL VIEWS IN SCHEMA {{ schema_name }} TO ROLE {{ role }};
{% endmacro %}
