use std::thread;
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::mpsc::{channel, Sender, Receiver, RecvError};
use basetypes::*;
use chess_move::*;
use tt::*;
use position::Position;


pub enum Command {
    Search {
        search_id: usize,
        position: Position,
        depth: u8,
        lower_bound: Value,
        upper_bound: Value,
    },
    Stop,
    Exit,
}


pub enum Report {
    Progress {
        search_id: usize,
        searched_nodes: NodeCount,
        depth: u8,
    },
    Done {
        search_id: usize,
        searched_nodes: NodeCount,
        value: Option<Value>,
    },
}


pub fn run_deepening(tt: Arc<TranspositionTable>,
                     commands: Receiver<Command>,
                     reports: Sender<Report>) {
    let (slave_commands_tx, slave_commands_rx) = channel();
    let (slave_reports_tx, slave_reports_rx) = channel();
    let slave = thread::spawn(move || {
        run(tt, slave_commands_rx, slave_reports_tx);
    });
    let mut pending_command = None;
    loop {
        // If there is a pending command, we take it, otherwise we
        // block and wait to receive a new one.
        let command = match pending_command.take() {
            Some(cmd) => cmd,
            None => commands.recv().or::<RecvError>(Ok(Command::Exit)).unwrap(),
        };

        match command {
            Command::Search { search_id, position, depth, lower_bound, upper_bound } => {
                let mut searched_nodes_final = 0;
                let mut value_final = None;
                'depthloop: for n in 1..(depth + 1) {
                    slave_commands_tx.send(Command::Search {
                                         search_id: n as usize,
                                         position: position.clone(),
                                         depth: n,
                                         lower_bound: lower_bound,
                                         upper_bound: upper_bound,
                                     })
                                     .unwrap();
                    loop {
                        match slave_reports_rx.recv().unwrap() {
                            Report::Progress { searched_nodes, .. } => {
                                reports.send(Report::Progress {
                                           search_id: search_id,
                                           searched_nodes: searched_nodes_final + searched_nodes,
                                           depth: n,
                                       })
                                       .ok();
                                if pending_command.is_none() {
                                    pending_command = match commands.try_recv() {
                                        Ok(cmd) => {
                                            slave_commands_tx.send(Command::Stop).unwrap();
                                            Some(cmd)
                                        }
                                        _ => None,
                                    }
                                }
                            }
                            Report::Done { searched_nodes, value, .. } => {
                                searched_nodes_final += searched_nodes;
                                if n == depth {
                                    value_final = value;
                                }
                                if pending_command.is_some() {
                                    break 'depthloop;
                                }
                                break;
                            }
                        }
                    }
                }
                reports.send(Report::Done {
                           search_id: search_id,
                           searched_nodes: searched_nodes_final,
                           value: value_final,
                       })
                       .ok();
            }
            Command::Stop => {
                slave_commands_tx.send(Command::Stop).unwrap();
                continue;
            }
            Command::Exit => {
                slave_commands_tx.send(Command::Exit).unwrap();
                break;
            }
        }
    }
    slave.join().unwrap();
}


pub fn run(tt: Arc<TranspositionTable>, commands: Receiver<Command>, reports: Sender<Report>) {
    thread_local!(
        static MOVE_STACK: UnsafeCell<MoveStack> = UnsafeCell::new(MoveStack::new())
    );
    MOVE_STACK.with(|s| {
        let mut move_stack = unsafe { &mut *s.get() };
        let mut pending_command = None;
        loop {
            // If there is a pending command, we take it, otherwise we
            // block and wait to receive a new one.
            let command = match pending_command.take() {
                Some(cmd) => cmd,
                None => commands.recv().or::<RecvError>(Ok(Command::Exit)).unwrap(),
            };

            match command {
                Command::Search { search_id, position, depth, lower_bound, upper_bound } => {
                    let mut report = |n| {
                        reports.send(Report::Progress {
                                   search_id: search_id,
                                   searched_nodes: n,
                                   depth: depth,
                               })
                               .ok();
                        if let Ok(cmd) = commands.try_recv() {
                            pending_command = Some(cmd);
                            true
                        } else {
                            false
                        }
                    };
                    let mut search = Search::new(position, &tt, move_stack, &mut report);
                    let value = search.run(lower_bound, upper_bound, depth).ok();
                    reports.send(Report::Progress {
                               search_id: search_id,
                               searched_nodes: search.node_count(),
                               depth: depth,
                           })
                           .ok();
                    reports.send(Report::Done {
                               search_id: search_id,
                               searched_nodes: search.node_count(),
                               value: value,
                           })
                           .ok();
                    search.reset();
                }
                Command::Stop => continue,
                Command::Exit => break,
            }
        }
    })
}


