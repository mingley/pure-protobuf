#![no_main]

use libfuzzer_sys::fuzz_target;
use pbrs::codegen::generate_from_code_generator_request;
use pbrs::DescriptorPool;

const MAX_FUZZ_INPUT_BYTES: usize = 64 * 1024; // 64 KiB
const MAX_NAMES_TO_EXERCISE: usize = 256;
const MAX_GENERATED_FILES: usize = 64;
const MAX_GENERATED_CONTENT_BYTES: usize = 256 * 1024; // 256 KiB

pub fn fuzz_descriptors(data: &[u8]) {
    let data = if data.len() > MAX_FUZZ_INPUT_BYTES {
        &data[..MAX_FUZZ_INPUT_BYTES]
    } else {
        data
    };

    // 1. DescriptorPool from raw FileDescriptorSet bytes
    match DescriptorPool::from_file_descriptor_set(data) {
        Ok(pool) => {
            let names = pool.collect_names();
            for name in names.iter().take(MAX_NAMES_TO_EXERCISE) {
                let msg = pool.get_message(name);
                assert!(msg.is_some(), "message from collect_names must be resolvable");
                let with_dot = format!(".{name}");
                let _ = pool.get_message(&with_dot);
            }
            for enum_name in pool.collect_enum_names().iter().take(MAX_NAMES_TO_EXERCISE) {
                let _ = pool.get_enum(enum_name);
            }
            for svc in pool.collect_services().iter().take(MAX_NAMES_TO_EXERCISE) {
                let _ = pool.get_service(&svc.full_name);
            }
        }
        Err(_err) => {
            // Malformed data safely returns Err without panics or aborts
        }
    }

    // 2. Codegen from raw CodeGeneratorRequest bytes
    match generate_from_code_generator_request(data) {
        Ok(files) => {
            // Bound inspection of generated output to avoid runaway allocations under hostile inputs
            for (filename, content) in files.iter().take(MAX_GENERATED_FILES) {
                assert!(!filename.is_empty(), "generated file name must not be empty");
                let bound = content.len().min(MAX_GENERATED_CONTENT_BYTES);
                let _ = &content[..bound];
            }
        }
        Err(_err) => {
            // Malformed data safely returns Err without panics or aborts
        }
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_descriptors(data);
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoke_descriptors() {
        let empty: &[u8] = b"";
        fuzz_descriptors(empty);

        let random_junk: &[u8] = &[0xff, 0xff, 0x01, 0x02, 0x7f, 0x00];
        fuzz_descriptors(random_junk);

        let differential_fds = include_bytes!("../../tests/fixtures/differential/differential.fds");
        fuzz_descriptors(differential_fds);
    }
}
