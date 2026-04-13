{{
  config(
    materialized='ephemeral'
  )
}}

{#
  intermediate: 每個使用者的訂單彙總
  用來給 mart_customers 做 customer 360
#}

WITH order_payments AS (
    SELECT * FROM {{ ref('int_order_payments') }}
),

user_summary AS (
    SELECT
        user_id,
        COUNT(DISTINCT order_id)       AS total_orders,
        SUM(payment_amount)            AS lifetime_value,
        MIN(order_date)                AS first_order_date,
        MAX(order_date)                AS last_order_date,
        COUNT(DISTINCT payment_method) AS payment_methods_used,

        -- 計算最近 30 天的訂單數
        COUNT(DISTINCT CASE
            WHEN order_date >= DATEADD('day', -30, CURRENT_DATE)
            THEN order_id
        END) AS orders_last_30d

    FROM order_payments
    GROUP BY user_id
)

SELECT * FROM user_summary
