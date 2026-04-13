{% macro cents_to_dollars(column_name, precision=2) %}
    ROUND(CAST({{ column_name }} AS NUMERIC) / 100.0, {{ precision }})
{% endmacro %}

{% macro dollars_to_cents(column_name) %}
    CAST({{ column_name }} * 100 AS INTEGER)
{% endmacro %}
