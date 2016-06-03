//! Defines what information a chess move should contain.

use basetypes::*;
use castling_rights::*;


/// Represents a move on the chessboard.
///
/// `Move` contains 3 types of information:
///
/// 1. Information about the played move itself.
///
/// 2. Information needed so as to be able to undo the move and
///    restore the board into the exact same state as before.
///
/// 3. The move score -- moves with higher score are tried
///    first. Ideally the best move should have the highest score.
///
/// `Move` is a 32-bit unsigned number. The lowest 16 bits contain the
/// whole needed information about the move itself (type 1). And is
/// laid out the following way:
///
///  ```text
///   15                                                           0
///  +---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+
///  |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |
///  | Move  |    Origin square      |   Destination square  | Aux   |
///  | type  |       6 bits          |        6 bits         | data  |
///  | 2 bits|   |   |   |   |   |   |   |   |   |   |   |   | 2 bits|
///  |   |   |   |   |   |   |   |   |   |   |   |   |   |   |       |
///  +---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+
///  ```
///
/// There are 4 "move type"s: `0`) en-passant capture; `1`) pawn
/// promotion; `2`) castling; `3`) normal move. "Aux data" encodes the
/// type of the promoted piece if the move type is a pawn promotion,
/// otherwise it encodes castling rights (see below).
///
/// The highest 16 bits contain the rest ot the info:
///
///  ```text
///   31                                                          16
///  +---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+
///  |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |
///  | Move  |  Captured | Reser-|  Played   | Cast- |   En-passant  |
///  | score |  piece    |  ved  |  piece    | ling  |      file     |
///  | 2 bits|  3 bits   | 2 bits|  3 bits   | 2 bits|     4 bits    |
///  |   |   |   |   |   |   |   |   |   |   |       |   |   |   |   |
///  +---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+
///  ```
///
/// "En-passant file" tells on what vertical line on the board there
/// was a passing pawn before the move was played. If there was no
/// passing pawn, "en-passant file" should be `8`.
///
/// Castling rights are a bit complex. The castling rights for the side
/// that makes the move, before the move was made, are stored in the
/// "Aux data" field. This is OK, because promoting a pawn never
/// changes the moving player's castling rights. The castling rights
/// for the opposite side are stored in "Castling" field. (A move can
/// change the castling rights for the other side when a rook in the
/// corner is captured.)
///
/// When "Captured piece" is stored, its bits are inverted, so that
/// MVV-LVA (Most valuable victim -- least valuable aggressor) ordering
/// of the moves is preserved, even when the "Move score" field stays
/// the same.
#[derive(Debug)]
#[derive(Clone, Copy)]
#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub struct Move(u32);


impl Move {
    /// Creates a new instance of `Move`.
    ///
    /// `us` is the side that makes the move. `castling` are the
    /// castling rights before the move was played. `en_passant_file`
    /// is the file on which there were a passing pawn before the move
    /// was played (or `8` if there was no passing
    /// pawn). `promoted_piece_code` should be a number between `0`
    /// and `3` and is used only when the `move_type` is a pawn
    /// promotion, otherwise it is ignored.
    #[inline]
    pub fn new(us: Color,
               score: usize,
               move_type: MoveType,
               piece: PieceType,
               orig_square: Square,
               dest_square: Square,
               captured_piece: PieceType,
               en_passant_file: File,
               castling: CastlingRights,
               promoted_piece_code: usize)
               -> Move {
        assert!(us <= 1);
        assert!(score <= 0b11);
        assert!(move_type <= 0x11);
        assert!(piece < NO_PIECE);
        assert!(orig_square <= 63);
        assert!(dest_square <= 63);
        assert!(captured_piece != KING && captured_piece <= NO_PIECE);
        assert!(en_passant_file <= 0b1000);
        assert!(promoted_piece_code <= 0b11);
        
        // We use the reserved field (2 bits) to properly order
        // "quiet" movies. Moves which destination square is more
        // advanced into enemy's territory are tried first.
        let reserved = if captured_piece == NO_PIECE {
            let rank = rank(dest_square);
            let advance = if us == WHITE {
                rank
            } else {
                7 - rank
            };
            match advance {
                0 => 0,
                x if x < 3 => 1,
                x if x < 5 => 2,
                _ => 3,
            }
        } else {
            0
        };
        let aux_data = match move_type {
            MOVE_PROMOTION => promoted_piece_code,
            _ => castling.get_for(us),
        };
        Move((score << M_SHIFT_SCORE | (!captured_piece & 0b111) << M_SHIFT_CAPTURED_PIECE |
              reserved << M_SHIFT_RESERVED | piece << M_SHIFT_PIECE |
              castling.get_for(1 ^ us) << M_SHIFT_CASTLING_DATA |
              en_passant_file << M_SHIFT_ENPASSANT_FILE |
              move_type << M_SHIFT_MOVE_TYPE | orig_square << M_SHIFT_ORIG_SQUARE |
              dest_square << M_SHIFT_DEST_SQUARE |
              aux_data << M_SHIFT_AUX_DATA) as u32)
    }

