//! Game state tracking and prompt construction, backed by `shakmaty`.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};

use crate::prompt::{Prompt, Vars};
use shakmaty::{
    CastlingMode, CastlingSide, Chess, Color, EnPassantMode, Move, Position, Role, Square,
    fen::Fen,
    san::{San, SanPlus},
    uci::UciMove,
};

pub struct Game {
    pos: Chess,
    history_san: Vec<String>,
    start_fen: String,
}

impl Default for Game {
    fn default() -> Self {
        let pos = Chess::default();
        Self {
            start_fen: fen_of(&pos),
            pos,
            history_san: Vec::new(),
        }
    }
}

fn fen_of(pos: &Chess) -> String {
    Fen::from_position(pos, EnPassantMode::Legal).to_string()
}

impl Game {
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
            Self {
                start_fen: fen_of(&pos),
                pos,
                history_san: Vec::new(),
            }
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
        Vars::from([
            ("side", side.to_string()),
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

        let mut details = String::new();
        if let Some(captured) = m.capture() {
            details.push_str(&format!(", captures {}", role_name(captured)));
        }
        if let Some(promo) = m.promotion() {
            details.push_str(&format!(", promotes to {}", role_name(promo)));
        }
        match m.castling_side() {
            Some(CastlingSide::KingSide) => details.push_str(", castles kingside"),
            Some(CastlingSide::QueenSide) => details.push_str(", castles queenside"),
            None => {}
        }
        if after.is_checkmate() {
            details.push_str(", delivers checkmate");
        } else if after.is_check() {
            details.push_str(", gives check");
        } else if after.is_stalemate() {
            details.push_str(", stalemates (draw)");
        }

        // shakmaty encodes castling as king-takes-rook; use the king's real destination.
        let to = m
            .castling_side()
            .map_or(m.to(), |side| side.king_to(self.pos.turn()));
        Vars::from([
            ("uci", Self::to_uci(m)),
            ("san", san),
            ("piece", role_name(m.role()).to_string()),
            ("from", m.from().map_or(String::new(), |sq| sq.to_string())),
            ("to", to.to_string()),
            ("details", details),
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

    #[test]
    fn options_are_uci_keys() {
        let g = Game::from_uci_position("startpos").unwrap();
        let opts = g.options(&g.legal_moves(), &Prompt::baseline());
        assert!(opts.contains_key("e2e4"));
        assert!(opts["g1f3"].starts_with("Nf3: knight from g1 to f3"));
    }
}
