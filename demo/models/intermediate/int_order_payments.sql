{{
  config(
    materialized='ephemeral'
  )
}}

{#
  intermediate: 訂單 + 付款的 grain-level join
  這個 model 是 ephemeral，不會實體化成表
  用來給下游 mart_orders / mart_customers 共用
#}

WITH orders AS (
    SELECT * FROM {{ ref('stg_orders') }}
),

payments AS (
    SELECT * FROM {{ ref('stg_payments') }}
),

order_payments AS (
    SELECT
        orders.order_id,
        orders.user_id,
        orders.order_status,
        orders.order_date,
        payments.payment_id,
        payments.payment_amount,
        payments.payment_method,
        payments.payment_date,

        -- 一張訂單可能有多筆付款，算總額
        SUM(payments.payment_amount) OVER (
            PARTITION BY orders.order_id
        ) AS order_total_amount

    FROM orders
    LEFT JOIN payments ON orders.order_id = payments.order_id
)

SELECT * FROM order_payments
