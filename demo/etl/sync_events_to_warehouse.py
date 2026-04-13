"""
舊版 Python ETL：把 raw events 清洗後寫回 warehouse。
這個 job 不在 dbt 管理範圍內，是之前的工程師手寫的。
"""
import pandas as pd
from sqlalchemy import create_engine

engine = create_engine("snowflake://...")

# 讀取原始事件
events = pd.read_sql("raw.events", con=engine)

# 讀取使用者資料做 enrichment
users = pd.read_sql("raw.users", con=engine)

# 合併
enriched = events.merge(users[["id", "email", "country"]], left_on="user_id", right_on="id")

# 寫回 warehouse
enriched.to_sql("staging.enriched_events", con=engine, if_exists="replace")
