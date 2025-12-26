use anyhow::{Context, Result};
use std::{
    path::{Path},
};
use walkdir::WalkDir;
use reqwest::Client;
use serde_json::Value;

use futures_util::stream::{self, StreamExt};
use dirs::data_dir;

#[tokio::main]
async fn main() -> Result<()> {
    // 1) Config (hardcode first, then turn into CLI flags later)
    let mods_dir = data_dir()
    .context("Could not find AppData directory")?
    .join(".minecraft")
    .join("mods");


    // 3) Scan local mods and hash them
    let local = scan_mods_dir(&mods_dir)?;    
    let client = Client::new(); 
    let res = client.get("https://mods.farmlandefficiency.co.nz/mods")
        .send()
        .await?
        .error_for_status()?;

    let json: Value = res.json().await?;

    let mut missing_mods = Vec::new();
    let mut remote_mod_count = 0;
    
    if let Some(mods) = json.get("mods").and_then(|v| v.as_array())  {
        for m in mods {
            if m == "fabric-installer-1.1.0.exe" {
                continue;
            }
            if let Some(mod_name) = m.as_str() {
                if local.contains(&mod_name.to_string()) {
                    remote_mod_count += 1;
                } else {
                    missing_mods.push(mod_name.to_string());
                }
            }
        }
    }

    println!("Remote mod count: {}", remote_mod_count);
    println!("Total local mods: {}", local.len());
    println!("Missing mods: {:?}", missing_mods.len());

    let client = Client::new();

    let concurrency: usize = 8; // tune this: 4, 8, 12, 16...
    stream::iter(missing_mods)
        .for_each_concurrent(concurrency, |mod_name| {
            let client = client.clone();
            let dest_path = mods_dir.join(&mod_name);

            async move {
                println!("Downloading: {}", mod_name);

                if let Err(e) = get_mod(&client, &mod_name, &dest_path).await {
                    eprintln!("Failed {}: {:?}", mod_name, e);
                } else {
                    println!("Done: {}", mod_name);
                }
            }
        })
        .await;

    Ok(())




}

use tokio::io::AsyncWriteExt;

async fn get_mod(client: &Client, mod_name: &str, dest: impl AsRef<Path>) -> Result<()> {
    let url = format!("https://mods.farmlandefficiency.co.nz/mods/{}", mod_name);

    let mut response = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?;

    // temp file then rename = avoids corrupt/incomplete jars if interrupted
    let dest = dest.as_ref();
    let tmp = dest.with_extension("part");

    let mut file = tokio::fs::File::create(&tmp).await?;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
    }

    file.flush().await?;
    drop(file);

    tokio::fs::rename(&tmp, dest).await?;
    Ok(())
}


fn scan_mods_dir(mods_dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();

    if !mods_dir.exists() {
        anyhow::bail!("mods dir does not exist: {}", mods_dir.display());
    }

    for entry in WalkDir::new(mods_dir).max_depth(1) {
        let entry = entry?;
        let p = entry.path();

        if p.is_dir() {
            continue;
        }

        // only jars (adjust if needed)
        if p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase() != "jar" {
            continue;
        }

        let file_name = p
            .file_name()
            .and_then(|n| n.to_str())
            .context("non-utf8 filename")?
            .to_string();

        out.push(file_name);
    }

    Ok(out)
}
