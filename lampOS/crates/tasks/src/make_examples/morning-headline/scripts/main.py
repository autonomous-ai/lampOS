import json
import urllib.request

# A public service, no key needed. urllib uses the run's proxy (HTTPS_PROXY)
# on its own. A time starts this skill, so it never says anything itself:
# it prints, and the lamp's agent's answer is what the lamp tells.
TOP = "https://hacker-news.firebaseio.com/v0/topstories.json"
ITEM = "https://hacker-news.firebaseio.com/v0/item/{}.json"


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "lamp-skill"})
    with urllib.request.urlopen(request, timeout=15) as reply:
        return json.load(reply)


try:
    first = fetch(TOP)[0]
    print(f"Top story on Hacker News: {fetch(ITEM.format(first))['title']}")
except Exception as error:
    print(f"Hacker News could not be reached: {error}")
