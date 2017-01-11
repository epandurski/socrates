//! Defines the `MoveGenerator` trait.

use std::mem::uninitialized;
use std::cmp::max;
use uci::SetOption;
use board::*;
use moves::*;
use value::*;
use evaluator::Evaluator;
use bitsets::*;
use utils::BoardGeometry;


/// A trait for move generators.
///
/// A `MoveGenerator` holds a chess position and can:
///
/// * Generate all legal moves, or a subset of all legal moves in the
///   current position.
///
/// * Perform static exchange evaluation for the generated moves.
///
/// * Play a selected move and take it back.
///
/// * Provide a static evaluator bound to the current position.
///
/// * Calculate Zobrist hashes.
///
/// **Important note:** `MoveGenerator` is unaware of repeating
/// positions and rule-50.
pub trait MoveGenerator: Sized + Send + Clone + SetOption {
    /// The type of static evaluator that the implementation works
    /// with.
    type Evaluator: Evaluator;

    /// Creates a new instance, consuming the supplied `Board`
    /// instance.
    ///
    /// Returns `Err(IllegalBoard)` if the position is illegal.
    fn from_board(board: Board) -> Result<Self, IllegalBoard>;

    /// Returns the Zobrist hash value for the underlying `Board`
    /// instance.
    ///
    /// Zobrist hashing is a technique to transform a board position
    /// into a number of a fixed length, with an equal distribution
    /// over all possible numbers, invented by Albert Zobrist. The key
    /// property of this method is that two similar positions generate
    /// entirely different hash numbers.
    ///
    /// **Important note:** This method will be relatively slow if the
    /// implementation calculates the hash value "from
    /// scratch". Inspect the implementation before using `hash` in
    /// time-critical paths. (See `do_move`.)
    fn hash(&self) -> u64;

    /// Returns a reference to the underlying `Board` instance.
    fn board(&self) -> &Board;

    /// Returns a bitboard with all pieces that attack `square`.
    fn attacks_to(&self, square: Square) -> Bitboard;

    /// Returns a bitboard with all enemy pieces that attack the king.
    ///
    /// **Important note:** The bitboard of checkers is calculated on
    /// the first call to `checkers`, and is stored in case another
    /// call is made before doing/undoing any moves. In that case
    /// `checkers` returns the saved bitboard instead of
    /// re-calculating it, thus saving time.
    fn checkers(&self) -> Bitboard;

    /// Returns a reference to a static evaluator bound to the current
    /// position.
    fn evaluator(&self) -> &Self::Evaluator;

    /// Generates all legal moves, possibly including some
    /// pseudo-legal moves too.
    ///
    /// The moves are added to `moves`. All generated moves with
    /// pieces other than the king will be legal. Some of the
    /// generated king's moves may be illegal because the destination
    /// square is under attack. This arrangement has two important
    /// advantages:
    ///
    /// * `do_move` can do its work without knowing the set of
    ///   checkers and pinned pieces, so there is no need to keep
    ///   those around.
    ///
    /// * A beta cut-off may make the verification that king's
    ///   destination square is not under attack unnecessary, thus
    ///   saving time.
    ///
    /// The initial move score for the generated moves is
    /// *unspecified*.
    ///
    /// **Note:** A pseudo-legal move is a move that is otherwise
    /// legal, except it might leave the king in check.
    fn generate_all<T: AddMove>(&self, moves: &mut T);

    /// Generates moves for the quiescence search.
    ///
    /// The moves are added to `moves`. This method always generates a
    /// **subset** of the moves generated by `generate_all`:
    ///
    /// * If the king is in check, all legal moves are included.
    ///
    /// * Captures and pawn promotions to queen are always included.
    ///
    /// * If `generate_checks` is `true`, moves that give check are
    ///   included too. Discovered checks and checks given by castling
    ///   can be omitted for speed.
    fn generate_forcing<T: AddMove>(&self, generate_checks: bool, moves: &mut T);

    /// Checks if `move_digest` represents a pseudo-legal move.
    ///
    /// If a move `m` exists that would be generated by
    /// `generate_all` if called for the current position on the
    /// board, and for that move `m.digest() == move_digest`, this
    /// method will return `Some(m)`. Otherwise it will return
    /// `None`. This is useful when playing moves from the
    /// transposition table, without calling `generate_all`.
    fn try_move_digest(&self, move_digest: MoveDigest) -> Option<Move>;

    /// Returns a null move.
    ///
    /// "Null move" is a pseudo-move that changes only the side to
    /// move. It is sometimes useful to include a speculative null
    /// move in the search tree so as to achieve more aggressive
    /// pruning. Null moves are represented as king's moves for which
    /// the origin and destination squares are the same.
    fn null_move(&self) -> Move;

    /// Plays a move on the board.
    ///
    /// It verifies if the move is legal. If the move is legal, the
    /// board is updated and an `u64` value is returned, which should
    /// be XOR-ed with old board's hash value to obtain new board's
    /// hash value. If the move is illegal, `None` is returned without
    /// updating the board. The move passed to this method **must**
    /// have been generated by `generate_all`, `generate_forcing`,
    /// `try_move_digest`, or `null_move` methods for the current
    /// position on the board.
    ///
    /// The moves generated by the `null_move` method are
    /// exceptions. For them `do_move` will return `None` if and only
    /// if the king is in check.
    fn do_move(&mut self, m: Move) -> Option<u64>;

