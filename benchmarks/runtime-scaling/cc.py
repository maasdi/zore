#!/usr/bin/env python3
import os
from pathlib import Path
import shutil
import subprocess
import sys

for argument in sys.argv[1:]:
    if argument.endswith('.ll') and Path(argument).is_file():
        shutil.copyfile(argument, os.environ['ZORE_SCALING_IR'])
raise SystemExit(subprocess.call([os.environ['ZORE_SCALING_CC'], *sys.argv[1:]]))
