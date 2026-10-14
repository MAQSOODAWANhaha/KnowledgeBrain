"""Only explicit Python process roles read these deployment settings."""
import os
from dataclasses import dataclass
from pathlib import Path


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
        dsn = os.environ.get("TENDER_DATABASE_URL", "")
        if not dsn:
            raise ValueError("TENDER_DATABASE_URL is required for the independent Python workflow")
        return cls(
            dsn=dsn,
            redis_url=os.environ.get("TENDER_REDIS_URL", "redis://127.0.0.1:16379/1"),
            artifact_root=Path(os.environ.get("TENDER_ARTIFACT_ROOT", "var/python-tender")),
        )
