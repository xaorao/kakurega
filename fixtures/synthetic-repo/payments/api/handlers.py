from payments.auth import session


def handle_charge(token: str, amount_cents: int):
    if not session.validate(token):
        return {"status": 401}
    if amount_cents <= 0:
        return {"status": 400}
    return {"status": 200, "charged": amount_cents}


def handle_logout(token: str):
    session.logout(token)
    return {"status": 204}
