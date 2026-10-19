"""Two-slot switch: which local dlserver slots lbserver sends traffic to.

The state file names one loopback slot per line. One line is the single-slot
switch used by deploy-dlserver.sh; two lines serve from both slots
(dlserver-mode.sh, #612). The scripts write the file and send SIGHUP; lbserver
re-reads it and writes an ack next to it. Only the loopback entry of
LB__BACKENDS is replaced, so remote slave nodes stay in rotation.
"""

import json
import os
from pathlib import Path
from urllib.parse import urlsplit

_LOOPBACK = {"127.0.0.1", "localhost", "::1"}


def is_loopback(url: str) -> bool:
    return (urlsplit(url).hostname or "") in _LOOPBACK


def read_active(state_file: Path) -> list[str]:
    """Active local backend URLs, one per line (line 1 = primary); [] if missing or blank."""
    try:
        text = state_file.read_text()
    except FileNotFoundError:
        return []
    urls: list[str] = []
    for line in text.splitlines():
        url = line.strip().rstrip("/")
        if not url:
            continue
        if not is_loopback(url):
            raise ValueError(f"{state_file}: {url!r} is not a loopback URL")
        if url not in urls:
            urls.append(url)
    return urls


def resolve_backends(configured: list[str], active_local: list[str]) -> list[str]:
    """Replace loopback entries of `configured` with every URL in `active_local`,
    keeping remote ones in order.
    """
    if not active_local or not any(is_loopback(b) for b in configured):
        return list(configured)
    out: list[str] = []
    for backend in configured:
        for chosen in (active_local if is_loopback(backend) else [backend]):
            if chosen not in out:
                out.append(chosen)
    return out


def write_ack(state_file: Path, backends: list[str]) -> None:
    """Record what this process applied; the deploy script trusts only this file."""
    ack = state_file.with_name(state_file.name + ".applied")
    tmp = ack.with_name(ack.name + ".tmp")
    tmp.write_text(json.dumps({"pid": os.getpid(), "backends": backends}))
    tmp.replace(ack)
