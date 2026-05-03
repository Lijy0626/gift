use gift::object;

fn main() -> Result<(), anyhow::Error> {
    let _ = env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .try_init();

    let (hash, content) = object::hash_object("a")?;
    println!("{}", hex::encode(&hash.as_bytes()));

    object::write_hash_object(".gift", &hash, &content)?;
    Ok(())
}
