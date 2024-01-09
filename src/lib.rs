use std::collections::HashSet;
use std::str::FromStr;

use errors::KemonoError;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod errors;

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
}

impl KemonoClient {
    pub fn base_url(&self) -> String {
        format!("https://{}/api/v1", self.hostname)
    }

    pub fn get_download_path(&self, service: &str, creator: &str) -> String {
        format!(
            "{}/{}/{}",
            self.download_path
                .clone()
                .unwrap_or("./download".to_string()),
            creator,
            service,
        )
    }

    pub fn max_per_page(&self) -> usize {
        50
    }

    pub fn new(hostname: &str) -> Self {
        Self {
            hostname: hostname.to_string(),
            download_path: None,
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
        println!("endpoint_url: {}", endpoint_url);
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
    pub async fn all_posts(&self, service: &str, creator: &str) -> Result<Vec<Post>, KemonoError> {
        let mut offset = 0;
        let mut posts = Vec::new();
        loop {
            let res = self.posts(service, creator, None, Some(offset)).await?;
            if res.is_empty() {
                break;
            }
            posts.extend(res);
            offset += self.max_per_page();
        }
        Ok(posts)
    }

    /// Gets a list of posts for a given service/creator, filterable by query or offset
    pub async fn posts(
        &self,
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
        let res = reqwest::get(endpoint_url).await?;
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
}
