use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let mode = args.next().ok_or("missing mode")?;
    let out = PathBuf::from(args.next().ok_or("missing output")?);
    let input = PathBuf::from(args.next().ok_or("missing input")?);
    if mode == "prost" {
        pbrs::codegen::prost_stubs::compile_descriptor_set(&input, &["case.proto"], &out)?;
    } else if mode == "reference" {
        let includes = input.parent().ok_or("missing proto parent")?.to_path_buf();
        tonic_prost_build::configure()
            .out_dir(&out)
            .codec_path("::protobuf_tonic::ProtobufCodec")
            .extern_path(
                ".proof.rpcnames.Input",
                "::protobuf_tonic::hello::HelloRequest",
            )
            .extern_path(
                ".proof.rpcnames.Output",
                "::protobuf_tonic::hello::HelloReply",
            )
            .compile_protos(&[input], &[includes])?;
    } else {
        return Err("unknown mode".into());
    }
    Ok(())
}
