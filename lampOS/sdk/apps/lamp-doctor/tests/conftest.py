import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
# `lampos` is the SDK beside this app (the lamp keeps it in its own folder and puts it on the path at run).
SDK = HERE.parents[2] / "python" / "src"
sys.path[:0] = [str(HERE), str(HERE.parent / "src"), str(SDK)]
