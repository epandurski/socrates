//! Defines data structures describing chess moves.

use std::fmt;
use super::*;


/// `MOVE_ENPASSANT`, `MOVE_PROMOTION`, `MOVE_CASTLING`, or
/// `MOVE_NORMAL`.
pub type MoveType = usize;

pub const MOVE_ENPASSANT: MoveType = 0;
pub const MOVE_PROMOTION: MoveType = 1;
pub const MOVE_CASTLING: MoveType = 2;
pub const MOVE_NORMAL: MoveType = 3;


/// Encodes the minimum needed information that unambiguously
/// describes a move.
///
/// It is laid out the following way:
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
/// type of the promoted piece if the move type is pawn promotion,
/// otherwise it is zero.  For valid moves the digest value will never
/// be `0`.
pub type MoveDigest = u16;


/// Extracts the move type from a `MoveDigest`.
#[inline(always)]
pub fn get_move_type(move_digest: MoveDigest) -> MoveType {
    ((move_digest & M_MASK_MOVE_TYPE as u16) >> M_SHIFT_MOVE_TYPE) as MoveType
}


/// Extracts the origin square from a `MoveDigest`.
#[inline(always)]
pub fn get_orig_square(move_digest: MoveDigest) -> Square {
    ((move_digest & M_MASK_ORIG_SQUARE as u16) >> M_SHIFT_ORIG_SQUARE) as Square
}


/// Extracts the destination square from a `MoveDigest`.
#[inline(always)]
pub fn get_dest_square(move_digest: MoveDigest) -> Square {
    ((move_digest & M_MASK_DEST_SQUARE as u16) >> M_SHIFT_DEST_SQUARE) as Square
}


/// Extracts the auxiliary data from a `MoveDigest`.
#[inline(always)]
pub fn get_aux_data(move_digest: MoveDigest) -> usize {
    ((move_digest & M_MASK_AUX_DATA as u16) >> M_SHIFT_AUX_DATA) as usize
}


/// Represents a move on the chessboard.
///
/// `Move` is a `usize` number. It contains 3 types of information:
///
/// 1. Information about the played move itself.
///
/// 2. Information needed so as to be able to undo the move and
///    restore the board into the exact same state as before.
///
/// 3. Move ordering info -- moves with higher move score are tried
///    first.
///
/// Bits 0-15 contain the whole information about the move itself
/// (type 1). This is called **"move digest"** and is laid out the
/// following way:
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
/// type of the promoted piece if the move type is pawn promotion,
/// otherwise it is zero.
///
/// Bits 16-31 contain the information needed to undo the move, as
/// well as move ordering info:
///
///  ```text
///   31                                                          16
///  +---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+
///  |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |
///  | Move  |  Captured |  Played   |   Castling    |   En-passant  |
///  | score |  piece    |  piece    |    rights     |      file     |
///  | 2 bits|  3 bits   |  3 bits   |    4 bits     |     4 bits    |
///  |   |   |   |   |   |   |   |   |   |   |   |       |   |   |   |   |
///  +---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+---+
///  ```
///
/// "En-passant file" tells on what vertical line on the board there
/// was a passing pawn before the move was played (a value between 0
/// and 7). If there was no passing pawn, "en-passant file" will be
/// 8. "Castling rights" holds the castling rights before the move was
/// played. When "Captured piece" is stored, its bits are inverted, so
/// that MVV-LVA (Most valuable victim -- least valuable aggressor)
/// ordering of the moves is preserved, even when the other fields
/// stay the same. The "Move score" field is used to influence move
/// ordering.
#[derive(Debug)]
#[derive(Clone, Copy)]
#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub struct Move(usize);

