-- BI 報表：月度營收總覽（來自 mart_revenue）
CREATE VIEW reports.monthly_summary AS
SELECT
    revenue_month,
    revenue_type,
    total_revenue,
    transaction_count,
    total_revenue / NULLIF(transaction_count, 0) AS avg_transaction_value
FROM mart.mart_revenue;
