fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "../../services/docreader/proto/docreader.proto";
    println!("cargo:rerun-if-changed={proto}");
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&[proto], &["../../services/docreader/proto"])?;
    Ok(())
}
