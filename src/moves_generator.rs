use color::*;
use piece::*;
use square::*;
use common::*;
use attack::Attack;
use attack::piece_attacks;
use bitboard::BitboardExt;
use game::Game;
use moves::*;
use piece::{PieceAttr, PieceChar};
use square::SquareExt;
use eval::Eval;

lazy_static! {
    // PxP =  7, PxN = 15, PxB = 23, PxR = 31, PxQ = 39, PxK = 47
    // NxP =  6, NxN = 14, NxB = 22, NxR = 30, NxQ = 38, NxK = 46
    // BxP =  5, BxN = 13, BxB = 21, BxR = 29, BxQ = 37, BxK = 45
    // RxP =  4, RxN = 12, RxB = 20, RxR = 28, RxQ = 36, RxK = 44
    // QxP =  3, QxN = 11, QxB = 19, QxR = 27, QxQ = 35, QxK = 43
    // KxP =  2, KxN = 10, KxB = 18, KxR = 26, KxQ = 34, KxK = 42
    pub static ref MVV_LVA_SCORES: [[u8; 13]; 13] = {
        let pieces = vec![EMPTY, PAWN, KNIGHT, BISHOP, ROOK, QUEEN, KING];
        let mut mvv_lva_scores = [[0; 13]; 13];
        for i in 1..7 {
            for j in 1..7 {
                let a = pieces[i as usize];
                let v = pieces[j as usize];
                mvv_lva_scores[a as usize][v as usize] = (8 * j) - i;
            }
        }
        mvv_lva_scores
    };
}

/// Moves generator
pub trait MovesGenerator {
    /// Generate the list of moves from the current game position
    fn generate_moves(&mut self);

    /// Sort the moves list to try the best first
    fn sort_moves(&mut self);

    /// Get the next capture from the moves list (for quiescence search)
    fn next_capture(&mut self) -> Option<Move>;

    /// Get the next move from the moves list
    fn next_move(&mut self) -> Option<Move>;

    /// Make the given move and update the game state
    fn make_move(&mut self, m: Move);

    /// Undo the given move and update the game state
    fn undo_move(&mut self, m: Move);

    /// Get move from the given SAN string
    fn move_from_can(&mut self, s: &str) -> Move;

    /// Get SAN string from the given move
    fn move_to_san(&mut self, m: Move) -> String;
}

trait MovesGeneratorExt {
    fn is_legal_move(&mut self, m: Move) -> bool;
    fn mvv_lva(&self, m: Move) -> u8;
    fn can_king_castle(&mut self, side: Color) -> bool;
    fn can_queen_castle(&mut self, side: Color) -> bool;
    fn can_castle_on(&mut self, side: Color, wing: Piece) -> bool;
}

impl MovesGenerator for Game {
    fn generate_moves(&mut self) {
        match self.moves.stage() {
            MovesStage::KillerMove => {
                if !self.moves.skip_killers {
                    for i in 0..2 {
                        let m = self.moves.get_killer_move(i);

                        if self.is_legal_move(m) {
                            self.moves.add_move(m);
                        }
                    }
                }
            },
            MovesStage::Capture | MovesStage::QuietMove => {
                let &position = self.positions.top();
                let side = position.side;
                let ep = position.en_passant;

                self.moves.add_pawns_moves(&self.bitboards, side, ep);
                self.moves.add_knights_moves(&self.bitboards, side);
                self.moves.add_king_moves(&self.bitboards, side);
                self.moves.add_bishops_moves(&self.bitboards, side);
                self.moves.add_rooks_moves(&self.bitboards, side);
                self.moves.add_queens_moves(&self.bitboards, side);

                if self.moves.stage() == MovesStage::Capture {
                    if !self.moves.skip_ordering {
                        self.sort_moves();
                    }
                } else { // Castlings
                    if self.can_king_castle(side) {
                        self.moves.add_king_castle(side);
                    }
                    if self.can_queen_castle(side) {
                        self.moves.add_queen_castle(side);
                    }
                }
            },
            _ => () // Nothing to do in `BestMove` or `Done` stages
        }
    }

