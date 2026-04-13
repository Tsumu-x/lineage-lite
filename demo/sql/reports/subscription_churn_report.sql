-- BI 報表：訂閱流失分析
CREATE VIEW reports.subscription_churn AS
SELECT
    DATE_TRUNC('month', ended_at) AS churn_month,
    plan_name,
    COUNT(DISTINCT subscription_id) AS churned_count,
    SUM(monthly_amount) AS lost_mrr
FROM mart.mart_subscriptions
WHERE subscription_status = 'cancelled'
GROUP BY 1, 2;
