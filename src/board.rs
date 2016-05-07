use basetypes::*;
use bitsets::*;

type PawnMoveType = usize;

// Pawn move types
const PAWN_PUSH: PawnMoveType = 0;
const PAWN_DOUBLE_PUSH: PawnMoveType = 1;
const PAWN_QUEENSIDE_CAPTURE: PawnMoveType = 2;
const PAWN_KINGSIDE_CAPTURE: PawnMoveType = 3;

// Pawn move tables
static PAWN_MOVE_QUIET: [u64; 4] = [UNIVERSAL_SET, UNIVERSAL_SET, EMPTY_SET, EMPTY_SET];
static PAWN_MOVE_SHIFTS: [[i8; 4]; 2] = [[8, 16, 7, 9], [-8, -16, -9, -7]];
static PAWN_MOVE_CANDIDATES: [u64; 4] = [!(BB_RANK_1 | BB_RANK_8),
                                         BB_RANK_2 | BB_RANK_7,
                                         !(BB_FILE_A | BB_RANK_1 | BB_RANK_8),
                                         !(BB_FILE_H | BB_RANK_1 | BB_RANK_8)];

// Pawn useful constants
const PAWN_PROMOTION_RANKS: u64 = BB_RANK_1 | BB_RANK_8;


pub struct Board {
    geometry: &'static BoardGeometry,
    pub piece_type: [u64; 6],
    pub color: [u64; 2],
    pub occupied: u64,
}

impl Board {
    // Create a new board instance.
    pub fn new(piece_type_array: &[u64; 6], color_array: &[u64; 2]) -> Board {
        // TODO: Make sure the position is valid. Or rather this is
        // responsibility for the "Position" type?!
        assert!(piece_type_array.into_iter().fold(0, |acc, x| acc | x) ==
                color_array[WHITE] | color_array[BLACK]);
        assert!(piece_type_array[PAWN] & PAWN_PROMOTION_RANKS == 0);
        assert!(piece_type_array[PAWN] & PAWN_PROMOTION_RANKS == 0);
        Board {
            geometry: board_geometry(),
            piece_type: *piece_type_array,
            color: *color_array,
            occupied: color_array[WHITE] | color_array[BLACK],
        }
    }

    // Return the set of squares that have on them pieces (or pawns)
    // of color "us" that attack the square "square" directly (no
    // x-rays).
    pub fn attacks_to(&self, us: Color, square: Square) -> u64 {
        attacks_to(self.geometry,
                   &self.piece_type,
                   &self.color,
                   self.occupied,
                   square,
                   us)
    }

    // A Static Exchange Evaluation (SEE) examines the consequence of
    // a series of exchanges on a single square after a given move,
    // and calculates the likely evaluation change (material) to be
    // lost or gained, Donald Michie coined the term swap-off value. A
    // positive static exchange indicates a "winning" move. For
    // example, PxQ will always be a win, since the Pawn side can
    // choose to stop the exchange after its Pawn is recaptured, and
    // still be ahead.
    //
    // The impemented algorithm creates a swap-list of best case
    // material gains by traversing a square attacked/defended by set
    // in least valuable piece order from pawn, knight, bishop, rook,
    // queen until king, with alternating sides. The swap-list, an
    // unary tree since there are no branches but just a series of
    // captures, is negamaxed for a final static exchange evaluation.
    //
    // The returned value is the material that is expected to be
    // gained in the exchange by the attacking side
    // ("attacking_color"), when capturing the "target_piece" on the
    // "target_square". The "from_square" specifies the square from
    // which the "attacking_piece" makes the capture.
    pub fn calc_see(&self,
                    mut attacking_color: Color,
                    from_square: Square,
                    mut attacking_piece: PieceType,
                    to_square: Square,
                    target_piece: PieceType)
                    -> Value {
        use std::mem::uninitialized;
        use std::cmp::max;
        static VALUE: [Value; 6] = [10000, 975, 500, 325, 325, 100];

        let geometry = self.geometry;
        let piece_type_array = &self.piece_type;
        let color_array = &self.color;
        let mut occupied = self.occupied;
        let mut depth = 0;
        let mut attackers_and_defenders = attacks_to(geometry,
                                                     piece_type_array,
                                                     color_array,
                                                     occupied,
                                                     to_square,
                                                     WHITE) |
                                          attacks_to(geometry,
                                                     piece_type_array,
                                                     color_array,
                                                     occupied,
                                                     to_square,
                                                     BLACK);
        let mut from_square_bb = 1 << from_square;

        // "may_xray" pieces may block x-ray attacks from other
        // pieces, so we must consider adding new attackers/defenders
        // every time a "may_xray"-piece makes a capture.
        let may_xray = piece_type_array[PAWN] | piece_type_array[BISHOP] | piece_type_array[ROOK] |
                       piece_type_array[QUEEN];
        unsafe {
            let mut gain: [Value; 33] = uninitialized();
            gain[depth] = VALUE[target_piece];
            while from_square_bb != EMPTY_SET {
                depth += 1;  // next depth
                attacking_color ^= 1;  // next side
                gain[depth] = VALUE[attacking_piece] - gain[depth - 1];  // speculative store, if defended
                if max(-gain[depth - 1], gain[depth]) < 0 {
                    break;  // pruning does not influence the outcome
                }
                attackers_and_defenders ^= from_square_bb;
                occupied ^= from_square_bb;
                if from_square_bb & may_xray != EMPTY_SET {
                    attackers_and_defenders |= consider_xrays(geometry,
                                                              piece_type_array,
                                                              occupied,
                                                              to_square,
                                                              bitscan_forward(from_square_bb));
                }
                assert_eq!(occupied | attackers_and_defenders, occupied);

                // find the next piece in the exchange
                let next_attack = get_least_valuable_piece_in_a_set(piece_type_array,
                                                                    attackers_and_defenders &
                                                                    color_array[attacking_color]);
                attacking_piece = next_attack.0;
                from_square_bb = next_attack.1;
            }
            depth -= 1;  // discard the speculative store
            while depth > 0 {
                gain[depth - 1] = -max(-gain[depth - 1], gain[depth]);
                depth -= 1;
            }
            gain[0]
        }
    }

