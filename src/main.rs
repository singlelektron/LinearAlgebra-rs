fn main() {
    if let Err(error) = linearalgebra::cli::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
