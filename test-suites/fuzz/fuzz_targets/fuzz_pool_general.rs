#![no_main]

use fuzz_common::pool_general::{run, Input};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: Input| {
    run::<false>(&input);
});
