//! Implements the rules of chess.

use std::mem::uninitialized;
use std::cell::Cell;
use basetypes::*;
use castling_rights::*;
use bitsets::*;
use chess_move::*;
use position::board_geometry::*;
use notation;


/// Represents an illegal board error.
pub struct IllegalBoard;


/// Holds the current chess position and "knows" the rules of chess.
///
/// `Board` can generate all possible moves in the current position,
/// play a selected move, and take it back. It can tell you which
/// pieces attack a specific square, and which are the checkers to the
/// king. It can also fabricate a speculative "null move" that can be
/// used to aggressively prune the search tree. `Board` does not know
/// anything about chess strategy or tactics.
#[derive(Clone)]
pub struct Board {
    geometry: &'static BoardGeometry,
    zobrist: &'static ZobristArrays,
    piece_type: [u64; 6],
    color: [u64; 2],
    to_move: Color,
    castling: CastlingRights,
    en_passant_file: usize,
    _occupied: u64, // this will always be equal to self.color[0] | self.color[1]
    _hash: u64, // Zobrist hash value
    _checkers: Cell<u64>, // lazily calculated, "UNIVERSAL_SET" if not calculated yet
    _pinned: Cell<u64>, // lazily calculated, "UNIVERSAL_SET" if not calculated yet
    _king_square: Cell<Square>, // lazily calculated, >= 64 if not calculated yet
}


impl Board {
    /// Creates a new board instance.
    ///
    /// This function makes expensive verification to make sure that
    /// the resulting new board is legal.
    pub fn create(placement: &notation::PiecesPlacement,
                  to_move: Color,
                  castling: CastlingRights,
                  en_passant_square: Option<Square>)
                  -> Result<Board, IllegalBoard> {

        let en_passant_rank = match to_move {
            WHITE => RANK_6,
            BLACK => RANK_3,
            _ => return Err(IllegalBoard),
        };
        let en_passant_file = match en_passant_square {
            None => NO_ENPASSANT_FILE,
            Some(x) if x <= 63 && rank(x) == en_passant_rank => file(x),
            _ => return Err(IllegalBoard),
        };
        let mut b = Board {
            geometry: BoardGeometry::get(),
            zobrist: ZobristArrays::get(),
            piece_type: placement.piece_type,
            color: placement.color,
            to_move: to_move,
            castling: castling,
            en_passant_file: en_passant_file,
            _occupied: placement.color[WHITE] | placement.color[BLACK],
            _hash: Default::default(),
            _checkers: Cell::new(UNIVERSAL_SET),
            _pinned: Cell::new(UNIVERSAL_SET),
            _king_square: Cell::new(64),
        };
        b._hash = b.calc_hash();

        if b.is_legal() {
            Ok(b)
        } else {
            Err(IllegalBoard)
        }
    }

    /// Creates a new board instance from a FEN string.
    ///
    /// A FEN (Forsyth–Edwards Notation) string defines a particular
    /// position using only the ASCII character set. This function
    /// makes expensive verification to make sure that the resulting
    /// new board is legal.
    pub fn from_fen(fen: &str) -> Result<Board, IllegalBoard> {
        let (ref placement, to_move, castling, en_passant_square, _, _) =
            try!(notation::parse_fen(fen).map_err(|_| IllegalBoard));

        Board::create(placement, to_move, castling, en_passant_square)
    }

    /// Returns a reference to a properly initialized `BoardGeometry`
    /// object.
    #[inline(always)]
    pub fn geometry(&self) -> &BoardGeometry {
        self.geometry
    }

    /// Returns an array of 6 occupation bitboards -- one for each
    /// piece type.
    #[inline(always)]
    pub fn piece_type(&self) -> &[u64; 6] {
        &self.piece_type
    }

    /// Returns an array of 2 occupation bitboards -- one for each
    /// side (color).
    #[inline(always)]
    pub fn color(&self) -> &[u64; 2] {
        &self.color
    }

    /// Returns a bitboard of all occupied squares.
    #[inline(always)]
    pub fn occupied(&self) -> u64 {
        self._occupied
    }

    /// Returns a bitboard of all checkers that are attacking the
    /// king.
    #[inline]
    pub fn checkers(&self) -> u64 {
        if self._checkers.get() == UNIVERSAL_SET {
            self._checkers.set(self.attacks_to(1 ^ self.to_move, self.king_square()));
        }
        self._checkers.get()
    }

    /// Returns a bitboard of all pinned pieces and pawns of the color
    /// of the side to move.
    #[inline]
    pub fn pinned(&self) -> u64 {
        if self._pinned.get() == UNIVERSAL_SET {
            self._pinned.set(self.find_pinned());
        }
        self._pinned.get()
    }

    /// Returns a bitboard of all pieces (or pawns) of color `us` that
    /// attack `square`.
    #[inline]
    pub fn attacks_to(&self, us: Color, square: Square) -> u64 {
        let occupied_by_us = self.color[us];
        if square > 63 {
            // We call "piece_attacks_from()" here many times, which for
            // performance reasons do not do array boundary checks. Since
            // "Board::attacks_to()" is a public function, we have to
            // guarantee memory safety for all its users.
            panic!("invalid square");
        }
        let square_bb = 1 << square;
        unsafe {
            let shifts: &[isize; 4] = PAWN_MOVE_SHIFTS.get_unchecked(us);

            (self.geometry.piece_attacks_from(self.occupied(), ROOK, square) & occupied_by_us &
             (self.piece_type[ROOK] | self.piece_type[QUEEN])) |
            (self.geometry.piece_attacks_from(self.occupied(), BISHOP, square) & occupied_by_us &
             (self.piece_type[BISHOP] | self.piece_type[QUEEN])) |
            (self.geometry.piece_attacks_from(self.occupied(), KNIGHT, square) & occupied_by_us &
             self.piece_type[KNIGHT]) |
            (self.geometry.piece_attacks_from(self.occupied(), KING, square) & occupied_by_us &
             self.piece_type[KING]) |
            (gen_shift(square_bb, -shifts[PAWN_EAST_CAPTURE]) & occupied_by_us &
             self.piece_type[PAWN] & !(BB_FILE_H | BB_RANK_1 | BB_RANK_8)) |
            (gen_shift(square_bb, -shifts[PAWN_WEST_CAPTURE]) & occupied_by_us &
             self.piece_type[PAWN] & !(BB_FILE_A | BB_RANK_1 | BB_RANK_8))
        }
    }

    /// Returns the side to move.
    #[inline(always)]
    pub fn to_move(&self) -> Color {
        self.to_move
    }

    /// Returns the castling rights.
    #[inline(always)]
    pub fn castling(&self) -> CastlingRights {
        self.castling
    }

    /// Returns the en-passant file, or `None` if there is none.
    #[inline(always)]
    pub fn en_passant_file(&self) -> Option<File> {
        if self.en_passant_file < 8 {
            Some(self.en_passant_file)
        } else {
            None
        }
    }

    /// Returns the Zobrist hash value for the current board.
    ///
    /// Zobrist Hashing is a technique to transform a board position
    /// of arbitrary size into a number of a set length, with an equal
    /// distribution over all possible numbers, invented by Albert
    /// Zobrist.  The main purpose of Zobrist hash codes in chess
    /// programming is to get an almost unique index number for any
    /// chess position, with a very important requirement that two
    /// similar positions generate entirely different indices. These
    /// index numbers are used for faster and more space efficient
    /// hash tables or databases, e.g. transposition tables and
    /// opening books.
    #[inline(always)]
    pub fn hash(&self) -> u64 {
        self._hash
    }

