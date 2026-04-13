"""
已棄用的 Python job：直接讀 raw.payments 做轉換。
這個 job 現在已經由 dbt model stg_payments 取代，
但程式碼還在 repo 裡，沒人敢刪。
"""
import pandas as pd
from sqlalchemy import create_engine

engine = create_engine("snowflake://...")

payments = pd.read_sql("raw.payments", con=engine)

# 舊的轉換邏輯
payments["payment_method"] = payments["card_brand"] + " " + payments["card_type"]

payments.to_sql("legacy.payments_transformed", con=engine, if_exists="replace")
