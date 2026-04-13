{{
  config(
    materialized='incremental',
    unique_key=['revenue_month', 'revenue_type'],
    schema='marts',
    tags=['marts', 'finance', 'critical'],
    post_hook=[
      "{{ grant_select_to_roles(['finance_readonly', 'exec_dashboard']) }}"
    ]
  )
}}

{#
  mart: 營收彙總表（訂單 + 訂閱）
  finance team 的主要資料來源
  每月結算時這張表會被嚴格 audit

  注意: 這是 incremental，compound unique key
  full refresh 大概跑 45 分鐘，小心不要在上班時間跑
#}

{% set revenue_types = ['order', 'subscription'] %}

WITH order_revenue AS (
    SELECT
        DATE_TRUNC('month', order_date)     AS revenue_month,
        'order'                             AS revenue_type,
        SUM(payment_amount)                 AS total_revenue_cents,
        COUNT(DISTINCT order_id)            AS transaction_count
    FROM {{ ref('mart_orders') }}
    WHERE order_status = 'completed'
    {% if is_incremental() %}
        AND order_date >= (SELECT DATEADD('month', -3, MAX(revenue_month)) FROM {{ this }})
    {% endif %}
    GROUP BY 1, 2
),

subscription_revenue AS (
    SELECT
        DATE_TRUNC('month', started_at)     AS revenue_month,
        'subscription'                      AS revenue_type,
        SUM(monthly_amount)                 AS total_revenue_cents,
        COUNT(DISTINCT subscription_id)     AS transaction_count
    FROM {{ ref('mart_subscriptions') }}
    WHERE is_active = TRUE
    {% if is_incremental() %}
        AND started_at >= (SELECT DATEADD('month', -3, MAX(revenue_month)) FROM {{ this }})
    {% endif %}
    GROUP BY 1, 2
),

unioned AS (
    {% for type in revenue_types %}
        SELECT * FROM {{ type }}_revenue
        {% if not loop.last %}UNION ALL{% endif %}
    {% endfor %}
),

final AS (
    SELECT
        revenue_month,
        revenue_type,
        {{ cents_to_dollars('total_revenue_cents') }} AS total_revenue,
        transaction_count,
        {{ safe_divide(
            cents_to_dollars('total_revenue_cents'),
            'transaction_count'
        ) }} AS avg_transaction_value,

        -- YoY 成長率（用 window function）
        LAG({{ cents_to_dollars('total_revenue_cents') }}, 12) OVER (
            PARTITION BY revenue_type
            ORDER BY revenue_month
        ) AS revenue_same_month_last_year,

        {{ safe_divide(
            cents_to_dollars('total_revenue_cents') ~ ' - LAG(' ~ cents_to_dollars('total_revenue_cents') ~ ', 12) OVER (PARTITION BY revenue_type ORDER BY revenue_month)',
            'NULLIF(LAG(' ~ cents_to_dollars('total_revenue_cents') ~ ', 12) OVER (PARTITION BY revenue_type ORDER BY revenue_month), 0)'
        ) }} AS yoy_growth_rate

    FROM unioned
)

SELECT * FROM final
