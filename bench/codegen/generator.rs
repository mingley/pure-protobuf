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

    // SB09_PBRS_STUBS selects service-stub emission for the SB-09 stub
    // matrix; unset keeps the exact CG-19 messages-only behavior.
    let stubs = std::env::var("SB09_PBRS_STUBS").unwrap_or_default();
    let bool_env = |name: &str| -> Result<Option<bool>, Error> {
        match std::env::var(name) {
            Ok(value) if value == "1" || value.eq_ignore_ascii_case("true") => Ok(Some(true)),
            Ok(value) if value == "0" || value.eq_ignore_ascii_case("false") => Ok(Some(false)),
            Ok(value) => Err(Error::new(
                ErrorKind::InvalidInput,
                format!("expected boolean {name}, got {value:?}"),
            )),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(err) => Err(Error::new(ErrorKind::InvalidInput, err)),
        }
    };
    let mut config = pbrs::codegen::Config::new();
    config
        .protoc_path(protoc)
        .out_dir(out)
        .emit_deps(false)
        .no_reflect(false)
        .include_source_info(false);
    if let Some(enable) = bool_env("SB09_PBRS_EMIT_REFLECTION")? {
        config.emit_reflection(enable);
    }
    if let Some(enable) = bool_env("SB09_PBRS_EMIT_JSON")? {
        config.emit_json(enable);
    }
    if let Some(enable) = bool_env("SB09_PBRS_EMIT_TEXT")? {
        config.emit_text(enable);
    }
    match stubs.as_str() {
        "" | "none" => {
            config.emit_kernel_stubs(false);
        }
        "native" => {
            config.emit_kernel_stubs(true);
        }
        "tonic" => {
            config.emit_tonic_stubs(true);
        }
        other => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("unknown SB09_PBRS_STUBS={other:?}"),
            )
            .into());
        }
    }
    config.compile_protos(&protos, &[&include])?;
    Ok(())
}
