//! Game state tracking and prompt construction, backed by `shakmaty`.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result, anyhow};

use crate::prompt::{Prompt, Vars};
use shakmaty::{
    Board, CastlingMode, CastlingSide, Chess, Color, EnPassantMode, Move, Position, Role, Square,
    attacks,
    fen::Fen,
    san::{San, SanPlus},
    uci::UciMove,
};

pub struct Game {
    pos: Chess,
    history_san: Vec<String>,
    start_fen: String,
    /// How often each position (FEN without move counters) has occurred.
    seen: HashMap<String, u32>,
}

impl Default for Game {
    fn default() -> Self {
        Self::new(Chess::default())
    }
}

fn fen_of(pos: &Chess) -> String {
    Fen::from_position(pos, EnPassantMode::Legal).to_string()
}

/// Identifies a position for repetition purposes: FEN without the move counters.
fn repetition_key(pos: &Chess) -> String {
    let fen = fen_of(pos);
    fen.rsplitn(3, ' ').last().unwrap_or(&fen).to_string()
}

impl Game {
    fn new(pos: Chess) -> Self {
        Self {
            start_fen: fen_of(&pos),
            seen: HashMap::from([(repetition_key(&pos), 1)]),
            pos,
            history_san: Vec::new(),
        }
    }

    /// Parses the arguments of a UCI `position` command.
    pub fn from_uci_position(args: &str) -> Result<Self> {
        let (setup, moves) = match args.split_once("moves") {
            Some((s, m)) => (s.trim(), m.trim()),
            None => (args.trim(), ""),
        };

        let mut game = if setup == "startpos" {
            Game::default()
        } else if let Some(fen) = setup.strip_prefix("fen") {
            let fen: Fen = fen.trim().parse().context("invalid FEN")?;
            let pos: Chess = fen
                .into_position(CastlingMode::Standard)
                .map_err(|e| anyhow!("{e}"))?;
            Self::new(pos)
        } else {
            return Err(anyhow!("expected 'startpos' or 'fen'"));
        };

        for token in moves.split_whitespace() {
            let uci: UciMove = token.parse().context("invalid UCI move")?;
            let m = uci
                .to_move(&game.pos)
                .map_err(|_| anyhow!("illegal move {token}"))?;
            game.play(m);
        }
        Ok(game)
    }

    fn play(&mut self, m: Move) {
        let san = SanPlus::from_move_and_play_unchecked(&mut self.pos, m);
        self.history_san.push(san.to_string());
        *self.seen.entry(repetition_key(&self.pos)).or_default() += 1;
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        self.pos.legal_moves().into_iter().collect()
    }

    pub fn to_uci(m: &Move) -> String {
        m.to_uci(CastlingMode::Standard).to_string()
    }

    /// Placeholder values describing the position, for `state` and `instructions`.
    pub fn state_vars(&self) -> Vars {
        let side = color_name(self.pos.turn());
        let moves = if self.history_san.is_empty() {
            "(none)".to_string()
        } else {
            self.history_san.join(" ")
        };
        let history = if self.history_san.is_empty() {
            "No moves have been played yet.".to_string()
        } else if self.start_fen != fen_of(&Chess::default()) {
            format!(
                "Game started from FEN: {}\nMoves so far: {moves}",
                self.start_fen
            )
        } else {
            format!("Moves so far: {moves}")
        };
        let check = if self.pos.is_check() {
            format!("{side} is in check.")
        } else {
            String::new()
        };
        let board = self.pos.board();
        let (white, black) = (material(board, Color::White), material(board, Color::Black));
        let balance = format!(
            "Material: White {white}, Black {black} ({}).",
            match white - black {
                0 => "even".to_string(),
                d if d > 0 => format!("White is ahead by {d}"),
                d => format!("Black is ahead by {}", -d),
            }
        );
        Vars::from([
            ("side", side.to_string()),
            ("balance", balance),
            ("opponent", color_name(!self.pos.turn()).to_string()),
            ("fen", fen_of(&self.pos)),
            ("board", self.ascii_board().trim_end().to_string()),
            ("moves", moves),
            ("history", history),
            ("check", check),
        ])
    }

