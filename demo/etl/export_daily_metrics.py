"""
舊版 Python job：每天從 mart 表匯出指標到外部系統。
沒人確定這個 job 還有沒有在跑。
"""
import pandas as pd
from sqlalchemy import create_engine

engine = create_engine("snowflake://...")

# 從 mart 表讀取
orders = pd.read_sql("mart.mart_orders", con=engine)
customers = pd.read_sql("mart.mart_customers", con=engine)

# 計算每日指標
daily = orders.groupby("order_date").agg(
    revenue=("amount", "sum"),
    order_count=("order_id", "nunique")
).reset_index()

# 寫到另一個 schema
daily.to_sql("exports.daily_metrics", con=engine, if_exists="replace")