    /// Generates pseudo-legal moves and pushes them to `move_stack`.
    ///
    /// When `all` is `true`, all pseudo-legal moves will be
    /// considered. When `all` is `false`, only captures, pawn
    /// promotions to queen, and check evasions will be considered.
    /// It is guaranteed, that all generated moves with pieces other
    /// than the king are legal. It is possible that some of the
    /// king's moves are illegal because the destination square is
    /// under check, or when castling, king's passing square is
    /// attacked. This is because verifying that these squares are not
    /// under attack is quite expensive, and therefore we hope that
    /// the alpha-beta pruning will eliminate the need for this
    /// verification at all.
    #[inline]
    pub fn generate_moves(&self, all: bool, move_stack: &mut MoveStack) {
        assert!(self.is_legal());
        let king_square = self.king_square();
        let checkers = self.checkers();
        let occupied_by_us = unsafe { *self.color.get_unchecked(self.to_move) };
        let occupied_by_them = self.occupied() ^ occupied_by_us;
        let generate_all_moves = all || checkers != 0;
        assert!(king_square <= 63);

        // When in check, for every move except king's moves, the only
        // legal destination squares are those lying on the line
        // between the checker and the king. Also, no piece can move
        // to a square that is occupied by a friendly piece.
        let legal_dests = !occupied_by_us &
                          match ls1b(checkers) {
            0 =>
                // Not in check -- every move destination may be
                // considered "covering".
                UNIVERSAL_SET,

            x if x == checkers =>
                // Single check -- calculate the check covering
                // destination subset (the squares between the king
                // and the checker). Notice that we must OR with "x"
                // itself, because knights give check not lying on a
                // line with the king.
                x |
                unsafe {
                    *self.geometry
                         .squares_between_including
                         .get_unchecked(king_square)
                         .get_unchecked(bitscan_1bit(x))
                },

            _ =>
                // Double check -- no covering moves.
                EMPTY_SET,
        };

        if legal_dests != EMPTY_SET {
            // This block is not executed when the king is in double
            // check.

            let pinned = self.pinned();
            let pin_lines = unsafe { self.geometry.squares_at_line.get_unchecked(king_square) };
            let en_passant_bb = self.en_passant_bb();

            // Find queen, rook, bishop, and knight moves.
            {
                // Reduce the set of legal destinations when searching
                // only for captures, pawn promotions to queen, and
                // check evasions.
                let legal_dests = if generate_all_moves {
                    legal_dests
                } else {
                    assert_eq!(legal_dests, !occupied_by_us);
                    occupied_by_them
                };

                for piece in QUEEN..PAWN {
                    let mut bb = unsafe { *self.piece_type.get_unchecked(piece) } & occupied_by_us;
                    while bb != EMPTY_SET {
                        let piece_bb = ls1b(bb);
                        bb ^= piece_bb;
                        let from_square = bitscan_1bit(piece_bb);
                        let piece_legal_dests = match piece_bb & pinned {
                            0 => legal_dests,
                            _ => unsafe { legal_dests & *pin_lines.get_unchecked(from_square) },
                        };
                        self.push_piece_moves_to_sink(piece,
                                                      from_square,
                                                      piece_legal_dests,
                                                      move_stack);
                    }
                }
            }

            // Find pawn moves.
            {
                // Reduce the set of legal destinations when searching
                // only for captures, pawn promotions to queen, and
                // check evasions.
                let legal_dests = if generate_all_moves {
                    legal_dests
                } else {
                    assert_eq!(legal_dests, !occupied_by_us);
                    legal_dests & (occupied_by_them | en_passant_bb | BB_PAWN_PROMOTION_RANKS)
                };

                // When in check, en-passant capture is a legal evasion
                // move only when the checking piece is the passing pawn
                // itself.
                let pawn_legal_dests = match checkers & self.piece_type[PAWN] {
                    0 => legal_dests,
                    _ => legal_dests | en_passant_bb,
                };

                // Find all free pawn moves at once.
                let all_pawns = self.piece_type[PAWN] & occupied_by_us;
                let mut pinned_pawns = all_pawns & pinned;
                let free_pawns = all_pawns ^ pinned_pawns;
                if free_pawns != EMPTY_SET {
                    self.push_pawn_moves_to_sink(free_pawns,
                                                 en_passant_bb,
                                                 pawn_legal_dests,
                                                 !generate_all_moves,
                                                 move_stack);
                }

                // Find pinned pawn moves pawn by pawn.
                while pinned_pawns != EMPTY_SET {
                    let pawn_bb = ls1b(pinned_pawns);
                    pinned_pawns ^= pawn_bb;
                    let pin_line = unsafe { *pin_lines.get_unchecked(bitscan_1bit(pawn_bb)) };
                    self.push_pawn_moves_to_sink(pawn_bb,
                                                 en_passant_bb,
                                                 pin_line & pawn_legal_dests,
                                                 !generate_all_moves,
                                                 move_stack);
                }
            }
        }

        // Find king moves (pseudo-legal, possibly moving into check
        // or passing through an attacked square when castling). This
        // is executed even when the king is in double check.
        {
            let king_dests = if generate_all_moves {
                self.push_castling_moves_to_sink(move_stack);
                !occupied_by_us
            } else {
                // Reduce the set of legal destinations when searching
                // only for captures, pawn promotions to queen, and
                // check evasions.
                occupied_by_them
            };

            self.push_piece_moves_to_sink(KING, king_square, king_dests, move_stack);
        }
    }

    /// Returns a null move.
    ///
    /// "Null move" is an illegal pseudo-move that changes nothing on
    /// the board except the side to move (and the en-passant file, of
    /// course). It is sometimes useful to include a speculative null
    /// move in the search tree so as to achieve more aggressive
    /// pruning. For the move generated by this method, `do_move(m)`
    /// will return `false` if and only if the king is in check.
    #[inline]
    pub fn null_move(&self) -> Move {
        let king_square = self.king_square();
        assert!(king_square <= 63);
        Move::new(self.to_move,
                  MOVE_NORMAL,
                  KING,
                  king_square,
                  king_square,
                  NO_PIECE,
                  self.en_passant_file,
                  self.castling,
                  0)
    }

    /// Returns if `m` is a null move.
    #[inline]
    pub fn is_null_move(m: Move) -> bool {
        m.orig_square() == m.dest_square()
    }

    /// Plays a move on the board.
    ///
    /// It verifies if the move is legal. If the move is legal, the
    /// board is updated and `true` is returned. If the move is
    /// illegal, `false` is returned without updating the board. The
    /// move passed to this method **must** have been generated by
    /// `generate_moves` or `null_move` for the current position on
    /// the board.
    ///
    /// Moves generated by the `null_move` method are exceptions. For
    /// them `do_move(m)` will return `false` if and only if the king
    /// is in check.
    #[inline]
    pub fn do_move(&mut self, m: Move) -> bool {
        let us = self.to_move;
        let them = 1 ^ us;
        let move_type = m.move_type();
        let orig_square = m.orig_square();
        let dest_square = m.dest_square();
        let piece = m.piece();
        let captured_piece = m.captured_piece();
        let mut hash = 0;
        assert!(us <= 1);
        assert!(piece < NO_PIECE);
        assert!(move_type <= 3);
        assert!(orig_square <= 63);
        assert!(dest_square <= 63);
        assert!(unsafe {
            Board::is_null_move(m) ||
            ::std::mem::transmute::<Move, u32>(m) & (!0 >> 2) ==
            ::std::mem::transmute::<Move, u32>(self.try_move16(m.move16()).unwrap()) & (!0 >> 2)
        });

        if piece >= NO_PIECE {
            // Since "Board::do_move()" is a public function, we have
            // to guarantee memory safety for all its users.
            panic!("invalid piece");
        }

        unsafe {
            // verify if the move will leave the king in check
            if piece == KING {
                if orig_square != dest_square {
                    if self.king_would_be_in_check(dest_square) {
                        return false;  // the king is in check -- illegal move
                    }
                } else {
                    if self.checkers() != 0 {
                        return false;  // invalid "null move"
                    }
                }
            }

            // move the rook if the move is castling
            if move_type == MOVE_CASTLING {
                if self.king_would_be_in_check((orig_square + dest_square) >> 1) {
                    return false;  // king's passing square is attacked -- illegal move
                }

                let side = if dest_square > orig_square {
                    KINGSIDE
                } else {
                    QUEENSIDE
                };
                let mask = CASTLING_ROOK_MASK[us][side];
                self.piece_type[ROOK] ^= mask;
                self.color[us] ^= mask;
                hash ^= self.zobrist.castling_rook_move[us][side];
            }

            let not_orig_bb = !(1 << orig_square);
            let dest_bb = 1 << dest_square;

            // empty the origin square
            *self.piece_type.get_unchecked_mut(piece) &= not_orig_bb;
            *self.color.get_unchecked_mut(us) &= not_orig_bb;
            hash ^= *self.zobrist
                         .pieces
                         .get_unchecked(us)
                         .get_unchecked(piece)
                         .get_unchecked(orig_square);

            // remove the captured piece (if any)
            if captured_piece < NO_PIECE {
                let not_captured_bb = if move_type == MOVE_ENPASSANT {
                    let shift = PAWN_MOVE_SHIFTS.get_unchecked(them)[PAWN_PUSH];
                    let captured_pawn_square = (dest_square as isize + shift) as Square;
                    hash ^= *self.zobrist
                                 .pieces
                                 .get_unchecked(them)
                                 .get_unchecked(captured_piece)
                                 .get_unchecked(captured_pawn_square);
                    !(1 << captured_pawn_square)
                } else {
                    hash ^= *self.zobrist
                                 .pieces
                                 .get_unchecked(them)
                                 .get_unchecked(captured_piece)
                                 .get_unchecked(dest_square);
                    !dest_bb
                };
                *self.piece_type.get_unchecked_mut(captured_piece) &= not_captured_bb;
                *self.color.get_unchecked_mut(them) &= not_captured_bb;
            }

            // occupy the destination square
            let dest_piece = if move_type == MOVE_PROMOTION {
                Move::piece_from_aux_data(m.aux_data())
            } else {
                piece
            };
            *self.piece_type.get_unchecked_mut(dest_piece) |= dest_bb;
            *self.color.get_unchecked_mut(us) |= dest_bb;
            hash ^= *self.zobrist
                         .pieces
                         .get_unchecked(us)
                         .get_unchecked(dest_piece)
                         .get_unchecked(dest_square);

            // update castling rights (null moves do not affect castling)
            if orig_square != dest_square {
                hash ^= *self.zobrist.castling.get_unchecked(self.castling.value());
                self.castling.update(orig_square, dest_square);
                hash ^= *self.zobrist.castling.get_unchecked(self.castling.value());
            }

            // update the en-passant file
            hash ^= *self.zobrist.en_passant.get_unchecked(self.en_passant_file);
            self.en_passant_file = if piece == PAWN {
                match dest_square as isize - orig_square as isize {
                    16 | -16 => {
                        let file = file(dest_square);
                        hash ^= *self.zobrist.en_passant.get_unchecked(file);
                        file
                    }
                    _ => NO_ENPASSANT_FILE,
                }
            } else {
                NO_ENPASSANT_FILE
            };

            // change the side to move
            self.to_move = them;
            hash ^= self.zobrist.to_move;

            // update "_occupied", "_hash", "_checkers", "_pinned",
            // and "_king_square"
            self._occupied = self.color[WHITE] | self.color[BLACK];
            self._hash ^= hash;
            self._checkers.set(UNIVERSAL_SET);
            self._pinned.set(UNIVERSAL_SET);
            self._king_square.set(64);
        }

        assert!(self.is_legal());
        true
    }