    /// Sets the "move score" field to a particular value.
    #[inline]
    pub fn set_score(&mut self, score: usize) {
        assert!(score <= 0b11);
        self.0 &= !M_MASK_SCORE;
        self.0 |= (score << M_SHIFT_SCORE) as u32;
    }

    /// Sets a particular bit in the "move score" field to `1`.
    #[inline]
    pub fn set_score_bit(&mut self, b: usize) {
        assert!(b <= 1);
        self.0 |= 1 << b << M_SHIFT_SCORE;
    }

    /// Sets a particular bit in the "move score" field to `0`.
    #[inline]
    pub fn clear_score_bit(&mut self, b: usize) {
        assert!(b <= 1);
        self.0 &= !(1 << b << M_SHIFT_SCORE);
    }

    #[inline]
    pub fn score(&self) -> usize {
        ((self.0 & M_MASK_SCORE) >> M_SHIFT_SCORE) as usize
    }

    #[inline]
    pub fn move_type(&self) -> MoveType {
        ((self.0 & M_MASK_MOVE_TYPE) >> M_SHIFT_MOVE_TYPE) as MoveType
    }

    #[inline]
    pub fn piece(&self) -> PieceType {
        ((self.0 & M_MASK_PIECE) >> M_SHIFT_PIECE) as PieceType
    }

    #[inline]
    pub fn orig_square(&self) -> Square {
        ((self.0 & M_MASK_ORIG_SQUARE) >> M_SHIFT_ORIG_SQUARE) as Square
    }

    #[inline]
    pub fn dest_square(&self) -> Square {
        ((self.0 & M_MASK_DEST_SQUARE) >> M_SHIFT_DEST_SQUARE) as Square
    }

    #[inline]
    pub fn captured_piece(&self) -> PieceType {
        ((!self.0 & M_MASK_CAPTURED_PIECE) >> M_SHIFT_CAPTURED_PIECE) as PieceType
    }

    /// Returns the file on which there were a passing pawn before the
    /// move was played (or `8` if there was no passing pawn).
    #[inline]
    pub fn en_passant_file(&self) -> File {
        ((self.0 & M_MASK_ENPASSANT_FILE) >> M_SHIFT_ENPASSANT_FILE) as File
    }

    /// Returns a 2-bit value representing the castling rights for the
    /// side that does not make the move, as they were before the move
    /// was played.
    #[inline(always)]
    pub fn castling_data(&self) -> usize {
        ((self.0 & M_MASK_CASTLING_DATA) >> M_SHIFT_CASTLING_DATA) as usize
    }

    /// Returns 2-bit value representing auxiliary data.
    ///
    /// When the move type is pawn promotion, "aux data" holds the
    /// promoted piece type encoded with a value from 0 to 3. For all
    /// other move types "aux data" holds the castling rights for the
    /// side that makes the move, as they were before the move was
    /// played.
    #[inline(always)]
    pub fn aux_data(&self) -> usize {
        ((self.0 & M_MASK_AUX_DATA) >> M_SHIFT_AUX_DATA) as usize
    }

    /// Returns the algebraic notation of the move.
    ///
    /// Examples: `e2e4`, `e7e5`, `e1g1` (white short castling),
    /// `e7e8q` (for promotion).
    pub fn notation(&self) -> String {
        format!("{}{}{}",
                notation(self.orig_square()),
                notation(self.dest_square()),
                match self.move_type() {
                    MOVE_PROMOTION => ["q", "r", "b", "n"][self.aux_data()],
                    _ => "",
                })
    }

    /// Decodes the promoted piece type from the raw value of "aux
    /// data".
    #[inline]
    pub fn piece_from_aux_data(pp_code: usize) -> PieceType {
        assert!(pp_code <= 3);
        match pp_code {
            0 => QUEEN,
            1 => ROOK,
            2 => BISHOP,
            _ => KNIGHT,
        }
    }

    #[allow(dead_code)]
    #[inline]
    fn reserved(&self) -> usize {
        ((self.0 & M_MASK_RESERVED) >> M_SHIFT_RESERVED) as usize
    }
}