    fn sort_moves(&mut self) {
        let n = self.moves.len();
        for i in 0..n {
            if self.moves[i].item.is_capture() {
                self.moves[i].score = self.mvv_lva(self.moves[i].item);
                if self.see(self.moves[i].item) >= 0 {
                    self.moves[i].score += GOOD_CAPTURE_SCORE;
                }
            }
            for j in 0..i {
                if self.moves[j].score < self.moves[i].score {
                    self.moves.swap(i, j);
                }
            }
        }
    }

    fn next_move(&mut self) -> Option<Move> {
        let mut next_move = self.moves.next();

        // Staged moves generation
        while next_move.is_none() && !self.moves.is_last_stage() {
            self.moves.next_stage();
            self.generate_moves();
            next_move = self.moves.next();
        }

        next_move
    }

    // Specialized version of `next_move` for quiescence search.
    fn next_capture(&mut self) -> Option<Move> {
        if self.moves.stage() == MovesStage::BestMove {
            self.moves.next_stage();
            self.generate_moves();
            debug_assert_eq!(self.moves.stage(), MovesStage::Capture);
        }

        // Skip bad captures
        let i = self.moves.index();
        let n = self.moves.len();
        if i < n {
            if self.moves[i].score < GOOD_CAPTURE_SCORE {
                return None;
            }
        }

        self.moves.next()
    }