    /// Takes back a previously played move.
    ///
    /// The move passed to this method **must** be the last move passed
    /// to `do_move`.
    #[inline]
    pub fn undo_move(&mut self, m: Move) {
        let them = self.to_move;
        let us = 1 ^ them;
        let move_type = m.move_type();
        let orig_square = m.orig_square();
        let dest_square = m.dest_square();
        let aux_data = m.aux_data();
        let piece = m.piece();
        let captured_piece = m.captured_piece();
        let mut hash = 0;
        assert!(them <= 1);
        assert!(piece < NO_PIECE);
        assert!(move_type <= 3);
        assert!(orig_square <= 63);
        assert!(dest_square <= 63);
        assert!(aux_data <= 3);
        assert!(m.castling_data() <= 3);
        assert!(m.en_passant_file() <= NO_ENPASSANT_FILE);

        if piece >= NO_PIECE {
            // Since "Board::undo_move()" is a public function, we
            // have to guarantee memory safety for all its users.
            panic!("invalid piece");
        }

        let orig_bb = 1 << orig_square;
        let not_dest_bb = !(1 << dest_square);

        unsafe {
            // change the side to move
            self.to_move = us;
            hash ^= self.zobrist.to_move;

            // restore the en-passant file
            hash ^= *self.zobrist.en_passant.get_unchecked(self.en_passant_file);
            self.en_passant_file = m.en_passant_file();
            hash ^= *self.zobrist.en_passant.get_unchecked(self.en_passant_file);

            // restore castling rights
            hash ^= *self.zobrist.castling.get_unchecked(self.castling.value());
            self.castling.set_for(them, m.castling_data());
            if move_type != MOVE_PROMOTION {
                self.castling.set_for(us, aux_data);
            }
            hash ^= *self.zobrist.castling.get_unchecked(self.castling.value());

            // empty the destination square
            let dest_piece = if move_type == MOVE_PROMOTION {
                Move::piece_from_aux_data(aux_data)
            } else {
                piece
            };
            *self.piece_type.get_unchecked_mut(dest_piece) &= not_dest_bb;
            *self.color.get_unchecked_mut(us) &= not_dest_bb;
            hash ^= *self.zobrist
                         .pieces
                         .get_unchecked(us)
                         .get_unchecked(dest_piece)
                         .get_unchecked(dest_square);

            // put back the captured piece (if any)
            if captured_piece < NO_PIECE {
                let captured_bb = if move_type == MOVE_ENPASSANT {
                    let shift = PAWN_MOVE_SHIFTS.get_unchecked(them)[PAWN_PUSH];
                    let captured_pawn_square = (dest_square as isize + shift) as Square;
                    hash ^= *self.zobrist
                                 .pieces
                                 .get_unchecked(them)
                                 .get_unchecked(captured_piece)
                                 .get_unchecked(captured_pawn_square);
                    1 << captured_pawn_square
                } else {
                    hash ^= *self.zobrist
                                 .pieces
                                 .get_unchecked(them)
                                 .get_unchecked(captured_piece)
                                 .get_unchecked(dest_square);
                    !not_dest_bb
                };
                *self.piece_type.get_unchecked_mut(captured_piece) |= captured_bb;
                *self.color.get_unchecked_mut(them) |= captured_bb;
            }

            // restore the piece on the origin square
            *self.piece_type.get_unchecked_mut(piece) |= orig_bb;
            *self.color.get_unchecked_mut(us) |= orig_bb;
            hash ^= *self.zobrist
                         .pieces
                         .get_unchecked(us)
                         .get_unchecked(piece)
                         .get_unchecked(orig_square);

            // move the rook back if the move is castling
            if move_type == MOVE_CASTLING {
                let side = if dest_square > orig_square {
                    KINGSIDE
                } else {
                    QUEENSIDE
                };
                let mask = CASTLING_ROOK_MASK[us][side];
                self.piece_type[ROOK] ^= mask;
                self.color[us] ^= mask;
                hash ^= self.zobrist.castling_rook_move[us][side];
            }

            // update "_occupied", "_hash", "_checkers", "_pinned",
            // and "_king_square"
            self._occupied = self.color[WHITE] | self.color[BLACK];
            self._hash ^= hash;
            self._checkers.set(UNIVERSAL_SET);
            self._pinned.set(UNIVERSAL_SET);
            self._king_square.set(64);
        }

        assert!(self.is_legal());
    }

    /// Check if an `u16` integer represents pseudo-legal move.
    #[inline]
    pub fn try_move16(&self, move16: u16) -> Option<Move> {
        const M_SHIFT_MOVE_TYPE: u16 = 14;
        const M_SHIFT_ORIG_SQUARE: u16 = 8;
        const M_SHIFT_DEST_SQUARE: u16 = 2;
        const M_SHIFT_AUX_DATA: u16 = 0;
        const M_MASK_MOVE_TYPE: u16 = 0b11 << M_SHIFT_MOVE_TYPE;
        const M_MASK_ORIG_SQUARE: u16 = 0b111111 << M_SHIFT_ORIG_SQUARE;
        const M_MASK_DEST_SQUARE: u16 = 0b111111 << M_SHIFT_DEST_SQUARE;
        const M_MASK_AUX_DATA: u16 = 0b11 << M_SHIFT_AUX_DATA;

        let move_type = ((move16 & M_MASK_MOVE_TYPE) >> M_SHIFT_MOVE_TYPE) as usize;
        let orig_square = ((move16 & M_MASK_ORIG_SQUARE) >> M_SHIFT_ORIG_SQUARE) as Square;
        let dest_square = ((move16 & M_MASK_DEST_SQUARE) >> M_SHIFT_DEST_SQUARE) as Square;
        let king_square = self.king_square();
        let checkers = self.checkers();
        assert!(self.to_move <= 1);
        assert!(move_type <= 3);
        assert!(orig_square <= 63);
        assert!(dest_square <= 63);

        if move_type == MOVE_CASTLING {
            let side = if dest_square < orig_square {
                QUEENSIDE
            } else {
                KINGSIDE
            };
            if checkers != 0 ||
               self.castling.obstacles(self.to_move, side) & self.occupied() != 0 ||
               orig_square != king_square ||
               dest_square != [[C1, C8], [G1, G8]][side][self.to_move] {
                return None;
            }
            return Some(Move::new(self.to_move,
                                  MOVE_CASTLING,
                                  KING,
                                  orig_square,
                                  dest_square,
                                  NO_PIECE,
                                  self.en_passant_file,
                                  self.castling,
                                  0));
        }

        // Figure out what is the moved piece.
        let occupied_by_us = unsafe { *self.color.get_unchecked(self.to_move) };
        let orig_square_bb = occupied_by_us & (1 << orig_square);
        let dest_square_bb = 1 << dest_square;
        let piece;
        'pieces: loop {
            for i in (KING..NO_PIECE).rev() {
                if orig_square_bb & unsafe { *self.piece_type.get_unchecked(i) } != 0 {
                    piece = i;
                    break 'pieces;
                }
            }
            return None;
        }
        assert!(piece <= PAWN);