    /// Placeholder values describing one legal move, for `option`.
    pub fn move_vars(&self, m: &Move) -> Vars {
        let san = San::from_move(&self.pos, *m).to_string();
        let mut after = self.pos.clone();
        after.play_unchecked(*m);

        let us = self.pos.turn();

        let capture = m
            .capture()
            .map_or(String::new(), |r| format!(", captures {}", role_name(r)));
        let promotion = m
            .promotion()
            .map_or(String::new(), |r| format!(", promotes to {}", role_name(r)));
        let castling = match m.castling_side() {
            Some(CastlingSide::KingSide) => ", castles kingside",
            Some(CastlingSide::QueenSide) => ", castles queenside",
            None => "",
        };
        let gives_check = if after.is_checkmate() {
            ", delivers checkmate"
        } else if after.is_check() {
            ", gives check"
        } else if after.is_stalemate() {
            ", stalemates (draw)"
        } else {
            ""
        };
        let repetition = match self.seen.get(&repetition_key(&after)).copied() {
            Some(n) if n >= 2 => ", repeats the position a third time (draw by repetition)",
            Some(_) => ", repeats an earlier position",
            None => "",
        };
        let mate = if after.is_checkmate() || after.is_stalemate() {
            gives_check
        } else {
            ""
        };
        let details = format!("{capture}{promotion}{castling}{gives_check}");

        // shakmaty encodes castling as king-takes-rook; use the king's real destination.
        let to = m.castling_side().map_or(m.to(), |side| side.king_to(us));
        let board = after.board();

        let safety = match board.role_at(to) {
            Some(role) if role != Role::King => match exposure(board, to, us) {
                Some(Exposure {
                    attacker,
                    defended: false,
                }) => format!(
                    ", the {} on {to} is undefended and can be captured by a {}",
                    role_name(role),
                    role_name(attacker)
                ),
                Some(Exposure { attacker, .. }) => format!(
                    ", the {} on {to} can be captured by a {}",
                    role_name(role),
                    role_name(attacker)
                ),
                None => String::new(),
            },
            _ => String::new(),
        };

        let hanging: Vec<String> = (board.by_color(us) & !board.kings())
            .into_iter()
            .filter(|&sq| sq != to && exposure(board, sq, us).is_some())
            .map(|sq| format!("{} on {sq}", role_name(board.role_at(sq).unwrap())))
            .collect();
        let hanging = if hanging.is_empty() {
            String::new()
        } else {
            format!(", leaves {} exposed to capture", join_and(&hanging))
        };

        let attacks = match board.piece_at(to) {
            Some(piece) => {
                let targets: Vec<String> = (attacks::attacks(to, piece, board.occupied())
                    & board.by_color(!us)
                    & !board.kings())
                .into_iter()
                .map(|sq| format!("{} on {sq}", role_name(board.role_at(sq).unwrap())))
                .collect();
                if targets.is_empty() {
                    String::new()
                } else {
                    format!(", attacks {}", join_and(&targets))
                }
            }
            None => String::new(),
        };

        let balance = material(board, Color::White) - material(board, Color::Black);
        let material = match balance {
            0 => ", material after: even".to_string(),
            b if b > 0 => format!(", material after: White ahead by {b}"),
            b => format!(", material after: Black ahead by {}", -b),
        };

        Vars::from([
            ("uci", Self::to_uci(m)),
            ("san", san.clone()),
            ("san_plain", san.replace('x', "")),
            ("piece", role_name(m.role()).to_string()),
            ("from", m.from().map_or(String::new(), |sq| sq.to_string())),
            ("to", to.to_string()),
            ("details", details),
            ("capture", capture),
            ("promotion", promotion),
            ("castling", castling.to_string()),
            ("gives_check", gives_check.to_string()),
            ("mate", mate.to_string()),
            ("safety", safety),
            ("hanging", hanging),
            ("attacks", attacks),
            ("material", material),
            ("repetition", repetition.to_string()),
        ])
    }

