{{
  config(
    materialized='view',
    tags=['staging', 'daily']
  )
}}

{#
  注意：card_brand + card_type 合併為 payment_method
  這是 Q3 schema migration 的一部分（RFC-2024-017）
  合併完成後這段 CONCAT 可以拿掉，改直接讀 payment_method 欄位
#}

{% set valid_methods = var('payment_methods', ['credit_card', 'debit_card']) %}

WITH source AS (
    SELECT * FROM {{ source('raw', 'payments') }}
),

renamed AS (
    SELECT
        id                                          AS payment_id,
        order_id,
        amount                                      AS payment_amount,

        -- schema migration: 合併 card_brand + card_type
        CASE
            WHEN payment_method IS NOT NULL THEN payment_method
            ELSE CONCAT(card_brand, '_', card_type)
        END                                         AS payment_method,

        created_at                                  AS payment_date
    FROM source
    WHERE amount > 0
)

SELECT * FROM renamed
WHERE payment_method IN (
    {% for method in valid_methods %}
        '{{ method }}'{% if not loop.last %},{% endif %}
    {% endfor %}
)
