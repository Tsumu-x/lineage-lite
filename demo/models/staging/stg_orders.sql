{{
  config(
    materialized='view',
    tags=['staging', 'daily']
  )
}}

-- staging: 清洗原始訂單資料，過濾已刪除與測試訂單

WITH source AS (
    SELECT * FROM {{ source('raw', 'orders') }}
),

renamed AS (
    SELECT
        id                          AS order_id,
        user_id,
        status                      AS order_status,
        created_at                  AS order_date,
        updated_at,
        _fivetran_synced            AS loaded_at
    FROM source
    WHERE
        is_deleted = false
        AND id NOT IN (SELECT id FROM {{ source('raw', 'orders') }} WHERE status = 'test')
)

SELECT * FROM renamed
