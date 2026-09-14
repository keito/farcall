use std::{env, process};

mod machine;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: pc86 <file_path>");
        return;
    }
    match machine::main_loop(&args[1]) {
        Ok(code) => {
            eprintln!("Exit code {}", code);
            process::exit(code as i32);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(-1);
        }
    }
}