    /// Options for Jev: UCI move -> description rendered with `prompt`.
    pub fn options(&self, moves: &[Move], prompt: &Prompt) -> BTreeMap<String, String> {
        moves
            .iter()
            .map(|m| (Self::to_uci(m), prompt.option(&self.move_vars(m))))
            .collect()
    }

    fn ascii_board(&self) -> String {
        let board = self.pos.board();
        let mut out = String::new();
        for rank in (0..8).rev() {
            out.push_str(&format!("{} ", rank + 1));
            for file in 0..8 {
                let sq = Square::from_coords(file.try_into().unwrap(), rank.try_into().unwrap());
                out.push(board.piece_at(sq).map_or('.', |p| p.char()));
                out.push(' ');
            }
            out.push('\n');
        }
        out.push_str("  a b c d e f g h\n");
        out
    }
}

fn color_name(c: Color) -> &'static str {
    match c {
        Color::White => "White",
        Color::Black => "Black",
    }
}

struct Exposure {
    /// Cheapest enemy piece that can capture on the square.
    attacker: Role,
    defended: bool,
}

/// Whether `owner`'s piece on `sq` can be won: it is attacked and either
/// undefended or attacked by a cheaper piece. A static one-ply check that
/// ignores pins and x-rays.
fn exposure(board: &Board, sq: Square, owner: Color) -> Option<Exposure> {
    let occupied = board.occupied();
    let defended = board.attacks_to(sq, owner, occupied).any();
    let attacker = board
        .attacks_to(sq, !owner, occupied)
        .into_iter()
        .filter_map(|a| board.role_at(a))
        .filter(|&r| !(defended && r == Role::King))
        .min_by_key(|&r| value(r))?;
    let target = board.role_at(sq)?;
    (!defended || value(attacker) < value(target)).then_some(Exposure { attacker, defended })
}

fn value(r: Role) -> i32 {
    match r {
        Role::Pawn => 1,
        Role::Knight | Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => 100,
    }
}

