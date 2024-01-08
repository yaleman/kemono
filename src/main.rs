use std::path::PathBuf;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use kemono::errors::KemonoError;
use kemono::{Attachment, KemonoClient, Post};
use rayon::prelude::*;

use reqwest::Url;

#[derive(Subcommand)]
enum Commands {
    /// Dumps a list of posts in JSON format
    Query {
        #[arg(env = "KEMONO_SERVICE")]
        service: String,
        #[arg(env = "KEMONO_CREATOR")]
        creator: String,
    },
    /// does testing things
    Download {
        #[arg(env = "KEMONO_SERVICE")]
        service: String,
        #[arg(env = "KEMONO_CREATOR")]
        creator: String,
    },
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct CliOpts {
    #[arg(env = "KEMONO_HOSTNAME")]
    hostname: String,

    #[command(subcommand)]
    command: Commands,
}

fn download_image(
    client: &KemonoClient,
    post: &Post,
    attachment: &Attachment,
    creator: &str,
    service: &str,
) -> Result<(), KemonoError> {
    if attachment.name.is_none() {
        return Err(KemonoError::from(format!(
            "Attachment has no name! {:?}",
            attachment
        )));
    }
    if attachment.path.is_none() {
        return Err(KemonoError::from(format!(
            "Attachment has no path! {:?}",
            attachment
        )));
    }

    let download_path = PathBuf::from(format!(
        "{}/{}-{}",
        client.get_download_path(service, creator),
        post.published.replace(':', "-"),
        attachment.name.clone().unwrap()
    ));
    if download_path.exists() {
        eprintln!(
            "Skipping {} because it already exists",
            download_path.display()
        );
        return Ok(());
    }

    let url = Url::from_str(&format!(
        "https://{}/{}",
        client.hostname,
        attachment.path.clone().unwrap()
    ))
    .map_err(|err| err.to_string())?;
    println!("Downloading {} to {}", url, download_path.display());

    let image_data = reqwest::blocking::get(url).map_err(|err| err.to_string())?;

    match image_data.bytes() {
        Ok(data) => {
            if !download_path.parent().unwrap().exists() {
                std::fs::create_dir_all(download_path.parent().unwrap())
                    .map_err(|err| format!("Failed to create parent dirs: {:?}", err))?;
            }
            std::fs::write(download_path, data)
                .map_err(|err| KemonoError::from(format!("Failed to write image data: {:?}", err)))
        }
        Err(err) => Err(KemonoError::from(err)),
    }
}

async fn do_query(client: KemonoClient, service: &str, creator: &str) {
    println!("service: {}, creator: {}", service, creator);
    let mut offset = 0;
    loop {
        let res = client.posts(service, creator, None, Some(offset)).await;
        match res {
            Ok(posts) => {
                for post in &posts {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&post).expect("Failed to serialize data")
                    );
                }
                if posts.len() != client.max_per_page() {
                    break;
                } else {
                    offset += client.max_per_page()
                }
            }
            Err(err) => {
                eprintln!(
                    "Failed to query hostname={} service={service} creator={creator} error={err:?}",
                    client.hostname,
                    service = service,
                    creator = creator,
                    err = err
                );
            }
        }
    }
}

async fn do_download(client: KemonoClient, service: &str, creator: &str) -> Result<(), String> {
    // let _download_path = format!("./download/{}/{}", creator, service);
    let mut offset = 0;
    let mut total_posts = 0;
    let mut files = vec![];
    loop {
        match client.posts(service, creator, None, Some(offset)).await {
            Ok(posts) => {
                let post_len = posts.len();
                total_posts += post_len;
                for post in posts {
                    let post_data = serde_json::to_string(&post).expect("Failed to serialize post");
                    let post_data_filepath = PathBuf::from(&format!(
                        "{}/metadata/{}.json",
                        client.get_download_path(service, creator),
                        post.id
                    ));
                    if !post_data_filepath.parent().unwrap().exists() {
                        std::fs::create_dir_all(post_data_filepath.parent().unwrap())
                            .expect("Failed to create parent dirs");
                    }

                    if !post_data_filepath.exists() {
                        std::fs::write(post_data_filepath, post_data)
                            .expect("Failed to write post data");
                    }
                    if post.file.name.is_some() && post.file.path.is_some() {
                        files.push((post.clone(), post.file.clone()));
                    }
                    if let Some(attachments) = post.attachments.clone() {
                        for attachment in attachments {
                            files.push((post.clone(), attachment));
                        }
                    }
                }
                if post_len != client.max_per_page() {
                    // panic!("whoa, got {}", client.max_per_page())
                    eprintln!("Got them all! Stopping at {}", total_posts);
                    break;
                }
                offset += client.max_per_page();
                eprintln!("grabbing next page, offset is now {}", offset);
            }
            Err(err) => {
                eprintln!(
                    "Failed to query hostname={} service={service} creator={creator} error={err:?}",
                    client.hostname,
                    service = service,
                    creator = creator,
                    err = err
                );
            }
        }
        eprintln!("Starting to download {} objects", files.len());
        files.par_iter().for_each(|image| {
            let (post, attachment) = image;

            if let Err(err) = download_image(&client, post, attachment, creator, service) {
                eprintln!("Failed to download image: {:?}", err);
            };
        });
    }
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = CliOpts::parse();
    println!("hostname: {}", cli.hostname);
    let client = KemonoClient::new(&cli.hostname);

    match cli.command {
        Commands::Query { service, creator } => {
            do_query(client, &service, &creator).await;
        }
        Commands::Download { service, creator } => {
            if let Err(err) = do_download(client, &service, &creator).await {
                eprintln!("Failed to complete download: {}", err);
            };
        }
    }
}
