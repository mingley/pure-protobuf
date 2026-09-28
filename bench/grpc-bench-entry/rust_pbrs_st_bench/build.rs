fn main() {
    pbrs::codegen::compile_protos(
        &["proto/helloworld/helloworld.proto"],
        &["proto"],
    )
    .unwrap();
}
