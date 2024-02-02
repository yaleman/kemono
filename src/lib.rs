use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use errors::KemonoError;
use log::warn;
use reqwest::cookie::Jar;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod errors;

pub static DEFAULT_DOWNLOAD_PATH: &str = "./download";

#[derive(Deserialize, Debug, Serialize)]
pub struct Creator {
    pub favorited: usize,
    pub id: String,
    pub indexed: usize,
    pub name: String,
    pub service: String,
    pub updated: usize,
}

#[derive(Deserialize, Debug, Serialize, Eq, PartialEq, Clone, Hash)]
pub struct Attachment {
    pub name: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Deserialize, Debug, Serialize)]
pub struct Post {
    pub id: String,
    pub user: String,
    pub service: String,
    pub title: String,
    pub content: Option<String>,
    pub embed: Value,
    pub shared_file: Option<bool>,
    pub file: Attachment,
    pub added: String,     // should be an offsetdatetime
    pub published: String, // should be an offsetdatetime
    pub edited: Option<bool>,
    pub poll: Option<bool>,
    pub captions: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub attachments: Option<HashSet<Attachment>>,
}

pub struct KemonoClient {
    pub hostname: String,
    pub download_path: Option<String>,
    pub session: Option<reqwest::blocking::Client>,

    pub cookies: Arc<Jar>,
    #[allow(dead_code)]
    pub username: Option<String>,
    #[allow(dead_code)]
    pub password: Option<String>,
}

impl KemonoClient {
    pub fn new_from(client: &KemonoClient) -> Self {
        Self {
            hostname: client.hostname.clone(),
            download_path: client.download_path.clone(),
            session: client.session.clone(),
            cookies: Arc::new(Jar::default()),
            username: client.username.clone(),
            password: client.password.clone(),
        }
    }

    pub fn base_url(&self) -> String {
        format!("https://{}/api/v1", self.hostname)
    }

    // pub fn user_agent(&self) -> String {
    //     format!("Rust Kemono Client v{}", env!("CARGO_PKG_VERSION"))
    // }

    pub fn new_session(&mut self) -> Result<(), KemonoError> {
        self.session = Some(
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(900))
                .cookie_store(true)
                .cookie_provider(self.cookies.clone())
                .build()?,
        );
        Ok(())
    }
    pub fn new_async_session(&mut self) -> Result<reqwest::Client, KemonoError> {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .cookie_store(true)
            .cookie_provider(self.cookies.clone())
            .build()
            .map_err(|err| err.into())
    }

    pub fn get_base_download_path(&self) -> String {
        self.download_path
            .clone()
            .unwrap_or(DEFAULT_DOWNLOAD_PATH.to_string())
    }

    /// Returns the base_path + creator + service
    pub fn get_download_path(&self, service: &str, creator: &str) -> String {
        format!("{}/{}/{}", self.get_base_download_path(), creator, service,)
    }

    pub fn max_per_page(&self) -> usize {
        50
    }

    pub fn new(hostname: &str) -> Self {
        Self {
            hostname: hostname.to_string(),
            download_path: None,
            session: None,
            username: None,
            password: None,
            cookies: Arc::new(Jar::default()),
        }
    }

    pub fn make_url(&self, endpoint: &str) -> Result<Url, KemonoError> {
        Url::from_str(&format!("{}/{}", self.base_url(), endpoint))
            .map_err(|e| KemonoError::from(e.to_string()))
    }

    /// Get the app version hash
    pub async fn app_version(&self) -> Result<String, KemonoError> {
        let endpoint_url = self.make_url("app_version")?;
        reqwest::get(endpoint_url)
            .await?
            .text()
            .await
            .map_err(KemonoError::from_stringable)
    }

    /// Get a list of creators
    pub async fn creators(&self) -> Result<Vec<Creator>, KemonoError> {
        let endpoint_url = self.make_url("creators.txt")?;
        // println!("endpoint_url: {}", endpoint_url);
        let res = reqwest::get(endpoint_url).await?;
        res.json::<Vec<Creator>>()
            .await
            .map_err(KemonoError::from_stringable)
    }

    /// Get a list of recent posts, filterable by query or offset
    pub async fn recent_posts(
        &self,
        query: Option<&str>,
        offset: Option<usize>,
    ) -> Result<Vec<Post>, KemonoError> {
        let mut endpoint_url = self.make_url("posts")?;
        if let Some(query) = query {
            endpoint_url.query_pairs_mut().append_pair("q", query);
        }
        if let Some(offset) = offset {
            endpoint_url
                .query_pairs_mut()
                .append_pair("o", offset.to_string().as_str());
        }
        let res = reqwest::get(endpoint_url).await?;
        res.json::<Vec<Post>>()
            .await
            .map_err(KemonoError::from_stringable)
    }

    /// get *all* posts for a creator/service combination
    pub async fn all_posts(
        &mut self,
        service: &str,
        creator: &str,
    ) -> Result<Vec<Post>, KemonoError> {
        let mut offset = 0;
        let mut posts = Vec::new();
        loop {
            let res = self.posts(service, creator, None, Some(offset)).await?;
            if res.is_empty() {
                warn!(
                    "Empty response from server {}/{} offset: {}",
                    service, creator, offset
                );
                break;
            }
            posts.extend(res);
            offset += self.max_per_page();
        }
        Ok(posts)
    }

    /// Gets a list of posts for a given service/creator, filterable by query or offset
    pub async fn posts(
        &mut self,
        service: &str,
        creator: &str,
        query: Option<&str>,
        offset: Option<usize>,
    ) -> Result<Vec<Post>, KemonoError> {
        let mut endpoint_url = self.make_url(&format!("{}/user/{}", service, creator))?;
        if let Some(query) = query {
            endpoint_url.query_pairs_mut().append_pair("q", query);
        }
        if let Some(offset) = offset {
            endpoint_url
                .query_pairs_mut()
                .append_pair("o", offset.to_string().as_str());
        }
        let client = self.new_async_session()?;

        let res = client.get(endpoint_url).send().await?;
        res.json::<Vec<Post>>()
            .await
            .map_err(KemonoError::from_stringable)
    }

    // TODO: /{service}/user/{creator_id}/announcements
    /*
    [
        {
        "service": "patreon",
        "user_id": "blep",
        "hash": "biglonghashnumber",
        "content": "message content",
        "added": "2023-01-31T05:16:15.462035"
        }
    ]
     */

    // TODO: /fanbox/user/{creator_id}/fancards
    /*
      [
        {
        "id": 108058645,
        "user_id": "3316400",
        "file_id": 108058645,
        "hash": "727bf3f0d774a98c80cf6c76c3fb0e049522b88eb7f02c8d3fc59bae20439fcf",
        "mtime": "2023-05-23T15:09:43.941195",
        "ctime": "2023-05-23T15:09:43.941195",
        "mime": "image/jpeg",
        "ext": ".jpg",
        "added": "2023-05-23T15:09:43.960578",
        "size": 339710,
        "ihash": null
        }
    ]
     */

    // TODO: /{service}/user/{creator_id}/post/{post_id}
    // Get a specific post

    pub async fn login(&mut self) -> Result<(), KemonoError> {
        let endpoint_url = Url::from_str(&format!("https://{}/account/login", self.hostname))
            .map_err(|err| err.to_string())?;

        let mut form = HashMap::new();
        if let Some(username) = self.username.clone() {
            form.insert("username", username);
        }

        if let Some(password) = self.password.clone() {
            form.insert("password", password);
        }

        let client = self.new_async_session()?;

        let res = client
            .post(endpoint_url)
            .header(
                "Referer",
                format!("https://{}/account/login", self.hostname),
            )
            .form(&form)
            .send()
            .await?
            .error_for_status()?;
        if res.url().as_str().contains("login") {
            return Err(KemonoError::from_stringable("Login failed"));
        }
        Ok(())
    }
}

