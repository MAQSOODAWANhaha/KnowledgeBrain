"""Redis transports small commands; PostgreSQL owns receipts and business results."""
import os

import dramatiq
from dramatiq.brokers.redis import RedisBroker
from dramatiq.middleware import AsyncIO

broker = RedisBroker(
    url=os.environ.get("TENDER_REDIS_URL", "redis://127.0.0.1:16379/1"),
    namespace="python-tender",
)
broker.add_middleware(AsyncIO())
dramatiq.set_broker(broker)
