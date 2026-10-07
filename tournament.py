#!/usr/bin/env python3
"""Run a round-robin tournament between jev-chess prompt variants.

Each prompt file becomes one engine. Games are played by fastchess
(https://github.com/Disservin/fastchess), which is downloaded on first use.
Every pair of engines plays each opening twice, once with each colour.

Examples:
    ./tournament.py prompts/baseline.toml --random
    ./tournament.py prompts/a.toml prompts/b.toml prompts/c.toml --rounds 20
    ./tournament.py prompts/a.toml prompts/a.toml@0.5 prompts/b.toml --temperature 0.3
    ./tournament.py --archive results/20261003-223135
    ./tournament.py prompts/threat-loses.toml --anchors maia1100,maia1300,maia1500,sf1320
    ./tournament.py prompts/threat-loses.toml openai:prompts/threat-loses.toml

With --anchors, the prompt engines instead play a gauntlet against engines of
known strength (Stockfish limited with UCI_Elo, or the human-like Maia networks
run by lc0), and the script estimates each prompt engine's rating from the
results.

A finished tournament is also archived to tournaments/<timestamp>/ (final table,
prompt files, settings, commit and compressed PGN, but not the large per-engine
logs) and listed in tournaments/README.md, so the record can be committed.

Engines use Jev unless --provider says otherwise; prefix a prompt file with
"jev:" or "openai:" to pick the API for that engine alone (OpenAI engines are
named "openai:<prompt>"). API keys are taken from JEV_API_KEY / OPENAI_API_KEY
(or --api-key / --openai-api-key) and passed to the engines through the
environment, so they never appear in logs or PGNs.
"""

import argparse
import datetime
import gzip
import json
import math
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
ARCHIVE = ROOT / "tournaments"
MAIA_URL = "https://github.com/CSSLab/maia-chess/releases/download/v1.0/maia-{}.pb.gz"
MAIA_LEVELS = range(1100, 2000, 100)
# Stockfish's UCI_Elo is calibrated against CCRL Blitz (2'+1"), so anchors play at that.
STOCKFISH_TC = "120+1"
ANCHOR = re.compile(r"^(sf|maia)(\d+)$")
PROVIDERS = {"jev": "JEV_API_KEY", "openai": "OPENAI_API_KEY"}
SCALES = {"sf": "Stockfish UCI_Elo (CCRL Blitz)", "maia": "Maia (Lichess)"}
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


def maia_weights(level):
    path = ROOT / "tools" / "maia" / f"maia-{level}.pb.gz"
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        print(f"Downloading {MAIA_URL.format(level)}")
        urllib.request.urlretrieve(MAIA_URL.format(level), path)
    return path


