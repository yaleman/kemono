use log::{debug, error, info};
use structured_logger::{async_json::new_writer, Builder};

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use kemono::errors::KemonoError;
use kemono::{get_mkv_filename, Attachment, KemonoClient, Post, DEFAULT_DOWNLOAD_PATH};
use rayon::{prelude::*, ThreadPoolBuilder};

use reqwest::Url;
use serde_json::json;

#[derive(Subcommand)]
enum Commands {
    /// Dumps a list of posts in JSON format
    Query {
        #[arg(env = "KEMONO_SERVICE")]
        service: String,
        #[arg(env = "KEMONO_CREATOR")]
        creator: String,
        #[clap(flatten)]
        copt: SharedCliOpts,
    },
    /// does testing things
    Download {
        #[arg(env = "KEMONO_SERVICE")]
        service: String,
        #[arg(env = "KEMONO_CREATOR")]
        creator: String,
        #[clap(flatten)]
        copt: SharedCliOpts,
    },
    Stats {
        #[arg(env = "KEMONO_SERVICE")]
        service: String,
        #[arg(env = "KEMONO_CREATOR")]
        creator: String,
        #[clap(flatten)]
        copt: SharedCliOpts,
    },
    /// Iterate through creator/service dirs and download all the filew we don't have.
    Update {
        #[arg(env = "KEMONO_CREATOR", short, long)]
        creator: Option<String>,
        #[arg(env = "KEMONO_SERVICE", short, long)]
        service: Option<String>,
        #[clap(flatten)]
        copt: SharedCliOpts,
    },
}

#[derive(Clone, Parser)]
struct SharedCliOpts {}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct CliOpts {
    #[arg(short = 'H', long, env = "KEMONO_HOSTNAME")]
    hostname: String,
    #[arg(env = "KEMONO_THREADS", short, long, default_value = "2")]
    threads: usize,

    #[arg(env = "KEMONO_USERNAME")]
    username: Option<String>,
    #[arg(env = "KEMONO_PASSWORD")]
    password: Option<String>,
    #[arg(env = "KEMONO_DEBUG", short, long)]
    debug: bool,

    #[arg(env = "KEMONO_MKVS", short, long)]
    /// If the "original" file is an mp4 or m4v then we might have a mkv file and that's OK
    mkvs: bool,

    #[arg(short, long)]
    filename: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

impl CliOpts {
    fn service(&self) -> String {
        match &self.command {
            Commands::Query { service, .. } => service.clone(),
            Commands::Download { service, .. } => service.clone(),
            Commands::Stats { service, .. } => service.clone(),
            Commands::Update { service, .. } => service.clone().unwrap_or("".to_string()),
        }
    }

    fn creator(&self) -> String {
        match &self.command {
            Commands::Query { creator, .. } => creator.clone(),
            Commands::Download { creator, .. } => creator.clone(),
            Commands::Stats { creator, .. } => creator.clone(),
            Commands::Update { creator, .. } => creator.clone().unwrap_or("".to_string()),
        }
    }
}

/// download a given file
fn download_content(
    cli: &CliOpts,
    client: &mut KemonoClient,
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
        Some(ap) => {
            let mut ap = ap.to_owned();

            if !ap.starts_with('/') {
                ap = format!("/{}", ap);
            }
            ap
        }
    };
    let download_filename = format!(
        "{}-{}",
        post.published.replace(':', "-"),
        attachment.name.clone().unwrap()
    );
    let download_path = PathBuf::from(format!(
        "{}/{}",
        client.get_download_path(&cli.service(), &cli.creator()),
        download_filename
    ));
    // check
    if download_path.exists() {
        if cli.debug {
            debug!(
                "Skipping {} because it already exists",
                download_path.display()
            );
        }
        return Ok(());
    }