    // Generate all legal moves in the current board position.
    //
    // "us" is the side to move. "king_square" should be the moving
    // side king's square. "checkers" should represent all pieces that
    // give check. "pinned" should represent all pinned pieces (and
    // pawns). "castling_rights" gives the current castling
    // rights. "en_passant_bb" is a bitboard that contains 1 for the
    // passing square (if there is one). "move_stack" is the global
    // moves stack.
    //
    // Returns the number of moves that have been generated.
    pub fn generate_moves(&self,
                          us: Color,
                          king_square: Square,
                          checkers: u64,
                          pinned: u64,
                          en_passant_bb: u64,
                          // castling_rights: CastlingRights,
                          move_stack: &mut MoveStack)
                          -> usize {
        let mut counter = 0;
        let geometry = self.geometry;
        let pin_lines = &geometry.squares_at_line[king_square];
        let piece_type_array = &self.piece_type;
        let occupied = self.occupied;
        let occupied_by_us = self.color[us];
        let legal_dests = !occupied_by_us &
                          match ls1b(checkers) {
            0 => {
                // Not in check -- every move destination may be
                // considered "covering".
                UNIVERSAL_SET
            }
            x if x == checkers => {
                // Single check -- calculate the check covering
                // destination subset (the squares between the king
                // and the checker). Notice that we must OR with "x"
                // itself, because knights give check not lying on a
                // line with the king.
                x | geometry.squares_between_including[king_square][bitscan_1bit(x)]
            }
            _ => {
                // Double check -- no covering moves.
                EMPTY_SET
            }
        };

        if legal_dests != EMPTY_SET {

            // Find all queen, rook, bishop, and knight moves.
            for piece in QUEEN..PAWN {
                let bb = piece_type_array[piece] & occupied_by_us;
                let mut pinned_pieces = bb & pinned;
                let mut free_pieces = bb ^ pinned_pieces;
                while pinned_pieces != EMPTY_SET {
                    let from_square = bitscan_and_clear(&mut pinned_pieces);
                    let mut dest_set = piece_attacks_from(geometry, occupied, from_square, piece) &
                                       pin_lines[from_square] &
                                       legal_dests;
                    counter += self.write_piece_moves_to_stack(piece,
                                                               from_square,
                                                               &mut dest_set,
                                                               move_stack);
                }
                while free_pieces != EMPTY_SET {
                    let from_square = bitscan_and_clear(&mut free_pieces);
                    let mut dest_set = piece_attacks_from(geometry, occupied, from_square, piece) &
                                       legal_dests;
                    counter += self.write_piece_moves_to_stack(piece,
                                                               from_square,
                                                               &mut dest_set,
                                                               move_stack);
                }
            }

            // When in check, en-passant capture is a legal evasion
            // move only when the checking piece is the passing pawn
            // itself. To determine if the checker is the passing
            // pawn, or if there is a discovered check we take
            // advantage of the fact that if the checker itself is the
            // only square on the check-line, then we can not have a
            // discovered check.
            let pawn_legal_dests = match legal_dests == checkers {
                false => legal_dests,
                true => legal_dests | en_passant_bb,
            };

            let all_pawns = piece_type_array[PAWN] & occupied_by_us;
            let mut pinned_pawns = all_pawns & pinned;
            let free_pawns = all_pawns ^ pinned_pawns;

            // Find pinned pawn moves pawn by pawn.
            while pinned_pawns != EMPTY_SET {
                let pawn_bb = ls1b(pinned_pawns);
                pinned_pawns ^= pawn_bb;
                let pin_line = pin_lines[bitscan_1bit(pawn_bb)];
                counter += self.write_pawn_moves_to_stack(us,
                                                          pawn_bb,
                                                          en_passant_bb,
                                                          pin_line & pawn_legal_dests,
                                                          move_stack);
            }

            // Find all free pawn moves at once.
            counter += self.write_pawn_moves_to_stack(us,
                                                      free_pawns,
                                                      en_passant_bb,
                                                      pawn_legal_dests,
                                                      move_stack);
        }

        // TODO: We must try to move the king here!
        counter
    }

    // Generate array with pawn destination sets.
    //
    // We differentiate 4 types of pawn moves: single push, double
    // push, queen-side capture (capturing toward queen side),
    // king-side capture (capturing toward king side). The benefit of
    // this separation is that knowing the destination square and the
    // pawn move type (the index in the destination sets array) is
    // enough to recover the origin square.
    //
    // "us" is the side to move, "pawns" is a bit-set of pawns which
    // we want to generate moves for, "en_passant_bb" is a bit-set
    // describing the en-passant square if there is one.
    //
    // Returns an array of 4 bit-sets (1 for each pawn move type),
    // describing all pseudo-legal destination squares. (Pseudo-legal
    // means that we may sill leave the king under check.)
    #[inline(always)]
    fn pawn_dest_sets(&self, us: Color, pawns: u64, en_passant_bb: u64) -> [u64; 4] {
        use std::mem::uninitialized;
        let shifts = &PAWN_MOVE_SHIFTS[us];
        let not_occupied_by_us = !self.color[us];
        let capture_targets = self.color[1 ^ us] | en_passant_bb;
        unsafe {
            let mut dest_sets: [u64; 4] = uninitialized();
            for move_type in 0..4 {
                dest_sets[move_type] = gen_shift(pawns & PAWN_MOVE_CANDIDATES[move_type],
                                                 shifts[move_type]) &
                                       not_occupied_by_us &
                                       (capture_targets ^ PAWN_MOVE_QUIET[move_type]);
            }
            // A double-push is legal only if a single-push is legal too.
            dest_sets[PAWN_DOUBLE_PUSH] &= gen_shift(dest_sets[PAWN_PUSH], shifts[PAWN_PUSH]);
            dest_sets
        }
    }