def anchor_engines(specs, args):
    """Returns fastchess -engine arguments for anchors such as sf1500 or maia1100."""
    engines = []
    for spec in specs:
        m = ANCHOR.match(spec)
        if not m:
            die(f"unknown anchor {spec!r}; use sfN (N = 1320..3190) or maiaN (N = 1100..1900)")
        kind, level = m[1], int(m[2])
        if kind == "sf":
            if not 1320 <= level <= 3190:
                die(f"{spec}: Stockfish UCI_Elo must be between 1320 and 3190")
            exe = args.stockfish or shutil.which("stockfish")
            if not exe:
                die("stockfish not found; install it (e.g. `brew install stockfish`) or pass --stockfish")
            engines.append([f"name={spec}", f"cmd={exe}", f"tc={STOCKFISH_TC}",
                            "option.UCI_LimitStrength=true", f"option.UCI_Elo={level}",
                            "option.Threads=1", "option.Hash=16"])
        else:
            if level not in MAIA_LEVELS:
                die(f"{spec}: Maia levels are 1100, 1200, ..., 1900")
            exe = args.lc0 or shutil.which("lc0")
            if not exe:
                die("lc0 not found; install it (e.g. `brew install lc0`) or pass --lc0")
            # Maia plays at its rating when it searches a single node.
            engines.append([f"name={spec}", f"cmd={exe}", f"tc={args.tc}", "nodes=1",
                            f"option.WeightsFile={maia_weights(level)}", "option.Threads=1"])
    return engines


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
    p.add_argument("prompts", nargs="*",
                   help="prompt files; one engine each. Append @T to set that engine's "
                        "temperature, e.g. prompts/bare.toml@0.5, and prefix jev: or openai: "
                        "to choose its API, e.g. openai:prompts/bare.toml")
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
    p.add_argument("--temperature", type=float, default=0.0,
                   help="sampling temperature for engines without @T (default 0: always "
                        "play the API's top move)")
    p.add_argument("--provider", choices=PROVIDERS, default="jev",
                   help="API for prompt files without a jev:/openai: prefix (default jev)")
    p.add_argument("--api-key", help="Jev API key (default: $JEV_API_KEY)")
    p.add_argument("--model", help="Jev model for all Jev engines")
    p.add_argument("--api-url", help="Jev endpoint for all Jev engines")
    p.add_argument("--openai-api-key", help="OpenAI API key (default: $OPENAI_API_KEY)")
    p.add_argument("--openai-model", help="OpenAI Decisions model for all OpenAI engines")
    p.add_argument("--openai-url", help="OpenAI Decisions endpoint for all OpenAI engines")
    p.add_argument("--fastchess", help="path to a fastchess binary (default: download it)")
    p.add_argument("--no-build", action="store_true", help="don't run cargo build first")
    p.add_argument("--anchors", type=lambda v: [a for a in v.split(",") if a], default=[],
                   help="play a gauntlet against these reference engines instead of a "
                        "round-robin, e.g. maia1100,maia1500,sf1320 (sfN: Stockfish at "
                        "UCI_Elo N, 1320-3190; maiaN: Maia N, 1100-1900)")
    p.add_argument("--stockfish", help="path to stockfish (default: from PATH)")
    p.add_argument("--lc0", help="path to lc0, used for Maia (default: from PATH)")
    p.add_argument("--no-archive", action="store_true",
                   help="don't copy the finished tournament to tournaments/")
    p.add_argument("--archive", type=Path, metavar="RUN_DIR",
                   help="only archive an existing results directory, then exit")
    args = p.parse_args()

    if args.archive:
        archive(args.archive.resolve())
        return 0

    engines = []  # (name, engine args)
    prompts = []
    providers = set()
    for spec in args.prompts:
        provider, colon, rest = spec.partition(":")
        if not colon or provider not in PROVIDERS:
            provider, rest = args.provider, spec
        path, at, temp = rest.rpartition("@") if "@" in rest else (rest, "", "")
        prompt = Path(path)
        if not prompt.is_file():
            die(f"{prompt} is not a file")
        try:
            temperature = float(temp) if at else args.temperature
        except ValueError:
            die(f"invalid temperature in {spec}")
        if temperature < 0:
            die(f"temperature must be non-negative: {spec}")
        name = ("" if provider == "jev" else f"{provider}:") + prompt.stem + (f"@{temp}" if at else "")
        prompts.append(prompt)
        providers.add(provider)
        engines.append((name, ["--provider", provider, "--prompt", str(prompt.resolve()),
                               "--temperature", str(temperature)]))
    if args.random:
        engines.append(("random", ["--random"]))
    anchors = anchor_engines(args.anchors, args)
    names = [n for n, _ in engines] + args.anchors
    if args.anchors and not engines:
        die("give at least one prompt file (or --random) to play the anchors")
    if len(names) < 2:
        die("need at least two engines (give two prompt files, or one plus --random or --anchors)")
    if len(set(names)) != len(names):
        die(f"engine names must be unique (they come from the provider, file name and @T): {names}")

    env = dict(os.environ)
    for flag, var in [(args.api_key, "JEV_API_KEY"), (args.model, "JEV_MODEL"),
                      (args.api_url, "JEV_API_URL"), (args.openai_api_key, "OPENAI_API_KEY"),
                      (args.openai_model, "OPENAI_DECISIONS_MODEL"),
                      (args.openai_url, "OPENAI_DECISIONS_URL")]:
        if flag:
            env[var] = flag
    for var in ["JEV_CHESS_LOG", "JEV_PROMPT", "JEV_TEMPERATURE", "JEV_PROVIDER"]:
        env.pop(var, None)
    for provider in sorted(providers):
        if not env.get(PROVIDERS[provider]):
            flag = "--api-key" if provider == "jev" else "--openai-api-key"
            die(f"set {PROVIDERS[provider]} or pass {flag}")

    binary = build_engine(args.no_build)
    for prompt in prompts:
        check = subprocess.run([binary, "--prompt", prompt, "--show-prompt"],
                               capture_output=True, text=True)
        if check.returncode != 0:
            die(check.stderr.strip())

    fastchess = fastchess_path(args.fastchess)
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    out = (args.out or ROOT / "results" / stamp).resolve()
    out.mkdir(parents=True, exist_ok=True)
    for prompt in set(prompts):
        shutil.copy(prompt, out / f"{prompt.stem}.toml")
    (out / "commit.txt").write_text(git_commit() + "\n")

    cmd = [
        str(fastchess),
        *(["-tournament", "gauntlet", "-seeds", str(len(engines))] if anchors
          else ["-tournament", "roundrobin"]),
        "-rounds", str(args.rounds),
        "-repeat",
        "-concurrency", str(args.concurrency),
        "-openings", f"file={args.openings.resolve()}", "format=epd", "order=random",
        "-maxmoves", str(args.maxmoves),
        "-pgnout", f"file={out / 'games.pgn'}",
        "-log", f"file={out / 'fastchess.log'}", "level=warn",
        "-recover",
        "-each", "proto=uci",
    ]
    if args.seed is not None:
        cmd += ["-srand", str(args.seed)]
    for name, engine_args in engines:
        engine_args = engine_args + ["--log-file", str(out / f"{log_name(name)}.log")]
        cmd += ["-engine", f"name={name}", f"cmd={binary}", f"tc={args.tc}",
                f"args={' '.join(engine_args)}"]
    for anchor in anchors:
        cmd += ["-engine", *anchor]

    (out / "command.txt").write_text(" ".join(cmd) + "\n")
    print(f"Engines: {', '.join(names)}")
    print(f"Results: {out}\n")

    # fastchess writes config.json (for resuming) to its working directory.
    lines = []
    with open(out / "output.txt", "w") as output:
        proc = subprocess.Popen(cmd, cwd=out, env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, bufsize=1)
        try:
            for line in proc.stdout:
                sys.stdout.write(line)
                output.write(line)
                lines.append(line)
        except KeyboardInterrupt:
            proc.terminate()
            print(f"\nInterrupted; resume with: cd {out} && {fastchess} -config file=config.json")
        status = proc.wait()

    # fastchess also prints interim tables while games are still running, so the
    # last table in the output is the final one.
    table = last_table(lines)
    if table:
        (out / "summary.txt").write_text(table)
        print(f"\n=== Final results ({out / 'summary.txt'}) ===\n{table}", end="")
    config_file = out / "config.json"
    if anchors and config_file.is_file():
        ratings = rating_report(json.loads(config_file.read_text()))
        (out / "ratings.txt").write_text(ratings)
        print(f"\n=== Estimated ratings ({out / 'ratings.txt'}) ===\n{ratings}", end="")
    if status == 0 and not args.no_archive and any(l.startswith("Finished match") for l in lines):
        archive(out)
    return status


