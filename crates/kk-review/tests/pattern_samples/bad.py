import hashlib, os, pickle, subprocess, yaml, requests


def run(user, db, blob, doc):
    eval(user)
    subprocess.run(user, shell=True)
    os.system(user)
    pickle.loads(blob)
    yaml.load(doc)
    hashlib.md5(user.encode())
    requests.get("https://x.test", verify=False)
    db.execute(f"select * from t where a = {user}")
    db.execute("select * from t where a = " + user)
    api_token = "abcdef123456"