    // Check for the special case when an en-passant capture discovers
    // check on 4/5-th rank. This is the very rare occasion when the
    // two pawns participating in en-passant capture, disappearing in
    // one move, discover an unexpected check along the horizontal
    // (rank 4 of 5).
    #[inline(always)]
    fn en_passant_special_check_ok(&self,
                                   us: Color,
                                   orig_square: Square,
                                   dest_square: Square)
                                   -> bool {
        const EN_PASSANT_SPECIAL_CHECK_RANKS: [u64; 2] = [BB_RANK_5, BB_RANK_4];
        let king_bb = self.piece_type[KING] & self.color[us];
        assert_eq!(king_bb, ls1b(king_bb));
        if king_bb & EN_PASSANT_SPECIAL_CHECK_RANKS[us] != 0 {
            let the_two_pawns = 1 << orig_square |
                                gen_shift(1, dest_square as i8 - PAWN_MOVE_SHIFTS[us][PAWN_PUSH]);
            let king_square = bitscan_1bit(king_bb);
            let occupied = self.occupied & !the_two_pawns;
            let occupied_by_them = self.color[1 ^ us] & !the_two_pawns;
            let checkers = piece_attacks_from(self.geometry, occupied, king_square, ROOK) &
                           occupied_by_them &
                           (self.piece_type[ROOK] | self.piece_type[QUEEN]);
            checkers == EMPTY_SET
        } else {
            true
        }
    }

    #[inline(always)]
    fn write_piece_moves_to_stack(&self,
                                  piece: PieceType,
                                  orig_square: Square,
                                  dest_set: &mut u64,
                                  move_stack: &mut MoveStack)
                                  -> usize {
        let mut counter = 0;
        let occupied = self.occupied;
        let piece_type_array = &self.piece_type;
        while *dest_set != EMPTY_SET {
            let dest_bb = ls1b(*dest_set);
            *dest_set ^= dest_bb;
            let dest_square = bitscan_1bit(dest_bb);
            let captured_piece = get_piece_type_at(occupied, piece_type_array, dest_bb);
            move_stack.push(Move::new(MOVE_NORMAL, orig_square, dest_square, 0),
                            MoveScore::new(piece, captured_piece));
            counter += 1;
        }
        counter
    }

    #[inline]
    fn write_pawn_moves_to_stack(&self,
                                 us: Color,
                                 pawns: u64,
                                 en_passant_bb: u64,
                                 legal_dests: u64,
                                 move_stack: &mut MoveStack)
                                 -> usize {
        let mut counter = 0;
        let occupied = self.occupied;
        let piece_type_array = &self.piece_type;
        let mut dest_sets = self.pawn_dest_sets(us, pawns, en_passant_bb);

        // Make sure all destination squares in all sets are legal.
        dest_sets[PAWN_PUSH] &= legal_dests;
        dest_sets[PAWN_DOUBLE_PUSH] &= legal_dests;
        dest_sets[PAWN_QUEENSIDE_CAPTURE] &= legal_dests;
        dest_sets[PAWN_KINGSIDE_CAPTURE] &= legal_dests;

        // Scan each destination set (push, double-push, queen-side
        // capture, king-side capture). For each move calculate the "to"
        // and "from" sqares, and determinne the move type (en-passant
        // capture, pawn promotion, or a normal move).
        let shifts = &PAWN_MOVE_SHIFTS[us];
        for move_type in 0..4 {
            let s = &mut dest_sets[move_type];
            while *s != EMPTY_SET {
                let pawn_bb = ls1b(*s);
                *s ^= pawn_bb;
                let dest_square = bitscan_1bit(pawn_bb);
                let orig_square = (dest_square as i8 - shifts[move_type]) as Square;
                match pawn_bb {
                    // en-passant capture
                    x if x == en_passant_bb => {
                        if self.en_passant_special_check_ok(us, orig_square, dest_square) {
                            counter += 1;
                            move_stack.push(Move::new(MOVE_ENPASSANT, orig_square, dest_square, 0),
                                            MoveScore::new(PAWN, PAWN));

                        }
                    }
                    // pawn promotion
                    x if x & PAWN_PROMOTION_RANKS != 0 => {
                        for pp_code in 0..4 {
                            counter += 1;
                            move_stack.push(Move::new(MOVE_PROMOTION,
                                                      orig_square,
                                                      dest_square,
                                                      pp_code),
                                            MoveScore::new(PAWN,
                                                           if pp_code == 0 {
                                                               QUEEN
                                                           } else {
                                                               ROOK  // a lie, helps move ordering
                                                           }));
                        }
                    }
                    // normal pawn move (push or plain capture)
                    _ => {
                        counter += 1;
                        move_stack.push(Move::new(MOVE_NORMAL, orig_square, dest_square, 0),
                                        MoveScore::new(PAWN,
                                                       get_piece_type_at(occupied,
                                                                         piece_type_array,
                                                                         pawn_bb)));
                    }
                }
            }
        }
        counter
    }
}


fn board_geometry() -> &'static BoardGeometry {
    use std::sync::{Once, ONCE_INIT};
    static INIT_GEOMETRY: Once = ONCE_INIT;
    static mut geometry: Option<BoardGeometry> = None;
    unsafe {
        INIT_GEOMETRY.call_once(|| {
            geometry = Some(BoardGeometry::new());
        });
        match geometry {
            Some(ref x) => x,
            None => panic!("board geometry not initialized"),
        }
    }
}


