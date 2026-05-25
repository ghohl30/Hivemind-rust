use hive_engine::State;

fn main() {
    let state = State::new();
    let moves = state.legal_moves();
    println!("Hive engine — Phase 1");
    println!("Opening legal move count: {}", moves.len());
}