        // We will gradually shrink the legal destinations set.
        let mut legal_dests = !occupied_by_us;

        if piece != KING {
            legal_dests &= match ls1b(checkers) {
                // not in check
                0 => UNIVERSAL_SET,

                // in check
                x if x == checkers => {
                    x |
                    unsafe {
                        *self.geometry
                             .squares_between_including
                             .get_unchecked(king_square)
                             .get_unchecked(bitscan_1bit(x))
                    }
                }

                // double check
                _ => return None,
            };
            if orig_square_bb & self.pinned() != 0 {
                // the piece is pinned
                legal_dests &= unsafe {
                    *self.geometry
                         .squares_at_line
                         .get_unchecked(king_square)
                         .get_unchecked(orig_square)
                }
            }
        };

        let mut promoted_piece_code = 0;
        let mut captured_piece = get_piece_type_at(&self.piece_type,
                                                   self.occupied(),
                                                   dest_square_bb);
        if piece == PAWN {
            // If we are in check, and the checking piece is the
            // passing pawn, the en-passant capture can be a legal
            // check evasion.
            let en_passant_bb = self.en_passant_bb();
            if checkers & self.piece_type[PAWN] != 0 {
                legal_dests |= en_passant_bb;
            }

            let mut dest_sets: [u64; 4] = unsafe { uninitialized() };
            self.calc_pawn_dest_sets(orig_square_bb, en_passant_bb, &mut dest_sets);
            legal_dests &= dest_sets[PAWN_PUSH] | dest_sets[PAWN_DOUBLE_PUSH] |
                           dest_sets[PAWN_WEST_CAPTURE] |
                           dest_sets[PAWN_EAST_CAPTURE];
            if legal_dests & dest_square_bb == 0 {
                return None;
            }

            match dest_square_bb {

                // en-passant capture
                x if x == en_passant_bb => {
                    if move_type != MOVE_ENPASSANT ||
                       !self.en_passant_special_check_ok(orig_square, dest_square) {
                        return None;
                    }
                    captured_piece = PAWN;
                }

                // pawn promotion
                x if x & BB_PAWN_PROMOTION_RANKS != 0 => {
                    if move_type != MOVE_PROMOTION {
                        return None;
                    }
                    promoted_piece_code = ((move16 & M_MASK_AUX_DATA) >> M_SHIFT_AUX_DATA) as usize;
                }

                // normal pawn move (push or plain capture)
                _ => {
                    if move_type != MOVE_NORMAL {
                        return None;
                    }
                }
            }
        } else {
            legal_dests &= unsafe {
                self.geometry.piece_attacks_from(self.occupied(), piece, orig_square)
            };
            if move_type != MOVE_NORMAL || legal_dests & dest_square_bb == 0 {
                return None;
            }
        }

