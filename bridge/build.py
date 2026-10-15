"""Compila o hon_bridge.py num executável e põe-no onde o Tauri o procura.

Uso: python bridge/build.py   (com as dependências de bridge/requirements.txt instaladas)
"""

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def rust_target() -> str:
    out = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
    return next(line.split(": ")[1] for line in out.splitlines() if line.startswith("host: "))


def main() -> None:
    # Fica como programa de consola (precisa de stdin/stdout); a app esconde a janela.
    build = ROOT / "bridge" / "build"
    subprocess.run(
        [sys.executable, "-m", "PyInstaller", "--onefile", "--clean",
         "--name", "hon-bridge", "--distpath", str(build / "dist"),
         "--workpath", str(build / "work"), "--specpath", str(build),
         "--collect-data", "pyhon", "--collect-data", "certifi",
         str(ROOT / "bridge" / "hon_bridge.py")],
        check=True,
    )
    ext = ".exe" if sys.platform == "win32" else ""
    target = ROOT / "src-tauri" / "binaries" / f"hon-bridge-{rust_target()}{ext}"
    target.parent.mkdir(exist_ok=True)
    shutil.copy2(build / "dist" / f"hon-bridge{ext}", target)
    print(f"-> {target}")


if __name__ == "__main__":
    main()
