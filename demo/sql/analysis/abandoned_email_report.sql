-- 已廢棄的 ad-hoc 分析：一年沒人用了
-- 原作者已離職，但檔案還在 repo 裡
CREATE TABLE analysis.old_email_blast AS
SELECT
    email,
    name,
    order_id,
    amount
FROM mart.mart_orders
WHERE order_status = 'completed';