        Some(Move::new(self.to_move,
                       move_type,
                       piece,
                       orig_square,
                       dest_square,
                       captured_piece,
                       self.en_passant_file,
                       self.castling,
                       promoted_piece_code))
    }

    // Analyzes the board and decides if it is a legal board.
    //
    // In addition to the obviously wrong boards (that for example
    // declare some pieces having no or more than one color), there
    // are many chess boards that are impossible to create from the
    // starting chess position. Here we are interested to detect and
    // guard against only those of the cases that have a chance of
    // disturbing some of our explicit and unavoidably, implicit
    // presumptions about what a chess position is when writing the
    // code.
    //
    // Invalid boards: 1. having more or less than 1 king from each
    // color; 2. having more than 8 pawns of a color; 3. having more
    // than 16 pieces (and pawns) of one color; 4. having the side not
    // to move in check; 5. having pawns on ranks 1 or 8; 6. having
    // castling rights when the king or the corresponding rook is not
    // on its initial square; 7. having an en-passant square that is
    // not having a pawn of corresponding color before, and an empty
    // square on it and behind it; 8. having an en-passant square
    // while the wrong side is to move; 9. having an en-passant square
    // while the king is in check not from the passing pawn and not
    // from a checker that was discovered by the passing pawn.
    fn is_legal(&self) -> bool {
        if self.to_move > 1 || self.en_passant_file > NO_ENPASSANT_FILE {
            return false;
        }
        let us = self.to_move;
        let en_passant_bb = self.en_passant_bb();
        let occupied = self.piece_type.into_iter().fold(0, |acc, x| {
            if acc & x == 0 {
                acc | x
            } else {
                UNIVERSAL_SET
            }
        });  // returns "UNIVERSAL_SET" if "self.piece_type" is messed up

        let them = 1 ^ us;
        let o_us = self.color[us];
        let o_them = self.color[them];
        let our_king_bb = self.piece_type[KING] & o_us;
        let their_king_bb = self.piece_type[KING] & o_them;
        let pawns = self.piece_type[PAWN];

        occupied != UNIVERSAL_SET && occupied == o_us | o_them && o_us & o_them == 0 &&
        pop_count(our_king_bb) == 1 && pop_count(their_king_bb) == 1 &&
        pop_count(pawns & o_us) <= 8 &&
        pop_count(pawns & o_them) <= 8 && pop_count(o_us) <= 16 &&
        pop_count(o_them) <= 16 &&
        self.attacks_to(us, bitscan_forward(their_king_bb)) == 0 &&
        pawns & BB_PAWN_PROMOTION_RANKS == 0 &&
        (!self.castling.can_castle(WHITE, QUEENSIDE) ||
         (self.piece_type[ROOK] & self.color[WHITE] & 1 << A1 != 0) &&
         (self.piece_type[KING] & self.color[WHITE] & 1 << E1 != 0)) &&
        (!self.castling.can_castle(WHITE, KINGSIDE) ||
         (self.piece_type[ROOK] & self.color[WHITE] & 1 << H1 != 0) &&
         (self.piece_type[KING] & self.color[WHITE] & 1 << E1 != 0)) &&
        (!self.castling.can_castle(BLACK, QUEENSIDE) ||
         (self.piece_type[ROOK] & self.color[BLACK] & 1 << A8 != 0) &&
         (self.piece_type[KING] & self.color[BLACK] & 1 << E8 != 0)) &&
        (!self.castling.can_castle(BLACK, KINGSIDE) ||
         (self.piece_type[ROOK] & self.color[BLACK] & 1 << H8 != 0) &&
         (self.piece_type[KING] & self.color[BLACK] & 1 << E8 != 0)) &&
        (en_passant_bb == EMPTY_SET ||
         {
            let dest_square_bb = gen_shift(en_passant_bb, PAWN_MOVE_SHIFTS[them][PAWN_PUSH]);
            let orig_square_bb = gen_shift(en_passant_bb, -PAWN_MOVE_SHIFTS[them][PAWN_PUSH]);
            let our_king_square = bitscan_forward(our_king_bb);
            let checkers = self.attacks_to(them, our_king_square);
            (dest_square_bb & pawns & o_them != 0) && (en_passant_bb & !occupied != 0) &&
            (orig_square_bb & !occupied != 0) &&
            (checkers == EMPTY_SET || checkers == dest_square_bb ||
             (pop_count(checkers) == 1 &&
              self.geometry.squares_between_including[our_king_square][bitscan_forward(checkers)] &
              orig_square_bb != 0))
        }) &&
        {
            assert_eq!(self._occupied, occupied);
            assert_eq!(self._hash, self.calc_hash());
            assert!(self._checkers.get() == UNIVERSAL_SET ||
                    self._checkers.get() == self.attacks_to(them, bitscan_1bit(our_king_bb)));
            assert!(self._pinned.get() == UNIVERSAL_SET ||
                    self._pinned.get() == self.find_pinned());
            assert!(self._king_square.get() > 63 ||
                    self._king_square.get() == bitscan_1bit(our_king_bb));
            true
        }
    }

    // A helper method for `push_piece_moves_to_sink`.
    //
    // It calculates pawn destination bitboards.
    #[inline]
    fn calc_pawn_dest_sets(&self, pawns: u64, en_passant_bb: u64, dest_sets: &mut [u64; 4]) {
        const PAWN_MOVE_QUIET: [u64; 4] = [UNIVERSAL_SET, UNIVERSAL_SET, EMPTY_SET, EMPTY_SET];
        const PAWN_MOVE_CANDIDATES: [u64; 4] = [!(BB_RANK_1 | BB_RANK_8),
                                                BB_RANK_2 | BB_RANK_7,
                                                !(BB_FILE_A | BB_RANK_1 | BB_RANK_8),
                                                !(BB_FILE_H | BB_RANK_1 | BB_RANK_8)];
        unsafe {
            let shifts: &[isize; 4] = PAWN_MOVE_SHIFTS.get_unchecked(self.to_move);
            let not_occupied_by_us = !*self.color.get_unchecked(self.to_move);
            let capture_targets = *self.color.get_unchecked(1 ^ self.to_move) | en_passant_bb;
            for i in 0..4 {
                *dest_sets.get_unchecked_mut(i) =
                    gen_shift(pawns & *PAWN_MOVE_CANDIDATES.get_unchecked(i),
                              *shifts.get_unchecked(i)) &
                    (capture_targets ^ *PAWN_MOVE_QUIET.get_unchecked(i)) &
                    not_occupied_by_us;
            }
            dest_sets[PAWN_DOUBLE_PUSH] &= gen_shift(dest_sets[PAWN_PUSH], shifts[PAWN_PUSH]);
        }
    }

    // A helper method for `generate_moves`.
    //
    // It finds all squares attacked by `piece` from square
    // `from_square`, and for each square that is within the
    // `legal_dests` set pushes a new move to `move_stack`. `piece`
    // can not be a pawn.
    #[inline]
    fn push_piece_moves_to_sink(&self,
                                piece: PieceType,
                                from_square: Square,
                                legal_dests: u64,
                                move_stack: &mut MoveStack) {
        assert!(piece < PAWN);
        assert!(from_square <= 63);
        let mut dest_set = unsafe {
            self.geometry.piece_attacks_from(self.occupied(), piece, from_square)
        } & legal_dests;
        while dest_set != EMPTY_SET {
            let dest_bb = ls1b(dest_set);
            dest_set ^= dest_bb;
            let dest_square = bitscan_1bit(dest_bb);
            let captured_piece = get_piece_type_at(&self.piece_type, self.occupied(), dest_bb);
            move_stack.push(Move::new(self.to_move,
                                      MOVE_NORMAL,
                                      piece,
                                      from_square,
                                      dest_square,
                                      captured_piece,
                                      self.en_passant_file,
                                      self.castling,
                                      0));
        }
    }

    // A helper method for `generate_moves()`.
    //
    // It finds all all possible moves by the set of pawns given by
    // `pawns`, making sure all pawn move destinations are within the
    // `legal_dests` set. Then it pushes the resulting moves to
    // `move_stack`. `en_passant_bb` represents the en-passant passing
    // square, if there is one. This function also recognizes and
    // discards the very rare case of pseudo-legal en-passant capture
    // that leaves discovered check on the 4/5-th rank.
    #[inline]
    fn push_pawn_moves_to_sink(&self,
                               pawns: u64,
                               en_passant_bb: u64,
                               legal_dests: u64,
                               only_queen_promotions: bool,
                               move_stack: &mut MoveStack) {
        // We differentiate 4 types of pawn moves: push, double push,
        // west-capture (capturing toward queen side), and
        // east-capture (capturing toward king side). The benefit of
        // this separation is that knowing the destination square and
        // the pawn move type (the index in the `dest_sets` array) is
        // enough to recover the origin square.
        let mut dest_sets: [u64; 4] = unsafe { uninitialized() };
        self.calc_pawn_dest_sets(pawns, en_passant_bb, &mut dest_sets);

        // Make sure all destination squares in all sets are legal.
        dest_sets[PAWN_DOUBLE_PUSH] &= legal_dests;
        dest_sets[PAWN_PUSH] &= legal_dests;
        dest_sets[PAWN_WEST_CAPTURE] &= legal_dests;
        dest_sets[PAWN_EAST_CAPTURE] &= legal_dests;

        // Scan each destination set (push, double push, west capture,
        // east capture). For each move calculate the "to" and "from"
        // sqares, and determinne the move type (en-passant capture,
        // pawn promotion, or a normal move).
        let shifts: &[isize; 4] = unsafe { PAWN_MOVE_SHIFTS.get_unchecked(self.to_move) };
        for i in 0..4 {
            let s = unsafe { dest_sets.get_unchecked_mut(i) };
            while *s != EMPTY_SET {
                let pawn_bb = ls1b(*s);
                *s ^= pawn_bb;
                let dest_square = bitscan_1bit(pawn_bb);
                let orig_square = (dest_square as isize -
                                   unsafe {
                    *shifts.get_unchecked(i)
                }) as Square;
                let captured_piece = get_piece_type_at(&self.piece_type, self.occupied(), pawn_bb);
                match pawn_bb {

                    // en-passant capture
                    x if x == en_passant_bb => {
                        if self.en_passant_special_check_ok(orig_square, dest_square) {
                            move_stack.push(Move::new(self.to_move,
                                                      MOVE_ENPASSANT,
                                                      PAWN,
                                                      orig_square,
                                                      dest_square,
                                                      PAWN,
                                                      self.en_passant_file,
                                                      self.castling,
                                                      0));
                        }
                    }

                    // pawn promotion
                    x if x & BB_PAWN_PROMOTION_RANKS != 0 => {
                        for p in 0..4 {
                            move_stack.push(Move::new(self.to_move,
                                                      MOVE_PROMOTION,
                                                      PAWN,
                                                      orig_square,
                                                      dest_square,
                                                      captured_piece,
                                                      self.en_passant_file,
                                                      self.castling,
                                                      p));
                            if only_queen_promotions {
                                break;
                            }
                        }
                    }

                    // normal pawn move (push or plain capture)
                    _ => {
                        move_stack.push(Move::new(self.to_move,
                                                  MOVE_NORMAL,
                                                  PAWN,
                                                  orig_square,
                                                  dest_square,
                                                  captured_piece,
                                                  self.en_passant_file,
                                                  self.castling,
                                                  0));
                    }
                }
            }
        }
    }

    // A helper method for `generate_moves`.
    //
    // It figures out which castling moves are pseudo-legal and pushes
    // them to `move_stack`.
    #[inline(always)]
    fn push_castling_moves_to_sink(&self, move_stack: &mut MoveStack) {

        // can not castle if in check
        if self.checkers() == EMPTY_SET {

            // try queen-side and king-side castling
            for side in 0..2 {

                // ensure squares between the king and the rook are empty
                if self.castling.obstacles(self.to_move, side) & self.occupied() == 0 {

                    // it seems castling is legal unless king's
                    // passing or final squares are attacked, but
                    // we do not care about that, because this
                    // will be verified in "do_move()".
                    move_stack.push(Move::new(self.to_move,
                                              MOVE_CASTLING,
                                              KING,
                                              self.king_square(),
                                              unsafe {
                                                  *[[C1, C8], [G1, G8]]
                                                       .get_unchecked(side)
                                                       .get_unchecked(self.to_move)
                                              },
                                              NO_PIECE,
                                              self.en_passant_file,
                                              self.castling,
                                              0));
                }
            }
        }
    }

    // A helper method for `generate_moves`.
    //
    // It returns all pinned pieces belonging to the side to
    // move. This is a relatively expensive operation.
    #[inline(always)]
    fn find_pinned(&self) -> u64 {
        let king_square = self.king_square();
        let occupied_by_them = unsafe { *self.color.get_unchecked(1 ^ self.to_move) };
        assert!(king_square <= 63);

        // To find all potential pinners, we remove all our pieces
        // from the board, and all enemy pieces that can not slide in
        // the particular manner (diagonally or straight). Then we
        // calculate what enemy pieces a bishop or a rook placed on
        // our king's square can attack. The attacked enemy pieces are
        // the potential pinners.
        let diag_sliders = occupied_by_them & (self.piece_type[QUEEN] | self.piece_type[BISHOP]);
        let straight_sliders = occupied_by_them & (self.piece_type[QUEEN] | self.piece_type[ROOK]);
        let mut pinners = unsafe {
            diag_sliders & self.geometry.piece_attacks_from(diag_sliders, BISHOP, king_square) |
            straight_sliders & self.geometry.piece_attacks_from(straight_sliders, ROOK, king_square)
        };

        if pinners == EMPTY_SET {
            EMPTY_SET
        } else {
            let occupied_by_us = unsafe { *self.color.get_unchecked(self.to_move) };
            let between_king_square_and = unsafe {
                self.geometry
                    .squares_between_including
                    .get_unchecked(king_square)
            };
            let blockers = occupied_by_us & !(1 << king_square) | (occupied_by_them & !pinners);
            let mut pinned_or_discovered_checkers = EMPTY_SET;

            // Scan all potential pinners and see if there is one and only
            // one piece between the pinner and our king.
            while pinners != EMPTY_SET {
                let pinner_square = bitscan_forward_and_reset(&mut pinners);
                let blockers_group = unsafe {
                    between_king_square_and.get_unchecked(pinner_square)
                } & blockers;
                if ls1b(blockers_group) == blockers_group {
                    // A group of blockers consisting of only one
                    // piece is either a pinned piece of ours or
                    // enemy's discovered checker.
                    pinned_or_discovered_checkers |= blockers_group;
                }
            }
            pinned_or_discovered_checkers & occupied_by_us
        }
    }

    // A helper method for `generate_moves`.
    //
    // It returns a bitboard representing the en-passant passing
    // square if there is one.
    #[inline]
    fn en_passant_bb(&self) -> u64 {
        assert!(self.en_passant_file <= NO_ENPASSANT_FILE);
        match self.en_passant_file {
            x if x >= NO_ENPASSANT_FILE => EMPTY_SET,
            x => {
                if self.to_move == WHITE {
                    1 << x << 40
                } else {
                    1 << x << 16
                }
            }
        }
    }

    // A helper method used by various other methods.
    //
    // It returns the square that the king of the side to move
    // occupies. The value is lazily calculated and saved for future
    // use.
    #[inline]
    fn king_square(&self) -> Square {
        if self._king_square.get() > 63 {
            self._king_square
                .set(bitscan_1bit(self.piece_type[KING] &
                                  unsafe { *self.color.get_unchecked(self.to_move) }));
        }
        self._king_square.get()
    }

    // A helper method for `do_move`.
    //
    // It returns `true` if had our king moved to square `square` it
    // would be in check, and `false` otherwise.
    #[inline]
    fn king_would_be_in_check(&self, square: Square) -> bool {
        let them = 1 ^ self.to_move;
        let occupied = self.occupied() & !(1 << self.king_square());
        assert!(them <= 1);
        assert!(square <= 63);
        unsafe {
            let occupied_by_them = *self.color.get_unchecked(them);

            (self.geometry.piece_attacks_from(occupied, ROOK, square) & occupied_by_them &
             (self.piece_type[ROOK] | self.piece_type[QUEEN])) != EMPTY_SET ||
            (self.geometry.piece_attacks_from(occupied, BISHOP, square) & occupied_by_them &
             (self.piece_type[BISHOP] | self.piece_type[QUEEN])) != EMPTY_SET ||
            (self.geometry.piece_attacks_from(occupied, KNIGHT, square) & occupied_by_them &
             self.piece_type[KNIGHT]) != EMPTY_SET ||
            (self.geometry.piece_attacks_from(occupied, KING, square) & occupied_by_them &
             self.piece_type[KING]) != EMPTY_SET ||
            {
                let shifts: &[isize; 4] = PAWN_MOVE_SHIFTS.get_unchecked(them);
                let square_bb = 1 << square;

                (gen_shift(square_bb, -shifts[PAWN_EAST_CAPTURE]) & occupied_by_them &
                 self.piece_type[PAWN] &
                 !(BB_FILE_H | BB_RANK_1 | BB_RANK_8)) != EMPTY_SET ||
                (gen_shift(square_bb, -shifts[PAWN_WEST_CAPTURE]) & occupied_by_them &
                 self.piece_type[PAWN] & !(BB_FILE_A | BB_RANK_1 | BB_RANK_8)) !=
                EMPTY_SET
            }
        }
    }

    // A helper method for `push_pawn_moves_to_sink`.
    //
    // It tests for the special case when an en-passant capture
    // discovers check on 4/5-th rank. This is the very rare occasion
    // when the two pawns participating in en-passant capture,
    // disappearing in one move, discover an unexpected check along
    // the horizontal (rank 4 of 5). `orig_square` and `dist_square`
    // are the origin square and the destination square of the
    // capturing pawn.
    fn en_passant_special_check_ok(&self, orig_square: Square, dest_square: Square) -> bool {
        let king_square = self.king_square();
        if (1 << king_square) & [BB_RANK_5, BB_RANK_4][self.to_move] == 0 {
            // the king is not on the 4/5-th rank -- we are done
            true
        } else {
            // the king is on the 4/5-th rank -- we have more work to do
            let the_two_pawns = 1 << orig_square |
                                gen_shift(1,
                                          dest_square as isize -
                                          PAWN_MOVE_SHIFTS[self.to_move][PAWN_PUSH]);
            let occupied = self.occupied() & !the_two_pawns;
            let occupied_by_them = self.color[1 ^ self.to_move] & !the_two_pawns;
            let checkers = unsafe {
                self.geometry.piece_attacks_from(occupied, ROOK, king_square)
            } & occupied_by_them &
                           (self.piece_type[ROOK] | self.piece_type[QUEEN]);
            checkers == EMPTY_SET
        }
    }

    // A helper method for `create`.
    //
    // It calculates the Zobrist hash for the board.
    fn calc_hash(&self) -> u64 {
        let mut hash = 0;
        for color in 0..2 {
            for piece in 0..6 {
                let mut bb = self.color[color] & self.piece_type[piece];
                while bb != EMPTY_SET {
                    let square = bitscan_forward_and_reset(&mut bb);
                    hash ^= self.zobrist.pieces[color][piece][square];
                }
            }
        }
        hash ^= self.zobrist.castling[self.castling.value()];
        if self.en_passant_file < 8 {
            hash ^= self.zobrist.en_passant[self.en_passant_file];
        }
        if self.to_move == BLACK {
            hash ^= self.zobrist.to_move;
        }
        hash
    }

    #[allow(dead_code)]
    fn pretty_string(&self) -> String {
        let mut s = String::new();
        for rank in (0..8).rev() {
            for file in 0..8 {
                let square = square(file, rank);
                let bb = 1 << square;
                let piece = match bb {
                    x if x & self.piece_type[KING] != 0 => 'k',
                    x if x & self.piece_type[QUEEN] != 0 => 'q',
                    x if x & self.piece_type[ROOK] != 0 => 'r',
                    x if x & self.piece_type[BISHOP] != 0 => 'b',
                    x if x & self.piece_type[KNIGHT] != 0 => 'n',
                    x if x & self.piece_type[PAWN] != 0 => 'p',
                    _ => '.',
                };
                if bb & self.color[WHITE] != 0 {
                    s.push(piece.to_uppercase().next().unwrap());
                } else {
                    s.push(piece);
                }
            }
            s.push('\n');
        }
        s
    }
}