/// Represents a terminated search condition.
pub struct TerminatedSearch;


enum NodePhase {
    Pristine,
    TriedHashMove,
    GeneratedMoves,
}


struct NodeState {
    phase: NodePhase,
    entry: EntryData,
}


const NODE_COUNT_REPORT_INTERVAL: NodeCount = 10000;


pub struct Search<'a> {
    tt: &'a TranspositionTable,
    position: Position,
    moves: &'a mut MoveStack,
    moves_starting_ply: usize,
    state_stack: Vec<NodeState>,
    reported_nodes: NodeCount,
    unreported_nodes: NodeCount,
    report_function: &'a mut FnMut(NodeCount) -> bool,
}


impl<'a> Search<'a> {
    /// Creates a new instance.
    ///
    /// `report_function` should be a function that registers the
    /// search progress. It will be called with the number of searched
    /// positions from the beginning of the search to this moment. The
    /// function should return `true` if the search should be
    /// terminated, otherwise it should return `false`.
    pub fn new(root: Position,
               tt: &'a TranspositionTable,
               moves: &'a mut MoveStack,
               report_function: &'a mut FnMut(NodeCount) -> bool)
               -> Search<'a> {
        let moves_starting_ply = moves.ply();
        Search {
            tt: tt,
            position: root,
            moves: moves,
            moves_starting_ply: moves_starting_ply,
            state_stack: Vec::with_capacity(32),
            reported_nodes: 0,
            unreported_nodes: 0,
            report_function: report_function,
        }
    }

    /// Performs a principal variation search and returns a result.
    ///
    /// **Important note**: This method may leave un-restored move
    /// lists in the move stack. Call `reset` if you want the move
    /// stack to be restored to the state it had when the search
    /// instance was created.
    pub fn run(&mut self,
               mut alpha: Value, // lower bound
               beta: Value, // upper bound
               depth: u8)
               -> Result<Value, TerminatedSearch> {
        assert!(alpha < beta);

        let entry = self.node_begin();

        // Check if the TT entry gives the result.
        if entry.depth() >= depth {
            let value = entry.value();
            let bound = entry.bound();
            if (value >= beta && bound == BOUND_LOWER) ||
               (value <= alpha && bound == BOUND_UPPER) || (bound == BOUND_EXACT) {
                self.node_end();
                return Ok(value);
            }
        }

        // Initial guests for the final result.
        let mut bound = BOUND_UPPER;
        let mut best_move = Move::invalid();

        if depth == 0 {
            // On leaf nodes, do quiescence search.
            let (value, nodes) = self.position
                                     .evaluate_quiescence(alpha, beta, Some(entry.eval_value()));
            try!(self.report_progress(nodes));

            // See how good this position is.
            if value >= beta {
                alpha = beta;
                bound = BOUND_LOWER;
            } else if value > alpha {
                alpha = value;
                bound = BOUND_EXACT;
            }

        } else {
            // On non-leaf nodes, try moves.
            let mut no_moves_yet = true;
            while let Some(m) = self.do_move() {
                try!(self.report_progress(1));

                // Make a recursive call.
                let value = if no_moves_yet {
                    // The first move we analyze with a fully open window
                    // (alpha, beta). If this happens to be a good move,
                    // it will probably raise `alpha`.
                    no_moves_yet = false;
                    -try!(self.run(-beta, -alpha, depth - 1))
                } else {
                    // For the next moves we first try to prove that they
                    // are not better than our current best move. For this
                    // purpose we analyze them with a null window (alpha,
                    // alpha + 1). This is faster than a full window
                    // search. Only if we are certain that the move is
                    // better than our current best move, we do a
                    // full-window search.
                    match -try!(self.run(-alpha - 1, -alpha, depth - 1)) {
                        x if x <= alpha => x,
                        _ => -try!(self.run(-beta, -alpha, depth - 1)),
                    }
                };
                self.undo_move();

                // See how good this move was.
                if value >= beta {
                    // This move is so good, that the opponent will
                    // probably not allow this line of play to
                    // happen. Therefore we should not lose any more time
                    // on this position.
                    alpha = beta;
                    bound = BOUND_LOWER;
                    best_move = m;
                    break;
                }
                if value > alpha {
                    // We found a new best move.
                    alpha = value;
                    bound = BOUND_EXACT;
                    best_move = m;
                }
            }

            // Check if we are in a final position (no legal moves).
            if no_moves_yet {
                alpha = self.position.evaluate_final();
                bound = BOUND_EXACT;
            }
        }

        self.store(alpha, bound, depth, best_move);
        self.node_end();
        Ok(alpha)
    }

    /// Returns the number of searched positions.
    #[inline(always)]
    pub fn node_count(&self) -> NodeCount {
        self.reported_nodes + self.unreported_nodes
    }

