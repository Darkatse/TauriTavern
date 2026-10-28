fn main() {
    if let Err(error) = tauritavern_lib::perf::run() {
        eprintln!("tt-perf: {error}");
        std::process::exit(1);
    }
}
