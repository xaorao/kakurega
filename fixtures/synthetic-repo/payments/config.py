import os

API_KEY = os.environ["PAYMENTS_API_KEY"]
DATABASE_URL = os.environ.get("DATABASE_URL", "sqlite:///payments.db")
