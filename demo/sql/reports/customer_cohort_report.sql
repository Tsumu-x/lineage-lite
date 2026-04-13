-- BI 報表：客戶分群報表
CREATE VIEW reports.customer_cohort AS
SELECT
    DATE_TRUNC('month', registered_at) AS cohort_month,
    country,
    COUNT(DISTINCT user_id) AS user_count,
    SUM(lifetime_value) AS cohort_ltv,
    AVG(total_orders) AS avg_orders_per_user
FROM mart.mart_customers
GROUP BY 1, 2;
