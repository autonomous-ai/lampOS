"""The app as the lamp runs it: `python3 -I -c 'import runpy, sys; sys.path[:0] = argv[1:3]; runpy.run_module(entry, "__main__")'`
with the app's `src` and the SDK folder, on the SDK's fake lamp's app socket. The panel and the day's note come back."""

import asyncio
import subprocess
import sys
from pathlib import Path

from lampos.testing import FakeLamp

HERE = Path(__file__).resolve().parent.parent
SDK = HERE.parents[1] / "python" / "src"
RUN = 'import runpy, sys; sys.path[:0] = sys.argv[1:3]; runpy.run_module(sys.argv[3], run_name="__main__", alter_sys=True)'


def test_the_entry_runs_isolated_and_writes_the_panel_and_note():
    async def go():
        async with FakeLamp(app=True, app_name="lamp-doctor", app_grants=("hear", "speech", "device", "notes")) as fake:
            env = {"LAMP_APP_SOCKET": fake.app_path, "PATH": "/usr/bin:/bin"}
            proc = await asyncio.create_subprocess_exec(
                sys.executable, "-I", "-c", RUN, str(HERE / "src"), str(SDK), "lamp_doctor.app",
                env=env, stderr=asyncio.subprocess.PIPE)
            try:
                for _ in range(100):
                    if fake.app_panel:
                        break
                    await asyncio.sleep(0.1)
                assert fake.app_panel and fake.app_panel["title"].startswith("Lamp Doctor")
                assert len(fake.app_panel["lines"]) <= 6 and all(len(x) <= 80 for x in fake.app_panel["lines"])
                await fake.emit("name.heard", score=0.97)
                await asyncio.sleep(0.2)
                await fake.emit("lamp.said", text="a private sentence", by="lamp", source="reply", cut=False)
                await asyncio.sleep(0.2)
                assert proc.returncode is None
            finally:
                proc.terminate()
                _, err = await proc.communicate()
            assert b"Traceback" not in err, err.decode()

    asyncio.run(asyncio.wait_for(go(), 30))
