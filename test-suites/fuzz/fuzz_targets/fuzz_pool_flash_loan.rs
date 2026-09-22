#![no_main]

use fuzz_common::{fuzz, Target};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| fuzz(Target::PoolFlashLoan, input));