def expected(r, opponent):
    return 1 / (1 + 10 ** ((opponent - r) / 400))


def estimate(results):
    """Maximum-likelihood rating from [(anchor rating, points, games)], with a 95% interval.

    Returns (rating, margin), or (None, text) when every game was won or lost."""
    points = sum(p for _, p, _ in results)
    games = sum(n for _, _, n in results)
    if points == 0:
        return None, f"< {min(r for r, _, _ in results)}"
    if points == games:
        return None, f"> {max(r for r, _, _ in results)}"
    lo, hi = -2000.0, 6000.0
    for _ in range(100):
        mid = (lo + hi) / 2
        if sum(n * expected(mid, r) for r, _, n in results) < points:
            lo = mid
        else:
            hi = mid
    info = sum(n * expected(lo, r) * (1 - expected(lo, r)) for r, _, n in results)
    return lo, 1.96 * 400 / math.log(10) / math.sqrt(info)


def rating_report(config):
    """Estimates every non-anchor engine's rating on each anchor scale used."""
    per = {}  # (engine, scale) -> [(anchor rating, points, games, anchor name)]
    for pair, st in config.get("stats", {}).items():
        a, b = pair.split(" vs ")
        games = st["wins"] + st["losses"] + st["draws"]
        for me, other, won, lost in [(a, b, st["wins"], st["losses"]),
                                     (b, a, st["losses"], st["wins"])]:
            m = ANCHOR.match(other)
            if m and not ANCHOR.match(me) and games:
                per.setdefault((me, m[1]), []).append(
                    (int(m[2]), won + st["draws"] / 2, games, other))
    lines = []
    for (engine, scale), results in sorted(per.items()):
        rating, margin = estimate([r[:3] for r in results])
        value = f"{rating:.0f} +/- {margin:.0f}" if rating is not None else margin
        detail = ", ".join(f"{p:g}/{n} vs {name}" for _, p, n, name in sorted(results))
        lines.append(f"{engine:24} {value:>14}  on {SCALES[scale]} scale ({detail})")
    return "\n".join(lines) + "\n" if lines else ""


