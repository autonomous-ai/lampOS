# A stand-in for lamp-skills' manage helper (U.pass-push-probe): serves two requests on <sock>, records each one, what
# it found in lamp-control's push folder and whether the ANY_ASKER drop-in was laid then, and answers as the helper
# does. install_pushed: the push.json sha256, the request's and package.tar's must agree; the skill's folder is made in
# skills-dev and task-yes.json changed (the yes the push gives). remove_pushed: the folder taken off.
#   fakehelper.py <sock> <push dir> <skills-dev> <drop-in> <task-yes.json> <log>
import hashlib, json, os, shutil, socket, sys
sock, push, dev, ad, yes, logf = sys.argv[1:7]
s = socket.socket(socket.AF_UNIX); s.bind(sock); s.listen(4); s.settimeout(60)
for _ in range(2):
    c, _ = s.accept(); b = b""
    while not b.endswith(b"\n"):
        x = c.recv(65536)
        if not x: break
        b += x
    q = json.loads(b); rec = {"q": q, "dropin": os.path.isfile(ad)}
    if "install_pushed" in q:
        try:
            pj = json.load(open(os.path.join(push, "push.json")))
            tar = hashlib.sha256(open(os.path.join(push, "package.tar"), "rb").read()).hexdigest()
        except Exception as e:
            pj, tar = {"error": str(e)}, None
        rec.update(push=pj, tar=tar)
        if pj.get("sha256") == q["install_pushed"] == tar:
            os.makedirs(os.path.join(dev, pj["name"])); open(yes, "w").write('{"yes":["%s"]}' % pj["name"])
            a = {"done": "installed " + pj["name"]}
        else:
            a = {"refused": "not that push"}
    elif "remove_pushed" in q:
        shutil.rmtree(os.path.join(dev, q["remove_pushed"]["name"]), ignore_errors=True); a = {"done": "removed"}
    else:
        a = {"refused": "no such request"}
    open(logf, "a").write(json.dumps(rec) + "\n"); c.sendall((json.dumps(a) + "\n").encode()); c.close()
