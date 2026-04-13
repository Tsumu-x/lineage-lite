{{
  config(
    materialized='view',
    tags=['staging']
  )
}}

WITH source AS (
    SELECT * FROM {{ source('raw', 'subscriptions') }}
),

renamed AS (
    SELECT
        id                          AS subscription_id,
        user_id,
        plan_name,
        status                      AS subscription_status,
        started_at,
        ended_at,
        monthly_amount,

        -- 計算訂閱天數
        DATEDIFF('day', started_at, COALESCE(ended_at, CURRENT_DATE)) AS subscription_days,

        CASE
            WHEN status = 'active' THEN TRUE
            ELSE FALSE
        END AS is_active

    FROM source
)

SELECT * FROM renamed