/// replace the extension in a filename with mkv
///
/// ```
/// use kemono::get_mkv_filename;
/// assert_eq!(get_mkv_filename("test.mp4"), "test.mkv");
///  ```
pub fn get_mkv_filename(filename: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_posts() {
        let data = include_str!("../test_data.json");
        let res = serde_json::from_str::<Vec<Post>>(data)
            .map_err(|err| panic!("Failed to deserialize data: {:?}", err))
            .expect("Failed to deserialize data");
        assert!(!res.is_empty());
        println!("number of results: {}", res.len());
    }

    #[cfg(feature = "test_live")]
    #[tokio::test]
    async fn test_live_creators() {
        let host = std::env::var("KEMONO_HOSTNAME").expect("Failed to get KEMONO_HOSTNAME env var");
        let client = KemonoClient::new(&host);

        let res = client.creators().await.expect("Failed to query data");
        assert!(!res.is_empty());
        // println!("res: {:?}", res);
        println!("number of results: {}", res.len());
    }

    #[cfg(feature = "test_live")]
    #[tokio::test]
    async fn test_live_posts() {
        let host = std::env::var("KEMONO_HOSTNAME").expect("Failed to get KEMONO_HOSTNAME env var");
        let client = KemonoClient::new(&host);

        let res = client
            .posts(
                &std::env::var("KEMONO_SERVICE").expect("Failed to get KEMONO_SERVICE env var"),
                &std::env::var("KEMONO_CREATOR").expect("Failed to get KEMONO_CREATOR env var"),
                None,
                None,
            )
            .await
            .expect("Failed to query endpoint");
        assert!(!res.is_empty());
        println!("res: {:?}", res);
    }

    #[tokio::test]
    async fn test_live_login() {
        let host = std::env::var("KEMONO_HOSTNAME").expect("Failed to get KEMONO_HOSTNAME env var");

        let mut client = KemonoClient::new(&host);
        client.username = Some(
            std::env::var("KEMONO_USERNAME")
                .expect("Couldn't get password from env var")
                .to_string(),
        );
        client.password = Some(
            std::env::var("KEMONO_PASSWORD")
                .expect("Couldn't get password from env var")
                .to_string(),
        );

        client.login().await.expect("Failed to login");
    }
}
