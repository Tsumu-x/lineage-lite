-- BI 報表：每日營收報表（接在 mart_orders 下游）
CREATE VIEW reports.daily_revenue AS
SELECT
    order_date,
    COUNT(DISTINCT order_id) AS order_count,
    SUM(amount) AS daily_revenue,
    AVG(amount) AS avg_order_value,
    payment_method
FROM mart.mart_orders
GROUP BY order_date, payment_method;
