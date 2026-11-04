import hashlib
import hmac
import os

_SECRET = os.environ.get("PAYMENTS_TOKEN_SECRET", "dev-only-secret").encode()


def issue(user_id: str, counter: int) -> str:
    payload = f"{user_id}:{counter}".encode()
    mac = hmac.new(_SECRET, payload, hashlib.sha256).hexdigest()[:32]
    return f"{user_id}.{counter}.{mac}"


def parse(token: str):
    try:
        user_id, counter, mac = token.split(".")
        return user_id, int(counter), mac
    except ValueError:
        return None


def is_authentic(token: str) -> bool:
    parts = parse(token)
    if parts is None:
        return False
    user_id, counter, mac = parts
    expected = hmac.new(_SECRET, f"{user_id}:{counter}".encode(), hashlib.sha256).hexdigest()[:32]
    return hmac.compare_digest(mac, expected)