fn material(board: &Board, color: Color) -> i32 {
    (board.by_color(color) & !board.kings())
        .into_iter()
        .filter_map(|sq| board.role_at(sq))
        .map(value)
        .sum()
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Pawn => "pawn",
        Role::Knight => "knight",
        Role::Bishop => "bishop",
        Role::Rook => "rook",
        Role::Queen => "queen",
        Role::King => "king",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_has_20_moves() {
        let g = Game::from_uci_position("startpos").unwrap();
        assert_eq!(g.legal_moves().len(), 20);
    }

    #[test]
    fn applies_moves() {
        let g = Game::from_uci_position("startpos moves e2e4 e7e5 g1f3").unwrap();
        assert_eq!(g.history_san, ["e4", "e5", "Nf3"]);
        assert_eq!(g.state_vars()["side"], "Black");
        assert_eq!(g.state_vars()["moves"], "e4 e5 Nf3");
    }

    #[test]
    fn parses_fen_and_castling() {
        let g =
            Game::from_uci_position("fen r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1 moves e1g1").unwrap();
        assert_eq!(g.history_san, ["O-O"]);
    }

    #[test]
    fn castling_description_uses_king_destination() {
        let g = Game::from_uci_position("fen r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1").unwrap();
        let opts = g.options(&g.legal_moves(), &Prompt::baseline());
        assert_eq!(opts["e8g8"], "O-O: king from e8 to g8, castles kingside");
        assert_eq!(opts["e8c8"], "O-O-O: king from e8 to c8, castles queenside");
    }

    #[test]
    fn rejects_illegal_move() {
        assert!(Game::from_uci_position("startpos moves e2e5").is_err());
    }

    fn vars(fen: &str, uci: &str) -> Vars {
        let g = Game::from_uci_position(&format!("fen {fen}")).unwrap();
        let m = uci.parse::<UciMove>().unwrap().to_move(&g.pos).unwrap();
        g.move_vars(&m)
    }

    #[test]
    fn safety_flags_piece_moving_into_attack() {
        let v = vars("4k3/8/8/5p2/8/8/8/3QK3 w - - 0 1", "d1g4");
        assert_eq!(
            v["safety"],
            ", the queen on g4 is undefended and can be captured by a pawn"
        );
        let v = vars("4k3/8/8/5p2/8/8/8/3QK3 w - - 0 1", "d1f3");
        assert_eq!(v["safety"], "");
        assert_eq!(v["attacks"], ", attacks pawn on f5");
    }

    #[test]
    fn hanging_lists_other_exposed_pieces() {
        let v = vars("4k3/8/8/8/8/2n5/8/1R2K3 w - - 0 1", "e1f1");
        assert_eq!(v["hanging"], ", leaves rook on b1 exposed to capture");
        let v = vars("4k3/8/8/8/8/2n5/8/1R2K3 w - - 0 1", "b1b8");
        assert_eq!(v["hanging"], "");
        assert_eq!(v["gives_check"], ", gives check");
    }

    #[test]
    fn material_and_plain_san() {
        let v = vars("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "e4d5");
        assert_eq!(v["san"], "exd5");
        assert_eq!(v["san_plain"], "ed5");
        assert_eq!(v["capture"], ", captures pawn");
        assert_eq!(v["material"], ", material after: White ahead by 1");
        assert_eq!(v["details"], ", captures pawn");
    }

    #[test]
    fn mate_flags_only_mate_and_stalemate() {
        let fen = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
        assert_eq!(vars(fen, "a1a8")["mate"], ", delivers checkmate");
        assert_eq!(vars(fen, "a1a7")["mate"], "");
        let fen = "7k/8/6K1/5Q2/8/8/8/8 w - - 0 1";
        assert_eq!(vars(fen, "f5f7")["mate"], ", stalemates (draw)");
        assert_eq!(vars(fen, "f5c8")["mate"], ", delivers checkmate");
        let check = vars(fen, "f5f6");
        assert_eq!(check["gives_check"], ", gives check");
        assert_eq!(check["mate"], "");
    }

    #[test]
    fn balance_in_state() {
        let g = Game::from_uci_position("startpos").unwrap();
        assert_eq!(
            g.state_vars()["balance"],
            "Material: White 39, Black 39 (even)."
        );
        let g = Game::from_uci_position("startpos moves e2e4 d7d5 e4d5").unwrap();
        assert_eq!(
            g.state_vars()["balance"],
            "Material: White 39, Black 38 (White is ahead by 1)."
        );
    }

    #[test]
    fn repetition_is_flagged() {
        let g = Game::from_uci_position("startpos moves g1f3 g8f6 f3g1").unwrap();
        let back = g
            .legal_moves()
            .into_iter()
            .find(|m| Game::to_uci(m) == "f6g8");
        assert_eq!(
            g.move_vars(&back.unwrap())["repetition"],
            ", repeats an earlier position"
        );
        let g =
            Game::from_uci_position("startpos moves g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1").unwrap();
        let vars: Vec<_> = g.legal_moves().iter().map(|m| g.move_vars(m)).collect();
        let third = vars.iter().find(|v| v["uci"] == "f6g8").unwrap();
        assert_eq!(
            third["repetition"],
            ", repeats the position a third time (draw by repetition)"
        );
        assert!(
            vars.iter()
                .filter(|v| v["uci"] != "f6g8")
                .all(|v| v["repetition"].is_empty())
        );
    }

    #[test]
    fn options_are_uci_keys() {
        let g = Game::from_uci_position("startpos").unwrap();
        let opts = g.options(&g.legal_moves(), &Prompt::baseline());
        assert!(opts.contains_key("e2e4"));
        assert!(opts["g1f3"].starts_with("Nf3: knight from g1 to f3"));
    }
}
