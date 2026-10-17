"""Only explicit Python process roles read these deployment settings."""
import os
from dataclasses import dataclass
from pathlib import Path


def get_redis_url():
    return (os.environ.get("TENDER_REDIS_URL") or os.environ.get("REDIS_URL")
            or "redis://127.0.0.1:16379/1")


@dataclass(frozen=True)
class Settings:
    dsn: str
    redis_url: str
    artifact_root: Path
    lease_seconds: int = 30
    heartbeat_seconds: float = 5
    redeliver_seconds: int = 60

    @classmethod
    def from_env(cls):
        dsn = os.environ.get("TENDER_DATABASE_URL") or os.environ.get("DATABASE_URL", "")
        if not dsn and not os.environ.get("PGHOST"):
            raise ValueError("Configure DATABASE_URL or PostgreSQL PGHOST connection settings")
        # Empty conninfo lets libpq consume PGHOST/PGUSER/PGPASSWORD directly;
        # passwords are never interpolated into a URL or shell command.
        return cls(
            dsn=dsn,
            redis_url=get_redis_url(),
            artifact_root=Path(os.environ.get("TENDER_ARTIFACT_ROOT", "var/python-tender")),
        )
