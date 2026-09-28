use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let include = args
        .next()
        .map(PathBuf::from)
        .ok_or("missing proto directory")?;
    let out = args.next().map(PathBuf::from).ok_or("missing out dir")?;
    let files: Vec<PathBuf> = args.map(|name| include.join(name)).collect();
    if files.is_empty() {
        return Err("no proto inputs".into());
    }
    // protoc discovery is tonic-build's: $PROTOC, then PATH. The harness
    // pins PROTOC to the recorded protoc for every measured run.
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .out_dir(&out)
        .compile_protos(&files, &[&include])?;
    Ok(())
}
