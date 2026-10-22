// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../proto");
    prost_build::Config::new()
        .protoc_executable(protoc_bin_vendored::protoc_bin_path()?)
        .compile_protos(
            &["../../proto/apeirogon/v1/apeirogon.proto"],
            &["../../proto"],
        )?;
    Ok(())
}
