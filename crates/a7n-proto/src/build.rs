// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    prost_build::compile_protos(
        &["../../proto/apeirogon/v1/apeirogon.proto"], 
        &["../../proto"])?;
    Ok(())
}