pub struct BoardGeometry {
    grid: [u8; 120],
    piece_grid_deltas: [[i8; 8]; 5],
    piece_longrange: [bool; 5],
    pub attacks: [[u64; 64]; 5],
    pub blockers_and_beyond: [[u64; 64]; 5],
    pub squares_at_line: [[u64; 64]; 64],
    pub squares_between_including: [[u64; 64]; 64],
    pub squares_behind_blocker: [[u64; 64]; 64],
}

impl BoardGeometry {
    pub fn new() -> BoardGeometry {
        // We use 10x12 grid (8x8 with guarding markers, 2 at top and
        // bottom, 1 at the sides), so that we can detect out-of-board
        // movements. Each cell in the grid contains the corresponding
        // square number (from 0 to 63) or 0xff (the guarding marker).
        let mut grid = [0xffu8; 120];
        for i in 0..64 {
            grid[BoardGeometry::grid_index_from_square(i)] = i as u8;
        }

        // "piece_deltas" represent the change in the grid-index when
        // sliding a particular piece by one square in a particular
        // direction. We are not concerned with pawns here.
        let mut piece_grid_deltas = [[0i8; 8]; 5];
        piece_grid_deltas[QUEEN] = [-11, -10, -9, -1, 1, 9, 10, 11];
        piece_grid_deltas[ROOK] = [0, -10, 0, -1, 1, 0, 10, 0];
        piece_grid_deltas[BISHOP] = [-11, 0, -9, 0, 0, 9, 0, 11];
        piece_grid_deltas[KNIGHT] = [-21, -19, -12, -8, 8, 12, 19, 21];
        piece_grid_deltas[KING] = [-11, -10, -9, -1, 1, 9, 10, 11];

        // All pieces except knights and kings are long-range (They
        // can slide by more than one square). We are not concerned
        // with pawns here.
        let mut piece_longrange = [true; 5];
        piece_longrange[KNIGHT] = false;
        piece_longrange[KING] = false;

        let mut bg = BoardGeometry {
            grid: grid,
            piece_grid_deltas: piece_grid_deltas,
            piece_longrange: piece_longrange,
            attacks: [[0u64; 64]; 5],
            blockers_and_beyond: [[0u64; 64]; 5],
            squares_at_line: [[0u64; 64]; 64],
            squares_between_including: [[0u64; 64]; 64],
            squares_behind_blocker: [[0u64; 64]; 64],
        };

        // "attacks" and "blockers_and_beyond" fields hold attack and
        // blockers bitsets for each piece on each possible square.
        // For example:
        //
        // g.attacks[QUEEN][D4]  g.blockers_and_beyond[QUEEN][D4]
        // . . . 1 . . . 1       . . . . . . . .
        // 1 . . 1 . . 1 .       . . . 1 . . 1 .
        // . 1 . 1 . 1 . .       . 1 . 1 . 1 . .
        // . . 1 1 1 . . .       . . 1 1 1 . . .
        // 1 1 1 Q 1 1 1 1       . 1 1 Q 1 1 1 .
        // . . 1 1 1 . . .       . . 1 1 1 . . .
        // . 1 . 1 . 1 . .       . 1 . 1 . 1 . .
        // 1 . . 1 . . 1 .       . . . . . . . .
        //
        // g.attacks[KNIGHT][D4] g.blockers_and_beyond[KNIGHT][D4]
        // . . . . . . . .       . . . . . . . .
        // . . . . . . . .       . . . . . . . .
        // . . 1 . 1 . . .       . . . . . . . .
        // . 1 . . . 1 . .       . . . . . . . .
        // . . . N . . . .       . . . N . . . .
        // . 1 . . . 1 . .       . . . . . . . .
        // . . 1 . 1 . . .       . . . . . . . .
        // . . . . . . . .       . . . . . . . .
        bg.fill_attack_and_blockers_and_beyond_arrays();

        // The "squares_behind_blocker" field holds bitsets that
        // describe all squares hidden behind a blocker from the
        // attacker's position. For example:
        //
        // g.squares_behind_blocker[B2][F6]
        // . . . . . . . 1
        // . . . . . . 1 .
        // . . . . . B . .
        // . . . . . . . .
        // . . . . . . . .
        // . . . . . . . .
        // . A . . . . . .
        // . . . . . . . .
        //
        // The "squares_between_including" field holds bitsets that
        // describe all squares between an attacker an a blocker
        // including the attacker's and blocker's fields
        // themselves. For example:
        //
        // g.squares_between_including[B2][F6]
        // . . . . . . . .
        // . . . . . . . .
        // . . . . . 1 . .
        // . . . . 1 . . .
        // . . . 1 . . . .
        // . . 1 . . . . .
        // . 1 . . . . . .
        // . . . . . . . .
        bg.fill_squares_between_including_and_squares_behind_blocker_arrays();

        // The "squares_at_line" field holds bitsets that describe all
        // squares lying at the line determined by the attacker and
        // the blocker. For example:
        //
        // g.squares_at_line[B2][F6]
        // . . . . . . . 1
        // . . . . . . 1 .
        // . . . . . 1 . .
        // . . . . 1 . . .
        // . . . 1 . . . .
        // . . 1 . . . . .
        // . 1 . . . . . .
        // 1 . . . . . . .
        bg.fill_squares_at_line_array();

        bg
    }

    fn grid_index(&self, i: Square) -> usize {
        Self::grid_index_from_square(i)
    }

    #[inline(always)]
    fn grid_index_from_square(i: Square) -> usize {
        ((i / 8) * 10 + (i % 8) + 21)
    }