    fn make_move(&mut self, m: Move) {
        let &old_position = self.positions.top();
        let mut new_position = old_position;
        let side = old_position.side;

        let piece = self.board[m.from() as usize];
        let capture = self.board[m.to() as usize]; // TODO: En passant

        new_position.halfmoves_count += 1;

        if m.is_null() {
            // TODO: remove duplicate code
            new_position.side ^= 1;
            new_position.hash ^= self.zobrist.side;
            new_position.en_passant = OUT;

            self.positions.push(new_position);
            self.moves.inc();

            return;
        }

        self.bitboards[piece as usize].toggle(m.from());
        self.board[m.from() as usize] = EMPTY;
        new_position.hash ^= self.zobrist.positions[piece as usize][m.from() as usize];

        // Update castling rights
        if piece.kind() == KING {
            if new_position.castling_rights[side as usize][(KING >> 3) as usize] {
                new_position.halfmoves_count = 0;
            }
            if new_position.castling_rights[side as usize][(QUEEN >> 3) as usize] {
                new_position.halfmoves_count = 0;
            }
            new_position.castling_rights[side as usize][(KING >> 3) as usize] = false;
            new_position.castling_rights[side as usize][(QUEEN >> 3) as usize] = false;
            new_position.hash ^= self.zobrist.castling_rights[side as usize][(KING >> 3) as usize];
            new_position.hash ^= self.zobrist.castling_rights[side as usize][(QUEEN >> 3) as usize];
        } else if piece.kind() == ROOK {
            if m.from() == H1.flip(side) {
                if new_position.castling_rights[side as usize][(KING >> 3) as usize] {
                    new_position.halfmoves_count = 0;
                }
                new_position.castling_rights[side as usize][(KING >> 3) as usize] = false;
                new_position.hash ^= self.zobrist.castling_rights[side as usize][(KING >> 3) as usize];
            }
            if m.from() == A1.flip(side) {
                if new_position.castling_rights[side as usize][(QUEEN >> 3) as usize] {
                    new_position.halfmoves_count = 0;
                }
                new_position.castling_rights[side as usize][(QUEEN >> 3) as usize] = false;
                new_position.hash ^= self.zobrist.castling_rights[side as usize][(QUEEN >> 3) as usize];
            }
        } else if piece.kind() == PAWN {
            new_position.halfmoves_count = 0;
        }

        if capture.kind() == ROOK {
            if m.to() == H1.flip(side ^ 1) {
                new_position.castling_rights[(side ^ 1) as usize][(KING >> 3) as usize] = false;
                new_position.hash ^= self.zobrist.castling_rights[(side ^ 1) as usize][(KING >> 3) as usize];
            }
            if m.to() == A1.flip(side ^ 1) {
                new_position.castling_rights[(side ^ 1) as usize][(QUEEN >> 3) as usize] = false;
                new_position.hash ^= self.zobrist.castling_rights[(side ^ 1) as usize][(QUEEN >> 3) as usize];
            }
        }

        if m.is_castle() {
            new_position.halfmoves_count = 0;
            let rook = side | ROOK;

            let (rook_from, rook_to) = if m.castle_kind() == KING {
                (H1.flip(side), F1.flip(side))
            } else {
                (A1.flip(side), D1.flip(side))
            };

            self.board[rook_from as usize] = EMPTY;
            self.board[rook_to as usize] = rook;
            self.bitboards[rook as usize].toggle(rook_from);
            self.bitboards[rook as usize].toggle(rook_to);
            self.bitboards[side as usize].toggle(rook_from);
            self.bitboards[side as usize].toggle(rook_to);
            new_position.hash ^= self.zobrist.positions[rook as usize][rook_from as usize];
            new_position.hash ^= self.zobrist.positions[rook as usize][rook_to as usize];
        }

        if m.is_promotion() {
            new_position.halfmoves_count = 0;
            let promoted_piece = side | m.promotion_kind();
            self.board[m.to() as usize] = promoted_piece;
            self.bitboards[promoted_piece as usize].toggle(m.to());
            new_position.hash ^= self.zobrist.positions[promoted_piece as usize][m.to() as usize];
        } else {
            self.board[m.to() as usize] = piece;
            self.bitboards[piece as usize].toggle(m.to());
            new_position.hash ^= self.zobrist.positions[piece as usize][m.to() as usize];
        }

        new_position.en_passant = if m.kind() == DOUBLE_PAWN_PUSH {
            ((((m.from().flip(side)) as Direction) + UP) as Square).flip(side)
        } else {
            OUT
        };

        if old_position.en_passant != OUT {
            new_position.hash ^= self.zobrist.en_passant[old_position.en_passant as usize]; // TODO ?
        }
        if new_position.en_passant != OUT {
            new_position.hash ^= self.zobrist.en_passant[new_position.en_passant as usize];
        }

        self.bitboards[side as usize].toggle(m.from());
        self.bitboards[side as usize].toggle(m.to());

        // if m.is_capture() {
        if capture != EMPTY {
            new_position.halfmoves_count = 0;
            self.bitboards[capture as usize].toggle(m.to());
            self.bitboards[(side ^ 1) as usize].toggle(m.to());
            new_position.hash ^= self.zobrist.positions[capture as usize][m.to() as usize];
        }

        if m.kind() == EN_PASSANT {
            new_position.halfmoves_count = 0;
            let square = (((m.to().flip(side) as Direction) + DOWN) as Square).flip(side);
            self.board[square as usize] = EMPTY;
            self.bitboards[(side ^ 1 | PAWN) as usize].toggle(square);
            self.bitboards[(side ^ 1) as usize].toggle(square);
            new_position.hash ^= self.zobrist.positions[(side ^ 1 | PAWN) as usize][square as usize];
        }

        // FIXME
        new_position.side ^= 1; // TODO: Define self.side.toggle(0)
        new_position.capture = capture;
        new_position.hash ^= self.zobrist.side;

        self.positions.push(new_position);
        self.moves.inc();
    }

