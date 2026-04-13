{#
  安全除法，避免 division by zero
  每個 model 都在用這個，因為 Snowflake 的 division by zero 行為每個版本不一樣
#}
{% macro safe_divide(numerator, denominator, default=0) %}
    CASE
        WHEN {{ denominator }} IS NULL OR {{ denominator }} = 0 THEN {{ default }}
        ELSE CAST({{ numerator }} AS NUMERIC) / CAST({{ denominator }} AS NUMERIC)
    END
{% endmacro %}