    fn fill_attack_and_blockers_and_beyond_arrays(&mut self) {
        for piece in 0..5 {
            for square in 0..64 {
                let mut attack = 0u64;
                let mut blockers = 0u64;
                for move_direction in 0..8 {
                    let delta = self.piece_grid_deltas[piece][move_direction];
                    if delta != 0 {
                        let mut last_mask = 0u64;
                        let mut curr_grid_index = self.grid_index(square);
                        loop {
                            curr_grid_index = (curr_grid_index as i8 + delta) as usize;
                            let curr_square = self.grid[curr_grid_index] as Square;
                            if curr_square != 0xff {
                                last_mask = 1 << curr_square;
                                attack |= last_mask;
                                blockers |= last_mask;
                                if self.piece_longrange[piece] {
                                    continue;
                                }
                            }
                            blockers &= !last_mask;
                            break;
                        }
                    }
                }
                self.attacks[piece][square] = attack;
                self.blockers_and_beyond[piece][square] = blockers;
            }
        }
    }

    fn fill_squares_between_including_and_squares_behind_blocker_arrays(&mut self) {
        for attacker in 0..64 {
            for blocker in 0..64 {
                // Try to find a grid-index increment (delta) that
                // will generate all squares at the line. If the
                // attacker and the blocker happens not to lie at a
                // straight line, then and we simply proceed to the
                // next attacker/blocker pair.
                let rank_diff = rank(blocker) as i8 - rank(attacker) as i8;
                let file_diff = file(blocker) as i8 - file(attacker) as i8;
                let delta = match (rank_diff, file_diff) {
                    (0, 0) => continue,
                    (0, f) => f.signum(),
                    (r, 0) => 10 * r.signum(),
                    (r, f) if r == f => 10 * r.signum() + r.signum(),
                    (r, f) if r == -f => 10 * r.signum() - r.signum(),
                    _ => continue,
                };

                // Starting from the attacker's square update
                // "squares_between_including" until the blocker's
                // square is encountered, then switch to updating
                // "squares_behind_blocker" until the end of the board
                // is reached.
                let mut squares_between_including = 0u64;
                let mut squares_behind_blocker = 0u64;
                let mut curr_grid_index = self.grid_index(attacker);
                let mut blocker_encountered = false;
                loop {
                    let curr_square = self.grid[curr_grid_index] as Square;
                    match curr_square {
                        0xff => {
                            break;
                        }
                        x if x == blocker => {
                            squares_between_including |= 1 << curr_square;
                            blocker_encountered = true;
                        }
                        _ => {
                            if blocker_encountered {
                                squares_behind_blocker |= 1 << curr_square;
                            } else {
                                squares_between_including |= 1 << curr_square;
                            }
                        }
                    }
                    curr_grid_index = (curr_grid_index as i8 + delta) as usize;
                }
                assert!(blocker_encountered);
                self.squares_between_including[attacker][blocker] = squares_between_including;
                self.squares_behind_blocker[attacker][blocker] = squares_behind_blocker;
            }
        }
    }

    fn fill_squares_at_line_array(&mut self) {
        for a in 0..64 {
            for b in 0..64 {
                self.squares_at_line[a][b] = self.squares_between_including[a][b] |
                                             self.squares_behind_blocker[a][b] |
                                             self.squares_behind_blocker[b][a];
            }
        }
    }
}


// Return the set of squares that have on them pieces (or pawns)
// of color "us" that attack the square "square" directly (no
// x-rays).
#[inline(always)]
fn attacks_to(geometry: &BoardGeometry,
              piece_type_array: &[u64; 6],
              color_array: &[u64; 2],
              occupied: u64,
              square: Square,
              us: Color)
              -> u64 {
    let occupied_by_us = color_array[us];
    let shifts = &PAWN_MOVE_SHIFTS[us];
    let square_bb = 1 << square;
    let pawns = piece_type_array[PAWN];
    let queens = piece_type_array[QUEEN];
    let mut attacks = piece_attacks_from(geometry, occupied, square, ROOK) & occupied_by_us &
                      (piece_type_array[ROOK] | queens);
    attacks |= piece_attacks_from(geometry, occupied, square, BISHOP) & occupied_by_us &
               (piece_type_array[BISHOP] | queens);
    attacks |= piece_attacks_from(geometry, occupied, square, KNIGHT) & occupied_by_us &
               piece_type_array[KNIGHT];
    attacks |= piece_attacks_from(geometry, occupied, square, KING) & occupied_by_us &
               piece_type_array[KING];
    attacks |= gen_shift(square_bb, -shifts[PAWN_KINGSIDE_CAPTURE]) & occupied_by_us &
               pawns & !(BB_FILE_H | BB_RANK_1 | BB_RANK_8);
    attacks |= gen_shift(square_bb, -shifts[PAWN_QUEENSIDE_CAPTURE]) & occupied_by_us &
               pawns & !(BB_FILE_A | BB_RANK_1 | BB_RANK_8);
    attacks
}


// Return a bit-set describing all pieces that can attack
// "target_square" once "xrayed_square" becomes vacant.
//
// This is a helper function for the static exchange evaluation
// (Board::calc_see)
#[inline(always)]
fn consider_xrays(geometry: &BoardGeometry,
                  piece_type_array: &[u64; 6],
                  occupied: u64,
                  target_square: Square,
                  xrayed_square: Square)
                  -> u64 {
    let candidates = occupied & geometry.squares_behind_blocker[target_square][xrayed_square];
    let diag_attackers = piece_attacks_from(geometry, candidates, target_square, BISHOP) &
                         (piece_type_array[QUEEN] | piece_type_array[BISHOP]);
    let line_attackers = piece_attacks_from(geometry, candidates, target_square, ROOK) &
                         (piece_type_array[QUEEN] | piece_type_array[ROOK]);
    assert_eq!(diag_attackers & line_attackers, EMPTY_SET);
    assert_eq!(ls1b(candidates & diag_attackers),
               candidates & diag_attackers);
    assert_eq!(ls1b(candidates & line_attackers),
               candidates & line_attackers);
    candidates & (diag_attackers | line_attackers)
}


