//! Game state tracking and prompt construction, backed by `shakmaty`.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use shakmaty::{
    CastlingMode, Chess, Color, EnPassantMode, Move, Position, Role, Square,
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

    /// Human-readable description of a move, used as the Jev option description.
    pub fn describe(&self, m: &Move) -> String {
        let san = San::from_move(&self.pos, *m).to_string();
        let mut after = self.pos.clone();
        after.play_unchecked(*m);

        let mut desc = format!("{san}: {} {}", role_name(m.role()), move_path(m));
        if let Some(captured) = m.capture() {
            desc.push_str(&format!(", captures {}", role_name(captured)));
        }
        if let Some(promo) = m.promotion() {
            desc.push_str(&format!(", promotes to {}", role_name(promo)));
        }
        if m.is_castle() {
            desc.push_str(", castles");
        }
        if after.is_checkmate() {
            desc.push_str(", delivers checkmate");
        } else if after.is_check() {
            desc.push_str(", gives check");
        } else if after.is_stalemate() {
            desc.push_str(", stalemates (draw)");
        }
        desc
    }

    /// Builds the `state` text describing the current position for Jev.
    pub fn describe_state(&self) -> String {
        let side = color_name(self.pos.turn());
        let mut s = String::new();
        s.push_str(&format!(
            "You are a strong chess engine playing {side}. It is {side}'s turn to move.\n\n"
        ));
        s.push_str(&format!(
            "Current position (FEN): {}\n\n",
            fen_of(&self.pos)
        ));
        s.push_str("Board (White pieces uppercase, Black lowercase, rank 8 at top):\n");
        s.push_str(&self.ascii_board());
        s.push('\n');
        if self.history_san.is_empty() {
            s.push_str("No moves have been played yet.\n");
        } else {
            if self.start_fen != fen_of(&Chess::default()) {
                s.push_str(&format!("Game started from FEN: {}\n", self.start_fen));
            }
            s.push_str("Moves so far: ");
            s.push_str(&self.history_san.join(" "));
            s.push('\n');
        }
        if self.pos.is_check() {
            s.push_str(&format!("{side} is in check.\n"));
        }
        s
    }

    pub fn instructions(&self) -> String {
        format!(
            "Which move is best for {} in this position? Pick the move most likely to win the game.",
            color_name(self.pos.turn())
        )
    }

    /// Options for Jev: UCI move -> description.
    pub fn options(&self, moves: &[Move]) -> BTreeMap<String, String> {
        moves
            .iter()
            .map(|m| (Self::to_uci(m), self.describe(m)))
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

fn move_path(m: &Move) -> String {
    match m.from() {
        Some(from) => format!("from {from} to {}", m.to()),
        None => format!("dropped on {}", m.to()),
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
        assert!(g.describe_state().contains("Black's turn"));
    }

    #[test]
    fn parses_fen_and_castling() {
        let g =
            Game::from_uci_position("fen r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1 moves e1g1").unwrap();
        assert_eq!(g.history_san, ["O-O"]);
    }

    #[test]
    fn rejects_illegal_move() {
        assert!(Game::from_uci_position("startpos moves e2e5").is_err());
    }

    #[test]
    fn options_are_uci_keys() {
        let g = Game::from_uci_position("startpos").unwrap();
        let opts = g.options(&g.legal_moves());
        assert!(opts.contains_key("e2e4"));
        assert!(opts["g1f3"].starts_with("Nf3: knight from g1 to f3"));
    }
}
