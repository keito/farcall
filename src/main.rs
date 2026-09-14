use std::env;

mod machine;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: pc86 <file_path>");
        return;
    }
    match machine::main_loop(&args[1]) {
        Ok(code) => eprintln!("Exit code {}", code),
        Err(e) => eprintln!("Error: {}", e),
    }
}