// Return the least valuble piece in the subset "set".
//
// This is a helper function for the static exchange evaluation
// (Board::calc_see)
#[inline(always)]
fn get_least_valuable_piece_in_a_set(piece_type_array: &[u64; 6], set: u64) -> (PieceType, u64) {
    for p in (0..6).rev() {
        let piece_subset = piece_type_array[p] & set;
        if piece_subset != EMPTY_SET {
            return (p, ls1b(piece_subset));
        }
    }
    (NO_PIECE, EMPTY_SET)
}


// Return the set of squares that are attacked by a piece (not a pawn)
// of type "piece" from the square "square", on a board which is
// occupied with other pieces according to the "occupied"
// bit-set. "geometry" supplies the look-up tables needed to perform
// the calculation.
#[inline(always)]
pub fn piece_attacks_from(geometry: &BoardGeometry,
                          occupied: u64,
                          square: Square,
                          piece: PieceType)
                          -> u64 {
    assert!(piece != PAWN);
    let behind = geometry.squares_behind_blocker[square];
    let mut attacks = geometry.attacks[piece][square];
    let mut blockers = occupied & geometry.blockers_and_beyond[piece][square];
    while blockers != EMPTY_SET {
        attacks &= !behind[bitscan_and_clear(&mut blockers)];
    }
    attacks
}


