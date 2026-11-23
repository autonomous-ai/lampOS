"""`lamp app studio`: a page on this computer for publishing an app. Drag in
the packed tar (or pack a folder with `lamp app pack` first) and its art,
fill in the listing, read the check, sign in with the emailed code, submit,
and follow the review. It calls the same Store API as `lamp app submit`,
through this process, so the token never reaches the page.

It listens on 127.0.0.1 only, under a path with a random secret, and
answers only a request whose Host is that address and, for a change, whose
Origin is the page's own: another site open in the browser cannot reach it.
"""

from __future__ import annotations

import base64
import binascii
import json
import secrets
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from . import app as apps
from .skillcheck import Report
from .store import Endpoints, Store, StoreError, forget_token, kept_token, save_token

# A request's body at most: an 18 MiB tar and two 2 MB PNGs, as base64 in JSON.
MOST_BODY = 32 * 1024 * 1024


def check_bytes(data: bytes) -> dict[str, Any]:
    """The check of a packed app's bytes, as the page shows it."""
    report = Report(Path("app.tar"))
    report.format = apps.FORMAT
    manifest, files = apps.read_tar(data, report)
    if manifest is not None:
        report.errors.extend(apps.manifest_errors(manifest))
        if isinstance(manifest, dict):
            apps._content(manifest, files, report)
    named = manifest if isinstance(manifest, dict) else {}
    return {
        "ok": report.ok,
        "errors": report.errors,
        "warnings": report.warnings,
        "name": named.get("name"),
        "title": named.get("title"),
        "version": named.get("version"),
    }


class Studio:
    """The page's server; `url` is where to open it."""

    def __init__(self, where: Endpoints, *, port: int = 0):
        self.where = where
        self.secret = secrets.token_urlsafe(24)
        studio = self

        class Handler(_Handler):
            owner = studio

        self.server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        self.port = self.server.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}/{self.secret}/"

    def store(self) -> Store:
        return Store.signed_in(where=self.where)

    def serve_forever(self) -> None:
        self.server.serve_forever()

    def start(self) -> threading.Thread:
        thread = threading.Thread(target=self.serve_forever, daemon=True)
        thread.start()
        return thread

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()

    # The page's calls: (method, name) -> answer.
    def answer(self, method: str, name: str, body: dict[str, Any]) -> Any:
        if (method, name) == ("GET", "me"):
            token, email = kept_token(self.where)
            return {"signed_in": bool(token), "email": email, "store": self.where.store}
        if (method, name) == ("POST", "check"):
            return check_bytes(_bytes(body, "file", required=True))
        if (method, name) == ("POST", "login/code"):
            Store(where=self.where).send_code(_text(body, "email"))
            return {"sent": True}
        if (method, name) == ("POST", "login"):
            email = _text(body, "email")
            token = Store(where=self.where).sign_in(email, _text(body, "code"))
            save_token(token, self.where, email)
            return {"signed_in": True, "email": email}
        if (method, name) == ("POST", "logout"):
            return {"signed_out": forget_token(self.where)}
        if (method, name) == ("POST", "submit"):
            package = _bytes(body, "file", required=True)
            checked = check_bytes(package)
            if not checked["ok"]:
                return {"refused_here": True, "check": checked}
            return self.store().submit(
                package,
                icon=_bytes(body, "icon"),
                cover=_bytes(body, "cover"),
                title=body.get("title") or None,
                description=body.get("description") or None,
                category=body.get("category") or None,
            )
        if (method, name) == ("GET", "apps"):
            listed, total = self.store().apps()
            return {"data": listed, "total": total}
        if method == "GET" and name.startswith("apps/"):
            return self.store().app(name.split("/", 1)[1])
        if method == "POST" and name.startswith("apps/") and name.endswith("/request-public"):
            return self.store().request_public(name.split("/")[1]) or {"asked": True}
        raise LookupError(name)


def _text(body: dict[str, Any], key: str) -> str:
    value = body.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{key}: required")
    return value.strip()


def _bytes(body: dict[str, Any], key: str, *, required: bool = False) -> bytes | None:
    value = body.get(key)
    if not value:
        if required:
            raise ValueError(f"{key}: required")
        return None
    try:
        return base64.b64decode(value, validate=True)
    except (binascii.Error, TypeError, ValueError):
        raise ValueError(f"{key}: not base64") from None


