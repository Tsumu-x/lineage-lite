{#
  覆寫 dbt 預設的 schema 產生邏輯
  prod: 直接用 custom schema name
  dev:  加上 user prefix 避免衝突
  這個 macro 很關鍵，搞壞了整個 warehouse 的 schema 都會亂掉
#}
{% macro generate_schema_name(custom_schema_name, node) -%}
    {%- set default_schema = target.schema -%}
    {%- if custom_schema_name is none -%}
        {{ default_schema }}
    {%- elif target.name == 'prod' -%}
        {{ custom_schema_name | trim }}
    {%- else -%}
        {{ default_schema }}_{{ custom_schema_name | trim }}
    {%- endif -%}
{%- endmacro %}