    /// Resets the instance to the state it had when it was created.
    #[inline]
    pub fn reset(&mut self) {
        while self.moves.ply() > self.moves_starting_ply {
            self.moves.restore();
        }
        self.state_stack.clear();
        self.reported_nodes = 0;
        self.unreported_nodes = 0;
    }

    // Declares that we are starting to process a new node.
    //
    // Each recursive call to `run` begins with a call to
    // `node_begin`. The returned value is a TT entry telling
    // everything we know about the current position.
    #[inline]
    fn node_begin(&mut self) -> EntryData {
        // Consult the transposition table.
        let entry = if let Some(e) = self.tt.probe(self.position.hash()) {
            e
        } else {
            EntryData::new(0, BOUND_NONE, 0, 0, self.position.evaluate_static())
        };
        self.state_stack.push(NodeState {
            phase: NodePhase::Pristine,
            entry: entry,
        });
        entry
    }

    // Declares that we are done processing the current node.
    //
    // Each recursive call to `run` ends with a call to `node_end`.
    #[inline]
    fn node_end(&mut self) {
        if let NodePhase::Pristine = self.state_stack.last().unwrap().phase {
            // For pristine nodes we have not saved the move list
            // yet, so we should not restore it.
        } else {
            self.moves.restore();
        }
        self.state_stack.pop();
    }

    // Plays the next legal move in the current position and returns
    // it.
    //
    // Each call to `do_move` for the same position will play and
    // return a different move. When all legal moves has been played,
    // `None` will be returned. `do_move` will do whatever it can to
    // play the best moves first, and the worst last. It will also try
    // to be efficient, for example it will generate the list of all
    // pseudo-legal moves at the last possible moment.
    #[inline]
    fn do_move(&mut self) -> Option<Move> {
        let state = self.state_stack.last_mut().unwrap();

        if let NodePhase::Pristine = state.phase {
            // We save the move list at the last possible moment,
            // because most of the nodes are leafs.
            self.moves.save();

            // We always try the hash move first.
            state.phase = NodePhase::TriedHashMove;
            if state.entry.move16() != 0 {
                if let Some(m) = self.position.try_move_digest(state.entry.move16()) {
                    if self.position.do_move(m) {
                        return Some(m);
                    }
                }
            }
        }

        if let NodePhase::TriedHashMove = state.phase {
            // After the hash move, we generate all pseudo-legal
            // moves. But we should not forget to remove the already
            // tried hash move from the list.
            self.position.generate_moves(self.moves);
            if state.entry.move16() != 0 {
                self.moves.remove_move(state.entry.move16());
            }
            state.phase = NodePhase::GeneratedMoves;
        }

        // For the last, we spit the generated moves out.
        while let Some(m) = self.moves.remove_best_move() {
            if self.position.do_move(m) {
                return Some(m);
            }
        }
        None
    }

    // Takes the last played move back.
    #[inline]
    fn undo_move(&mut self) {
        self.position.undo_move();
    }

    // Stores updated node information in the transposition table.
    #[inline]
    fn store(&mut self, value: Value, bound: BoundType, depth: u8, best_move: Move) {
        let entry = &self.state_stack.last().unwrap().entry;
        let move16 = match best_move.digest() {
            0 => entry.move16(),
            x => x,
        };
        self.tt.store(self.position.hash(),
                      EntryData::new(value, bound, depth, move16, entry.eval_value()));
    }

    // Reports search progress.
    //
    // From time to time, we should report how many nodes had been
    // searched since the beginning of the search. This also gives an
    // opportunity for the search to be terminated.
    #[inline]
    fn report_progress(&mut self, new_nodes: NodeCount) -> Result<(), TerminatedSearch> {
        self.unreported_nodes += new_nodes;
        if self.unreported_nodes > NODE_COUNT_REPORT_INTERVAL {
            self.reported_nodes += self.unreported_nodes;
            self.unreported_nodes = 0;
            if (*self.report_function)(self.reported_nodes) {
                return Err(TerminatedSearch);
            }
        }
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::Search;
    use chess_move::*;
    use tt::*;
    use position::Position;

    #[test]
    fn test_search() {
        let p = Position::from_fen("8/8/8/8/3q3k/7n/6PP/2Q2R1K b - - 0 1").ok().unwrap();
        let tt = TranspositionTable::new();
        let mut moves = MoveStack::new();
        let mut report = |_| false;
        let mut search = Search::new(p, &tt, &mut moves, &mut report);
        let value = search.run(-30000, 30000, 2)
                          .ok()
                          .unwrap();
        assert!(value < -300);
        search.reset();
        let value = search.run(-30000, 30000, 4)
                          .ok()
                          .unwrap();
        assert!(value >= 20000);
    }
}
