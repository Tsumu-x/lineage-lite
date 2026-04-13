{{
  config(
    materialized='incremental',
    unique_key='order_id',
    schema='marts',
    tags=['marts', 'bi', 'critical'],
    post_hook=[
      "{{ grant_select_to_roles(['bi_readonly', 'analyst', 'finance_readonly']) }}"
    ]
  )
}}

{#
  mart: 訂單寬表 — BI team 主要使用的報表來源
  downstream 包含多份 BI 報表與 Python export job
  變更前務必先跑 lineage-lite impact

  ⚠️ 這是 incremental model，改 unique_key 或 join 邏輯前要先 full refresh
  上次有人沒做 full refresh 直接改欄位，結果 BI 報表出現重複資料報了一週
#}

WITH order_payments AS (
    SELECT * FROM {{ ref('int_order_payments') }}
    {% if is_incremental() %}
    WHERE order_date >= (SELECT DATEADD('day', -3, MAX(order_date)) FROM {{ this }})
    {% endif %}
),

users AS (
    SELECT * FROM {{ ref('stg_users') }}
),

{# 計算每筆訂單的付款方式分佈 — 用自定義 pivot macro #}
payment_pivot AS (
    SELECT
        order_id,
        {{ pivot_values(
            column='payment_method',
            values=var('payment_methods'),
            agg='SUM',
            value_column='payment_amount',
            prefix='amount_'
        ) }}
    FROM order_payments
    GROUP BY order_id
),

final AS (
    SELECT
        op.order_id,
        op.user_id,
        users.user_name,
        users.email,
        users.country_code,
        op.order_status,
        op.order_date,
        op.payment_id,

        -- 金額轉換：原始資料是 cents
        {{ cents_to_dollars('op.payment_amount') }} AS payment_amount,
        op.payment_method,
        op.payment_date,
        {{ cents_to_dollars('op.order_total_amount') }} AS order_total_amount,

        -- pivot 結果
        {% for method in var('payment_methods') %}
            COALESCE(pp.amount_{{ method }}, 0) AS amount_{{ method }}
            {% if not loop.last %},{% endif %}
        {% endfor %}

    FROM order_payments op
    LEFT JOIN users ON op.user_id = users.user_id
    LEFT JOIN payment_pivot pp ON op.order_id = pp.order_id
    WHERE op.order_status != 'test'
)

SELECT * FROM final
