-- ad-hoc 分析：付款方式分佈（只是一次性 export）
CREATE TABLE analysis.payment_breakdown AS
SELECT
    payment_method,
    COUNT(*) AS txn_count,
    SUM(amount) AS total_amount
FROM mart.mart_orders
GROUP BY payment_method;
