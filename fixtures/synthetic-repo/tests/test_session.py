from payments.auth import session


def test_login_token_is_valid():
    t = session.login("alice")
    assert session.validate(t)
