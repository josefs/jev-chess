# jev-chess

A chess bot that uses [Jev](https://typesafe.ai)
(TypeSafe AI's "System One" decision model) to pick it's move.

Unsurprisingly, jev-chess plays very poorly. It doesn't do any reasoning, it relies "System One" thinking in the words of typesafe. It's ELO is ~940, measured against maia 1100. The prompt given to jev definitely affects its strength and the prompt templates below is a trace of my attempts at optimizing it.

Note that you will need an API Key from typesafe in order to use jev-chess.

## Configuration

Each setting can be given as a command-line flag or an environment variable
(flags take precedence):

| Flag        | Variable      | Default                                 | Description                     |
| ----------- | ------------- | --------------------------------------- | ------------------------------- |
| `--api-key` | `JEV_API_KEY` | (required)                              | Sent as a Bearer token          |
| `--api-url` | `JEV_API_URL` | `https://api.typesafe.ai/v1/systemone`  | Decision endpoint               |
| `--model`   | `JEV_MODEL`   | `jev-latest`                            | Jev model to use                |
| `--log-file`| `JEV_CHESS_LOG` | (unset)                               | Append UCI traffic and Jev's top-ranked moves to this file |
| `--prompt`  | `JEV_PROMPT`  | built-in `prompts/threat-loses.toml`    | Prompt template file (see below) |
| `--temperature` | `JEV_TEMPERATURE` | `0`                         | `0` plays Jev's most likely move; above 0, samples moves with weight p^(1/T) (`1` = Jev's own probabilities) |

Additional flags:

- `--random` plays uniformly random legal moves without calling Jev (a
  reference opponent for tournaments).
- `--show-prompt [FEN|startpos [moves ...]]` prints the rendered state, instructions and
  options for a position and exits; handy when writing prompt variants.

Flags are convenient for GUIs such as BanksiaGUI that launch the engine with
fixed arguments, e.g. `jev-chess --api-key sk-...`. Note that
command-line arguments are visible to other users via `ps`. If the GUI passes
all arguments as one string, it is split on whitespace. Invalid arguments are
reported on stderr but never stop the engine from speaking UCI.

The same settings are also exposed as UCI options `ApiKey`, `ApiUrl` and
`Model`, configurable from the GUI's engine options dialog. An empty option
value falls back to the flag/environment setting.

## Usage

```sh
cargo build --release
./target/release/jev-chess --api-key ...
```

Register `target/release/jev-chess` as a UCI engine in any chess GUI
(Cute Chess, Arena, BanksiaGUI, ...), or talk to it directly:

```
uci
position startpos moves e2e4
go
```

Before `bestmove` the engine emits `info depth 1 score cp 0 pv <move>` (a
neutral score; Jev doesn't evaluate positions) so that match runners such as
fastchess accept it.

## Prompt templates

The text sent to Jev comes from a TOML template. Copy
[`prompts/baseline.toml`](prompts/baseline.toml) to start a variant; it
documents the available placeholders:

- `state`, `instructions`: `{side}`, `{opponent}`, `{fen}`, `{board}`,
  `{moves}`, `{history}`, `{check}`, `{balance}` (material count)
- `option` (one per legal move):
  - naming the move: `{uci}`, `{san}`, `{san_plain}` (no `x`), `{piece}`,
    `{from}`, `{to}`
  - move facts: `{capture}`, `{promotion}`, `{castling}`, `{gives_check}`, `{mate}` (only checkmate/stalemate), or
    all four as `{details}`; `{exchange}` (net material after recaptures, for
    captures); `{threat}` (the move allows mate in one); `{repetition}` (the
    move returns to an earlier position)
  - static one-ply analysis: `{safety}` (can the moved piece be captured?),
    `{hanging}` (other pieces left exposed), `{attacks}` (enemy pieces the moved
    piece attacks), `{material}` (balance after the move)

Move facts and analysis are empty or start with `, `, so they can be chained.
The wording of the fixed labels (check, checkmate, stalemate, repetition,
threat) can be changed in an optional `[labels]` table; see `baseline.toml`.
Example variants in `prompts/`:

| File | Options |
| --- | --- |
| `baseline.toml` | SAN, squares, captures/checks (the original prompt) |
| `bare.toml` | move name and squares only, no consequences |
| `uci-only.toml` | just the UCI move |
| `safety-only.toml` | bare plus `{safety}` and `{hanging}` warnings |
| `annotated.toml` | baseline plus all analysis placeholders |
| `safety-capture.toml` | safety-only plus `{capture}` |
| `safety-check.toml` | safety-only plus `{gives_check}` |
| `safety-material.toml` | safety-only plus `{material}` |
| `safety-capture-rep.toml` | safety-capture plus `{repetition}` |
| `draw-instr.toml` | safety-capture-rep, instructions say draws are bad unless behind |
| `draw-instr-balance.toml` | draw-instr plus `{balance}` in the state |
| `draw-instr-balance-mate.toml` | draw-instr-balance plus `{mate}` |
| `draw-instr-balance-check.toml` | draw-instr-balance plus `{gives_check}` |
| `mate-threat.toml` | draw-instr-balance-mate plus `{threat}` |
| `mate-exchange.toml` | draw-instr-balance-mate plus `{exchange}` |
| `mate-threat-exchange.toml` | draw-instr-balance-mate plus both |
| `threat-blunder.toml`, `threat-loses.toml`, `threat-opponent-wins.toml` | mate-threat-exchange with the threat label reworded |

Unknown placeholders are rejected when the template is loaded. Use
`jev-chess --prompt my.toml --show-prompt` to preview the result.

## Tournaments

`tournament.py` runs a round-robin between prompt variants using
[fastchess](https://github.com/Disservin/fastchess). The script downloads
fastchess into `tools/` on first use and builds the engine.

```sh
export JEV_API_KEY=...
./tournament.py prompts/baseline.toml prompts/my-variant.toml --random --rounds 5
```

Each prompt file becomes one engine, named after the file stem. `--random`
adds the random-move engine as a reference. Each round is a pair of games
from a random opening in `openings.epd`, with colours swapped.

Append `@T` to a prompt file to give that engine its own temperature, e.g.
`prompts/bare.toml@0.5` (engine name `bare@0.5`); `--temperature` sets it for
the rest. Sampling breaks repetition loops between deterministic engines and
lets repeated openings play out differently.

Main options: `--rounds` (default 10), `--concurrency` (default 4), `--tc`
(default `300+5`), `--maxmoves` (default 150, then adjudicated as a draw),
`--seed`, `--model`, `--api-url` and `--out`. See `./tournament.py --help`.

Results go to `results/<timestamp>/`:

- `summary.txt`: the final Elo table (fastchess also prints interim tables
  while games are still running; the script re-prints the final one at the end)
- `output.txt`: the full fastchess console output
- `games.pgn`
- `fastchess.log`
- one log per engine, with Jev's ranked moves
- copies of the prompts used

`results/` is not committed (the engine logs are large). When a match finishes,
the parts worth keeping are copied to `tournaments/<timestamp>/`: the final
table, the prompt files, the settings and commit (`run.json`), and the games as
`games.pgn.gz`. An entry is also added to
[`tournaments/README.md`](tournaments/README.md), the log of every tournament
so far. Replace its `Notes: TODO` with what the run tested and showed, then
commit. Use `--no-archive` to skip this, or `./tournament.py --archive
results/<timestamp>` to archive an earlier run.

### Rating against anchors

The Elo figures in a round-robin only compare the prompts with each other. To
estimate how strong the bot actually is, play a gauntlet against engines of
known strength with `--anchors`:

```sh
brew install stockfish lc0   # once
./tournament.py prompts/threat-loses.toml --anchors maia1100,maia1300,maia1500,sf1320
```

- `maiaN` (N = 1100, 1200, …, 1900) is [Maia](https://maiachess.com), a neural
  network trained to play like Lichess players rated N, run by lc0 at one node
  per move. The weights are downloaded to `tools/maia/` on first use.
- `sfN` (N = 1320–3190) is Stockfish limited with `UCI_Elo=N`, which is
  calibrated against CCRL Blitz ratings. These anchors always play at 120+1,
  the time control of that calibration.

Each prompt engine plays every anchor, and the anchors don't play each other.
At the end the script estimates each prompt engine's rating (maximum
likelihood, with a 95% interval) and saves it in `ratings.txt`. It gives a
separate figure for the Maia and the Stockfish anchors, because Lichess and
CCRL ratings are different scales. Pick anchors either side of the bot's
strength: if it wins or loses every game, the script can only report a bound
such as `< 1100`.

Every Jev engine makes one API call per move, so a 10-round match between two
prompts costs about 20 games × ~40 calls per side.

## Development

```sh
cargo test
cargo clippy
```
