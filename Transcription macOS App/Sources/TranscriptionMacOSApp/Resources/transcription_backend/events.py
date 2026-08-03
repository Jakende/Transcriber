from __future__ import annotations

import json
import sys
from typing import Any


def emit(event_type: str, **payload: Any) -> None:
    event = {"type": event_type, **payload}
    print(json.dumps(event, ensure_ascii=False, separators=(",", ":")), flush=True)


def emit_diagnostic(message: str) -> None:
    print(message, file=sys.stderr, flush=True)
