{#
  動態 pivot：把某個欄位的值轉成 columns
  這個 macro 當初是從 dbt_utils fork 過來改的
  因為原版不支援 Snowflake 的某個怪行為

  用法:
  {{ pivot_values(
      column='payment_method',
      values=['credit_card', 'debit_card', 'apple_pay'],
      agg='SUM',
      value_column='amount',
      then_value=1,
      else_value=0,
      prefix='total_'
  ) }}
#}
{% macro pivot_values(column, values, agg='COUNT', value_column=None, then_value=1, else_value=0, prefix='', suffix='') %}
    {% for val in values %}
        {{ agg }}(
            CASE
                WHEN {{ column }} = '{{ val }}'
                THEN {% if value_column %}{{ value_column }}{% else %}{{ then_value }}{% endif %}
                ELSE {% if value_column %}NULL{% else %}{{ else_value }}{% endif %}
            END
        ) AS {{ prefix }}{{ val | replace(' ', '_') | lower }}{{ suffix }}
        {% if not loop.last %},{% endif %}
    {% endfor %}
{% endmacro %}
