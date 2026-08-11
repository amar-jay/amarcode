fn main() -> std::io::Result<()> {
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../application/src/generated/protocol.ts");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, amarcode_protocol::typescript_bindings())?;
    println!("generated {}", output.display());
    Ok(())
}
