{#
  動態 UNION 多張同結構的表
  用法: {{ union_tables(ref('events'), 'events_', ['web', 'mobile', 'app']) }}
  會產生:
    SELECT * FROM events_web
    UNION ALL
    SELECT * FROM events_mobile
    UNION ALL
    SELECT * FROM events_app

  當時花了兩天 debug 這個 macro，因為有人在 suffix list 裡多打了一個空格…
#}
{% macro union_tables(base_ref, prefix, suffixes) %}
    {% for suffix in suffixes %}
        SELECT
            '{{ suffix }}' AS _source,
            *
        FROM {{ base_ref }}_{{ suffix }}
        {% if not loop.last %}UNION ALL{% endif %}
    {% endfor %}
{% endmacro %}
