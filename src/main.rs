use std::env;

use crate::machine::main_loop;

mod machine;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: pc86 <file_path>");
        return;
    }
    if let Err(e) = main_loop(&args[1]) {
        eprintln!("Error: {}", e);
    }
}
