use gift::*;

fn main() -> Result<(), anyhow::Error> {
    gift::init(".gift")?;
    let (hash, content) = object::hash_object("a")?;
    println!("{}", hex::encode(&hash.as_bytes()));

    object::write_hash_object(".gift", &hash, &content)?;
    Ok(())
}
