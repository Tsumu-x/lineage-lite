{{
  config(
    materialized='table',
    schema='marts',
    tags=['marts', 'bi']
  )
}}

{% set churn_days = var('churn_threshold_days', 90) %}

WITH subscriptions AS (
    SELECT * FROM {{ ref('stg_subscriptions') }}
),

users AS (
    SELECT * FROM {{ ref('stg_users') }}
),

final AS (
    SELECT
        s.subscription_id,
        s.user_id,
        users.user_name,
        users.email,
        s.plan_name,
        s.subscription_status,
        s.started_at,
        s.ended_at,
        s.monthly_amount,
        s.subscription_days,
        s.is_active,

        -- churn risk: 活躍但超過 N 天沒有訂單
        CASE
            WHEN s.is_active
                AND s.subscription_days > {{ churn_days }}
            THEN TRUE
            ELSE FALSE
        END AS is_churn_risk

    FROM subscriptions s
    LEFT JOIN users ON s.user_id = users.user_id
)

SELECT * FROM final
