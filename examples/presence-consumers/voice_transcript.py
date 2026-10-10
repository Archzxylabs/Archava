"""Standalone standard-library transport; no parent imports, DB access or provider keys."""
import json
import sys
import urllib.request
from urllib.parse import urlparse


def main():
    config = json.load(sys.stdin)
    origin = config["origin"]
    parsed = urlparse(origin)
    if parsed.scheme != "http" or parsed.hostname != "127.0.0.1" or parsed.path:
        raise ValueError("Offline fixture requires a loopback origin")
    request = dict(config["request"])
    request["transport"] = "synthetic-voice-transcript"
    response = urllib.request.urlopen(urllib.request.Request(
        origin + "/api/presence/turn",
        data=json.dumps(request).encode("utf-8"),
        headers={"Content-Type": "application/json", "Origin": origin, "Cookie": config["guest_cookie"]},
        method="POST"), timeout=5)
    result = json.load(response)
    for field in ("version", "sessionId", "requestId", "pageRevision", "transport"):
        if result.get(field) != request[field]:
            raise ValueError("Presence response identity mismatch")
    if result["allowedTools"] != ["public.help"] or result["source"] == "crm":
        raise ValueError("Guest received private projection")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
