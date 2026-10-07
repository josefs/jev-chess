# Tournament log

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

Jev version: runs up to and including 20261004-131810 used `jev-latest`, which
pointed to `jev-1.13.0` at the time: TypeSafe's models page listed it as the
only release, and the API reported `jev-1.13.0` for `jev-latest` on
2026-10-04 at 13:49. The engine did not record the version per move until
later; entries from then on list it after "Jev:".

## 20261003-205141

Engines: baseline, bare, random. Commit 4ef4684 (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 baseline                      168.40     138.41     234.37     152.27         20      72.5%      20.0%      [0, 1, 2, 4, 3]
   2 bare                           -0.00      68.99       0.00     152.27         20      50.0%      60.0%      [0, 2, 6, 2, 0]
   3 random                       -168.40     119.34    -266.18     152.27         20      27.5%      40.0%      [3, 3, 4, 0, 0]
```

Question: does describing the options help at all? baseline (SAN, piece, squares and
capture/check/promotion details) beat bare (UCI moves only) clearly and random
by more. Most draws were early threefold repetitions.

## 20261003-205821

Engines: baseline, safety-only, annotated. Commit 4ef4684 (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 safety-only                   147.19      62.46     289.78     107.67         40      70.0%      20.0%     [0, 1, 4, 13, 2]
   2 annotated                      26.11      74.90      38.26     107.67         40      53.8%      35.0%      [0, 6, 7, 5, 2]
   3 baseline                     -179.45      67.15    -348.89     107.67         40      26.2%      25.0%     [4, 11, 5, 0, 0]
```

Question: which extra move information helps? safety-only (baseline plus a
warning when the moved piece is left undefended and attacked) won clearly.
annotated (capture and check labels up front plus a list of attacked pieces)
was in between: more information is not automatically better.

## 20261003-210456

Engines: safety-only, safety-capture, safety-check, safety-material. Commit 35dc33a (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 safety-capture                 46.60      36.44     114.24      87.91         60      56.7%      70.0%     [0, 1, 21, 7, 1]
   2 safety-check                    5.79      47.05      10.89      87.91         60      50.8%      53.3%     [0, 7, 16, 6, 1]
   3 safety-only                   -11.59      31.96     -31.98      87.91         60      48.3%      73.3%     [0, 5, 22, 3, 0]
   4 safety-material               -40.72      45.37     -80.10      87.91         60      44.2%      63.3%     [2, 6, 19, 3, 0]
```

Question: what to add on top of safety-only? safety-capture (also names the
captured piece) came first, but 70% draws and overlapping error bars make the
differences inconclusive. The PGN showed 97 of 120 games ended in early
threefold repetition, which led to the `{repetition}` label.

## 20261003-211657

Engines: safety-capture, safety-capture-rep, safety-capture@0.5. Commit 4d9f562 (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 safety-capture-rep             43.66      74.01      65.20     107.67         40      56.2%      35.0%      [0, 5, 7, 6, 2]
   2 safety-capture                -17.39      64.12     -29.58     107.67         40      47.5%      45.0%      [1, 5, 9, 5, 0]
   3 safety-capture@0.5            -26.11      61.29     -46.52     107.67         40      46.2%      50.0%     [1, 5, 10, 4, 0]
```

Question: does marking repeating moves fix the draws, and does sampling
(temperature 0.5) help? Adding `{repetition}` cut draws and won; temperature
did not help. safety-capture-rep became the reference engine.

## 20261003-212939

Engines: safety-capture-rep, draw-instr, draw-instr-balance. Commit 6197ec9 (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 draw-instr-balance             34.86      63.18      60.48     107.67         40      55.0%      60.0%     [1, 1, 12, 5, 1]
   2 safety-capture-rep            -17.39      73.07     -26.04     107.67         40      47.5%      55.0%     [2, 3, 11, 3, 1]
   3 draw-instr                    -17.39      59.20     -31.98     107.67         40      47.5%      55.0%     [0, 6, 11, 2, 1]
```

Question: does telling Jev that draws are bad (draw-instr) and showing the
material balance (draw-instr-balance) help? draw-instr-balance won, but draws
stayed high. They were now mostly won positions that were not converted (move
limit, insufficient material, stalemate, 50-move rule), not repetitions.

## 20261003-214738

Engines: draw-instr-balance, draw-instr-balance-mate, draw-instr-balance-check. Commit cf2ad2f (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 draw-instr-balance-mate        88.74      93.40     109.87     107.67         40      62.5%      25.0%      [1, 3, 5, 7, 4]
   2 draw-instr-balance-check       -0.00      85.06       0.00     107.67         40      50.0%      25.0%      [2, 5, 5, 7, 1]
   3 draw-instr-balance            -88.74      66.42    -152.36     107.67         40      37.5%      40.0%      [2, 8, 8, 2, 0]
```

Question: does labelling checkmating (and stalemating) moves help conversion?
`{mate}` made a big difference: draw rate fell to 25%. Adding check labels on
top did not beat mate alone, but the difference is within noise, so this does
not show that check labels hurt. draw-instr-balance-mate became the reference.

## 20261003-221057

Engines: draw-instr-balance-mate, mate-threat, mate-exchange, mate-threat-exchange. Commit 805c7ec (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 mate-exchange                 101.21      72.57     132.01      87.91         60      64.2%      46.7%     [1, 2, 14, 5, 8]
   2 mate-threat-exchange           52.51      78.69      60.65      87.91         60      57.5%      43.3%     [3, 3, 13, 4, 7]
   3 draw-instr-balance-mate       -40.72      85.83     -42.98      87.91         60      44.2%      26.7%      [7, 6, 8, 5, 4]
   4 mate-threat                  -113.94      88.92    -124.35      87.91         60      34.2%      43.3%    [11, 3, 13, 0, 3]
```

Question: does warning about moves that allow mate in one (`{threat}`) and
showing static exchange results for captures (`{exchange}`) help? mate-exchange
won. mate-threat did worse than the reference and walked into mate more often
with the warning than without; the wording "allows checkmate in one" seemed to
attract Jev rather than deter it. Also found the interim-vs-final table
confusion, fixed in 1990ae4.

## 20261003-223135

Engines: mate-exchange, threat-blunder, threat-loses, threat-opponent-wins. Commit 2fd79bd (inferred from the run time).

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 threat-loses                   96.50      76.41     133.19      98.29         48      63.5%      33.3%      [0, 4, 8, 7, 5]
   2 threat-opponent-wins           14.48      89.34      16.31      98.29         48      52.1%      29.2%      [2, 7, 7, 3, 5]
   3 threat-blunder                 -7.24      74.83      -9.66      98.29         48      49.0%      37.5%      [2, 6, 9, 5, 2]
   4 mate-exchange                -104.37      83.80    -133.08      98.29         48      35.4%      50.0%     [8, 1, 12, 3, 0]
```

Question: does rewording the threat warning fix it? Yes: all three rewordings
beat mate-exchange, and avoidable mate-in-one blunders fell from about 15 to
1-3 per engine. threat-loses ("Black loses: White can end the game next move")
was best and became the built-in default in 9b6755a.

## 20261004-131810

Engines: threat-loses, maia1100, maia1300, sf1320. Commit 38dcda9.

```
Rank Name                             Elo        +/-       nElo        +/-      Games      Score       Draw           Ptnml(0-2)
   1 sf1320                           inf        nan        inf        nan         10     100.0%       0.0%      [0, 0, 0, 0, 5]
   2 maia1100                      301.33        nan     429.93     215.34         10      85.0%      20.0%      [0, 0, 1, 1, 3]
   3 maia1300                      190.85     140.99     388.44     215.34         10      75.0%      20.0%      [0, 0, 1, 3, 1]
   4 threat-loses                 -325.17     162.73    -501.83     124.33         30      13.3%      13.3%      [9, 4, 2, 0, 0]
```

Estimated ratings:

```
threat-loses                942 +/- 195  on Maia (Lichess) scale (1.5/10 vs maia1100, 2.5/10 vs maia1300)
threat-loses                     < 1320  on Stockfish UCI_Elo (CCRL Blitz) scale (0/10 vs sf1320)
```

Question: how strong is the default prompt (threat-loses) in absolute terms?
Gauntlet against Maia 1100, Maia 1300 and Stockfish at UCI_Elo 1320, 10 games
each. Estimated about 940 +/- 195 on the Maia (Lichess) scale. It lost all 10
games to Stockfish 1320, so on the CCRL scale it is only known to be below 1320.
The 2.5/10 against Maia 1300 vs 1.5/10 against Maia 1100 is noise. Almost every
loss was a mate after falling behind in material, usually already by move 20,
so losing material in the middlegame, rather than blundering into mate, is now
the main weakness.

## 20261007-225114

Engines: threat-loses, openai:threat-loses. Commit 8b3c0f5. Models: gpt-6-luna, jev-1.13.0.

```
Results of threat-loses vs openai:threat-loses (300+5, NULL, NULL, openings.epd):
Elo: 34.86 +/- 66.06, nElo: 81.89 +/- 152.27
LOS: 85.41 %, DrawRatio: 60.00 %, PairsRatio: 3.00
Games: 20, Wins: 5, Losses: 3, Draws: 12, Points: 11.0 (55.00 %)
Ptnml(0-2): [0, 1, 6, 3, 0], WL/DD Ratio: 0.50
```

Notes: First Jev vs OpenAI Decisions (gpt-6-luna) match, same prompt
(threat-loses, tuned on Jev). Jev ahead 11-9 but inconclusive (LOS 85%);
continued as an SPRT in the next run. OpenAI refused 4 moves (the engine then
plays its first legal move).

## 20261007-230830

Engines: threat-loses, openai:threat-loses. Commit 1968e94. Models: gpt-6-luna, jev-1.13.0.

```
Results of threat-loses vs openai:threat-loses (300+5, NULL, NULL, openings.epd):
Elo: 77.71 +/- 51.18, nElo: 107.75 +/- 68.10
LOS: 99.90 %, DrawRatio: 28.00 %, PairsRatio: 2.27
Games: 100, Wins: 37, Losses: 15, Draws: 48, Points: 61.0 (61.00 %)
Ptnml(0-2): [0, 11, 14, 17, 8], WL/DD Ratio: 0.40
LLR: 3.09 (104.9%) (-2.94, 2.94) [0.00, 30.00]
```

Notes: SPRT [0, 30] between Jev and OpenAI Decisions (gpt-6-luna), same
prompt. H1 accepted after 100 games: Jev is the stronger move picker, about
+78 +/- 51 Elo (61%), with 52 decisive games, mostly by mate.
OpenAI was faster per call (median 0.14 s vs 0.25 s) but had
26 failures in ~10,400 calls (24 refusals, 2 x 503 "Decision inference is
unavailable"), each replaced by the first legal move; too few to explain the
gap. Caveat: the prompt was optimised for Jev, so this compares the models on
Jev's home ground.