class _Handler(BaseHTTPRequestHandler):
    owner: Studio

    def log_message(self, *args: Any) -> None:  # the page's calls are not logged
        pass

    def _send(self, status: int, body: bytes, kind: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Frame-Options", "DENY")
        self.send_header("Content-Security-Policy", "default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:")
        self.end_headers()
        self.wfile.write(body)

    def _json(self, status: int, value: Any) -> None:
        self._send(status, json.dumps(value).encode(), "application/json")

    def _route(self) -> str | None:
        """The call's name under the secret path, or None for any request
        this server does not answer."""
        hosts = {f"127.0.0.1:{self.owner.port}", f"localhost:{self.owner.port}"}
        if self.headers.get("Host") not in hosts:
            return None
        prefix = f"/{self.owner.secret}/"
        if not self.path.startswith(prefix):
            return None
        return self.path[len(prefix) :].split("?", 1)[0]

    def do_GET(self) -> None:  # noqa: N802
        route = self._route()
        if route is None:
            return self._send(404, b"not found", "text/plain")
        if route == "":
            return self._send(200, PAGE.encode(), "text/html; charset=utf-8")
        if route.startswith("api/"):
            return self._api("GET", route[4:], {})
        return self._send(404, b"not found", "text/plain")

    def do_POST(self) -> None:  # noqa: N802
        route = self._route()
        origin = self.headers.get("Origin")
        if route is None or not route.startswith("api/") or origin not in (None, f"http://127.0.0.1:{self.owner.port}", f"http://localhost:{self.owner.port}"):
            return self._send(404, b"not found", "text/plain")
        if self.headers.get("Content-Type", "").split(";")[0] != "application/json":
            return self._json(415, {"error": "a JSON body"})
        length = int(self.headers.get("Content-Length") or 0)
        if length > MOST_BODY:
            return self._json(413, {"error": "too big: an app is 18 MiB at most"})
        try:
            body = json.loads(self.rfile.read(length) or b"{}")
        except ValueError:
            return self._json(400, {"error": "not JSON"})
        self._api("POST", route[4:], body if isinstance(body, dict) else {})

    def _api(self, method: str, name: str, body: dict[str, Any]) -> None:
        try:
            self._json(200, {"ok": True, "data": self.owner.answer(method, name, body)})
        except StoreError as error:
            self._json(200, {"ok": False, "code": error.code, "error": error.why})
        except (ValueError, LookupError) as error:
            self._json(400, {"ok": False, "code": "bad_request", "error": str(error)})


PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Lamp app studio</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
body{font:15px/1.45 -apple-system,system-ui,sans-serif;max-width:760px;margin:2rem auto;padding:0 1rem;color:#1d1d1f}
h1{font-size:1.6rem;margin:0 0 .2rem}h2{font-size:1.05rem;margin:1.6rem 0 .5rem}
section{border:1px solid #d2d2d7;border-radius:12px;padding:1rem 1.2rem;margin:1rem 0}
label{display:block;margin:.5rem 0 .2rem;font-weight:600}input[type=text],input[type=email],textarea{width:100%;box-sizing:border-box;padding:.5rem;border:1px solid #d2d2d7;border-radius:8px;font:inherit}
button{font:inherit;padding:.45rem 1rem;border-radius:8px;border:0;background:#0071e3;color:#fff;cursor:pointer}button:disabled{background:#a1a1a6}
.drop{border:2px dashed #a1a1a6;border-radius:12px;padding:1.2rem;text-align:center;color:#6e6e73}.drop.on{border-color:#0071e3;color:#0071e3}
.err{color:#c9252d}.warn{color:#a05a00}.ok{color:#1a7f37}.muted{color:#6e6e73}ul{margin:.3rem 0;padding-left:1.2rem}
table{width:100%;border-collapse:collapse}td,th{text-align:left;padding:.35rem .3rem;border-bottom:1px solid #eee;vertical-align:top}
</style></head><body>
<h1>Lamp app studio</h1><div class="muted" id="where"></div>
<section id="account"><h2>Your Autonomous account</h2><div id="who"></div>
<div id="signin" hidden><label for="email">Email</label><input type="email" id="email" autocomplete="email">
<p><button id="send">Email me a code</button></p>
<div id="codebox" hidden><label for="code">The code from the email</label><input type="text" id="code" inputmode="numeric" autocomplete="one-time-code">
<p><button id="verify">Sign in</button></p></div></div><div id="accountmsg"></div></section>
<section><h2>1. The app</h2><div class="drop" id="drop">Drop the app's .tar here (from <code>lamp app pack</code>), or <input type="file" id="tar" accept=".tar"></div>
<div id="check"></div></section>
<section><h2>2. The listing</h2>
<label for="title">Title</label><input type="text" id="title" maxlength="100">
<label for="description">Description</label><textarea id="description" rows="4" maxlength="1000"></textarea>
<label for="category">Category</label><input type="text" id="category" placeholder="a Store category slug">
<label for="icon">Icon (PNG, 1024 x 1024)</label><input type="file" id="icon" accept="image/png">
<label for="cover">Cover (PNG, 1600 x 900)</label><input type="file" id="cover" accept="image/png"><div id="art"></div></section>
<section><h2>3. Submit</h2><p class="muted">The Store checks the package again, then a reviewer reads it. It stays hidden until approved; going public is a separate ask.</p>
<button id="submit" disabled>Submit for review</button><div id="submitmsg"></div></section>
<section><h2>Your apps</h2><div id="apps" class="muted">Sign in to see them.</div></section>
<script>
const api=(name,body)=>fetch('api/'+name,body===undefined?{}:{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)}).then(r=>r.json());
const $=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const b64=file=>new Promise((ok,no)=>{const r=new FileReader();r.onload=()=>ok(r.result.split(',')[1]);r.onerror=no;r.readAsDataURL(file)});
let tar=null,checked=null,signedIn=false;
function canSubmit(){$('submit').disabled=!(tar&&checked&&checked.ok&&signedIn)}
async function me(){const a=await api('me');const m=a.data;$('where').textContent='Store: '+m.store;signedIn=m.signed_in;
 $('who').innerHTML=m.signed_in?'Signed in'+(m.email?' as <b>'+esc(m.email)+'</b>':' (a token from LAMP_STORE_TOKEN)')+' <button id="out">Sign out</button>':'';
 $('signin').hidden=m.signed_in;if(m.signed_in){$('out').onclick=async()=>{await api('logout',{});me()};apps()}canSubmit()}
$('send').onclick=async()=>{const a=await api('login/code',{email:$('email').value});$('accountmsg').innerHTML=a.ok?'<span class="ok">A code is on its way to that email.</span>':'<span class="err">'+esc(a.error)+'</span>';if(a.ok)$('codebox').hidden=false};
$('verify').onclick=async()=>{const a=await api('login',{email:$('email').value,code:$('code').value});$('accountmsg').innerHTML=a.ok?'':'<span class="err">'+esc(a.error)+'</span>';me()};
async function takeTar(file){tar=await b64(file);$('check').innerHTML='<p class="muted">Checking…</p>';const a=await api('check',{file:tar});checked=a.ok?a.data:{ok:false,errors:[a.error],warnings:[]};
 const c=checked;let h=c.ok?'<p class="ok">'+esc(c.name)+' '+esc(c.version)+': passes the check.</p>':'<p class="err">Refused here, so nothing is sent:</p>';
 if(c.errors.length)h+='<ul>'+c.errors.map(e=>'<li class="err">'+esc(e)+'</li>').join('')+'</ul>';
 if(c.warnings.length)h+='<ul>'+c.warnings.map(e=>'<li class="warn">'+esc(e)+'</li>').join('')+'</ul>';
 $('check').innerHTML=h;if(c.ok&&!$('title').value&&c.title)$('title').value=c.title;canSubmit()}
$('tar').onchange=e=>e.target.files[0]&&takeTar(e.target.files[0]);
const drop=$('drop');drop.ondragover=e=>{e.preventDefault();drop.classList.add('on')};drop.ondragleave=()=>drop.classList.remove('on');
drop.ondrop=e=>{e.preventDefault();drop.classList.remove('on');const f=e.dataTransfer.files[0];if(f)takeTar(f)};
$('submit').onclick=async()=>{$('submit').disabled=true;$('submitmsg').innerHTML='<p class="muted">Uploading…</p>';
 const body={file:tar,title:$('title').value,description:$('description').value,category:$('category').value};
 if($('icon').files[0])body.icon=await b64($('icon').files[0]);if($('cover').files[0])body.cover=await b64($('cover').files[0]);
 const a=await api('submit',body);
 if(a.ok&&a.data.refused_here)$('submitmsg').innerHTML='<p class="err">The check refused it; nothing was sent.</p>';
 else $('submitmsg').innerHTML=a.ok?'<p class="ok">Submitted: '+esc(a.data.name)+' '+esc(a.data.version)+', review '+esc(a.data.review_status)+'.</p>':'<p class="err">'+esc(a.error)+'</p>';
 canSubmit();apps()};
async function apps(){const a=await api('apps');if(!a.ok){$('apps').innerHTML='<span class="err">'+esc(a.error)+'</span>';return}
 const list=a.data.data;if(!list.length){$('apps').textContent='None yet.';return}
 $('apps').innerHTML='<table><tr><th>App</th><th>Review</th><th>Shown</th><th></th></tr>'+list.map(x=>'<tr><td>'+esc(x.title||x.name)+' '+esc(x.version)+'<div class="muted">'+esc(x.id)+'</div></td><td>'+esc(x.review_status)+(x.review_note?'<div class="muted">'+esc(x.review_note)+'</div>':'')+'</td><td>'+esc(x.visibility||'hidden')+(x.public_requested?' (public asked)':'')+'</td><td>'+(x.review_status==='approved'&&x.visibility!=='public'&&!x.public_requested?'<button data-id="'+esc(x.id)+'">Ask to go public</button>':'')+'</td></tr>').join('')+'</table>';
 for(const b of $('apps').querySelectorAll('button[data-id]'))b.onclick=async()=>{const r=await api('apps/'+encodeURIComponent(b.dataset.id)+'/request-public',{});if(!r.ok)alert(r.error);apps()}}
['icon','cover'].forEach(id=>$(id).onchange=()=>{$('art').textContent=''});
me();
</script></body></html>
"""
