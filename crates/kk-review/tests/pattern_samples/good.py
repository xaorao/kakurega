import hashlib, subprocess, yaml, requests


def run(user, db, doc):
    eval("1 + 1")
    subprocess.run(["ls", user])
    yaml.load(doc, Loader=yaml.SafeLoader)
    hashlib.sha256(user.encode())
    requests.get("https://x.test")
    db.execute("select * from t where a = ?", (user,))
    api_token = ""
    token_count = 3