// Return the piece type at the square represented by the bit-set
// "square_bb", on a board which is occupied with other pieces
// according to the "piece_type" array and "occupied" bit-set and.
#[inline(always)]
fn get_piece_type_at(occupied: u64, piece_type_array: &[u64; 6], square_bb: u64) -> PieceType {
    assert!(square_bb != EMPTY_SET);
    assert_eq!(square_bb, ls1b(square_bb));
    match square_bb & occupied {
        EMPTY_SET => NO_PIECE,
        x if x & piece_type_array[PAWN] != 0 => PAWN,
        x if x & piece_type_array[KNIGHT] != 0 => KNIGHT,
        x if x & piece_type_array[BISHOP] != 0 => BISHOP,
        x if x & piece_type_array[ROOK] != 0 => ROOK,
        x if x & piece_type_array[QUEEN] != 0 => QUEEN,
        x if x & piece_type_array[KING] != 0 => KING,
        _ => panic!("invalid board"),
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
    use super::board_geometry;

    #[test]
    fn test_attack_sets() {
        use basetypes::*;
        let g = board_geometry();
        assert_eq!(g.attacks[KING][A1], 0b11 << 8 | 0b10);
        assert_eq!(g.blockers_and_beyond[KING][A1], 0);
        assert_eq!(g.attacks[ROOK][A1],
                   0b11111110 | 1 << 8 | 1 << 16 | 1 << 24 | 1 << 32 | 1 << 40 | 1 << 48 | 1 << 56);
        assert_eq!(g.blockers_and_beyond[ROOK][A1],
                   0b01111110 | 1 << 8 | 1 << 16 | 1 << 24 | 1 << 32 | 1 << 40 | 1 << 48 | 0 << 56);
        assert_eq!(g.attacks[KING][D4], g.attacks[KING][E4] >> 1);
        assert_eq!(g.attacks[KING][D4], g.attacks[KING][D5] >> 8);
        assert_eq!(g.attacks[KNIGHT][D4], g.attacks[KNIGHT][D5] >> 8);
        assert_eq!(g.attacks[KNIGHT][D4] & g.attacks[KING][D5],
                   1 << C6 | 1 << E6);
        assert_eq!(g.attacks[ROOK][D4] | g.attacks[BISHOP][D4],
                   g.attacks[QUEEN][D4]);
        assert_eq!(g.attacks[ROOK][D4] & g.attacks[BISHOP][D4], 0);
        assert_eq!(g.attacks[KING][D4] & g.attacks[QUEEN][D4],
                   g.attacks[KING][D4]);
        assert_eq!(g.attacks[BISHOP][E1] & g.attacks[KNIGHT][H1],
                   1 << F2 | 1 << G3);
    }

    #[test]
    fn test_line_sets() {
        use basetypes::*;
        let g = board_geometry();
        assert_eq!(g.squares_at_line[B1][G1], 0b11111111);
        assert_eq!(g.squares_at_line[G8][B8], 0b11111111 << 56);
        assert_eq!(g.squares_between_including[B1][G1], 0b01111110);
        assert_eq!(g.squares_between_including[G8][B8], 0b01111110 << 56);
        assert_eq!(g.squares_behind_blocker[B1][G1], 1 << H1);
        assert_eq!(g.squares_behind_blocker[G8][B8], 1 << A8);
        assert_eq!(g.squares_behind_blocker[A1][G7], 1 << H8);
        assert_eq!(g.squares_behind_blocker[H1][B7], 1 << A8);
        assert_eq!(g.squares_behind_blocker[B7][G2], 1 << H1);
        assert_eq!(g.squares_behind_blocker[G7][B2], 1 << A1);
        assert_eq!(g.squares_behind_blocker[D7][D7], 0);
        assert_eq!(g.squares_behind_blocker[D7][F8], 0);
        assert_eq!(g.squares_between_including[A1][A4] | g.squares_behind_blocker[A1][A4],
                   g.squares_at_line[A1][A4]);
    }

    #[test]
    fn test_attacks_from() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[PAWN] |= 1 << D4;
        piece_type[PAWN] |= 1 << G7;
        color[WHITE] = piece_type[PAWN];
        let b = Board::new(&piece_type, &color);
        let g = board_geometry();
        assert_eq!(piece_attacks_from(g, b.occupied, A1, BISHOP),
                   1 << B2 | 1 << C3 | 1 << D4);
        assert_eq!(piece_attacks_from(g, b.occupied, A1, BISHOP),
                   1 << B2 | 1 << C3 | 1 << D4);
        assert_eq!(piece_attacks_from(g, b.occupied, A1, KNIGHT),
                   1 << B3 | 1 << C2);
    }

    #[test]
    fn test_attacks_to() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[PAWN] |= 1 << D3;
        color[WHITE] |= 1 << D3;
        piece_type[PAWN] |= 1 << H5;
        color[WHITE] |= 1 << H5;
        piece_type[KNIGHT] |= 1 << G3;
        color[WHITE] |= 1 << G3;
        piece_type[BISHOP] |= 1 << B1;
        color[WHITE] |= 1 << B1;
        piece_type[QUEEN] |= 1 << H1;
        color[WHITE] |= 1 << H1;
        piece_type[KING] |= 1 << D5;
        color[WHITE] |= 1 << D5;
        piece_type[PAWN] |= 1 << H2;
        color[BLACK] |= 1 << H2;
        piece_type[PAWN] |= 1 << F5;
        color[BLACK] |= 1 << F5;
        piece_type[ROOK] |= 1 << A4;
        color[BLACK] |= 1 << A4;
        piece_type[QUEEN] |= 1 << E3;
        color[BLACK] |= 1 << E3;
        piece_type[KING] |= 1 << F4;
        color[BLACK] |= 1 << F4;
        let b = Board::new(&piece_type, &color);
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
        use basetypes::*;
        assert_eq!(KING, 0);
        assert_eq!(QUEEN, 1);
        assert_eq!(ROOK, 2);
        assert_eq!(BISHOP, 3);
        assert_eq!(KNIGHT, 4);
        assert_eq!(PAWN, 5);
    }

    #[test]
    fn test_static_exchange_evaluation() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[KING] |= 1 << A3;
        color[BLACK] |= 1 << A3;
        piece_type[QUEEN] |= 1 << E5;
        color[BLACK] |= 1 << E5;
        piece_type[ROOK] |= 1 << F8;
        color[BLACK] |= 1 << F8;
        piece_type[BISHOP] |= 1 << D2;
        color[BLACK] |= 1 << D2;
        piece_type[PAWN] |= 1 << G5;
        color[BLACK] |= 1 << G5;
        piece_type[KING] |= 1 << A1;
        color[WHITE] |= 1 << A1;
        piece_type[PAWN] |= 1 << A2;
        color[WHITE] |= 1 << A2;
        piece_type[PAWN] |= 1 << E3;
        color[WHITE] |= 1 << E3;
        piece_type[PAWN] |= 1 << G3;
        color[WHITE] |= 1 << G3;
        piece_type[PAWN] |= 1 << D4;
        color[WHITE] |= 1 << D4;
        piece_type[BISHOP] |= 1 << H2;
        color[WHITE] |= 1 << H2;
        piece_type[ROOK] |= 1 << F1;
        color[WHITE] |= 1 << F1;
        piece_type[ROOK] |= 1 << F2;
        color[WHITE] |= 1 << F2;
        let b = Board::new(&piece_type, &color);
        assert_eq!(b.calc_see(BLACK, E5, QUEEN, E3, PAWN), 100);
        assert_eq!(b.calc_see(BLACK, E5, QUEEN, D4, PAWN), -875);
        assert_eq!(b.calc_see(WHITE, G3, PAWN, F4, PAWN), 100);
        assert_eq!(b.calc_see(BLACK, A3, KING, A2, PAWN), -9900);
    }

    #[test]
    fn test_move_scores() {
        use basetypes::*;
        let mut ms = MoveScore::new(PAWN, QUEEN);
        assert_eq!(ms.attacking_piece(), PAWN);
        assert_eq!(ms.target_piece(), QUEEN);
        assert!(ms > MoveScore::new(KNIGHT, QUEEN));
        assert!(ms > MoveScore::new(PAWN, ROOK));
        assert_eq!(ms, MoveScore::new(PAWN, QUEEN));
        let ms2 = ms;
        assert_eq!(ms, ms2);
        ms.set_bit(6);
        assert!(ms > ms2);
        assert_eq!(ms.attacking_piece(), PAWN);
        assert_eq!(ms.target_piece(), QUEEN);
        ms.clear_bit(6);
        assert_eq!(ms, ms2);
    }

    #[test]
    fn test_pawn_dest_sets() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[PAWN] |= 1 << E7;
        color[WHITE] |= 1 << E7;
        piece_type[PAWN] |= 1 << H2;
        color[WHITE] |= 1 << H2;
        piece_type[PAWN] |= 1 << G4;
        color[WHITE] |= 1 << G4;
        piece_type[PAWN] |= 1 << G5;
        color[WHITE] |= 1 << G5;
        piece_type[PAWN] |= 1 << F6;
        color[WHITE] |= 1 << F6;
        piece_type[PAWN] |= 1 << F7;
        color[BLACK] |= 1 << F7;
        piece_type[PAWN] |= 1 << G7;
        color[BLACK] |= 1 << G7;
        piece_type[PAWN] |= 1 << H5;
        color[BLACK] |= 1 << H5;
        piece_type[QUEEN] |= 1 << D8;
        color[BLACK] |= 1 << D8;
        let b = Board::new(&piece_type, &color);
        let ds = b.pawn_dest_sets(WHITE, b.piece_type[PAWN] & b.color[WHITE], 1 << H6);
        assert_eq!(ds[0], 1 << H3 | 1 << G6 | 1 << E8);
        assert_eq!(ds[1], 1 << H4);
        assert_eq!(ds[3], 1 << H5 | 1 << G7 | 1 << H6);
        assert_eq!(ds[2], 1 << D8);
        let ds = b.pawn_dest_sets(BLACK, b.piece_type[PAWN] & b.color[BLACK], 0);
        assert_eq!(ds[0], 1 << H4 | 1 << G6);
        assert_eq!(ds[1], 0);
        assert_eq!(ds[3], 0);
        assert_eq!(ds[2], 1 << G4 | 1 << F6);
    }

    #[test]
    fn test_move_generation_1() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[PAWN] |= 1 << D5;
        color[WHITE] |= 1 << D5;
        piece_type[PAWN] |= 1 << D2;
        color[WHITE] |= 1 << D2;
        piece_type[PAWN] |= 1 << D4;
        color[WHITE] |= 1 << D4;
        piece_type[ROOK] |= 1 << E4;
        color[WHITE] |= 1 << E4;
        piece_type[PAWN] |= 1 << C5;
        color[BLACK] |= 1 << C5;
        piece_type[KNIGHT] |= 1 << G6;
        color[WHITE] |= 1 << G6;
        piece_type[BISHOP] |= 1 << C3;
        color[BLACK] |= 1 << C3;
        piece_type[QUEEN] |= 1 << E3;
        color[BLACK] |= 1 << E3;
        let b = Board::new(&piece_type, &color);

        // White to move, king on E1:
        assert_eq!(b.generate_moves(WHITE, E1, 1 << E3, 1 << D2, 0, &mut MoveStack::new()),
                   1);
        // White to move, king on G1:
        assert_eq!(b.generate_moves(WHITE, G1, 1 << E3, 0, 0, &mut MoveStack::new()),
                   2);
        // White to move, king on H6:
        assert_eq!(b.generate_moves(WHITE, H6, 1 << E3, 0, 0, &mut MoveStack::new()),
                   4);
        // White to move, king on H1 (no check):
        assert_eq!(b.generate_moves(WHITE, H1, 0, 0, 0, &mut MoveStack::new()),
                   19);
        // White to move, king on H1 (no check), en-passant on C6:
        assert_eq!(b.generate_moves(WHITE, H1, 0, 0, 1 << C6, &mut MoveStack::new()),
                   20);
        // Black to move, king on H1 (no check):
        assert_eq!(b.generate_moves(BLACK, H1, 0, 0, 0, &mut MoveStack::new()),
                   22);
        // Black to move, king on H4:
        assert_eq!(b.generate_moves(BLACK, H4, 1 << E4 | 1 << G6, 0, 0, &mut MoveStack::new()),
                   0);
    }

    #[test]
    fn test_move_generation_2() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[PAWN] |= 1 << G4;
        color[WHITE] |= 1 << G4;
        piece_type[ROOK] |= 1 << F1;
        color[WHITE] |= 1 << F1;
        piece_type[PAWN] |= 1 << F4;
        color[BLACK] |= 1 << F4;
        piece_type[KING] |= 1 << H5;
        color[BLACK] |= 1 << H5;
        let b = Board::new(&piece_type, &color);
        assert_eq!(b.generate_moves(BLACK, H5, 1 << G4, 0, 1 << G3, &mut MoveStack::new()),
                   1);

        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[PAWN] |= 1 << G4;
        color[WHITE] |= 1 << G4;
        piece_type[ROOK] |= 1 << F1;
        color[WHITE] |= 1 << F1;
        piece_type[PAWN] |= 1 << F4;
        color[BLACK] |= 1 << F4;
        piece_type[KING] |= 1 << F5;
        color[BLACK] |= 1 << F5;
        let b = Board::new(&piece_type, &color);
        assert_eq!(b.generate_moves(BLACK, F5, 1 << G4, 1 << F4, 1 << G3, &mut MoveStack::new()),
                   0);

        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[PAWN] |= 1 << G4;
        color[WHITE] |= 1 << G4;
        piece_type[BISHOP] |= 1 << F1;
        color[WHITE] |= 1 << F1;
        piece_type[PAWN] |= 1 << F4;
        color[BLACK] |= 1 << F4;
        piece_type[KING] |= 1 << H3;
        color[BLACK] |= 1 << H3;
        let b = Board::new(&piece_type, &color);
        assert_eq!(b.generate_moves(BLACK, H3, 1 << F1, 0, 1 << G3, &mut MoveStack::new()),
                   0);
    }

    #[test]
    fn test_move_generation_3() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[KING] |= 1 << H1;
        color[WHITE] |= 1 << H1;
        piece_type[PAWN] |= 1 << G4;
        color[WHITE] |= 1 << G4;
        piece_type[ROOK] |= 1 << E4;
        color[WHITE] |= 1 << E4;
        piece_type[KING] |= 1 << H4;
        color[BLACK] |= 1 << H4;
        piece_type[PAWN] |= 1 << F4;
        color[BLACK] |= 1 << F4;
        let b = Board::new(&piece_type, &color);
        assert_eq!(b.generate_moves(BLACK, H4, 0, 0, 1 << G3, &mut MoveStack::new()),
                   1);
    }

    #[test]
    fn test_move_generation_4() {
        use basetypes::*;
        let mut piece_type = [0u64; 6];
        let mut color = [0u64; 2];
        piece_type[KING] |= 1 << H1;
        color[WHITE] |= 1 << H1;
        piece_type[PAWN] |= 1 << G4;
        color[WHITE] |= 1 << G4;
        piece_type[PAWN] |= 1 << E4;
        color[WHITE] |= 1 << E4;
        piece_type[QUEEN] |= 1 << D4;
        color[WHITE] |= 1 << D4;
        piece_type[KING] |= 1 << H4;
        color[BLACK] |= 1 << H4;
        piece_type[PAWN] |= 1 << F4;
        color[BLACK] |= 1 << F4;
        let b = Board::new(&piece_type, &color);
        assert_eq!(b.generate_moves(BLACK, H4, 0, 0, 1 << G3, &mut MoveStack::new()),
                   2);
    }
}