    /// Takes back last played move.
    ///
    /// The move passed to this method **must** be the last move passed
    /// to `do_move`.
    fn undo_move(&mut self, m: Move);

    /// Returns the likely evaluation change (material) to be lost or
    /// gained as a result of a given move.
    ///
    /// This method performs static exchange evaluation (SEE). It
    /// examines the consequence of a series of exchanges on the
    /// destination square after a given move. A positive returned
    /// value indicates a "winning" move. For example, "PxQ" will
    /// always be a win, since the pawn side can choose to stop the
    /// exchange after its pawn is recaptured, and still be ahead. SEE
    /// is just an evaluation calculated without actually trying moves
    /// on the board, and therefore the returned value might be
    /// incorrect.
    ///
    /// The move passed to this method must have been generated by
    /// `generate_all`, `generate_forcing`, `try_move_digest`, or
    /// `null_move` methods for the current position on the board.
    fn evaluate_move(&self, m: Move) -> Value {
        debug_assert!(m.played_piece() < PIECE_NONE);
        debug_assert!(m.captured_piece() <= PIECE_NONE);
        const PIECE_VALUES: [Value; 8] = [10000, 975, 500, 325, 325, 100, 0, 0];

        unsafe {
            let mut piece = m.played_piece();
            let captured_piece = m.captured_piece();

            // Try not to waste CPU cycles when the played piece is
            // less valuable than the captured piece.
            if piece > captured_piece {
                return *PIECE_VALUES.get_unchecked(captured_piece);
            }

            // This is the square on which all the action takes place.
            let exchange_square = m.dest_square();

            let color: &[Bitboard; 2] = &self.board().pieces.color;
            let piece_type: &[Bitboard; 6] = &self.board().pieces.piece_type;
            let straight_sliders = piece_type[QUEEN] | piece_type[ROOK];
            let diag_sliders = piece_type[QUEEN] | piece_type[BISHOP];
            let geometry = BoardGeometry::get();
            let behind_blocker: &[Bitboard; 64] = geometry.squares_behind_blocker
                                                          .get_unchecked(exchange_square);

            // These variables (along with `piece`) will be updated on each capture:
            let mut us = self.board().to_move;
            let mut depth = 0;
            let mut orig_square_bb = 1 << m.orig_square();
            let mut attackers_and_defenders = self.attacks_to(exchange_square);

            // The `gain` array will hold the total material gained at
            // each `depth`, from the viewpoint of the side that made the
            // last capture (`us`).
            let mut gain: [Value; 34] = uninitialized();
            gain[0] = if m.move_type() == MOVE_PROMOTION {
                piece = Move::piece_from_aux_data(m.aux_data());
                PIECE_VALUES[captured_piece] + PIECE_VALUES[piece] - PIECE_VALUES[PAWN]
            } else {
                *PIECE_VALUES.get_unchecked(captured_piece)
            };

            // Examine the possible exchanges, fill the `gain` array.
            'exchange: while orig_square_bb != 0 {
                let current_gain = *gain.get_unchecked(depth);

                // Store a speculative value that will be used if the
                // captured piece happens to be defended.
                let speculative_gain: &mut Value = gain.get_unchecked_mut(depth + 1);
                *speculative_gain = *PIECE_VALUES.get_unchecked(piece) - current_gain;

                if max(-current_gain, *speculative_gain) < 0 {
                    // The side that made the last capture wins even if
                    // the captured piece happens to be defended. So, we
                    // stop here to save precious CPU cycles. Note that
                    // here we may happen to return an incorrect SEE
                    // value, but the sign will be correct, which is by
                    // far the most important information.
                    break;
                }

                // Register that capturing piece's origin square is now vacant.
                attackers_and_defenders &= !orig_square_bb;

                // Consider adding new attackers/defenders, now that
                // capturing piece's origin square is vacant.
                let behind = self.board().occupied &
                             *behind_blocker.get_unchecked(bsf(orig_square_bb));
                if behind & (straight_sliders | diag_sliders) != 0 && piece != KING {
                    attackers_and_defenders |=
                        match behind & straight_sliders &
                              geometry.attacks_from_unsafe(ROOK, exchange_square, behind) {
                            0 => {
                                // Not a straight slider, possibly a diagonal slider.
                                behind & diag_sliders &
                                geometry.attacks_from_unsafe(BISHOP, exchange_square, behind)
                            }
                            bb => {
                                // A straight slider.
                                bb
                            }
                        };
                }

                // Change the side to move.
                us ^= 1;

                // Find the next piece to enter the exchange. (The least
                // valuable piece belonging to the side to move.)
                let candidates = attackers_and_defenders & *color.get_unchecked(us);
                if candidates != 0 {
                    for p in (KING..PIECE_NONE).rev() {
                        let bb = candidates & piece_type[p];
                        if bb != 0 {
                            depth += 1;
                            piece = p;
                            orig_square_bb = lsb(bb);
                            continue 'exchange;
                        }
                    }
                }
                break 'exchange;
            }

            // Negamax the `gain` array for the final static exchange
            // evaluation. (The `gain` array actually represents an unary
            // tree, at each node of which the player can either continue
            // the exchange or back off.)
            while depth > 0 {
                *gain.get_unchecked_mut(depth - 1) = -max(-*gain.get_unchecked(depth - 1),
                                                          *gain.get_unchecked(depth));
                depth -= 1;
            }
            gain[0]
        }
    }
}
