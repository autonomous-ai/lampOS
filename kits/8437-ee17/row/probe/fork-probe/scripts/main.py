import asyncio
import subprocess
import sys

from lampos import Lamp


def spawn(n):
    kids, refused = [], 0
    for _ in range(n):
        try:
            kids.append(subprocess.Popen(["/bin/sleep", "12"]))
        except OSError:
            refused += 1
    return kids, refused


async def main():
    lamp = await Lamp.local()
    kids, refused = spawn(120)
    await lamp.say("Started %d of 120; %d were refused." % (len(kids), refused), wait=True)
    for k in kids:
        k.wait()
    await lamp.close()
    # The run's stderr is never journaled: the exit code tells the row whether the cap held.
    return 0 if refused > 0 else 3


sys.exit(asyncio.run(main()))
