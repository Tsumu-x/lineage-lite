{{
  config(
    materialized='view',
    tags=['staging', 'pii']
  )
}}

{#
  ⚠️ 此 model 含 PII 欄位（email, phone）
  下游使用時需確認是否在允許的 domain 內
  治理 policy: PII 只能出現在 staging + marts.customers
#}

WITH source AS (
    SELECT * FROM {{ source('raw', 'users') }}
),

renamed AS (
    SELECT
        id                          AS user_id,
        TRIM(name)                  AS user_name,
        LOWER(TRIM(email))          AS email,
        phone,
        created_at                  AS registered_at,
        UPPER(country)              AS country_code,

        -- 用 dbt_utils 產生 surrogate key
        {{ dbt_utils.generate_surrogate_key(['id', 'email']) }} AS user_sk

    FROM source
    WHERE email IS NOT NULL
)

SELECT * FROM renamed