// No passing pawn file.
const NO_ENPASSANT_FILE: usize = 8;


// Pawn move types
const PAWN_PUSH: usize = 0;
const PAWN_DOUBLE_PUSH: usize = 1;
const PAWN_WEST_CAPTURE: usize = 2;
const PAWN_EAST_CAPTURE: usize = 3;


// Pawn move shifts (one for each color and move type)
static PAWN_MOVE_SHIFTS: [[isize; 4]; 2] = [[8, 16, 7, 9], [-8, -16, -9, -7]];


// Bitboards that describe how the castling rook moves during the
// castling move.
const CASTLING_ROOK_MASK: [[u64; 2]; 2] = [[1 << A1 | 1 << D1, 1 << H1 | 1 << F1],
                                           [1 << A8 | 1 << D8, 1 << H8 | 1 << F8]];


/// Returns the type of the piece at a given square.
///
/// This function returns the piece type at the square represented by
/// the bitboard `square_bb`, on a board which is occupied with other
/// pieces according to the `piece_type_array` array and `occupied`
/// bitboard.
#[inline(always)]
fn get_piece_type_at(piece_type_array: &[u64; 6], occupied: u64, square_bb: u64) -> PieceType {
    assert!(square_bb != EMPTY_SET);
    assert_eq!(square_bb, ls1b(square_bb));
    let bb = square_bb & occupied;
    if bb == 0 {
        return NO_PIECE;
    }
    for i in (KING..NO_PIECE).rev() {
        if bb & unsafe { *piece_type_array.get_unchecked(i) } != 0 {
            return i;
        }
    }
    panic!("invalid board");
}


