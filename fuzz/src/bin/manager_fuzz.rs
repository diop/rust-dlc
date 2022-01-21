use dlc_fuzz::manager::*;
use honggfuzz::fuzz;

fn main() {
    fuzz!(|data| {
        manager_run(data);
    });
}