impl Move {
    /// Creates a new instance.
    ///
    /// `castling` are the castling rights before the move was
    /// played. `en_passant_file` is the file on which there were a
    /// passing pawn before the move was played (a value between 0 and
    /// 7), or `8` if there was no passing pawn. `promoted_piece_code`
    /// should be a number between 0 and 3 and is used only when the
    /// `move_type` is a pawn promotion, otherwise it is ignored.
    ///
    /// The initial move score for the new move will be:
    ///
    /// * `0` for all pawn promotions to pieces other than queen.
    ///
    /// * `3` for captures and pawn promotions to queen.
    /// 
    /// * `0` for all other moves.
    #[inline(always)]
    pub fn new(move_type: MoveType,
               piece: PieceType,
               orig_square: Square,
               dest_square: Square,
               captured_piece: PieceType,
               en_passant_file: usize,
               castling: CastlingRights,
               promoted_piece_code: usize)
               -> Move {
        debug_assert!(move_type <= 0x11);
        debug_assert!(piece < NO_PIECE);
        debug_assert!(orig_square <= 63);
        debug_assert!(dest_square <= 63);
        debug_assert!(captured_piece != KING && captured_piece <= NO_PIECE);
        debug_assert!(en_passant_file <= 8);
        debug_assert!(promoted_piece_code <= 0b11);
        debug_assert!(orig_square != dest_square ||
                      move_type == MOVE_NORMAL && captured_piece == NO_PIECE);

        // Captures get higher move scores than quiet moves.
        let mut score_shifted = if captured_piece == NO_PIECE {
            0 << M_SHIFT_SCORE
        } else {
            MOVE_SCORE_MAX << M_SHIFT_SCORE
        };

        // Figure out what `aux_data` should contain. In the mean
        // time, make sure pawn promotions get appropriate move
        // scores.
        let aux_data = if move_type == MOVE_PROMOTION {
            score_shifted = if promoted_piece_code == 0 {
                MOVE_SCORE_MAX << M_SHIFT_SCORE
            } else {
                0 << M_SHIFT_SCORE
            };
            promoted_piece_code
        } else {
            0
        };

        Move(score_shifted | (!captured_piece & 0b111) << M_SHIFT_CAPTURED_PIECE |
             piece << M_SHIFT_PIECE | castling.value() << M_SHIFT_CASTLING_DATA |
             en_passant_file << M_SHIFT_ENPASSANT_FILE |
             move_type << M_SHIFT_MOVE_TYPE | orig_square << M_SHIFT_ORIG_SQUARE |
             dest_square << M_SHIFT_DEST_SQUARE | aux_data << M_SHIFT_AUX_DATA)
    }

    /// Creates an invalid move instance.
    ///
    /// The returned instance tries to mimic a legal move, but its
    /// move digest equals `0`. This is sometimes useful in places
    /// where any move is required but no is available.
    #[inline(always)]
    pub fn invalid() -> Move {
        Move((!NO_PIECE & 0b111) << M_SHIFT_CAPTURED_PIECE | KING << M_SHIFT_PIECE)
    }

    /// Decodes the promoted piece type from the raw value returned by
    /// `aux_data`.
    ///
    /// The interpretation of the raw value is: `0` -- queen, `1` --
    /// rook, `2` -- bishop, `3` -- knight.
    #[inline]
    pub fn piece_from_aux_data(pp_code: usize) -> PieceType {
        debug_assert!(pp_code <= 3);
        match pp_code {
            0 => QUEEN,
            1 => ROOK,
            2 => BISHOP,
            3 => KNIGHT,
            _ => panic!("invalid pp_code"),
        }
    }

    /// Assigns a new score for the move.
    ///
    /// `score` must be between `0` and `3`.
    #[inline(always)]
    pub fn set_score(&mut self, score: usize) {
        debug_assert!(score <= MOVE_SCORE_MAX);
        self.0 &= !M_MASK_SCORE;
        self.0 |= score << M_SHIFT_SCORE;
    }

    /// Returns the assigned move score.
    #[inline(always)]
    pub fn score(&self) -> usize {
        self.0 >> M_SHIFT_SCORE
    }

    /// Returns the move type.
    #[inline(always)]
    pub fn move_type(&self) -> MoveType {
        (self.0 & M_MASK_MOVE_TYPE) >> M_SHIFT_MOVE_TYPE
    }

    /// Returns the played piece type.
    ///
    /// Castling is considered as king's move.
    #[inline(always)]
    pub fn piece(&self) -> PieceType {
        (self.0 & M_MASK_PIECE) >> M_SHIFT_PIECE
    }

    /// Returns the origin square of the played piece.
    #[inline(always)]
    pub fn orig_square(&self) -> Square {
        (self.0 & M_MASK_ORIG_SQUARE) >> M_SHIFT_ORIG_SQUARE
    }

