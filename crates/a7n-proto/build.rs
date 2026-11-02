// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Cargo packages follow this symlink; Windows checkouts use the workspace schema.
    let proto_dir = if std::path::Path::new("../../proto").is_dir() {
        "../../proto"
    } else {
        "proto"
    };
    println!("cargo:rerun-if-changed={proto_dir}");
    prost_build::Config::new()
        .protoc_executable(protoc_bin_vendored::protoc_bin_path()?)
        .compile_protos(
            &[format!("{proto_dir}/apeirogon/v1/apeirogon.proto")],
            &[proto_dir],
        )?;
    Ok(())
}