def log_name(engine):
    return engine.replace(":", "_")


def api_models(run):
    """Returns the versioned model IDs that answered, from the engine logs."""
    found = set()
    for log in run.glob("*.log"):
        with open(log, errors="replace") as f:
            for line in f:
                m = re.search(r"(?:jev|openai) \(([^)]+)\) top moves", line)
                if m and m[1] != "unknown model":
                    found.add(m[1])
    return sorted(found)


def git_commit():
    """Returns HEAD's hash, with "-dirty" if tracked files have local changes."""
    try:
        head = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT,
                              capture_output=True, text=True, check=True).stdout.strip()
        dirty = subprocess.run(["git", "diff", "--quiet", "HEAD"], cwd=ROOT).returncode != 0
        return head + ("-dirty" if dirty else "")
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def commit_at(when):
    """Best guess at the commit checked out at `when`: the last one made before it."""
    out = subprocess.run(["git", "log", "--format=%h %ct", f"--until={when.isoformat()}", "-1"],
                         cwd=ROOT, capture_output=True, text=True).stdout.split()
    return f"{out[0]} (inferred from the run time)" if out else "unknown"


def archive(run):
    """Copies the parts of a results directory worth keeping to tournaments/<name>/."""
    config_file = run / "config.json"
    if not config_file.is_file():
        die(f"{config_file} not found; is {run} a tournament results directory?")
    config = json.loads(config_file.read_text())

    table = ""
    for name in ["output.txt", "summary.txt"]:
        if (run / name).is_file():
            table = last_table((run / name).read_text().splitlines(keepends=True))
            if table:
                break
    if not table:
        die(f"no results table in {run}")

    dest = ARCHIVE / run.name
    if dest.exists():
        shutil.rmtree(dest)
    (dest / "prompts").mkdir(parents=True)
    (dest / "summary.txt").write_text(table)
    for prompt in sorted(run.glob("*.toml")):
        shutil.copy(prompt, dest / "prompts" / prompt.name)
    with open(run / "games.pgn", "rb") as src, \
            gzip.GzipFile(dest / "games.pgn.gz", "wb", mtime=0) as gz:
        shutil.copyfileobj(src, gz)

    try:
        started = datetime.datetime.strptime(run.name, "%Y%m%d-%H%M%S")
    except ValueError:
        started = datetime.datetime.fromtimestamp(config_file.stat().st_ctime)
    commit_file = run / "commit.txt"
    commit = commit_file.read_text().strip() if commit_file.is_file() else commit_at(started)

    engines = []
    for e in config["engines"]:
        if ANCHOR.match(e["name"]):
            engines.append({
                "name": e["name"],
                "anchor": Path(e["cmd"]).name,
                "options": e.get("options", []),
                "nodes": e["limit"]["nodes"] or None,
                "time_control": f"{e['limit']['tc']['time'] / 1000:g}+"
                                f"{e['limit']['tc']['increment'] / 1000:g}",
            })
            continue
        words = e["args"].split()
        opt = lambda flag: words[words.index(flag) + 1] if flag in words else None
        prompt = opt("--prompt")
        engines.append({
            "name": e["name"],
            "provider": None if "--random" in words else opt("--provider") or "jev",
            "prompt": f"prompts/{Path(prompt).name}" if prompt else None,
            "random": "--random" in words,
            "temperature": float(opt("--temperature")) if opt("--temperature") else None,
        })
    tc = config["engines"][0]["limit"]["tc"]
    models = api_models(run)
    run_info = {
        "started": started.isoformat(timespec="seconds"),
        "commit": commit,
        "models": models,
        "engines": engines,
        "settings": {
            "rounds": config["rounds"],
            "games_per_opening": config["games"],
            "time_control": f"{tc['time'] / 1000:g}+{tc['increment'] / 1000:g}",
            "maxmoves": config["maxmoves"]["move_count"] if config["maxmoves"]["enabled"] else None,
            "openings": Path(config["opening"]["file"]).name,
            "seed": config["seed"],
        },
        "pairs": config.get("stats", {}),
    }
    ratings = rating_report(config)
    if ratings:
        run_info["ratings"] = ratings.splitlines()
        (dest / "ratings.txt").write_text(ratings)
    (dest / "run.json").write_text(json.dumps(run_info, indent=2) + "\n")

    index = ARCHIVE / "README.md"
    if not index.exists():
        index.write_text(INDEX_HEADER)
    text = index.read_text()
    heading = f"## {run.name}"
    if heading not in text:
        names = ", ".join(e["name"] for e in engines)
        rows = "".join(l for l in table.splitlines(keepends=True) if not re.match(r"^-+$", l.strip()))
        index.write_text(text.rstrip("\n") + f"\n\n{heading}\n\n"
                         f"Engines: {names}. Commit {commit}. "
                         f"Models: {', '.join(models) or 'not recorded'}.\n\n"
                         f"```\n{rows.strip(chr(10))}\n```\n\n"
                         + (f"Estimated ratings:\n\n```\n{ratings}```\n\n" if ratings else "") +
                         f"Notes: TODO\n")
    print(f"Archived to {dest.relative_to(ROOT)}; add notes to {index.relative_to(ROOT)}")


INDEX_HEADER = """# Tournament log

Every tournament run with `tournament.py`, oldest first. Each directory holds:

- `summary.txt`: the final fastchess table.
- `ratings.txt`: for gauntlets against `--anchors`, the estimated rating of
  each prompt engine.
- `prompts/`: the exact prompt files that played.
- `run.json`: engines, temperatures, settings, the commit the engine was built
  from, the API model versions that answered, and per-pairing win/draw/loss
  counts.
- `games.pgn.gz`: every game (`gunzip -k` to read it).

The per-engine logs with every prompt and API reply are left out because they
are large; they stay in `results/` (not committed).
"""


def last_table(lines):
    """Returns the last block of fastchess output delimited by dashed lines."""
    rules = [i for i, line in enumerate(lines) if line.startswith("-----")]
    if len(rules) < 2:
        return ""
    return "".join(lines[rules[-2]:rules[-1] + 1])


if __name__ == "__main__":
    sys.exit(main())