    /// Returns the destination square for the played piece.
    #[inline(always)]
    pub fn dest_square(&self) -> Square {
        (self.0 as usize & M_MASK_DEST_SQUARE) >> M_SHIFT_DEST_SQUARE
    }

    /// Returns the captured piece type.
    #[inline(always)]
    pub fn captured_piece(&self) -> PieceType {
        (!self.0 & M_MASK_CAPTURED_PIECE) >> M_SHIFT_CAPTURED_PIECE
    }

    /// Returns the file on which there were a passing pawn before the
    /// move was played (a value between 0 and 7), or `8` if there was
    /// no passing pawn.
    #[inline(always)]
    pub fn en_passant_file(&self) -> usize {
        (self.0 & M_MASK_ENPASSANT_FILE) >> M_SHIFT_ENPASSANT_FILE
    }

    /// Returns the castling rights as they were before the move was
    /// played.
    #[inline(always)]
    pub fn castling(&self) -> CastlingRights {
        CastlingRights::new(self.0 >> M_SHIFT_CASTLING_DATA)
    }

    /// Returns a value between 0 and 3 representing the auxiliary
    /// data.
    ///
    /// When the move type is pawn promotion, "aux data" encodes the
    /// promoted piece type. For all other move types "aux data" is
    /// zero.
    #[inline(always)]
    pub fn aux_data(&self) -> usize {
        (self.0 & M_MASK_AUX_DATA) >> M_SHIFT_AUX_DATA
    }

    /// Returns the least significant 16 bits of the raw move value.
    #[inline(always)]
    pub fn digest(&self) -> MoveDigest {
        self.0 as MoveDigest
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

    /// Returns `true` if the move is a pawn advance or a capture,
    /// `false` otherwise.
    #[inline]
    pub fn is_pawn_advance_or_capure(&self) -> bool {
        // We use clever bit manipulations to avoid branches.
        const P: usize = (!PAWN & 0b111) << M_SHIFT_PIECE;
        const C: usize = (!NO_PIECE & 0b111) << M_SHIFT_CAPTURED_PIECE;
        (self.0 & M_MASK_PIECE | C) ^ (self.0 & M_MASK_CAPTURED_PIECE | P) >= M_MASK_PIECE
    }

    /// Returns if the move is a null move.
    ///
    /// "Null move" is a pseudo-move that changes nothing on the board
    /// except the side to move. It is sometimes useful to include a
    /// speculative null move in the search tree to achieve more
    /// aggressive pruning. Null moves are represented as normal moves
    /// for which the origin and destination squares are the same.
    #[inline]
    pub fn is_null(&self) -> bool {
        self.orig_square() == self.dest_square() && self.move_type() == MOVE_NORMAL
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.notation())
    }
}


/// The highest possible move score.
const MOVE_SCORE_MAX: usize = 3;


// Field shifts
const M_SHIFT_SCORE: usize = 30;
const M_SHIFT_CAPTURED_PIECE: usize = 27;
const M_SHIFT_PIECE: usize = 24;
const M_SHIFT_CASTLING_DATA: usize = 20;
const M_SHIFT_ENPASSANT_FILE: usize = 16;
const M_SHIFT_MOVE_TYPE: usize = 14;
const M_SHIFT_ORIG_SQUARE: usize = 8;
const M_SHIFT_DEST_SQUARE: usize = 2;
const M_SHIFT_AUX_DATA: usize = 0;


// Field masks
#[allow(dead_code)]
const M_MASK_CASTLING_DATA: usize = 0b1111 << M_SHIFT_CASTLING_DATA;
const M_MASK_SCORE: usize = ::std::usize::MAX << M_SHIFT_SCORE;
const M_MASK_CAPTURED_PIECE: usize = 0b111 << M_SHIFT_CAPTURED_PIECE;
const M_MASK_PIECE: usize = 0b111 << M_SHIFT_PIECE;
const M_MASK_ENPASSANT_FILE: usize = 0b1111 << M_SHIFT_ENPASSANT_FILE;
const M_MASK_MOVE_TYPE: usize = 0b11 << M_SHIFT_MOVE_TYPE;
const M_MASK_ORIG_SQUARE: usize = 0b111111 << M_SHIFT_ORIG_SQUARE;
const M_MASK_DEST_SQUARE: usize = 0b111111 << M_SHIFT_DEST_SQUARE;
const M_MASK_AUX_DATA: usize = 0b11 << M_SHIFT_AUX_DATA;


