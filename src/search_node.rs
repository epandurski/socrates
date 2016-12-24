//! Defines the `SearchNode` trait.

use uci::SetOption;
use board::{Board, IllegalBoard};
use moves::{Move, MoveDigest, AddMove};
use value::*;
use evaluator::Evaluator;
use qsearch::QsearchResult;


/// A trait for chess positions -- a convenient interface for the
/// tree-searching algorithm.
///
/// A `SearchNode` can generate all legal moves in the current
/// position, play a selected move and take it back. It can also
/// quickly (without doing extensive tree-searching) evaluate the
/// chances of the sides, so that the tree-searching algorithm can use
/// this evaluation to assign realistic game outcomes to its leaf
/// nodes. `SearchNode` improves on `MoveGenerator` by adding the
/// following functionality:
///
/// 1. Smart position hashing.
/// 2. Exact evaluation of final positions.
/// 3. Quiescence search.
/// 4. 50 move rule awareness.
/// 5. Threefold/twofold repetition detection.
///
/// **Important note:** Repeating positions are considered a draw
/// after the first repetition, not after the second one as the chess
/// rules prescribe. In order to compensate for that,
/// `SearchNode::from_history` "forgets" all positions that have
/// occurred exactly once. Also, the newly created instance is never
/// deemed a draw due to repetition or rule-50.
pub trait SearchNode: Send + Clone + SetOption {
    /// The type of static evaluator that the implementation works
    /// with.
    type Evaluator: Evaluator;

    /// The type of result object that `evaluate_quiescence` returns.
    type QsearchResult: QsearchResult;

    /// Instantiates a new chess position from playing history.
    ///
    /// `fen` should be the Forsyth–Edwards Notation of a legal
    /// starting position. `moves` should be an iterator over all the
    /// moves that were played from that position. The move format is
    /// long algebraic notation. Examples: `e2e4`, `e7e5`, `e1g1`
    /// (white short castling), `e7e8q` (for promotion).
    fn from_history(fen: &str, moves: &mut Iterator<Item = &str>) -> Result<Self, IllegalBoard>;

    /// Returns an almost unique hash value for the position.
    ///
    /// The returned value is good for use as transposition table key.
    fn hash(&self) -> u64;

    /// Returns a reference to the underlying `Board` instance.
    fn board(&self) -> &Board;

    /// Returns the number of half-moves since the last piece capture
    /// or pawn advance.
    fn halfmove_clock(&self) -> u8;

    /// Returns if the side to move is in check.
    fn is_check(&self) -> bool;

    /// Returns a reference to a static evaluator bound to the current
    /// position.
    fn evaluator(&self) -> &Self::Evaluator;

    /// Evaluates a final position.
    ///
    /// In final positions this method will return the correct value
    /// of the position (`0` for a draw, `VALUE_MIN` for a
    /// checkmate). A position is guaranteed to be final if
    /// `generate_moves` method generates no legal moves. (It may
    /// generate some pseudo-legal moves, but if none of them is
    /// legal, then the position is final.)
    fn evaluate_final(&self) -> Value;

    /// Performs quiescence search and returns a result.
    ///
    /// Quiescence search is a restricted search which considers only
    /// a limited set of moves (for example: winning captures, pawn
    /// promotions to queen, check evasions). The goal is to
    /// statically evaluate only "quiet" positions (positions where
    /// there are no winning tactical moves to be made). Although this
    /// search can cheaply and correctly resolve many simple tactical
    /// issues, it is completely blind to the more complex ones.
    ///
    /// `lower_bound` and `upper_bound` together give the interval
    /// within which an as precise as possible evaluation is
    /// required. If during the calculation is determined that the
    /// exact evaluation is outside of this interval, this method may
    /// return a value that is closer to the the interval bounds than
    /// the exact evaluation, but always staying on the correct side
    /// of the interval. `static_evaluation` should be position's
    /// static evaluation, or `VALUE_UNKNOWN`.
    ///
    /// **Important note:** This method will return a reliable result
    /// even when the side to move is in check. Repeated and rule-50
    /// positions are always evaluated to `0`.
    fn evaluate_quiescence(&self,
                           lower_bound: Value,
                           upper_bound: Value,
                           static_evaluation: Value)
                           -> Self::QsearchResult;

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
    /// `generate_moves`, `try_move_digest`, or `null_move` methods
    /// for the current position on the board.
    fn evaluate_move(&self, m: Move) -> Value;

    /// Generates all legal moves, possibly including some
    /// pseudo-legal moves too.
    ///
    /// A pseudo-legal move is a move that is otherwise legal, except
    /// it might leave the king in check. Every legal move is a
    /// pseudo-legal move, but not every pseudo-legal move is legal.
    /// The generated moves will be added to `moves`. If all of the
    /// moves generated by this methods are illegal (this means that
    /// `do_move(m)` returns `false` for all of them), then the
    /// position is final, and `evaluate_final()` will return its
    /// correct value.
    ///
    /// **Important note:** No moves will be generated in repeated and
    /// rule-50 positions.
    fn generate_moves<T: AddMove>(&self, moves: &mut T);

    /// Checks if `move_digest` represents a pseudo-legal move.
    ///
    /// If a move `m` exists that would be generated by
    /// `generate_moves` if called for the current position, and for
    /// that move `m.digest() == move_digest`, this method will
    /// return `Some(m)`. Otherwise it will return `None`. This is
    /// useful when playing moves from the transposition table,
    /// without calling `generate_moves`.
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
    /// It the move leaves the king in check, `false` is returned
    /// without updating the board. Otherwise the board is updated and
    /// `true` is returned. The move passed to this method must have
    /// been generated by `generate_moves`, `try_move_digest`, or
    /// `null_move` methods for the current position on the board.
    ///
    /// **Important note:** For null moves, if the position is a draw
    /// due to repetition or rule-50, `do_move` will return `false`.
    fn do_move(&mut self, m: Move) -> bool;

    /// Takes back the last played move.
    fn undo_move(&mut self);

    /// Returns all legal moves in the position.
    ///
    /// No moves are returned for repeated and rule-50 positions.
    ///
    /// **Important note:** This method is slower than
    /// `generate_moves` because it ensures that all returned moves
    /// are legal.
    fn legal_moves(&self) -> Vec<Move> {
        let mut position = self.clone();
        let mut legal_moves = Vec::with_capacity(96);
        let mut v = Vec::with_capacity(96);
        position.generate_moves(&mut v);
        for m in v.iter() {
            if position.do_move(*m) {
                legal_moves.push(*m);
                position.undo_move();
            }
        }
        legal_moves
    }
}