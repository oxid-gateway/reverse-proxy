use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    tonic_build::configure()
        .build_server(true)
        .file_descriptor_set_path(out_dir.join("proxy_descriptor.bin"))
        .compile_protos(&["proto/proxy.proto"], &["proto"])?;
    Ok(())
}
