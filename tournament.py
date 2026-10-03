#!/usr/bin/env python3
"""Run a round-robin tournament between jev-chess prompt variants.

Each prompt file becomes one engine. Games are played by fastchess
(https://github.com/Disservin/fastchess), which is downloaded on first use.
Every pair of engines plays each opening twice, once with each colour.

Examples:
    ./tournament.py prompts/baseline.toml --random
    ./tournament.py prompts/a.toml prompts/b.toml prompts/c.toml --rounds 20

The Jev API key is taken from JEV_API_KEY (or --api-key) and passed to the
engines through the environment, so it never appears in logs or PGNs.
"""

import argparse
import datetime
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
FASTCHESS_VERSION = "v1.8.2-alpha"
FASTCHESS_ASSETS = {
    ("Darwin", "arm64"): "fastchess-mac-arm64.tar",
    ("Darwin", "x86_64"): "fastchess-mac-x86-64.tar",
    ("Linux", "x86_64"): "fastchess-linux-x86-64.tar",
    ("Linux", "aarch64"): "fastchess-linux-arm64.tar",
    ("Windows", "AMD64"): "fastchess-windows-x86-64.zip",
}


def die(msg):
    sys.exit(f"error: {msg}")


def fastchess_path(explicit):
    if explicit:
        return Path(explicit).resolve()
    if found := shutil.which("fastchess"):
        return Path(found)

    exe = "fastchess.exe" if platform.system() == "Windows" else "fastchess"
    install_dir = ROOT / "tools" / f"fastchess-{FASTCHESS_VERSION}"
    if existing := next(install_dir.rglob(exe), None) if install_dir.exists() else None:
        return existing

    asset = FASTCHESS_ASSETS.get((platform.system(), platform.machine()))
    if not asset:
        die(f"no fastchess build for {platform.system()} {platform.machine()}; "
            "install it yourself and pass --fastchess")
    url = f"https://github.com/Disservin/fastchess/releases/download/{FASTCHESS_VERSION}/{asset}"
    print(f"Downloading {url}")
    install_dir.mkdir(parents=True, exist_ok=True)
    archive = install_dir / asset
    urllib.request.urlretrieve(url, archive)
    if asset.endswith(".zip"):
        with zipfile.ZipFile(archive) as z:
            z.extractall(install_dir)
    else:
        with tarfile.open(archive) as t:
            t.extractall(install_dir, filter="data")
    archive.unlink()
    exe_path = next(install_dir.rglob(exe), None)
    if not exe_path:
        die(f"{exe} not found in {asset}")
    exe_path.chmod(0o755)
    return exe_path


def build_engine(skip):
    binary = ROOT / "target" / "release" / ("jev-chess.exe" if os.name == "nt" else "jev-chess")
    if not skip:
        cargo = shutil.which("cargo")
        if cargo:
            subprocess.run([cargo, "build", "--release", "--quiet"], cwd=ROOT, check=True)
        elif binary.exists():
            print("warning: cargo not found; using the existing release binary")
        else:
            die("cargo not found and no release binary; run `cargo build --release`")
    if not binary.exists():
        die(f"{binary} does not exist")
    return binary


def main():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("prompts", nargs="*", type=Path, help="prompt files; one engine each")
    p.add_argument("--random", action="store_true",
                   help="add an engine that plays random legal moves, as a reference")
    p.add_argument("--rounds", type=int, default=10,
                   help="openings per pairing; each is played twice with colours swapped (default 10)")
    p.add_argument("--concurrency", type=int, default=4, help="games played in parallel (default 4)")
    p.add_argument("--tc", default="300+5",
                   help="time control per game, seconds+increment (default 300+5)")
    p.add_argument("--maxmoves", type=int, default=150,
                   help="adjudicate a draw after this many moves (default 150)")
    p.add_argument("--openings", type=Path, default=ROOT / "openings.epd", help="EPD opening book")
    p.add_argument("--seed", type=int, help="seed for opening order (default: random)")
    p.add_argument("--out", type=Path, help="results directory (default results/<timestamp>)")
    p.add_argument("--api-key", help="Jev API key (default: $JEV_API_KEY)")
    p.add_argument("--model", help="Jev model for all engines")
    p.add_argument("--api-url", help="Jev endpoint for all engines")
    p.add_argument("--fastchess", help="path to a fastchess binary (default: download it)")
    p.add_argument("--no-build", action="store_true", help="don't run cargo build first")
    args = p.parse_args()

    engines = []  # (name, engine args)
    for prompt in args.prompts:
        if not prompt.is_file():
            die(f"{prompt} is not a file")
        engines.append((prompt.stem, ["--prompt", str(prompt.resolve())]))
    if args.random:
        engines.append(("random", ["--random"]))
    names = [n for n, _ in engines]
    if len(engines) < 2:
        die("need at least two engines (give two prompt files, or one plus --random)")
    if len(set(names)) != len(names):
        die(f"engine names must be unique (they come from the file names): {names}")

    env = dict(os.environ)
    if args.api_key:
        env["JEV_API_KEY"] = args.api_key
    for flag, var in [(args.model, "JEV_MODEL"), (args.api_url, "JEV_API_URL")]:
        if flag:
            env[var] = flag
    env.pop("JEV_CHESS_LOG", None)
    env.pop("JEV_PROMPT", None)
    if args.prompts and not env.get("JEV_API_KEY"):
        die("set JEV_API_KEY or pass --api-key")

    binary = build_engine(args.no_build)
    for prompt in args.prompts:
        check = subprocess.run([binary, "--prompt", prompt, "--show-prompt"],
                               capture_output=True, text=True)
        if check.returncode != 0:
            die(check.stderr.strip())

    fastchess = fastchess_path(args.fastchess)
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    out = (args.out or ROOT / "results" / stamp).resolve()
    out.mkdir(parents=True, exist_ok=True)
    for prompt in args.prompts:
        shutil.copy(prompt, out / f"{prompt.stem}.toml")

    cmd = [
        str(fastchess),
        "-tournament", "roundrobin",
        "-rounds", str(args.rounds),
        "-repeat",
        "-concurrency", str(args.concurrency),
        "-openings", f"file={args.openings.resolve()}", "format=epd", "order=random",
        "-maxmoves", str(args.maxmoves),
        "-pgnout", f"file={out / 'games.pgn'}",
        "-log", f"file={out / 'fastchess.log'}", "level=warn",
        "-recover",
        "-each", f"cmd={binary}", "proto=uci", f"tc={args.tc}",
    ]
    if args.seed is not None:
        cmd += ["-srand", str(args.seed)]
    for name, engine_args in engines:
        engine_args = engine_args + ["--log-file", str(out / f"{name}.log")]
        cmd += ["-engine", f"name={name}", f"args={' '.join(engine_args)}"]

    (out / "command.txt").write_text(" ".join(cmd) + "\n")
    print(f"Engines: {', '.join(names)}")
    print(f"Results: {out}\n")

    # fastchess writes config.json (for resuming) to its working directory.
    with open(out / "summary.txt", "w") as summary:
        proc = subprocess.Popen(cmd, cwd=out, env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, bufsize=1)
        try:
            for line in proc.stdout:
                sys.stdout.write(line)
                summary.write(line)
        except KeyboardInterrupt:
            proc.terminate()
            print(f"\nInterrupted; resume with: cd {out} && {fastchess} -config file=config.json")
        return proc.wait()


if __name__ == "__main__":
    sys.exit(main())
