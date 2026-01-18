fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_path = std::env::var("PROTO_PATH").unwrap_or_else(|_| "../../proto".to_string());
    let proto_file = format!("{}/adsb.proto", proto_path);

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile(&[&proto_file], &[&proto_path])?;

    println!("cargo:rerun-if-changed={}", proto_file);
    Ok(())
}
