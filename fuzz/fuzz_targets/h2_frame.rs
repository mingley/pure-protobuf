#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    pbrs_grpc::__fuzz_h2_frames(data);
});