/// Loop-up tables for calculating Zobrist hashes.
struct ZobristArrays {
    pub to_move: u64,
    pub pieces: [[[u64; 64]; 6]; 2],
    pub castling: [u64; 16],

    /// Only the first 8 indexes of the `en_passant` array are
    /// initialized -- the rest remain zero. (They exist only for
    /// performance and memory safety reasons.)
    pub en_passant: [u64; 16],

    /// Derived from `pieces` for convenience. Contains the constants
    /// with which the Zobrist hash value should be XOR-ed to reflect
    /// the movement of the rook during castling.
    pub castling_rook_move: [[u64; 2]; 2],
}


impl ZobristArrays {
    /// Creates and initializes a new instance.
    pub fn new() -> ZobristArrays {
        use rand::{Rng, SeedableRng};
        use rand::isaac::Isaac64Rng;

        let seed: &[_] = &[1, 2, 3, 4];
        let mut rng: Isaac64Rng = SeedableRng::from_seed(seed);

        let to_move = rng.gen();
        let mut pieces = [[[0; 64]; 6]; 2];
        let mut castling = [0; 16];
        let mut en_passant = [0; 16];
        let mut castling_rook_move = [[0; 2]; 2];

        for color in 0..2 {
            for piece in 0..6 {
                for square in 0..64 {
                    pieces[color][piece][square] = rng.gen();
                }
            }
        }

        for value in 0..16 {
            castling[value] = rng.gen();
        }

        for file in 0..8 {
            en_passant[file] = rng.gen();
        }

        castling_rook_move[WHITE][QUEENSIDE] = pieces[WHITE][ROOK][A1] ^ pieces[WHITE][ROOK][D1];
        castling_rook_move[WHITE][KINGSIDE] = pieces[WHITE][ROOK][H1] ^ pieces[WHITE][ROOK][F1];
        castling_rook_move[BLACK][QUEENSIDE] = pieces[BLACK][ROOK][A8] ^ pieces[BLACK][ROOK][D8];
        castling_rook_move[BLACK][KINGSIDE] = pieces[BLACK][ROOK][H8] ^ pieces[BLACK][ROOK][F8];

        ZobristArrays {
            to_move: to_move,
            pieces: pieces,
            castling: castling,
            en_passant: en_passant,
            castling_rook_move: castling_rook_move,
        }
    }

    /// Returns a reference to an initialized `ZobristArrays` object.
    ///
    /// The object is created only during the first call. All next
    /// calls will return a reference to the same object. This is done
    /// in a thread-safe manner.
    pub fn get() -> &'static ZobristArrays {
        use std::sync::{Once, ONCE_INIT};
        static INIT_ARRAYS: Once = ONCE_INIT;
        static mut arrays: Option<ZobristArrays> = None;
        unsafe {
            INIT_ARRAYS.call_once(|| {
                arrays = Some(ZobristArrays::new());
            });
            arrays.as_ref().unwrap()
        }
    }
}


// The StateInfo struct stores information needed to restore a Position
// object to its previous state when we retract a move. Whenever a move
// is made on the board (by calling Position::do_move), a StateInfo
// object must be passed as a parameter.

// struct StateInfo {
//   Key pawnKey, materialKey;
//   Value npMaterial[COLOR_NB];
//   int castlingRights, rule50, pliesFromNull;
//   Score psq;
//   Square epSquare;

//   Key key;
//   Bitboard checkersBB;
//   PieceType capturedType;
//   StateInfo* previous;
// };


#[cfg(test)]
mod tests {
    use super::*;
    use basetypes::*;
    use chess_move::*;

    #[test]
    fn test_attacks_from() {
        use position::board_geometry::*;
        let b = Board::from_fen("k7/8/8/8/3P4/8/8/7K w - - 0 1").ok().unwrap();
        let g = BoardGeometry::get();
        unsafe {
            assert_eq!(g.piece_attacks_from(b.color[WHITE] | b.color[BLACK], BISHOP, A1),
                       1 << B2 | 1 << C3 | 1 << D4);
            assert_eq!(g.piece_attacks_from(b.color[WHITE] | b.color[BLACK], BISHOP, A1),
                       1 << B2 | 1 << C3 | 1 << D4);
            assert_eq!(g.piece_attacks_from(b.color[WHITE] | b.color[BLACK], KNIGHT, A1),
                       1 << B3 | 1 << C2);
        }
    }

    #[test]
    fn test_attacks_to() {
        let b = Board::from_fen("8/8/8/3K1p1P/r4k2/3Pq1N1/7p/1B5Q w - - 0 1").ok().unwrap();
        assert_eq!(b.attacks_to(WHITE, E4),
                   1 << D3 | 1 << G3 | 1 << D5 | 1 << H1);
        assert_eq!(b.attacks_to(BLACK, E4),
                   1 << E3 | 1 << F4 | 1 << F5 | 1 << A4);
        assert_eq!(b.attacks_to(BLACK, G6), 0);
        assert_eq!(b.attacks_to(WHITE, G6), 1 << H5);
        assert_eq!(b.attacks_to(WHITE, C2), 1 << B1);
        assert_eq!(b.attacks_to(WHITE, F4), 0);
        assert_eq!(b.attacks_to(BLACK, F4), 1 << A4 | 1 << E3);
        assert_eq!(b.attacks_to(BLACK, F5), 1 << F4);
        assert_eq!(b.attacks_to(WHITE, A6), 0);
        assert_eq!(b.attacks_to(BLACK, G1), 1 << H2 | 1 << E3);
        assert_eq!(b.attacks_to(BLACK, A1), 1 << A4);
    }

    #[test]
    fn test_piece_type_constants_constraints() {
        assert_eq!(KING, 0);
        assert_eq!(QUEEN, 1);
        assert_eq!(ROOK, 2);
        assert_eq!(BISHOP, 3);
        assert_eq!(KNIGHT, 4);
        assert_eq!(PAWN, 5);
    }

