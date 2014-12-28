use std;

use littlewing::common::*;
use littlewing::bitboard::BitwiseOperations;
use littlewing::fen::FEN;
use littlewing::moves;
use littlewing::moves::Move;
use littlewing::moves::Moves;
use littlewing::moves::MovesOperations;
use littlewing::position::Position;
use littlewing::position::Stack;

pub struct Game {
    board: [Piece, ..64],
    bitboards: [Bitboard, ..14],
    positions: Vec<Position>
}

impl Game {
    fn new() -> Game {
        //moves::Init::knight_sqbb();
        //moves::Init::king_sqbb();
        Game {
            board: [EMPTY, ..64],
            bitboards: [0, ..14],
            positions: Vec::with_capacity(512)
        }
    }

    pub fn from_fen(fen: &str) -> Game {
        let mut game = Game::new();
        let mut fields = fen.words();

        let mut sq = A8;
        for c in fields.next().unwrap().chars() {
            sq += if c == '/' {
                2 * DOWN
            } else if '1' <= c && c <= '8' {
                c.to_digit(10).unwrap()
            } else {
                let p = FEN::decode_piece(c);
                game.board[sq] = p;
                game.bitboards[p].set(sq);
                game.bitboards[p & 1].set(sq); // TODO: p.color()

                1
            };
        }

        let mut position = Position::new();

        position.side = match fields.next().unwrap() {
            "w" => WHITE,
            "b" => BLACK,
            _   => BLACK // FIXME
        };

        game.positions.push(position);
        game
    }

    pub fn to_fen(&self) -> String {
        let mut fen = String::new();
        let mut n = 0u;
        let mut sq = A8;
        loop {
            let p = self.board[sq];

            if p == EMPTY {
                n += 1;
            } else {
                if n > 0 {
                    let c = std::char::from_digit(n, 10).unwrap();
                    fen.push(c);
                    n = 0;
                }
                fen.push(FEN::encode_piece(p));
            }

            if sq == H1 {
                break;
            }

            if sq & H1 == H1 { // TODO: is_file_h!(sq)
                if n > 0 { // TODO: DRY
                    let c = std::char::from_digit(n, 10).unwrap();
                    fen.push(c);
                    n = 0;
                }
                fen.push('/');
                sq += 2 * DOWN;
            }

            sq += RIGHT;
        }

        fen.push(' ');
        if self.positions.top().side == WHITE {
            fen.push('w');
        } else {
            fen.push('b');
        }

        fen
    }

    fn to_string(&self) -> String {
        // FIXME: Testing `map` and `fold` for the lulz

        let sep = range(0u, 8)
            .map(|_| "+---")
            .fold(String::new(), |r, s| r + s) + "+\n";

        String::new() + sep.as_slice() + range(0u, 8).map(|i| {
            range(0u, 8)
                .map(|j| {
                    let p = FEN::encode_piece(self.board[8 * (7 - i) + j]);
                    String::from_chars(['|', ' ', p, ' '].as_slice())
                })
                .fold(String::new(), |r, s| {
                    r + s.as_slice()
                }) + "|\n" + sep.as_slice()
        }).fold(String::new(), |r, s| r + s.as_slice()).as_slice()
    }

    pub fn perft(&mut self, depth: uint) -> uint {
        if depth == 0 {
            return 1
        } else {
            self.generate_moves().iter().fold(0, |r, &m| {
                self.make_move(m);
                let n = self.perft(depth - 1);
                self.undo_move(m);
                r + n
            })
        }
    }

    pub fn make_move(&mut self, m: Move) {
        let &mut position = self.positions.top();

        let piece = self.board[m.from];
        let capture = self.board[m.to]; // TODO: En passant

        self.board[m.from] = EMPTY;
        self.board[m.to] = piece;

        self.bitboards[piece].toggle(m.from);
        self.bitboards[piece].toggle(m.to);
        self.bitboards[position.side].toggle(m.from);
        self.bitboards[position.side].toggle(m.to);
        if capture != EMPTY {
            self.bitboards[capture].toggle(m.to);
            self.bitboards[position.side ^ 1].toggle(m.to);
        }

        position.side ^= 1; // TODO: Define self.side.toggle(0)
        position.capture = capture;

        self.positions.push(position);
    }

    pub fn undo_move(&mut self, m: Move) {
        let piece = self.board[m.to];
        let capture = self.positions.top().capture;

        self.positions.pop();

        self.board[m.from] = piece;
        self.board[m.to] = capture;

        self.bitboards[piece].toggle(m.from);
        self.bitboards[piece].toggle(m.to);
        self.bitboards[self.positions.top().side].toggle(m.from);
        self.bitboards[self.positions.top().side].toggle(m.to);
        if capture != EMPTY {
            self.bitboards[capture].toggle(m.to);
            self.bitboards[self.positions.top().side ^ 1].toggle(m.to);
        }
    }

