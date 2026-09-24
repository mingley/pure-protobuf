use std::ffi::OsString;
use std::io::{Error, ErrorKind};
use std::path::PathBuf;

fn required_arg(args: &mut impl Iterator<Item = OsString>, name: &str) -> Result<PathBuf, Error> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, format!("missing {name}")))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let include = required_arg(&mut args, "proto directory")?;
    let out = required_arg(&mut args, "output directory")?;
    let protoc = required_arg(&mut args, "protoc executable")?;
    let protos: Vec<PathBuf> = args.map(|name| include.join(name)).collect();
    if protos.is_empty() {
        return Err(Error::new(ErrorKind::InvalidInput, "no proto inputs").into());
    }

    pbrs::codegen::Config::new()
        .protoc_path(protoc)
        .out_dir(out)
        .emit_kernel_stubs(false)
        .emit_deps(false)
        .no_reflect(false)
        .include_source_info(false)
        .compile_protos(&protos, &[&include])?;
    Ok(())
}