/// Returns the algebraic notation for a given square.
fn notation(square: Square) -> &'static str {
    lazy_static! {
        static ref NOTATION: Vec<String> = (0..64).map(|i| format!("{}{}",
            ["a", "b", "c", "d", "e", "f", "g", "h"][file(i)],
            ["1", "2", "3", "4", "5", "6", "7", "8"][rank(i)])
        ).collect();
    }
    NOTATION[square].as_str()
}


#[cfg(test)]
mod tests {
    use super::MOVE_SCORE_MAX;
    use basetypes::*;
    const NO_ENPASSANT_FILE: usize = 8;

    #[test]
    fn test_castling_rights() {
        use basetypes::*;

        let mut c = CastlingRights::new(0b1110);
        assert_eq!(c.can_castle(WHITE, QUEENSIDE), false);
        assert_eq!(c.can_castle(WHITE, KINGSIDE), true);
        assert_eq!(c.can_castle(BLACK, QUEENSIDE), true);
        assert_eq!(c.can_castle(BLACK, KINGSIDE), true);
        c.update(H8, H7);
        assert_eq!(c.can_castle(WHITE, QUEENSIDE), false);
        assert_eq!(c.can_castle(WHITE, KINGSIDE), true);
        assert_eq!(c.can_castle(BLACK, QUEENSIDE), true);
        assert_eq!(c.can_castle(BLACK, KINGSIDE), false);
        assert_eq!(c.value(), 0b0110);
        let granted = c.grant(BLACK, KINGSIDE);
        assert_eq!(granted, true);
        let granted = c.grant(BLACK, KINGSIDE);
        assert_eq!(granted, false);
        assert_eq!(c.value(), 0b1110);
    }

    #[test]
    fn test_move() {
        assert!(MOVE_SCORE_MAX >= 3);
        let cr = CastlingRights::new(0b1011);
        let mut m = Move::new(MOVE_NORMAL,
                              PAWN,
                              E2,
                              E4,
                              NO_PIECE,
                              NO_ENPASSANT_FILE,
                              cr,
                              0);
        let n1 = Move::new(MOVE_NORMAL,
                           PAWN,
                           F3,
                           E4,
                           KNIGHT,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(0),
                           0);
        let n2 = Move::new(MOVE_NORMAL,
                           KING,
                           F3,
                           E4,
                           NO_PIECE,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(0),
                           0);
        let n3 = Move::new(MOVE_PROMOTION,
                           PAWN,
                           F2,
                           F1,
                           NO_PIECE,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(0),
                           1);
        let n4 = Move::new(MOVE_NORMAL,
                           BISHOP,
                           F2,
                           E3,
                           KNIGHT,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(0),
                           0);
        let n5 = Move::new(MOVE_NORMAL,
                           PAWN,
                           F2,
                           E3,
                           KNIGHT,
                           NO_ENPASSANT_FILE,
                           CastlingRights::new(0),
                           0);
        assert!(n1 > m);
        assert!(n2 < m);
        assert_eq!(m.piece(), PAWN);
        assert_eq!(m.captured_piece(), NO_PIECE);
        assert_eq!(m.orig_square(), E2);
        assert_eq!(m.dest_square(), E4);
        assert_eq!(m.en_passant_file(), 8);
        assert_eq!(m.aux_data(), 0);
        assert_eq!(m.castling().value(), 0b1011);
        let m2 = m;
        assert_eq!(m, m2);
        m.set_score(MOVE_SCORE_MAX);
        assert_eq!(m.score(), MOVE_SCORE_MAX);
        m.set_score(3);
        assert_eq!(m.score(), 3);
        assert!(m > m2);
        m.set_score(0);
        assert_eq!(m.score(), 0);
        assert_eq!(n3.aux_data(), 1);
        assert_eq!(n1.digest(), (n1.0 & 0xffff) as MoveDigest);
        assert!(m.is_pawn_advance_or_capure());
        assert!(!n2.is_pawn_advance_or_capure());
        assert!(n4.is_pawn_advance_or_capure());
        assert!(n5.is_pawn_advance_or_capure());
        assert!(!Move::invalid().is_null());
        assert_eq!(Move::invalid().digest(), 0);
    }
}