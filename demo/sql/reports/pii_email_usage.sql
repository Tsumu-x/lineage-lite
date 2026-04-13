-- ad-hoc 報表：某 analyst 隨手建的，包含 email（PII 違規）
CREATE TABLE reports.user_email_export AS
SELECT
    user_id,
    user_name,
    email,
    total_orders,
    lifetime_value
FROM mart.mart_customers
WHERE country = 'TW';
