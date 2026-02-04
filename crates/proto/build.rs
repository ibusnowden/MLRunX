//! Build script for `MLRunX` protobuf code generation.
//!
//! This generates Rust code from the proto files in /proto/mlrunx/v1/.
//! The generated code is placed in `OUT_DIR` and included via `include!` macro.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Proto source directory
    let proto_dir = PathBuf::from("../../proto");

    // Proto files to compile
    let protos = &[
        proto_dir.join("mlrunx/v1/common.proto"),
        proto_dir.join("mlrunx/v1/ingest.proto"),
        proto_dir.join("mlrunx/v1/query.proto"),
    ];

    // Include paths
    let includes = std::slice::from_ref(&proto_dir);

    // Tell cargo to rerun if proto files change
    for proto in protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }

    // Configure tonic-build
    tonic_build::configure()
        // Generate server code
        .build_server(true)
        // Generate client code
        .build_client(true)
        // Compile protos
        .compile_protos(protos, includes)?;

    Ok(())
}
