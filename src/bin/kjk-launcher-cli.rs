fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = kjk_launcher::cli::run(args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
