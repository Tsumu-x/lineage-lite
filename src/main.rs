use std::process;

fn main() {
    if let Err(e) = lineage_lite::cli::run() {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
