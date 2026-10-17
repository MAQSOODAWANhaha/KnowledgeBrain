"""Redis transports small commands; PostgreSQL owns receipts and business results."""
import dramatiq
from dramatiq.brokers.redis import RedisBroker
from dramatiq.middleware import AsyncIO

from docreader.jobs.settings import get_redis_url

broker = RedisBroker(
    url=get_redis_url(),
    namespace="python-tender",
)
broker.add_middleware(AsyncIO())
dramatiq.set_broker(broker)
