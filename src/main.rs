#[macro_use]
extern crate lazy_static;
extern crate regex;
extern crate rand;

pub mod chesstypes;
pub mod uci;
pub mod board;
pub mod search;
pub mod engine;

use std::process::exit;
use search::deepening::Deepening;
use search::searchers::StandardSearcher;
use search::tt::Tt;
use board::Position;
use board::evaluators::RandomEvaluator;
use engine::run_server;

fn main() {
    exit(match run_server::<Deepening<StandardSearcher<Tt, Position<RandomEvaluator>>>>() {
        Ok(_) => 0,
        Err(_) => 1,
    })
}
