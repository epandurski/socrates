//! Implements search parallelization routines.

use std::thread;
use std::cmp::{min, max};
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::mpsc::{channel, Sender, Receiver, RecvError};
use basetypes::*;
use chess_move::*;
use tt::*;
use position::Position;
use super::search::Search;


/// Represents a command to a search thread.
pub enum Command {
    /// Requests a new search.
    Search {
        /// A number identifying the new search.
        search_id: usize,
        
        /// The root position.
        position: Position,
        
        /// The requested search depth.
        depth: u8,
        
        /// The lower bound for the new search.
        lower_bound: Value,
        
        /// The upper bound for the new search.
        upper_bound: Value,
    },
    
    /// Stops the currently running search.
    Stop,
    
    /// Stops the currently running search and exits the search
    /// thread.
    Exit,
}


/// Represents a report from a search thread.
pub enum Report {
    /// Reports search progress.
    Progress {
        /// The ID passed with the search command.
        search_id: usize,
        
        /// The number of positions searched so far.
        searched_nodes: NodeCount,
        
        /// The search depth completed so far.
        searched_depth: u8,
        
        /// The evaluation of the root position so far.
        value: Option<Value>,
    },
    
    /// Reports that the search is finished.
    Done {
        /// The ID passed with the search command.
        search_id: usize,
        
        /// The total number of positions searched.
        searched_nodes: NodeCount,
        
        /// The search depth completed.
        searched_depth: u8,
        
        /// The evaluation of the root position.
        value: Option<Value>,
    },
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
                    let mut report = |searched_nodes| {
                        reports.send(Report::Progress {
                                   search_id: search_id,
                                   searched_nodes: searched_nodes,
                                   searched_depth: 0,
                                   value: None,
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
                    let searched_depth = if value.is_some() {
                        depth
                    } else {
                        0
                    };
                    reports.send(Report::Progress {
                               search_id: search_id,
                               searched_nodes: search.node_count(),
                               searched_depth: searched_depth,
                               value: value,
                           })
                           .ok();
                    reports.send(Report::Done {
                               search_id: search_id,
                               searched_nodes: search.node_count(),
                               searched_depth: searched_depth,
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


pub fn run_deepening(tt: Arc<TranspositionTable>,
                     commands: Receiver<Command>,
                     reports: Sender<Report>) {
    // Start a slave thread that will be commanded to run searches
    // with increasing depths (search deepening).
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
                let mut current_searched_nodes = 0;
                let mut current_value = None;
                let mut current_depth = 1;

                'depthloop: while current_depth <= depth {
                    // First we set up the aspiration window. Aspiration windows are a way
                    // to reduce the search space in the search. We use `current_value`
                    // from the last iteration of `depth`, and calculate a window around
                    // this as the alpha-beta bounds. Because the window is narrower, more
                    // beta cutoffs are achieved, and the search takes a shorter time. The
                    // drawback is that if the true score is outside this window, then a
                    // costly re-search must be made. But then most probably the re-search
                    // will be much faster, because many positions will be remembered from
                    // the TT.
                    //
                    // Here `delta` is the initial half-width of the window, which will be
                    // increased each time the search failed. We use `isize` type to avoid
                    // overflows.
                    let mut delta = super::DELTA as isize;
                    let (mut alpha, mut beta) = if current_depth < 5 {
                        (lower_bound, upper_bound)
                    } else {
                        let v = current_value.unwrap() as isize;
                        (max(lower_bound as isize, v - delta) as Value,
                         min(v + delta, upper_bound as isize) as Value)
                    };

                    'aspiration: loop {
                        // Command the slave thread to run a search.
                        slave_commands_tx.send(Command::Search {
                                             search_id: current_depth as usize,
                                             position: position.clone(),
                                             depth: current_depth,
                                             lower_bound: alpha,
                                             upper_bound: beta,
                                         })
                                         .unwrap();

                        'report: loop {
                            // In this loop we process the reports coming from the slave
                            // thread, but we also constantly check if there is a new
                            // pending command for us, in which case we have to terminate
                            // the search.
                            match slave_reports_rx.recv().unwrap() {
                                Report::Progress { searched_nodes, .. } => {
                                    reports.send(Report::Progress {
                                               search_id: search_id,
                                               searched_nodes: current_searched_nodes +
                                                               searched_nodes,
                                               searched_depth: current_depth - 1,
                                               value: current_value,
                                           })
                                           .ok();
                                    if pending_command.is_none() {
                                        if let Ok(cmd) = commands.try_recv() {
                                            slave_commands_tx.send(Command::Stop).unwrap();
                                            pending_command = Some(cmd);
                                        }
                                    }
                                }
                                Report::Done { searched_nodes, value, .. } => {
                                    current_searched_nodes += searched_nodes;
                                    if pending_command.is_none() {
                                        current_value = value;
                                        break 'report;
                                    } else {
                                        break 'depthloop;
                                    }
                                }
                            }
                        } // end of 'report

                        // Check if the `current_value` is within the aspiration window
                        // (alpha, beta). If not so, we must consider running a re-search.
                        let v = current_value.unwrap() as isize;
                        if current_value.unwrap() <= alpha && lower_bound < alpha {
                            alpha = max(lower_bound as isize, v - delta) as Value;
                        } else if current_value.unwrap() >= beta && upper_bound > beta {
                            beta = min(v + delta, upper_bound as isize) as Value;
                        } else {
                            break 'aspiration;
                        }

                        // Increase the half-width of the aspiration window.
                        delta += 3 * delta / 8;
                        if delta > 1500 {
                            delta = 1_000_000;
                        }

                    } // end of 'aspiration

                    // Send a progress report with `current_value` for
                    // every completed depth.
                    reports.send(Report::Progress {
                               search_id: search_id,
                               searched_nodes: current_searched_nodes,
                               searched_depth: current_depth,
                               value: current_value,
                           })
                           .ok();
                    current_depth += 1;

                } // end of 'depthloop

                // The search is done -- send a final report.
                reports.send(Report::Done {
                           search_id: search_id,
                           searched_nodes: current_searched_nodes,
                           searched_depth: current_depth - 1,
                           value: current_value,
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
