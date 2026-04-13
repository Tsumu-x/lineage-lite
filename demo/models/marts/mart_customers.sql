{{
  config(
    materialized='table',
    schema='marts',
    tags=['marts', 'bi', 'critical', 'pii'],
    pre_hook=[
      "{{ log('Building mart_customers at ' ~ run_started_at, info=True) }}"
    ]
  )
}}

{#
  mart: 客戶 360 度視圖
  ⚠️ 含 PII 欄位（email），已確認在允許的 domain 內
  治理 policy 允許 marts.customers 保留 email

  這個 model 很肥，跑完大概要 20 分鐘
  有人提過要改成 incremental 但 unique_key 不好選（user 會 merge）
#}

{%- set segment_thresholds = {
    'vip': 10000,
    'regular': 1000,
    'casual': 0
} -%}

WITH users AS (
    SELECT * FROM {{ ref('stg_users') }}
),

user_orders AS (
    SELECT * FROM {{ ref('int_user_order_summary') }}
),

subscriptions AS (
    SELECT
        user_id,
        COUNT(DISTINCT subscription_id)                     AS total_subscriptions,
        SUM(CASE WHEN is_active THEN 1 ELSE 0 END)         AS active_subscriptions,
        SUM(CASE WHEN is_active THEN monthly_amount ELSE 0 END) AS total_mrr,
        MIN(started_at)                                     AS first_subscription_date,
        MAX(CASE WHEN is_active THEN started_at END)        AS latest_active_subscription
    FROM {{ ref('stg_subscriptions') }}
    GROUP BY user_id
),

final AS (
    SELECT
        users.user_id,
        users.user_sk,
        users.user_name,
        users.email,
        users.country_code,
        users.registered_at,

        -- 訂單
        COALESCE(user_orders.total_orders, 0)           AS total_orders,
        {{ cents_to_dollars('COALESCE(user_orders.lifetime_value, 0)') }} AS lifetime_value,
        user_orders.first_order_date,
        user_orders.last_order_date,
        COALESCE(user_orders.orders_last_30d, 0)        AS orders_last_30d,

        -- 訂閱
        COALESCE(subscriptions.total_subscriptions, 0)  AS total_subscriptions,
        COALESCE(subscriptions.active_subscriptions, 0) AS active_subscriptions,
        {{ cents_to_dollars('COALESCE(subscriptions.total_mrr, 0)') }} AS total_mrr,
        subscriptions.first_subscription_date,
        subscriptions.latest_active_subscription,

        -- 客戶分群（用 Jinja 動態產生 CASE WHEN）
        CASE
            {% for segment, threshold in segment_thresholds.items() %}
            WHEN COALESCE(user_orders.lifetime_value, 0) >= {{ threshold }}
                THEN '{{ segment }}'
            {% endfor %}
            ELSE 'prospect'
        END AS customer_segment,

        -- 客戶健康度
        {{ safe_divide(
            'user_orders.orders_last_30d',
            'NULLIF(user_orders.total_orders, 0)'
        ) }} AS recent_order_ratio,

        -- 上次活動距今天數
        DATEDIFF('day',
            GREATEST(
                COALESCE(user_orders.last_order_date, '1970-01-01'),
                COALESCE(subscriptions.latest_active_subscription, '1970-01-01')
            ),
            CURRENT_DATE
        ) AS days_since_last_activity

    FROM users
    LEFT JOIN user_orders   ON users.user_id = user_orders.user_id
    LEFT JOIN subscriptions ON users.user_id = subscriptions.user_id
)

SELECT * FROM final