    fn undo_move(&mut self, m: Move) {
        let piece = self.board[m.to() as usize];
        let capture = self.positions.top().capture;

        self.positions.pop();
        self.moves.dec();

        if m.is_null() {
            return;
        }

        let &position = self.positions.top();
        let side = position.side;

        if m.is_castle() {
            let rook = side | ROOK;

            let (rook_from, rook_to) = if m.castle_kind() == KING {
                (H1.flip(side), F1.flip(side))
            } else {
                (A1.flip(side), D1.flip(side))
            };

            self.board[rook_from as usize] = rook;
            self.board[rook_to as usize] = EMPTY;
            self.bitboards[rook as usize].toggle(rook_from);
            self.bitboards[rook as usize].toggle(rook_to);
            self.bitboards[side as usize].toggle(rook_from);
            self.bitboards[side as usize].toggle(rook_to);
        }

        if m.is_promotion() {
            let pawn = position.side | PAWN;
            self.board[m.from() as usize] = pawn;
            self.bitboards[pawn as usize].toggle(m.from());
        } else {
            self.board[m.from() as usize] = piece;
            self.bitboards[piece as usize].toggle(m.from());
        }

        if m.kind() == EN_PASSANT {
            let square = (((m.to().flip(side) as Direction) + DOWN) as Square).flip(side);
            self.board[square as usize] = side ^ 1 | PAWN;
            self.bitboards[(side ^ 1 | PAWN) as usize].toggle(square);
            self.bitboards[(side ^ 1) as usize].toggle(square);
        }

        self.board[m.to() as usize] = capture;
        self.bitboards[piece as usize].toggle(m.to());

        self.bitboards[position.side as usize].toggle(m.from());
        self.bitboards[position.side as usize].toggle(m.to());

        if capture != EMPTY {
            self.bitboards[capture as usize].toggle(m.to());
            self.bitboards[(position.side ^ 1) as usize].toggle(m.to());
        }
    }

    fn move_from_can(&mut self, s: &str) -> Move {
        debug_assert!(s.len() == 4 || s.len() == 5);

        let side = self.positions.top().side;
        let (a, b) = s.split_at(2);
        let from = Square::from_coord(String::from(a));
        let to = Square::from_coord(String::from(b));
        let piece = self.board[from as usize];
        let capture = self.board[to as usize];

        let mt = if s.len() == 5 {
            let promotion = match s.chars().nth(4) {
                Some('n') => KNIGHT_PROMOTION,
                Some('b') => BISHOP_PROMOTION,
                Some('r') => ROOK_PROMOTION,
                Some('q') => QUEEN_PROMOTION,
                _         => panic!("could not parse promotion")
            };
            if capture == EMPTY {
                promotion
            } else {
                promotion | CAPTURE
            }
        } else if piece.kind() == KING && from == E1.flip(side) && to == G1.flip(side) {
            KING_CASTLE
        } else if piece.kind() == KING && from == E1.flip(side) && to == C1.flip(side) {
            QUEEN_CASTLE
        } else if capture == EMPTY {
            let d = (to.flip(side) as Direction) - (from.flip(side) as Direction);
            if piece.kind() == PAWN && (d == 2 * UP) {
                DOUBLE_PAWN_PUSH
            } else if piece.kind() == PAWN && to == self.positions.top().en_passant {
                EN_PASSANT
            } else {
                QUIET_MOVE
            }
        } else {
            CAPTURE
        };

        Move::new(from, to, mt)
    }

    // NOTE: this function assumes that the move has not been played yet
    fn move_to_san(&mut self, m: Move) -> String {
        let piece = self.board[m.from() as usize];

        let mut out = String::new();

        if m.is_castle() {
            if m.castle_kind() == KING {
                out.push_str("O-O");
            } else {
                out.push_str("O-O-O");
            }
            return out;
        }

        if piece.kind() != PAWN {
            out.push(piece.kind().to_char());
        }

        // Piece disambiguation or pawn capture
        if piece.kind() != PAWN || m.is_capture() {
            let occupied = self.bitboards[piece as usize];
            let attackers = piece_attacks(piece, m.to(), occupied) & occupied;
            if attackers.count() > 1 || piece.kind() == PAWN {
                let rank = m.from().to_coord().as_str().chars().nth(0).unwrap();
                out.push(rank);
            }
            // TODO: Pawn disambiguation
        }

        if m.is_capture() {
            out.push('x');
        }

        out.push_str(m.to().to_coord().as_str());

        if m.is_promotion() {
            out.push('=');
            out.push(m.promotion_kind().to_char());
        }

        out
    }
}