    pub fn generate_moves(&self) -> Moves {
        let mut moves = Vec::with_capacity(64);

        let bitboards = self.bitboards.as_slice();
        let side = self.positions.top().side;
        moves.add_pawns_moves(bitboards, side);
        moves.add_knights_moves(bitboards, side);
        moves.add_king_moves(bitboards, side);
        moves
    }
}

#[cfg(test)]
mod tests {
    use littlewing::common::*;
    use littlewing::moves::Move;
    use littlewing::position::Stack;
    use super::Game;

    #[test]
    fn test_from_fen() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w";
        let game = Game::from_fen(fen);
        assert_eq!(game.board[E2], WHITE_PAWN);
    }

    #[test]
    fn test_to_fen() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w",
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b",
            "rnbqkbnr/pp1ppppp/8/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R w"
        ];
        for &fen in fens.iter() {
            let game = Game::from_fen(fen);
            assert_eq!(game.to_fen().as_slice(), fen);
        }
    }

    #[test]
    fn test_perft() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w";
        let mut game = Game::from_fen(fen);
        assert_eq!(game.perft(1), 20u);
        assert_eq!(game.perft(1), 20u); // Test reproductivity
        assert_eq!(game.perft(2), 400u);
        //assert_eq!(game.perft(3), 8902u);
    }

    #[test]
    fn test_generate_moves() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w";
        let game = Game::from_fen(fen);
        let moves = game.generate_moves();
        println!("{}", game.to_string());
        assert_eq!(moves.len(), 20);

        // Pawn right capture
        let fen = "8/8/4k3/4p3/3P4/3K4/8/8 b";
        let game = Game::from_fen(fen);
        let moves = game.generate_moves();
        println!("{}", game.to_string());
        assert_eq!(moves.len(), 9);
        let fen = "8/8/4k3/4p3/3P4/3K4/8/8 w";
        let game = Game::from_fen(fen);
        let moves = game.generate_moves();
        println!("{}", game.to_string());
        assert_eq!(moves.len(), 9);

        // Pawn left capture
        let fen = "8/8/2p5/2p1P3/1p1P4/3P4/8/8 w";
        let game = Game::from_fen(fen);
        let moves = game.generate_moves();
        println!("{}", game.to_string());
        assert_eq!(moves.len(), 3);
        let fen = "8/8/2p5/2p1P3/1p1P4/3P4/8/8 b";
        let game = Game::from_fen(fen);
        let moves = game.generate_moves();
        println!("{}", game.to_string());
        assert_eq!(moves.len(), 3);
    }

    #[test]
    fn test_make_move() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w",
            "rnbqkbnr/pppppppp/8/8/8/4P3/PPPP1PPP/RNBQKBNR b"
        ];
        let m = Move::new(E2, E3, QUIET_MOVE);

        let mut game = Game::from_fen(fens[0]);
        assert_eq!(game.to_fen().as_slice(), fens[0]);

        game.make_move(m);
        assert_eq!(game.to_fen().as_slice(), fens[1]);
    }

    #[test]
    fn test_undo_move() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w",
            "rnbqkbnr/pppppppp/8/8/8/4P3/PPPP1PPP/RNBQKBNR b"
        ];
        let m = Move::new(E2, E3, QUIET_MOVE);

        let mut game = Game::from_fen(fens[0]);

        game.make_move(m);
        assert_eq!(game.to_fen().as_slice(), fens[1]);

        game.undo_move(m);
        assert_eq!(game.to_fen().as_slice(), fens[0]);
    }

    #[test]
    fn test_capture() {
        let fens = [
            "r1bqkbnr/1ppp1ppp/p1n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w",
            "r1bqkbnr/1ppp1ppp/p1B5/4p3/4P3/5N2/PPPP1PPP/RNBQK2R b"
        ];
        let m = Move::new(B5, C6, CAPTURE);

        let mut game = Game::from_fen(fens[0]);
        assert_eq!(game.to_fen().as_slice(), fens[0]);
        assert_eq!(game.positions.len(), 1);
        assert_eq!(game.positions.top().capture, EMPTY);
        assert_eq!(game.positions[0].capture, EMPTY);
        assert_eq!(game.positions[0].side, WHITE);

        game.make_move(m);
        assert_eq!(game.to_fen().as_slice(), fens[1]);
        assert_eq!(game.positions.len(), 2);
        assert_eq!(game.positions.top().capture, BLACK_KNIGHT);
        assert_eq!(game.positions[0].capture, EMPTY);
        assert_eq!(game.positions[0].side, WHITE);
        assert_eq!(game.positions[1].capture, BLACK_KNIGHT);
        assert_eq!(game.positions[1].side, BLACK);

        game.undo_move(m);
        assert_eq!(game.to_fen().as_slice(), fens[0]);
        assert_eq!(game.positions.len(), 1);
        assert_eq!(game.positions.top().capture, EMPTY);
        assert_eq!(game.positions[0].capture, EMPTY);
        assert_eq!(game.positions[0].side, WHITE);
    }
}