    #[test]
    fn test_pawn_dest_sets() {
        let mut stack = MoveStack::new();

        let b = Board::from_fen("k2q4/4Ppp1/5P2/6Pp/6P1/8/7P/7K w - h6 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        let mut pawn_dests = 0u64;
        while let Some(m) = stack.pop() {
            if m.piece() == PAWN {
                pawn_dests |= 1 << m.dest_square();
            }
        }
        assert_eq!(pawn_dests,
                   1 << H3 | 1 << H4 | 1 << G6 | 1 << E8 | 1 << H5 | 1 << G7 | 1 << H6 | 1 << D8);

        let b = Board::from_fen("k2q4/4Ppp1/5P2/6Pp/6P1/8/7P/7K b - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        let mut pawn_dests = 0u64;
        while let Some(m) = stack.pop() {
            if m.piece() == PAWN {
                pawn_dests |= 1 << m.dest_square();
            }
        }
        assert_eq!(pawn_dests, 1 << H4 | 1 << G6 | 1 << G4 | 1 << F6);
    }

    #[test]
    fn test_move_generation_1() {
        let mut stack = MoveStack::new();

        let b = Board::from_fen("8/8/6Nk/2pP4/3PR3/2b1q3/3P4/4K3 w - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 5);
        stack.clear();

        let b = Board::from_fen("8/8/6Nk/2pP4/3PR3/2b1q3/3P4/6K1 w - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 7);
        stack.clear();

        let b = Board::from_fen("8/8/6NK/2pP4/3PR3/2b1q3/3P4/7k w - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 8);
        stack.clear();

        let b = Board::from_fen("8/8/6Nk/2pP4/3PR3/2b1q3/3P4/7K w - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 22);
        stack.clear();

        let b = Board::from_fen("8/8/6Nk/2pP4/3PR3/2b1q3/3P4/7K w - c6 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 23);
        stack.clear();

        let b = Board::from_fen("K7/8/6N1/2pP4/3PR3/2b1q3/3P4/7k b - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 25);
        stack.clear();

        let b = Board::from_fen("K7/8/6N1/2pP4/3PR2k/2b1q3/3P4/8 b - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 5);
        stack.clear();
    }

    #[test]
    fn test_move_generation_2() {
        let mut stack = MoveStack::new();

        let b = Board::from_fen("8/8/8/7k/5pP1/8/8/5R1K b - g3 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 6);
        stack.clear();

        let b = Board::from_fen("8/8/8/5k2/5pP1/8/8/5R1K b - g3 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 7);
        stack.clear();

        let b = Board::from_fen("8/8/8/8/5pP1/7k/8/5B1K b - g3 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 5);
        stack.clear();
    }

    #[test]
    fn test_move_generation_3() {
        let mut stack = MoveStack::new();

        let b = Board::from_fen("8/8/8/8/4RpPk/8/8/7K b - g3 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 6);
        stack.clear();
    }

    #[test]
    fn test_move_generation_4() {
        let mut stack = MoveStack::new();

        let b = Board::from_fen("8/8/8/8/3QPpPk/8/8/7K b - g3 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 7);
        stack.clear();
    }

    #[test]
    fn test_move_generation_5() {
        let mut stack = MoveStack::new();

        let b = Board::from_fen("rn2k2r/8/8/8/8/8/8/R3K2R w - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 5);
        stack.clear();

        let b = Board::from_fen("rn2k2r/8/8/8/8/8/8/R3K2R w K - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 6);
        stack.clear();

        let b = Board::from_fen("rn2k2r/8/8/8/8/8/8/R3K2R w KQ - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 7);
        stack.clear();

        let b = Board::from_fen("rn2k2r/8/8/8/8/8/8/R3K2R b KQ - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 5);
        stack.clear();

        let b = Board::from_fen("rn2k2r/8/8/8/8/8/8/R3K2R b KQk - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 6);
        stack.clear();

        let b = Board::from_fen("4k3/8/8/8/8/5n2/8/R3K2R w KQ - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 5);
        stack.clear();

        let mut b = Board::from_fen("4k3/8/8/8/8/6n1/8/R3K2R w KQ - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        let mut count = 0;
        while let Some(m) = stack.pop() {
            if b.do_move(m) {
                count += 1;
                b.undo_move(m);
            }
        }
        assert_eq!(count, 19 + 4);

        let b = Board::from_fen("4k3/8/8/8/8/4n3/8/R3K2R w KQ - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 7);
        stack.clear();

        let b = Board::from_fen("4k3/8/8/8/8/4n3/8/R3K2R w - - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 5);
        stack.clear();

        let b = Board::from_fen("4k3/8/1b6/8/8/8/8/R3K2R w KQ - 0 1").ok().unwrap();
        b.generate_moves(true, &mut stack);
        assert_eq!(stack.len(), 19 + 7);
        stack.clear();
    }

    #[test]
    fn test_do_undo_move() {
        let mut stack = MoveStack::new();

        let mut b = Board::from_fen("b3k2r/6P1/8/5pP1/8/8/6P1/R3K2R w kKQ f6 0 1").ok().unwrap();
        let hash = b.hash();
        b.generate_moves(true, &mut stack);
        let count = stack.len();
        while let Some(m) = stack.pop() {
            if b.do_move(m) {
                assert!(hash != b.hash());
                b.undo_move(m);
                let mut other_stack = MoveStack::new();
                b.generate_moves(true, &mut other_stack);
                assert_eq!(count, other_stack.len());
                assert_eq!(hash, b.hash());
            }
        }
        assert_eq!(stack.len(), 0);
        let mut b = Board::from_fen("b3k2r/6P1/8/5pP1/8/8/8/R3K2R b kKQ - 0 1").ok().unwrap();
        let hash = b.hash();
        b.generate_moves(true, &mut stack);
        let count = stack.len();
        while let Some(m) = stack.pop() {
            if b.do_move(m) {
                assert!(hash != b.hash());
                b.undo_move(m);
                let mut other_stack = MoveStack::new();
                b.generate_moves(true, &mut other_stack);
                assert_eq!(count, other_stack.len());
                assert_eq!(hash, b.hash());
            }
        }
    }

    #[test]
    fn test_find_pinned() {
        use basetypes::*;
        let b = Board::from_fen("k2r4/3r4/3N4/5n2/qp1K2Pq/8/3PPR2/6b1 w - - 0 1").ok().unwrap();
        assert_eq!(b.find_pinned(), 1 << F2 | 1 << D6 | 1 << G4);
    }

    #[test]
    fn test_generate_only_captures() {
        let mut stack = MoveStack::new();

        let b = Board::from_fen("k6r/P7/8/6p1/6pP/8/8/7K b - h3 0 1").ok().unwrap();
        b.generate_moves(false, &mut stack);
        assert_eq!(stack.len(), 4);
        stack.clear();

        let b = Board::from_fen("k7/8/8/4Pp2/4K3/8/8/8 w - f6 0 1").ok().unwrap();
        b.generate_moves(false, &mut stack);
        assert_eq!(stack.len(), 8);
        stack.clear();

        let b = Board::from_fen("k7/8/8/4Pb2/4K3/8/8/8 w - - 0 1").ok().unwrap();
        b.generate_moves(false, &mut stack);
        assert_eq!(stack.len(), 7);
        stack.clear();
    }

    #[test]
    fn test_null_move() {
        let mut stack = MoveStack::new();

        let mut b = Board::from_fen("k7/8/8/5Pp1/8/8/8/4K2R w K g6 0 1").ok().unwrap();
        let hash = b.hash();
        b.generate_moves(true, &mut stack);
        let count = stack.len();
        stack.clear();
        let m = b.null_move();
        assert_eq!(b.do_move(m), true);
        assert!(hash != b.hash());
        b.undo_move(m);
        assert_eq!(hash, b.hash());
        b.generate_moves(true, &mut stack);
        assert_eq!(count, stack.len());
        stack.clear();

        let mut b = Board::from_fen("k7/4r3/8/8/8/8/8/4K3 w - - 0 1").ok().unwrap();
        let hash = b.hash();
        let m = b.null_move();
        assert_eq!(b.do_move(m), false);
        assert_eq!(hash, b.hash());
    }

    #[test]
    fn test_move_into_check_bug() {
        let mut stack = MoveStack::new();

        let mut b = Board::from_fen("rnbq1bn1/pppP3k/8/3P2B1/2B5/5N2/PPPN1PP1/2K4R b - - 0 1")
                        .ok()
                        .unwrap();
        b.generate_moves(true, &mut stack);
        let m = stack.pop().unwrap();
        b.do_move(m);
        assert!(b.is_legal());
    }

    #[test]
    fn test_try_move16() {
        fn try_all(b: &Board, stack: &MoveStack) {
            let mut i = 0;
            loop {
                if let Some(m) = b.try_move16(i) {
                    assert!(stack.iter().find(|x| **x == m).is_some());
                }
                if i == 0xffff {
                    break;
                } else {
                    i += 1;
                }
            }
        }

        let mut stack = MoveStack::new();
        let b = Board::from_fen("rnbqk2r/p1p1pppp/8/8/2Pp4/5NP1/pP1PPPBP/RNBQK2R b KQkq c3 0 \
                                     1")
                    .ok()
                    .unwrap();
        b.generate_moves(true, &mut stack);
        try_all(&b, &stack);

        stack.clear();
        let b = Board::from_fen("rnbqk2r/p1p1pppp/8/8/Q1Pp4/5NP1/pP1PPPBP/RNB1K2R b KQkq - 0 \
                                 1")
                    .ok()
                    .unwrap();
        b.generate_moves(true, &mut stack);
        try_all(&b, &stack);

        stack.clear();
        let b = Board::from_fen("rnbqk2r/p1p1pppp/3N4/8/Q1Pp4/6P1/pP1PPPBP/RNB1K2R b KQkq - 0 \
                                 1")
                    .ok()
                    .unwrap();
        b.generate_moves(true, &mut stack);
        try_all(&b, &stack);

        stack.clear();
        let b = Board::from_fen("rnbq3r/p1p1pppp/8/3k4/2Pp4/5NP1/pP1PPPBP/RNBQK2R b KQ c3 0 \
                                     1")
                    .ok()
                    .unwrap();
        b.generate_moves(true, &mut stack);
        try_all(&b, &stack);

        stack.clear();
        let b = Board::from_fen("rn1qk2r/p1pbpppp/8/8/Q1Pp4/5NP1/pP1PPPBP/RNB1K2R b KQkq - 0 \
                                 1")
                    .ok()
                    .unwrap();
        b.generate_moves(true, &mut stack);
        try_all(&b, &stack);

        stack.clear();
        let b = Board::from_fen("8/8/8/8/4RpPk/8/8/7K b - g3 0 1")
                    .ok()
                    .unwrap();
        b.generate_moves(true, &mut stack);
        try_all(&b, &stack);

        stack.clear();
        let b = Board::from_fen("8/8/8/8/5pPk/8/8/7K b - g3 0 1")
                    .ok()
                    .unwrap();
        b.generate_moves(true, &mut stack);
        try_all(&b, &stack);
    }
}
