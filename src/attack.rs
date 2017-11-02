use color::*;
use piece::*;
use square::*;
use common::*;
use bitboard::{Bitboard, BitboardExt};
use bitboard::dumb7fill;
use game::Game;
use piece::PieceAttr;

pub trait Attack {
    fn is_check(&self, side: Color) -> bool;
    fn is_attacked(&self, square: Square, side: Color) -> bool;
    fn attacks_to(&self, square: Square, occupied: Bitboard) -> Bitboard;
}

impl Attack for Game {
    fn is_check(&self, side: Color) -> bool {
        let king = self.bitboards[(side | KING) as usize];
        if king == 0 {
            return true; // FIXME: Obviously...
        }
        let square = king.scan() as Square;
        self.is_attacked(square, side)
    }

    fn is_attacked(&self, square: Square, side: Color) -> bool {
        let occupied = self.bitboards[WHITE as usize] | self.bitboards[BLACK as usize];

        let pawns = self.bitboards[(side ^ 1 | PAWN) as usize];
        let attacks = PAWN_ATTACKS[side as usize][square as usize];
        if attacks & pawns > 0 {
            return true;
        }

        let knights = self.bitboards[(side ^ 1 | KNIGHT) as usize];
        let attacks = PIECE_MASKS[KNIGHT as usize][square as usize];
        if attacks & knights > 0 {
            return true;
        }

        let king = self.bitboards[(side ^ 1 | KING) as usize];
        let attacks = PIECE_MASKS[KING as usize][square as usize];
        if attacks & king > 0 {
            return true;
        }

        let queens = self.bitboards[(side ^ 1 | QUEEN) as usize];

        let bishops = self.bitboards[(side ^ 1 | BISHOP) as usize];
        let attacks = bishop_attacks(square, occupied);
        if attacks & (bishops | queens) > 0 {
            return true;
        }

        let rooks = self.bitboards[(side ^ 1 | ROOK) as usize];
        let attacks = rook_attacks(square, occupied);
        if attacks & (rooks | queens) > 0 {
            return true;
        }

        false
    }

    fn attacks_to(&self, square: Square, occupied: Bitboard) -> Bitboard {
        let bbs = &self.bitboards;

        // Read the array in sequential order from bbs[0] to bbs[13]
        let wpawns  = bbs[WHITE_PAWN   as usize];
        let bpawns  = bbs[BLACK_PAWN   as usize];
        let knights = bbs[WHITE_KNIGHT as usize] | bbs[BLACK_KNIGHT as usize];
        let kings   = bbs[WHITE_KING   as usize] | bbs[BLACK_KING   as usize];
        let bishops = bbs[WHITE_BISHOP as usize] | bbs[BLACK_BISHOP as usize];
        let rooks   = bbs[WHITE_ROOK   as usize] | bbs[BLACK_ROOK   as usize];
        let queens  = bbs[WHITE_QUEEN  as usize] | bbs[BLACK_QUEEN  as usize];

        (wpawns             & piece_attacks(BLACK_PAWN, square, occupied)) |
        (bpawns             & piece_attacks(WHITE_PAWN, square, occupied)) |
        (knights            & piece_attacks(KNIGHT,     square, occupied)) |
        (kings              & piece_attacks(KING,       square, occupied)) |
        ((queens | bishops) & piece_attacks(BISHOP,     square, occupied)) |
        ((queens | rooks)   & piece_attacks(ROOK,       square, occupied))
    }
}

pub fn piece_attacks(piece: Piece, square: Square, occupied: Bitboard) -> Bitboard {
    match piece.kind() {
        PAWN   => PAWN_ATTACKS[piece.color() as usize][square as usize],
        KNIGHT => PIECE_MASKS[KNIGHT as usize][square as usize],
        KING   => PIECE_MASKS[KING as usize][square as usize],
        BISHOP => bishop_attacks(square, occupied),
        ROOK   => rook_attacks(square, occupied),
        QUEEN  => bishop_attacks(square, occupied) | rook_attacks(square, occupied),
        _      => unreachable!()
    }
}

pub fn bishop_attacks(from: Square, occupied: Bitboard) -> Bitboard {
    let mut targets = 0;

    let occluded = dumb7fill(1 << from, !occupied & 0x7F7F7F7F7F7F7F7F, UP + LEFT);
    targets |= 0x7F7F7F7F7F7F7F7F & occluded.shift(UP + LEFT);
    let occluded = dumb7fill(1 << from, !occupied & 0x7F7F7F7F7F7F7F7F, DOWN + LEFT);
    targets |= 0x7F7F7F7F7F7F7F7F & occluded.shift(DOWN + LEFT);
    let occluded = dumb7fill(1 << from, !occupied & 0xFEFEFEFEFEFEFEFE, DOWN + RIGHT);
    targets |= 0xFEFEFEFEFEFEFEFE & occluded.shift(DOWN + RIGHT);
    let occluded = dumb7fill(1 << from, !occupied & 0xFEFEFEFEFEFEFEFE, UP + RIGHT);
    targets |= 0xFEFEFEFEFEFEFEFE & occluded.shift(UP + RIGHT);

    targets
}

pub fn rook_attacks(from: Square, occupied: Bitboard) -> Bitboard {
    let mut targets = 0;

    let occluded = dumb7fill(1 << from, !occupied & 0xFFFFFFFFFFFFFFFF, UP);
    targets |= 0xFFFFFFFFFFFFFFFF & occluded.shift(UP);
    let occluded = dumb7fill(1 << from, !occupied & 0xFFFFFFFFFFFFFFFF, DOWN);
    targets |= 0xFFFFFFFFFFFFFFFF & occluded.shift(DOWN);
    let occluded = dumb7fill(1 << from, !occupied & 0x7F7F7F7F7F7F7F7F, LEFT);
    targets |= 0x7F7F7F7F7F7F7F7F & occluded.shift(LEFT);
    let occluded = dumb7fill(1 << from, !occupied & 0xFEFEFEFEFEFEFEFE, RIGHT);
    targets |= 0xFEFEFEFEFEFEFEFE & occluded.shift(RIGHT);

    targets
}

lazy_static! {
    pub static ref PAWN_ATTACKS: [[Bitboard; 64]; 2] = {
        let xdirs = [LEFT, RIGHT];
        let ydirs = [DOWN, UP];
        let files = [FILE_H, FILE_A];
        let mut attacks = [[0; 64]; 2];
        for side in 0..2 {
            for square in 0..64 {
                for i in 0..2 {
                    let dir = ydirs[side ^ 1] + xdirs[i];
                    attacks[side][square] |= (1 << square).shift(dir) & !files[i];
                }
            }
        }
        attacks
    };
}

/*
#[cfg(test)]
mod tests {
    extern crate test;

    //use self::test::Bencher;
    use common::*;
    use attack::{bishop_attacks, rook_attacks};

    #[bench]
    fn bench_bishop_attacks(b: &mut Bencher) {
        b.iter(|| {
            bishop_attacks(E4, 0u64)
        })
    }

    #[bench]
    fn bench_rook_attacks(b: &mut Bencher) {
        b.iter(|| {
            rook_attacks(E4, 0u64)
        })
    }
}
*/