    if cli.mkvs {
        let mkv_path = PathBuf::from(get_mkv_filename(&download_filename));
        let full_mkv_path = PathBuf::from(client.get_download_path(&cli.service(), &cli.creator()))
            .join(mkv_path.clone());
        if full_mkv_path.exists() {
            debug!(
                "Skipping mkv {} because it already exists",
                full_mkv_path.display()
            );
            return Ok(());
        } else {
            debug!("Couldn't find mkv {}", full_mkv_path.display());
        }
    }

    let url = Url::from_str(&format!("https://{}{}", client.hostname, attachment_path,))
        .map_err(KemonoError::from_stringable)?;
    let jsonmsg = json!({
        "action" : "download",
        "filename" : download_path.display().to_string(),
        "url" :url.to_string(),}
    );
    println!("{}", serde_json::to_string(&jsonmsg)?);

    if client.session.is_none() {
        client.new_session()?;
    }

    let response = client
        .session
        .as_mut()
        .unwrap()
        .get(url)
        .send()?
        .error_for_status()?;
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

async fn do_query(cli: CliOpts, client: &mut KemonoClient) -> Result<(), KemonoError> {
    let posts = client.all_posts(&cli.service(), &cli.creator()).await?;
    for post in posts {
        println!("{}", serde_json::to_string_pretty(&post)?);
    }
    Ok(())
}

async fn do_download(cli: CliOpts, client: &mut KemonoClient) -> Result<(), KemonoError> {
    let mut files = Vec::new();

    for post in client.all_posts(&cli.service(), &cli.creator()).await? {
        let post_data_filepath = PathBuf::from(&format!(
            "{}/metadata/{}.json",
            client.get_download_path(&cli.service(), &cli.creator()),
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

    info!("Found {} objects", files.len());
    let res = files.par_iter().map(|image| {
        if let Some(filename) = cli.filename.clone() {
            if let Some(post_file_name) = image.1.name.clone() {
                if !post_file_name.contains(&filename) {
                    if cli.debug {
                        debug!("Skipping {} as doesn't match {}", post_file_name, filename);
                    }
                    return Ok(());
                }
            }
        }
        let (post, attachment) = image;
        let mut client = KemonoClient::new_from(client);

        if let Err(err) = download_content(&cli, &mut client, post, attachment)
        // })
        {
            match err {
                KemonoError::Reqwest(req_error) => {
                    if let Some(status_code) = req_error.status() {
                        if status_code.as_u16() == 429 {
                            eprintln!("Got rate limited, bailing for now!");
                            error!("Got rate limited, bailing for now!");
                            return Err(KemonoError::RateLimited);
                        }
                    } else {
                        error!("Failed to download {:?} {:?}", attachment, req_error);
                    }
                }
                _ => error!("Failed to download {:?} {:?}", attachment, err), // KemonoError::Generic(_) => todo!(),
                                                                              // KemonoError::SerdeJson(_) => todo!(),
            }
        };
        Ok(())
    });

    if let Err(err) = res.collect::<Result<Vec<_>, _>>() {
        return Err(err);
    }
    Ok(())
}

async fn do_stats(client: &mut KemonoClient, cli: &CliOpts) -> Result<(), KemonoError> {
    let posts = client.all_posts(&cli.service(), &cli.creator()).await?;

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
        "service": cli.service(),
        "creator": cli.creator(),
    });

    println!("{}", serde_json::to_string_pretty(&stats)?);

    Ok(())
}

/// Update everything based on the file paths in the download dir
async fn do_update(client: &mut KemonoClient, cli: &CliOpts) -> Result<(), KemonoError> {
    // get the targets
    //
    let base_path = PathBuf::from(&client.get_base_download_path());

    eprintln!("Checking {}", base_path.display());

    for creator in base_path.read_dir().map_err(|err| err.to_string())? {
        let creator = creator.map_err(|err| err.to_string())?;
        // find the services
        let creator_name = creator.file_name();

        let creator_name = creator_name.to_str().expect("Failed to string-ify creator");

        if !cli.creator().is_empty() && creator_name != cli.creator() {
            debug!(
                "Skipping {} as doesn't match {}",
                creator_name,
                cli.creator()
            );
            continue;
        }
        debug!("Updating {}", creator_name);

        if creator.path().is_dir() {
            for service in creator.path().read_dir().map_err(|err| err.to_string())? {
                let service = service
                    .map_err(|err| format!("failed to get direntry: {}", err))?
                    .path();
                if !service.is_dir() {
                    eprintln!("Skipping service {:?}", service);
                    continue;
                }
                let service = service
                    .file_name()
                    .map(|s| s.to_str().expect("Failed to string-ify service"))
                    .expect("Failed to get service name");

                eprintln!(
                    "{}",
                    serde_json::to_string(&json!({"creator": creator_name,"service" : service}))?
                );

                if !cli.service().is_empty() && cli.service() != service {
                    info!(
                        "Skipping service {} for creator {} as didn't match {}",
                        service,
                        creator_name,
                        cli.service()
                    );
                    continue;
                } else {
                    eprintln!(
                        "Continuing with creator: {} service: {}",
                        creator_name, service
                    );
                }

                if let Err(err) = do_download(
                    CliOpts {
                        command: Commands::Download {
                            copt: SharedCliOpts {},
                            service: service.to_string(),
                            creator: creator_name.to_string(),
                        },
                        debug: cli.debug,
                        mkvs: cli.mkvs,
                        hostname: cli.hostname.clone(),
                        username: cli.username.clone(),
                        password: cli.password.clone(),
                        threads: cli.threads,
                        filename: cli.filename.clone(),
                    },
                    client,
                )
                .await
                {
                    eprintln!(
                        "Failed to update creator: {} service: {} {:?}",
                        creator_name, service, err
                    );
                };
            }
        }
    }

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = CliOpts::parse();

    let log_level = match cli.debug {
        false => "info",
        true => "debug",
    };

    Builder::with_level(log_level)
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    let mut client = KemonoClient::new(&cli.hostname.clone());
    client.username = cli.username.clone();
    client.password = cli.password.clone();
    if cli.mkvs && cli.debug {
        debug!("MKV checking mode enabled");
    }
    // if client.username.is_some() {
    //     if let Err(err) = client.login().await {
    //         error!("Failed to login: {:?}", err);
    //         return;
    //     }
    // }

    // build the threadpool for rayon so we don't get rate limited
    ThreadPoolBuilder::new()
        .num_threads(cli.threads)
        .build_global()
        .unwrap();

    match cli.command {
        Commands::Stats { .. } => {
            info!(
                "Pulling stats for {}/{}/{}",
                cli.hostname,
                cli.service(),
                cli.creator()
            );
            if let Err(err) = do_stats(&mut client, &cli).await {
                error!("Failed to complete stats: {:?}", err);
            };
        }
        Commands::Query { .. } => {
            info!(
                "Pulling API data for {}/{}/{}",
                cli.hostname,
                cli.service(),
                cli.creator()
            );
            if let Err(err) = do_query(cli, &mut client).await {
                error!("Failed to complete query: {:?}", err);
            };
        }
        Commands::Download { .. } => {
            info!(
                "Downloading all content for {}/{}/{}",
                cli.hostname,
                cli.service(),
                cli.creator()
            );
            if let Err(err) = do_download(cli, &mut client).await {
                error!("Failed to complete download: {:?}", err);
            };
        }
        Commands::Update { .. } => {
            info!(
                "Updating all content for creators/services in {} service: {}",
                client
                    .download_path
                    .clone()
                    .unwrap_or(DEFAULT_DOWNLOAD_PATH.to_string()),
                client.hostname
            );
            match do_update(&mut client, &cli).await {
                Err(err) => eprintln!("Failed to complete update: {:?}", err),
                Ok(()) => eprintln!("Update complete"),
            };
        }
    }
}
