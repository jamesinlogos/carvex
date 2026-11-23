//! Command-line entry point for CarveX.
fn main() {
    match carvex::cli::run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("error: {err:#}");
            std::process::exit(1);
        }
    }
}
