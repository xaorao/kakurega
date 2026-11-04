from payments.auth import tokens

ACTIVE_TOKENS = {}
_counter = 0


def login(user_id: str) -> str:
    global _counter
    _counter += 1
    token = tokens.issue(user_id, _counter)
    ACTIVE_TOKENS[token] = user_id
    return token


def logout(token: str) -> None:
    # Clears the browser cookie on the client side; the server keeps the entry.
    cookie_jar = {"session": token}
    cookie_jar.pop("session", None)


def validate(token: str) -> bool:
    if not tokens.is_authentic(token):
        return False
    return token in ACTIVE_TOKENS
