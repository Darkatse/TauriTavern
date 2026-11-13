/// The replay counts every allocation it makes.
#[global_allocator]
static ALLOCATOR: tauritavern_lib::perf::Allocator = tauritavern_lib::perf::Allocator;

fn main() {
    if let Err(error) = tauritavern_lib::perf::run() {
        eprintln!("tt-perf: {error}");
        std::process::exit(1);
    }
}