impl MovesGeneratorExt for Game {
    fn can_castle_on(&mut self, side: Color, wing: Piece) -> bool {
        match wing {
            QUEEN => self.can_queen_castle(side),
            KING  => self.can_king_castle(side),
            _     => unreachable!()
        }
    }

    fn can_king_castle(&mut self, side: Color) -> bool {
        let &position = self.positions.top();
        let occupied = self.bitboards[WHITE as usize] | self.bitboards[BLACK as usize];
        let mask = CASTLING_MASKS[side as usize][(KING >> 3) as usize];

        !occupied & mask == mask &&
        self.board[E1.flip(side) as usize] == side | KING &&
        self.board[H1.flip(side) as usize] == side | ROOK &&
        position.has_castling_right_on(side, KING) &&
        !self.is_attacked(E1.flip(side), side) &&
        !self.is_attacked(F1.flip(side), side) &&
        !self.is_attacked(G1.flip(side), side) // TODO: Duplicate with is_check() ?
    }

    fn can_queen_castle(&mut self, side: Color) -> bool {
        let &position = self.positions.top();
        let occupied = self.bitboards[WHITE as usize] | self.bitboards[BLACK as usize];
        let mask = CASTLING_MASKS[side as usize][(QUEEN >> 3) as usize];

        !occupied & mask == mask &&
        self.board[E1.flip(side) as usize] == side | KING &&
        self.board[A1.flip(side) as usize] == side | ROOK &&
        position.has_castling_right_on(side, QUEEN) &&
        !self.is_attacked(E1.flip(side), side) &&
        !self.is_attacked(D1.flip(side), side) &&
        !self.is_attacked(C1.flip(side), side)
    }

    // Pseudo legal move checker (limited to moves generated by the engine)
    fn is_legal_move(&mut self, m: Move) -> bool {
        if m.is_null() {
            return false;
        }

        let &position = self.positions.top();
        let side = position.side;

        let p = self.board[m.from() as usize];

        // There must be a piece to play
        if p == EMPTY {
            return false;
        }
        if p.color() != side {
            return false;
        }

        if m.is_promotion() || m.kind() == DOUBLE_PAWN_PUSH {
            if p.kind() != PAWN {
                return false;
            }
        }

        if m.is_en_passant() {
            if p.kind() != PAWN {
                return false;
            }

            if m.to() != position.en_passant {
                return false;
            }

            return true;
        }

        if m.is_castle() {
            let wing = m.castle_kind();

            return self.can_castle_on(side, wing);
        }

        // The piece must be able to reach its destination
        let pieces = self.bitboards[side as usize];
        let targets = self.bitboards[(side ^ 1) as usize];
        let occupied = pieces | targets;
        let attacks = piece_attacks(p, m.from(), occupied);

        if m.is_capture() {
            (attacks & targets).get(m.to())
        } else if p.kind() == PAWN {
            let d = YDIRS[side as usize];
            let mut s = m.from();

            s = ((s as Direction) + d) as Square;
            if m.kind() == DOUBLE_PAWN_PUSH {
                if occupied.get(s) {
                    return false;
                }
                s = ((s as Direction) + d) as Square;
            }

            if m.to() != s {
                return false;
            }

            if (RANK_1 | RANK_8).get(s) {
                if !m.is_promotion() {
                    return false;
                }
            }

            !occupied.get(m.to())
        } else {
            (attacks & !occupied).get(m.to())
        }
    }

    fn mvv_lva(&self, m: Move) -> u8 {
        let a = self.board[m.from() as usize].kind();
        let v = if m.is_en_passant() {
            PAWN
        } else {
            self.board[m.to() as usize].kind()
        };

        MVV_LVA_SCORES[a as usize][v as usize]
    }
}

#[cfg(test)]
mod tests {
    use color::*;
    use piece::*;
    use common::*;
    use moves::Move;
    use fen::FEN;
    use game::Game;
    use super::*;

    fn perft(fen: &str) -> usize {
        let mut game = Game::from_fen(fen);

        game.moves.next_stage();
        game.generate_moves(); // Captures

        game.moves.next_stage(); // Killer Moves

        game.moves.next_stage();
        game.generate_moves(); // Quiet moves

        game.moves.len()
    }

