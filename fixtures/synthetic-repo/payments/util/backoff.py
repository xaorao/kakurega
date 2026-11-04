import random


def retry_delay(attempt: int) -> float:
    # Jitter for retry timing only. Not used for tokens, keys or any secret.
    base = min(2 ** attempt, 30)
    return base * (0.5 + random.random() / 2)
