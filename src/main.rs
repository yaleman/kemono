use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use kemono::errors::KemonoError;
use kemono::{Attachment, KemonoClient, Post};
use rayon::prelude::*;

use reqwest::Url;
use retry::delay::Exponential;
use serde_json::json;

#[derive(Subcommand)]
enum Commands {
    /// Dumps a list of posts in JSON format
    Query,
    /// does testing things
    Download,
    Stats,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct CliOpts {
    #[command(subcommand)]
    command: Commands,

    #[arg(env = "KEMONO_HOSTNAME")]
    hostname: String,
    #[arg(env = "KEMONO_SERVICE")]
    service: String,
    #[arg(env = "KEMONO_CREATOR")]
    creator: String,
    #[arg(env = "KEMONO_DEBUG", short, long)]
    debug: bool,
    #[arg(env = "KEMONO_DEBUG", short, long)]
    /// If the "original" file is an mp4 or m4v then we might have a mkv file and that's OK
    mkvs: bool,
}

/// replace the extension in a filename with mkv
fn get_mkv_filename(filename: &str) -> String {
    let parts = filename.split('.');
    let mut new_filename = String::new();
    let mut first = true;
    for part in parts {
        if !first {
            new_filename.push('.');
        }
        if part == "mp4" || part == "m4v" {
            new_filename.push_str("mkv");
        } else {
            new_filename.push_str(part);
        }
        first = false;
    }
    new_filename
}

/// download a given image
fn download_image(
    cli: &CliOpts,
    client: &KemonoClient,
    post: &Post,
    attachment: &Attachment,
) -> Result<(), KemonoError> {
    if attachment.name.is_none() {
        return Err(KemonoError::from(format!(
            "Attachment has no name! {:?}",
            attachment
        )));
    }
    let attachment_path = match &attachment.path {
        None => {
            return Err(KemonoError::from(format!(
                "Attachment has no path! {:?}",
                attachment
            )));
        }
        Some(ap) => ap.to_owned(),
    };
    let download_filename = format!(
        "{}-{}",
        post.published.replace(':', "-"),
        attachment.name.clone().unwrap()
    );
    let download_path = PathBuf::from(format!(
        "{}/{}",
        client.get_download_path(&cli.service, &cli.creator),
        download_filename
    ));
    // check
    if download_path.exists() {
        eprintln!(
            "Skipping {} because it already exists",
            download_path.display()
        );
        return Ok(());
    }

    if cli.mkvs {
        let mkv_path = PathBuf::from(get_mkv_filename(&download_filename));
        if mkv_path.exists() {
            eprintln!("Skipping {} because it already exists", mkv_path.display());
            return Ok(());
        }
    }

    let url = Url::from_str(&format!("https://{}{}", client.hostname, attachment_path,))
        .map_err(KemonoError::from_stringable)?;
    println!("Downloading {} to {}", url, download_path.display());

    let response = reqwest::blocking::get(url)?.error_for_status()?;
    match response.bytes() {
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

async fn do_query(cli: CliOpts, client: KemonoClient) -> Result<(), KemonoError> {
    let posts = client.all_posts(&cli.service, &cli.creator).await?;
    for post in posts {
        println!("{}", serde_json::to_string_pretty(&post)?);
    }
    Ok(())
}

async fn do_download(cli: CliOpts, client: KemonoClient) -> Result<(), KemonoError> {
    let mut files = Vec::new();

    for post in client.all_posts(&cli.service, &cli.creator).await? {
        let post_data_filepath = PathBuf::from(&format!(
            "{}/metadata/{}.json",
            client.get_download_path(&cli.service, &cli.creator),
            post.id
        ));

        if !post_data_filepath.parent().unwrap().exists() {
            std::fs::create_dir_all(post_data_filepath.parent().unwrap())
                .expect("Failed to create parent dirs");
        }

        if !post_data_filepath.exists() {
            std::fs::write(post_data_filepath, serde_json::to_string_pretty(&post)?)
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

    eprintln!("Starting to download {} objects", files.len());

    files.par_iter().for_each(|image| {
        let (post, attachment) = image;

        if let Err(err) =
            retry::retry_with_index(Exponential::from_millis(3000).take(5), |current_try| {
                if current_try > 0 {
                    eprintln!(
                        "Retrying download of {} (try {})",
                        attachment.name.clone().unwrap(),
                        current_try
                    );
                }
                download_image(&cli, &client, post, attachment)
            })
        {
            eprintln!("Failed to download file: {:?}", err);
        };
    });
    Ok(())
}

async fn do_stats(client: KemonoClient, cli: &CliOpts) -> Result<(), KemonoError> {
    let posts = client.all_posts(&cli.service, &cli.creator).await?;

    let post_count = posts.len();
    let mut filetypes: HashMap<String, usize> = HashMap::new();
    let mut file_count = 0;

    for post in posts {
        if let Some(attachments) = post.attachments {
            for attachment in attachments {
                if let Some(name) = attachment.name {
                    let ext = name.split('.').last().unwrap().to_string();
                    let count = filetypes.entry(ext).or_insert(0);
                    *count += 1;
                    file_count += 1;
                }
            }
        }
        if let Some(name) = post.file.name {
            let ext = name.split('.').last().unwrap().to_string();
            let count = filetypes.entry(ext).or_insert(0);
            *count += 1;
            file_count += 1;
        }
    }

    let stats = json!({
        "post_count": post_count,
        "file_count" : file_count,
        "filetypes": filetypes,
    });

    println!("{}", serde_json::to_string_pretty(&stats)?);

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = CliOpts::parse();
    let client = KemonoClient::new(&cli.hostname);
    match cli.command {
        Commands::Stats => {
            eprintln!(
                "Pulling stats for {}/{}/{}",
                cli.hostname, cli.service, cli.creator
            );
            if let Err(err) = do_stats(client, &cli).await {
                eprintln!("Failed to complete stats: {:?}", err);
            };
        }
        Commands::Query => {
            eprintln!(
                "Pulling API data for {}/{}/{}",
                cli.hostname, cli.service, cli.creator
            );
            if let Err(err) = do_query(cli, client).await {
                eprintln!("Failed to complete query: {:?}", err);
            };
        }
        Commands::Download => {
            eprintln!(
                "Downloading all content for {}/{}/{}",
                cli.hostname, cli.service, cli.creator
            );
            if let Err(err) = do_download(cli, client).await {
                eprintln!("Failed to complete download: {:?}", err);
            };
        }
    }
}