    #[test]
    fn test_generate_moves() {
        let fen = DEFAULT_FEN;
        assert_eq!(perft(fen), 20);

        // Pawn right capture
        let fen = "8/8/4k3/4p3/3P4/3K4/8/8 b - -";
        assert_eq!(perft(fen), 9);

        let fen = "8/8/4k3/4p3/3P4/3K4/8/8 w - -";
        assert_eq!(perft(fen), 9);

        // Pawn left capture
        let fen = "8/8/2p5/2p1P3/1p1P4/3P4/8/8 w - -";
        assert_eq!(perft(fen), 3);

        let fen = "8/8/2p5/2p1P3/1p1P4/3P4/8/8 b - -";
        assert_eq!(perft(fen), 3);

        // Bishop
        let fen = "8/8/8/8/3B4/8/8/8 w - -";
        assert_eq!(perft(fen), 13);

        // Rook
        let fen = "8/8/8/8/1r1R4/8/8/8 w - -";
        assert_eq!(perft(fen), 13);
    }

    #[test]
    fn test_make_move() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/4P3/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        ];
        let m = Move::new(E2, E3, QUIET_MOVE);

        let mut game = Game::from_fen(fens[0]);
        assert_eq!(game.to_fen().as_str(), fens[0]);

        game.make_move(m);
        assert_eq!(game.to_fen().as_str(), fens[1]);
    }

    #[test]
    fn test_undo_move() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/4P3/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        ];
        let m = Move::new(E2, E3, QUIET_MOVE);

        let mut game = Game::from_fen(fens[0]);

        game.make_move(m);
        assert_eq!(game.to_fen().as_str(), fens[1]);

        game.undo_move(m);
        assert_eq!(game.to_fen().as_str(), fens[0]);
    }

    #[test]
    fn test_capture() {
        let fens = [
            "r1bqkbnr/1ppp1ppp/p1n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
            "r1bqkbnr/1ppp1ppp/p1B5/4p3/4P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 0 1"
        ];
        let m = Move::new(B5, C6, CAPTURE);

        let mut game = Game::from_fen(fens[0]);
        assert_eq!(game.to_fen().as_str(), fens[0]);
        assert_eq!(game.positions.len(), 1);
        assert_eq!(game.positions.top().capture, EMPTY);
        assert_eq!(game.positions[0].capture, EMPTY);
        assert_eq!(game.positions[0].side, WHITE);

        game.make_move(m);
        assert_eq!(game.to_fen().as_str(), fens[1]);
        assert_eq!(game.positions.len(), 2);
        assert_eq!(game.positions.top().capture, BLACK_KNIGHT);
        assert_eq!(game.positions[0].capture, EMPTY);
        assert_eq!(game.positions[0].side, WHITE);
        assert_eq!(game.positions[1].capture, BLACK_KNIGHT);
        assert_eq!(game.positions[1].side, BLACK);

        game.undo_move(m);
        assert_eq!(game.to_fen().as_str(), fens[0]);
        assert_eq!(game.positions.len(), 1);
        assert_eq!(game.positions.top().capture, EMPTY);
        assert_eq!(game.positions[0].capture, EMPTY);
        assert_eq!(game.positions[0].side, WHITE);
    }

    #[test]
    fn test_mvv_lva() {
        let mut game = Game::from_fen("8/8/8/8/8/1Qn5/1PpK1k2/8 w - - 0 1");

        assert_eq!(game.mvv_lva(Move::new(B2, C3, CAPTURE)), 15); // PxN
        assert_eq!(game.mvv_lva(Move::new(B3, C3, CAPTURE)), 11); // QxN
        assert_eq!(game.mvv_lva(Move::new(D2, C3, CAPTURE)), 10); // KxN
        assert_eq!(game.mvv_lva(Move::new(B3, C2, CAPTURE)),  3); // QxP
        assert_eq!(game.mvv_lva(Move::new(D2, C2, CAPTURE)),  2); // KxP

        game.moves.next_stage(); // Captures
        game.generate_moves();
        game.moves.next_stage(); // Killer moves
        game.moves.next_stage(); // Quiet moves
        game.generate_moves();

        assert_eq!(game.moves.next(), Some(Move::new(B2, C3, CAPTURE)));
        assert_eq!(game.moves.next(), Some(Move::new(B3, C3, CAPTURE)));
        assert_eq!(game.moves.next(), Some(Move::new(D2, C3, CAPTURE)));
        assert_eq!(game.moves.next(), Some(Move::new(B3, C2, CAPTURE)));
        assert_eq!(game.moves.next(), Some(Move::new(D2, C2, CAPTURE)));

        assert!(!game.moves.next().unwrap().is_capture());
    }

    #[test]
    fn test_move_from_can() {
        let mut game = Game::from_fen(DEFAULT_FEN);

        let m = game.move_from_can("e2e4");
        assert_eq!(m, Move::new(E2, E4, DOUBLE_PAWN_PUSH));

        let m = game.move_from_can("g1f3");
        assert_eq!(m, Move::new(G1, F3, QUIET_MOVE));
    }

    #[test]
    fn test_make_move_update_halfmoves_count() {
        let fen = "7r/k7/7p/r2p3P/p2PqB2/2R3P1/5K2/3Q3R w - - 25 45";
        let mut game = Game::from_fen(fen);

        assert_eq!(game.positions.top().halfmoves_count, 25);

        game.make_move(Move::new(F2, G1, QUIET_MOVE));
        assert_eq!(game.positions.top().halfmoves_count, 26);

        game.make_move(Move::new(A7, B7, QUIET_MOVE));
        assert_eq!(game.positions.top().halfmoves_count, 27);

        game.make_move(Move::new(F4, H6, CAPTURE));
        assert_eq!(game.positions.top().halfmoves_count, 0);
    }

    #[test]
    fn test_move_to_san() {
        let fen = "7k/3P1ppp/4PQ2/8/8/8/8/6RK w - - 0 1";
        let mut game = Game::from_fen(fen);

        // NOTE: this move should really end with `#`, but this is done
        // in `search::get_pv()`.
        assert_eq!(game.move_to_san(Move::new(F6, G7, CAPTURE)), "Qxg7");
    }

    #[test]
    fn test_next_move() {
        let fen = "k1K5/8/8/8/8/1p6/2P5/N7 w - - 0 1";
        let mut game = Game::from_fen(fen);

        game.moves.add_move(Move::new(C2, C3, QUIET_MOVE)); // Best move

        assert_eq!(game.next_move(), Some(Move::new(C2, C3, QUIET_MOVE)));
        assert_eq!(game.next_move(), Some(Move::new(C2, B3, CAPTURE)));
        assert_eq!(game.next_move(), Some(Move::new(A1, B3, CAPTURE)));
        assert_eq!(game.next_move(), Some(Move::new(C2, C4, DOUBLE_PAWN_PUSH)));
        assert_eq!(game.next_move(), Some(Move::new(C8, B7, QUIET_MOVE))); // Illegal
        assert_eq!(game.next_move(), Some(Move::new(C8, C7, QUIET_MOVE)));
        assert_eq!(game.next_move(), Some(Move::new(C8, D7, QUIET_MOVE)));
        assert_eq!(game.next_move(), Some(Move::new(C8, B8, QUIET_MOVE))); // Illegal
        assert_eq!(game.next_move(), Some(Move::new(C8, D8, QUIET_MOVE)));
        assert_eq!(game.next_move(), None);
    }

    #[test]
    fn test_next_capture() {
        let fen = "k1K5/8/8/8/8/1p6/2P5/N7 w - - 0 1";
        let mut game = Game::from_fen(fen);

        assert_eq!(game.next_capture(), Some(Move::new(C2, B3, CAPTURE)));
        assert_eq!(game.next_capture(), Some(Move::new(A1, B3, CAPTURE)));
        assert_eq!(game.next_capture(), None);

        let fen = "k1K5/8/2p1N3/1p6/2rp1n2/1P2P3/3Q4/8 w - - 0 1";
        let mut game = Game::from_fen(fen);

        let b3c4 = Move::new(B3, C4, CAPTURE);
        let e3f4 = Move::new(E3, F4, CAPTURE);
        let e6f4 = Move::new(E6, F4, CAPTURE);
        let e3d4 = Move::new(E3, D4, CAPTURE);
        let e6d4 = Move::new(E6, D4, CAPTURE);
        let d2d4 = Move::new(D2, D4, CAPTURE);
        println!("{}: {}", b3c4, game.see(b3c4));
        println!("{}: {}", e3f4, game.see(e3f4));
        println!("{}: {}", e6f4, game.see(e6f4));
        println!("{}: {}", e3d4, game.see(e3d4));
        println!("{}: {}", e6d4, game.see(e6d4));
        println!("{}: {}", d2d4, game.see(d2d4));
        assert_eq!(game.next_capture(), Some(b3c4));
        assert_eq!(game.next_capture(), Some(e3f4));
        assert_eq!(game.next_capture(), Some(e6f4));
        assert_eq!(game.next_capture(), Some(e3d4));
        assert_eq!(game.next_capture(), Some(e6d4));
        //assert_eq!(game.next_capture(), Some(d2d4)); // Skip bad capture
        assert_eq!(game.next_capture(), None);
    }

    #[test]
    fn test_is_legal_move() {
        let fen = "k1K5/8/8/8/8/1p6/2P5/N7 w - - 0 1";
        let mut game = Game::from_fen(fen);

        assert!(game.is_legal_move(Move::new(C2, C3, QUIET_MOVE)));
        assert!(game.is_legal_move(Move::new(C2, B3, CAPTURE)));
        assert!(game.is_legal_move(Move::new(A1, B3, CAPTURE)));
        assert!(game.is_legal_move(Move::new(C2, C4, DOUBLE_PAWN_PUSH)));
        assert!(game.is_legal_move(Move::new(C8, C7, QUIET_MOVE)));
        assert!(game.is_legal_move(Move::new(C8, D7, QUIET_MOVE)));
        assert!(game.is_legal_move(Move::new(C8, D8, QUIET_MOVE)));

        assert!(!game.is_legal_move(Move::new_null()));

        assert!(!game.is_legal_move(Move::new(H1, H5, QUIET_MOVE)));

        // Cannot be done with pseudo legal move checking
        //assert!(!game.is_legal_move(Move::new(C8, B8, QUIET_MOVE))); // Illegal
        //assert!(!game.is_legal_move(Move::new(C8, B7, QUIET_MOVE))); // Illegal
    }

    #[test]
    fn test_moves_order() {
        let fen = "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2";
        let mut game = Game::from_fen(fen);

        let capture = game.move_from_can("e4d5");
        let first_quiet_move = game.move_from_can("a2a3");

        game.moves.clear();

        let mut n = 0;
        while let Some(m) = game.next_move() {
            match n {
                0 => assert_eq!(m, capture),
                1 => assert_eq!(m, first_quiet_move),
                _ => {}
            }
            n += 1;
        }
        assert_eq!(n, 31);
    }

    #[test]
    fn test_moves_order_with_best_and_killer_moves() {
        let fen = "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2";
        let mut game = Game::from_fen(fen);

        let capture = game.move_from_can("e4d5");
        let first_quiet_move = game.move_from_can("a2a3");

        let first_killer_move = game.move_from_can("f1b5");
        game.moves.add_killer_move(first_killer_move);

        game.moves.clear();

        let best_move = game.move_from_can("b1c3");
        game.moves.add_move(best_move);

        let mut n = 0;
        while let Some(m) = game.next_move() {
            match n {
                0 => assert_eq!(m, best_move),
                1 => assert_eq!(m, capture),
                2 => assert_eq!(m, first_killer_move),
                3 => assert_eq!(m, first_quiet_move),
                _ => {}
            }
            n += 1;
        }
        assert_eq!(n, 31);
    }
}
