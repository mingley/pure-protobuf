use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let include = args
        .next()
        .map(PathBuf::from)
        .ok_or("missing proto directory")?;
    let out = args.next().map(PathBuf::from).ok_or("missing out dir")?;
    let protos: Vec<PathBuf> = args.map(|name| include.join(name)).collect();
    if protos.is_empty() {
        return Err("no proto inputs".into());
    }
    // protoc discovery is prost-build's: $PROTOC, then PATH. The harness
    // pins PROTOC to the recorded protoc for every measured run.
    prost_build::Config::new()
        .out_dir(&out)
        .compile_protos(&protos, &[&include])?;
    Ok(())
}
