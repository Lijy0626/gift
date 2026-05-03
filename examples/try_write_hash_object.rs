use gift::*;

fn main() -> Result<(), anyhow::Error> {
    gift::init(".gift")?;
    let (hash, content) = add_commit::hash_object("a")?;
    println!("{}", hex::encode(&hash.as_bytes()));

    add_commit::write_hash_object(".gift", &hash, &content)?;
    Ok(())
}
