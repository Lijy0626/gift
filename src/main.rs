use gift::add_commit;

fn main() -> Result<(), anyhow::Error> {
    let _ = env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .try_init();

    let (hash, content) = add_commit::hash_object("a")?;
    println!("{}", hex::encode(&hash.as_bytes()));

    add_commit::write_hash_object(".gift", &hash, &content)?;
    Ok(())
}