/// `MOVE_ENPASSANT`, `MOVE_PROMOTION`, `MOVE_CASTLING`, or
/// `MOVE_NORMAL`.
pub type MoveType = usize;

/// En-passant capture move type.
pub const MOVE_ENPASSANT: MoveType = 0;

/// Pawn promotion move type.
pub const MOVE_PROMOTION: MoveType = 1;

/// Castling move type.
pub const MOVE_CASTLING: MoveType = 2;

/// Normal move type.
pub const MOVE_NORMAL: MoveType = 3;


// Field shifts
const M_SHIFT_SCORE: u32 = 30;
const M_SHIFT_CAPTURED_PIECE: u32 = 27;
const M_SHIFT_RESERVED: u32 = 25;
const M_SHIFT_PIECE: u32 = 22;
const M_SHIFT_CASTLING_DATA: u32 = 20;
const M_SHIFT_ENPASSANT_FILE: u32 = 16;
const M_SHIFT_MOVE_TYPE: u32 = 14;
const M_SHIFT_ORIG_SQUARE: u32 = 8;
const M_SHIFT_DEST_SQUARE: u32 = 2;
const M_SHIFT_AUX_DATA: u32 = 0;

// Field masks
const M_MASK_SCORE: u32 = 0b11 << M_SHIFT_SCORE;
const M_MASK_CAPTURED_PIECE: u32 = 0b111 << M_SHIFT_CAPTURED_PIECE;
const M_MASK_RESERVED: u32 = 0b11 << M_SHIFT_RESERVED;
const M_MASK_PIECE: u32 = 0b111 << M_SHIFT_PIECE;
const M_MASK_CASTLING_DATA: u32 = 0b11 << M_SHIFT_CASTLING_DATA;
const M_MASK_ENPASSANT_FILE: u32 = 0b1111 << M_SHIFT_ENPASSANT_FILE;
const M_MASK_MOVE_TYPE: u32 = 0b11 << M_SHIFT_MOVE_TYPE;
const M_MASK_ORIG_SQUARE: u32 = 0b111111 << M_SHIFT_ORIG_SQUARE;
const M_MASK_DEST_SQUARE: u32 = 0b111111 << M_SHIFT_DEST_SQUARE;
const M_MASK_AUX_DATA: u32 = 0b11 << M_SHIFT_AUX_DATA;


#[cfg(test)]
mod tests {
    use super::*;
    use castling_rights::CastlingRights;

    #[test]
    fn test_move() {
        use basetypes::*;
        const NO_ENPASSANT_FILE: File = 8;

        let mut cr = CastlingRights::new();
        cr.set_for(WHITE, 0b10);
        cr.set_for(BLACK, 0b11);
        let mut m = Move::new(WHITE,
                              2,
                              MOVE_NORMAL,
                              PAWN,
                              E2,
                              E4,
                              NO_PIECE,
                              NO_ENPASSANT_FILE,
                              cr,
                              0);
        let n1 = Move::new(WHITE,
                           2,
                           MOVE_NORMAL,
                           PAWN,
                           F3,
                           E4,
                           KNIGHT,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(),
                           0);
        let n2 = Move::new(WHITE,
                           2,
                           MOVE_NORMAL,
                           KING,
                           F3,
                           E4,
                           NO_PIECE,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(),
                           0);
        let n3 = Move::new(BLACK,
                           0,
                           MOVE_PROMOTION,
                           PAWN,
                           F2,
                           F1,
                           NO_PIECE,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(),
                           1);
        assert!(n1 > m);
        assert!(n2 < m);
        assert_eq!(m.score(), 2);
        assert_eq!(m.piece(), PAWN);
        assert_eq!(m.captured_piece(), NO_PIECE);
        assert_eq!(m.orig_square(), E2);
        assert_eq!(m.dest_square(), E4);
        assert_eq!(m.en_passant_file(), 8);
        assert_eq!(m.aux_data(), 0b10);
        assert_eq!(m.castling_data(), 0b11);
        let m2 = m;
        assert_eq!(m, m2);
        m.set_score(3);
        assert_eq!(m.score(), 3);
        assert!(m > m2);
        m.clear_score_bit(0);
        assert_eq!(m, m2);
        m.set_score_bit(0);
        assert_eq!(m.score(), 3);
        m.set_score(0);
        assert_eq!(m.score(), 0);
        assert_eq!(n3.aux_data(), 1);
        assert_eq!(n1.reserved(), 0);
    }
}