#[cfg(feature = "blocking")]
use block_on_proc::block_on;
#[cfg(feature = "tags")]
use minidom::Element;
use serde_xml_rs as serde_xml;
use std::collections::HashMap;
use std::mem;
use std::time::Duration;

use crate::bucket_ops::{BucketConfiguration, CreateBucketResponse};
use crate::command::{Command, Multipart};
use crate::creds::Credentials;
use crate::region::Region;
use std::str::FromStr;

pub type Query = HashMap<String, String>;

#[cfg(feature = "with-tokio")]
use crate::request::Reqwest as RequestImpl;
#[cfg(feature = "with-async-std")]
use crate::surf_request::SurfRequest as RequestImpl;
// #[cfg(feature = "with-async-std")]
// use async_std::{fs::File, path::Path};
// #[cfg(feature = "with-tokio")]
// use tokio::fs::File;

#[cfg(feature = "with-async-std")]
use futures_io::{AsyncRead, AsyncWrite};
#[cfg(feature = "with-tokio")]
use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(feature = "sync")]
use crate::blocking::AttoRequest as RequestImpl;
// #[cfg(feature = "sync")]
// use std::fs::File;
#[cfg(feature = "sync")]
use std::io::Read;
// #[cfg(any(feature = "sync", feature = "with-tokio"))]
// use std::path::Path;

use crate::error::S3Error;
use crate::request_trait::Request;
use crate::serde_types::{
    BucketLocationResult, CompleteMultipartUploadData, HeadObjectResult,
    InitiateMultipartUploadResponse, ListBucketResult, ListMultipartUploadsResult, Part,
};
use crate::utils::error_from_response_data;
use http::header::HeaderName;
use http::HeaderMap;

pub const CHUNK_SIZE: usize = 8_388_608; // 8 Mebibytes, min is 5 (5_242_880);

const DEFAULT_REQUEST_TIMEOUT: Option<Duration> = Some(Duration::from_secs(60));

#[derive(Debug, PartialEq)]
pub struct Tag {
    key: String,
    value: String,
}

impl Tag {
    pub fn key(&self) -> String {
        self.key.to_owned()
    }

    pub fn value(&self) -> String {
        self.value.to_owned()
    }
}

/// Instantiate an existing Bucket
///
/// # Example
///
/// ```no_run
/// use s3::bucket::Bucket;
/// use s3::creds::Credentials;
///
/// let bucket_name = "rust-s3-test";
/// let region = "us-east-1".parse().unwrap();
/// let credentials = Credentials::default().unwrap();
///
/// let bucket = Bucket::new(bucket_name, region, credentials);
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bucket {
    pub name: String,
    pub region: Region,
    pub credentials: Credentials,
    pub extra_headers: HeaderMap,
    pub extra_query: Query,
    pub request_timeout: Option<Duration>,
    path_style: bool,
    listobjects_v2: bool,
}

fn validate_expiry(expiry_secs: u32) -> Result<(), S3Error> {
    if 604800 < expiry_secs {
        return Err(S3Error::MaxExpiry(expiry_secs));
    }
    Ok(())
}

#[cfg_attr(all(feature = "with-tokio", feature = "blocking"), block_on("tokio"))]
#[cfg_attr(
    all(feature = "with-async-std", feature = "blocking"),
    block_on("async-std")
)]
impl Bucket {
    /// Get a presigned url for getting object on a given path
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use std::collections::HashMap;
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    /// let credentials = Credentials::default().unwrap();
    /// let bucket = Bucket::new(bucket_name, region, credentials).unwrap();
    ///
    /// // Add optional custom queries
    /// let mut custom_queries = HashMap::new();
    /// custom_queries.insert(
    ///    "response-content-disposition".into(),
    ///    "attachment; filename=\"test.png\"".into(),
    /// );
    ///
    /// let url = bucket.presign_get("/test.file", 86400, Some(custom_queries)).unwrap();
    /// println!("Presigned url: {}", url);
    /// ```
    pub fn presign_get<S: AsRef<str>>(
        &self,
        path: S,
        expiry_secs: u32,
        custom_queries: Option<HashMap<String, String>>,
    ) -> Result<String, S3Error> {
        validate_expiry(expiry_secs)?;
        let request = RequestImpl::new(
            self,
            path.as_ref(),
            Command::PresignGet {
                expiry_secs,
                custom_queries,
            },
        );
        request.presigned()
    }

    /// Get a presigned url for putting object to a given path
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use http::HeaderMap;
    /// use http::header::HeaderName;
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    /// let credentials = Credentials::default().unwrap();
    /// let bucket = Bucket::new(bucket_name, region, credentials).unwrap();
    ///
    /// // Add optional custom headers
    /// let mut custom_headers = HeaderMap::new();
    /// custom_headers.insert(
    ///    HeaderName::from_static("custom_header"),
    ///    "custom_value".parse().unwrap(),
    /// );
    ///
    /// let url = bucket.presign_put("/test.file", 86400, Some(custom_headers)).unwrap();
    /// println!("Presigned url: {}", url);
    /// ```
    pub fn presign_put<S: AsRef<str>>(
        &self,
        path: S,
        expiry_secs: u32,
        custom_headers: Option<HeaderMap>,
    ) -> Result<String, S3Error> {
        validate_expiry(expiry_secs)?;
        let request = RequestImpl::new(
            self,
            path.as_ref(),
            Command::PresignPut {
                expiry_secs,
                custom_headers,
            },
        );
        request.presigned()
    }

    /// Get a presigned url for deleting object on a given path
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    /// let credentials = Credentials::default().unwrap();
    /// let bucket = Bucket::new(bucket_name, region, credentials).unwrap();
    ///
    /// let url = bucket.presign_delete("/test.file", 86400).unwrap();
    /// println!("Presigned url: {}", url);
    /// ```
    pub fn presign_delete<S: AsRef<str>>(
        &self,
        path: S,
        expiry_secs: u32,
    ) -> Result<String, S3Error> {
        validate_expiry(expiry_secs)?;
        let request = RequestImpl::new(self, path.as_ref(), Command::PresignDelete { expiry_secs });
        request.presigned()
    }

    /// Create a new `Bucket` and instantiate it
    ///
    /// ```no_run
    /// use s3::{Bucket, BucketConfiguration};
    /// use s3::creds::Credentials;
    /// # use s3::region::Region;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let config = BucketConfiguration::default();
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let create_bucket_response = Bucket::create(bucket_name, region, credentials, config).await?;
    ///
    /// // `sync` fature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let create_bucket_response = Bucket::create(bucket_name, region, credentials, config)?;
    ///
    /// # let region: Region = "us-east-1".parse()?;
    /// # let credentials = Credentials::default()?;
    /// # let config = BucketConfiguration::default();
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let create_bucket_response = Bucket::create_blocking(bucket_name, region, credentials, config)?;
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn create(
        name: &str,
        region: Region,
        credentials: Credentials,
        config: BucketConfiguration,
    ) -> Result<CreateBucketResponse, S3Error> {
        let mut config = config;
        config.set_region(region.clone());
        let command = Command::CreateBucket { config };
        let bucket = Bucket::new(name, region, credentials)?;
        let request = RequestImpl::new(&bucket, "", command);
        let (data, response_code) = request.response_data(false).await?;
        let response_text = std::str::from_utf8(&data)?;
        Ok(CreateBucketResponse {
            bucket,
            response_text: response_text.to_string(),
            response_code,
        })
    }

    /// Create a new `Bucket` with path style and instantiate it
    ///
    /// ```no_run
    /// use s3::{Bucket, BucketConfiguration};
    /// use s3::creds::Credentials;
    /// # use s3::region::Region;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let config = BucketConfiguration::default();
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let create_bucket_response = Bucket::create_with_path_style(bucket_name, region, credentials, config).await?;
    ///
    /// // `sync` fature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let create_bucket_response = Bucket::create_with_path_style(bucket_name, region, credentials, config)?;
    ///
    /// # let region: Region = "us-east-1".parse()?;
    /// # let credentials = Credentials::default()?;
    /// # let config = BucketConfiguration::default();
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let create_bucket_response = Bucket::create_with_path_style_blocking(bucket_name, region, credentials, config)?;
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn create_with_path_style(
        name: &str,
        region: Region,
        credentials: Credentials,
        config: BucketConfiguration,
    ) -> Result<CreateBucketResponse, S3Error> {
        let mut config = config;
        config.set_region(region.clone());
        let command = Command::CreateBucket { config };
        let bucket = Bucket::new(name, region, credentials)?.with_path_style();
        let request = RequestImpl::new(&bucket, "", command);
        let (data, response_code) = request.response_data(false).await?;
        let response_text = std::str::from_utf8(&data)?;
        Ok(CreateBucketResponse {
            bucket,
            response_text: response_text.to_string(),
            response_code,
        })
    }

    /// Delete existing `Bucket`
    ///
    /// # Example
    /// ```rust,no_run
    /// use s3::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    /// let credentials = Credentials::default().unwrap();
    /// let bucket = Bucket::new(bucket_name, region, credentials).unwrap();
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// bucket.delete().await.unwrap();
    /// // `sync` fature will produce an identical method
    ///
    /// #[cfg(feature = "sync")]
    /// bucket.delete().unwrap();
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    ///
    /// #[cfg(feature = "blocking")]
    /// bucket.delete_blocking().unwrap();
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn delete(&self) -> Result<u16, S3Error> {
        let command = Command::DeleteBucket;
        let request = RequestImpl::new(self, "", command);
        let (_, response_code) = request.response_data(false).await?;
        Ok(response_code)
    }

    /// Instantiate an existing `Bucket`.
    ///
    /// # Example
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    ///
    /// // Fake  credentials so we don't access user's real credentials in tests
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    /// let credentials = Credentials::default().unwrap();
    ///
    /// let bucket = Bucket::new(bucket_name, region, credentials).unwrap();
    /// ```
    pub fn new(name: &str, region: Region, credentials: Credentials) -> Result<Bucket, S3Error> {
        Ok(Bucket {
            name: name.into(),
            region,
            credentials,
            extra_headers: HeaderMap::new(),
            extra_query: HashMap::new(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            path_style: false,
            listobjects_v2: true,
        })
    }

    /// Instantiate a public existing `Bucket`.
    ///
    /// # Example
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    ///
    /// let bucket = Bucket::new_public(bucket_name, region).unwrap();
    /// ```
    pub fn new_public(name: &str, region: Region) -> Result<Bucket, S3Error> {
        Ok(Bucket {
            name: name.into(),
            region,
            credentials: Credentials::anonymous()?,
            extra_headers: HeaderMap::new(),
            extra_query: HashMap::new(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            path_style: false,
            listobjects_v2: true,
        })
    }

    pub fn with_path_style(&self) -> Bucket {
        Bucket {
            name: self.name.clone(),
            region: self.region.clone(),
            credentials: self.credentials.clone(),
            extra_headers: self.extra_headers.clone(),
            extra_query: self.extra_query.clone(),
            request_timeout: self.request_timeout,
            path_style: true,
            listobjects_v2: self.listobjects_v2,
        }
    }

    pub fn with_extra_headers(&self, extra_headers: HeaderMap) -> Bucket {
        Bucket {
            name: self.name.clone(),
            region: self.region.clone(),
            credentials: self.credentials.clone(),
            extra_headers,
            extra_query: self.extra_query.clone(),
            request_timeout: self.request_timeout,
            path_style: self.path_style,
            listobjects_v2: self.listobjects_v2,
        }
    }

    pub fn with_extra_query(&self, extra_query: HashMap<String, String>) -> Bucket {
        Bucket {
            name: self.name.clone(),
            region: self.region.clone(),
            credentials: self.credentials.clone(),
            extra_headers: self.extra_headers.clone(),
            extra_query,
            request_timeout: self.request_timeout,
            path_style: self.path_style,
            listobjects_v2: self.listobjects_v2,
        }
    }

    pub fn with_request_timeout(&self, request_timeout: Duration) -> Bucket {
        Bucket {
            name: self.name.clone(),
            region: self.region.clone(),
            credentials: self.credentials.clone(),
            extra_headers: self.extra_headers.clone(),
            extra_query: self.extra_query.clone(),
            request_timeout: Some(request_timeout),
            path_style: self.path_style,
            listobjects_v2: self.listobjects_v2,
        }
    }

    pub fn with_listobjects_v1(&self) -> Bucket {
        Bucket {
            name: self.name.clone(),
            region: self.region.clone(),
            credentials: self.credentials.clone(),
            extra_headers: self.extra_headers.clone(),
            extra_query: self.extra_query.clone(),
            request_timeout: self.request_timeout,
            path_style: self.path_style,
            listobjects_v2: false,
        }
    }

    /// Instantiate an existing `Bucket` with path style addressing. Useful for compatibility with some storage APIs, like MinIO.
    ///
    /// # Example
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    /// let credentials = Credentials::default().unwrap();
    ///
    /// let bucket = Bucket::new_with_path_style(bucket_name, region, credentials).unwrap();
    /// ```
    #[deprecated(
        since = "0.31.0",
        note = "please use `new` and modify with `with_path_style`"
    )]
    pub fn new_with_path_style(
        name: &str,
        region: Region,
        credentials: Credentials,
    ) -> Result<Bucket, S3Error> {
        Ok(Bucket {
            name: name.into(),
            region,
            credentials,
            extra_headers: HeaderMap::new(),
            extra_query: HashMap::new(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            path_style: true,
            listobjects_v2: true,
        })
    }

    /// Instantiate a public existing `Bucket` with path style addressing. Useful for compatibility with some storage APIs, like MinIO.
    ///
    /// # Example
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse().unwrap();
    ///
    /// let bucket = Bucket::new_public_with_path_style(bucket_name, region).unwrap();
    /// ```
    #[deprecated(
        since = "0.31.0",
        note = "please use `new_public` and modify with `with_path_style`"
    )]
    pub fn new_public_with_path_style(name: &str, region: Region) -> Result<Bucket, S3Error> {
        Ok(Bucket {
            name: name.into(),
            region,
            credentials: Credentials::anonymous()?,
            extra_headers: HeaderMap::new(),
            extra_query: HashMap::new(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            path_style: true,
            listobjects_v2: true,
        })
    }

    /// Copy file from an S3 path, internally within the same bucket.
    ///
    /// # Example:
    ///
    /// ```rust,no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let code = bucket.copy_object_internal("/from.file", "/to.file").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let code = bucket.copy_object_internal("/from.file", "/to.file")?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn copy_object_internal<F: AsRef<str>, T: AsRef<str>>(
        &self,
        from: F,
        to: T,
    ) -> Result<u16, S3Error> {
        let fq_from = {
            let from = from.as_ref();
            let from = from.strip_prefix('/').unwrap_or(from);
            format!("{bucket}/{path}", bucket = self.name(), path = from)
        };
        self.copy_object(fq_from, to).await
    }

    #[maybe_async::maybe_async]
    async fn copy_object<F: AsRef<str>, T: AsRef<str>>(
        &self,
        from: F,
        to: T,
    ) -> Result<u16, S3Error> {
        let command = Command::CopyObject {
            from: from.as_ref(),
        };
        let request = RequestImpl::new(self, to.as_ref(), command);
        let (_, code) = request.response_data(false).await?;
        Ok(code)
    }

    /// Gets file from an S3 path.
    ///
    /// # Example:
    ///
    /// ```rust,no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (data, code) = bucket.get_object("/test.file").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (data, code) = bucket.get_object("/test.file")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (data, code) = bucket.get_object_blocking("/test.file")?;
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn get_object<S: AsRef<str>>(&self, path: S) -> Result<(Vec<u8>, u16), S3Error> {
        let command = Command::GetObject;
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data(false).await
    }

    /// Gets torrent from an S3 path.
    ///
    /// # Example:
    ///
    /// ```rust,no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (data, code) = bucket.get_object_torrent("/test.file").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (data, code) = bucket.get_object_torrent("/test.file")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (data, code) = bucket.get_object_torrent_blocking("/test.file")?;
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn get_object_torrent<S: AsRef<str>>(
        &self,
        path: S,
    ) -> Result<(Vec<u8>, u16), S3Error> {
        let command = Command::GetObjectTorrent;
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data(false).await
    }

    /// Gets specified inclusive byte range of file from an S3 path.
    ///
    /// # Example:
    ///
    /// ```rust,no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (data, code) = bucket.get_object_range("/test.file", 0, Some(31)).await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (data, code) = bucket.get_object_range("/test.file", 0, Some(31))?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (data, code) = bucket.get_object_range_blocking("/test.file", 0, Some(31))?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn get_object_range<S: AsRef<str>>(
        &self,
        path: S,
        start: u64,
        end: Option<u64>,
    ) -> Result<(Vec<u8>, u16), S3Error> {
        if let Some(end) = end {
            assert!(start < end);
        }

        let command = Command::GetObjectRange { start, end };
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data(false).await
    }

    /// Stream file from S3 path to a local file, generic over T: Write.
    ///
    /// # Example:
    ///
    /// ```rust,no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    /// use std::fs::File;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    /// let mut output_file = File::create("output_file").expect("Unable to create file");
    /// let mut async_output_file = tokio::fs::File::create("async_output_file").await.expect("Unable to create file");
    /// #[cfg(feature = "with-async-std")]
    /// let mut async_output_file = async_std::fs::File::create("async_output_file").await.expect("Unable to create file");
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let status_code = bucket.get_object_stream("/test.file", &mut async_output_file).await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let status_code = bucket.get_object_stream("/test.file", &mut output_file)?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features. Based of the async branch
    /// #[cfg(feature = "blocking")]
    /// let status_code = bucket.get_object_stream_blocking("/test.file", &mut async_output_file)?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::async_impl]
    pub async fn get_object_stream<T: AsyncWrite + Send + Unpin, S: AsRef<str>>(
        &self,
        path: S,
        writer: &mut T,
    ) -> Result<u16, S3Error> {
        let command = Command::GetObject;
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data_to_writer(writer).await
    }

    #[maybe_async::sync_impl]
    pub fn get_object_stream<T: std::io::Write + Send, S: AsRef<str>>(
        &self,
        path: S,
        writer: &mut T,
    ) -> Result<u16, S3Error> {
        let command = Command::GetObject;
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data_to_writer(writer)
    }

    /// Stream file from local path to s3, generic over T: Write.
    ///
    /// # Example:
    ///
    /// ```rust,no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    /// use std::fs::File;
    /// use std::io::Write;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    /// let path = "path";
    /// let test: Vec<u8> = (0..1000).map(|_| 42).collect();
    /// let mut file = File::create(path)?;
    /// file.write_all(&test)?;
    ///
    /// #[cfg(feature = "with-tokio")]
    /// let mut path = tokio::fs::File::open(path).await?;
    ///
    /// #[cfg(feature = "with-async-std")]
    /// let mut path = async_std::fs::File::open(path).await?;
    /// // Async variant with `tokio` or `async-std` features
    /// // Generic over futures_io::AsyncRead|tokio::io::AsyncRead + Unpin
    /// let status_code = bucket.put_object_stream(&mut path, "/path").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// // Generic over std::io::Read
    /// let status_code = bucket.put_object_stream(&mut path, "/path")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let status_code = bucket.put_object_stream_blocking(&mut path, "/path")?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::async_impl]
    pub async fn put_object_stream<R: AsyncRead + Unpin>(
        &self,
        reader: &mut R,
        s3_path: impl AsRef<str>,
    ) -> Result<u16, S3Error> {
        self._put_object_stream(reader, s3_path.as_ref()).await
    }

    #[maybe_async::sync_impl]
    pub fn put_object_stream<R: Read>(
        &self,
        reader: &mut R,
        s3_path: impl AsRef<str>,
    ) -> Result<u16, S3Error> {
        self._put_object_stream(reader, s3_path.as_ref())
    }

    #[maybe_async::async_impl]
    async fn _put_object_stream<R: AsyncRead + Unpin>(
        &self,
        reader: &mut R,
        s3_path: &str,
    ) -> Result<u16, S3Error> {
        // If the file is smaller CHUNK_SIZE, just do a regular upload.
        // Otherwise perform a multi-part upload.
        let first_chunk = crate::utils::read_chunk(reader).await?;
        if first_chunk.len() < CHUNK_SIZE {
            let (data, code) = self.put_object(s3_path, first_chunk.as_slice()).await?;
            if code >= 300 {
                return Err(error_from_response_data(data, code)?);
            }
            return Ok(code);
        }

        let command = Command::InitiateMultipartUpload;
        let request = RequestImpl::new(self, s3_path, command);
        let (data, code) = request.response_data(false).await?;
        if code >= 300 {
            return Err(error_from_response_data(data, code)?);
        }

        let msg: InitiateMultipartUploadResponse =
            serde_xml::from_str(std::str::from_utf8(data.as_slice())?)?;
        let path = msg.key;
        let upload_id = &msg.upload_id;

        let mut part_number: u32 = 0;
        let mut etags = Vec::new();

        let mut chunk = first_chunk;
        loop {
            // Upload this chunk
            part_number += 1;
            let command = Command::PutObject {
                // part_number,
                content: &chunk,
                content_type: "application/octet-stream",
                multipart: Some(Multipart::new(part_number, upload_id)), // upload_id: &msg.upload_id,
            };
            let request = RequestImpl::new(self, &path, command);
            let (data, _code) = request.response_data(true).await?;
            let etag = std::str::from_utf8(data.as_slice())?;
            etags.push(etag.to_string());

            if chunk.len() < CHUNK_SIZE {
                break;
            }

            chunk = crate::utils::read_chunk(reader).await?
        }

        // Finish the upload
        let inner_data = etags
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, x)| Part {
                etag: x,
                part_number: i as u32 + 1,
            })
            .collect::<Vec<Part>>();
        let data = CompleteMultipartUploadData { parts: inner_data };
        let complete = Command::CompleteMultipartUpload {
            upload_id: &msg.upload_id,
            data,
        };
        let complete_request = RequestImpl::new(self, &path, complete);
        let (_data, _code) = complete_request.response_data(false).await?;
        // let response = std::str::from_utf8(data.as_slice())?;

        Ok(code)
    }

    #[maybe_async::sync_impl]
    fn _put_object_stream<R: Read>(&self, reader: &mut R, s3_path: &str) -> Result<u16, S3Error> {
        let command = Command::InitiateMultipartUpload;
        let request = RequestImpl::new(self, s3_path, command);
        let (data, code) = request.response_data(false)?;
        if code >= 300 {
            return Err(error_from_response_data(data, code)?);
        }
        let msg: InitiateMultipartUploadResponse =
            serde_xml::from_str(std::str::from_utf8(data.as_slice())?)?;

        let path = msg.key;
        let upload_id = &msg.upload_id;

        let mut part_number: u32 = 0;
        let mut etags = Vec::new();
        loop {
            let chunk = crate::utils::read_chunk(reader)?;

            if chunk.len() < CHUNK_SIZE {
                if part_number == 0 {
                    // Files is not big enough for multipart upload, going with regular put_object
                    self.abort_upload(&path, upload_id)?;

                    self.put_object(s3_path, chunk.as_slice())?;
                } else {
                    part_number += 1;
                    let command = Command::PutObject {
                        // part_number,
                        content: &chunk,
                        content_type: "application/octet-stream",
                        multipart: Some(Multipart::new(part_number, upload_id)), // upload_id: &msg.upload_id,
                    };
                    let request = RequestImpl::new(self, &path, command);
                    let (data, _code) = request.response_data(true)?;
                    let etag = std::str::from_utf8(data.as_slice())?;
                    etags.push(etag.to_string());
                    let inner_data = etags
                        .into_iter()
                        .enumerate()
                        .map(|(i, x)| Part {
                            etag: x,
                            part_number: i as u32 + 1,
                        })
                        .collect::<Vec<Part>>();
                    let data = CompleteMultipartUploadData { parts: inner_data };
                    let complete = Command::CompleteMultipartUpload {
                        upload_id: &msg.upload_id,
                        data,
                    };
                    let complete_request = RequestImpl::new(self, &path, complete);
                    let (_data, _code) = complete_request.response_data(false)?;
                    // let response = std::str::from_utf8(data.as_slice())?;
                }
                break;
            } else {
                part_number += 1;
                let command = Command::PutObject {
                    content: &chunk,
                    content_type: "application/octet-stream",
                    multipart: Some(Multipart::new(part_number, upload_id)),
                };
                let request = RequestImpl::new(self, &path, command);
                let (data, _code) = request.response_data(true)?;
                let etag = std::str::from_utf8(data.as_slice())?;
                etags.push(etag.to_string());
            }
        }
        Ok(code)
    }

    /// Get Bucket location.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (region, status_code) = bucket.location().await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (region, status_code) = bucket.location()?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (region, status_code) = bucket.location_blocking()?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn location(&self) -> Result<(Region, u16), S3Error> {
        let request = RequestImpl::new(self, "?location", Command::GetBucketLocation);
        let result = request.response_data(false).await?;
        let region_string = String::from_utf8_lossy(&result.0);
        let region = match serde_xml::from_reader(region_string.as_bytes()) {
            Ok(r) => {
                let location_result: BucketLocationResult = r;
                location_result.region.parse()?
            }
            Err(e) => {
                if result.1 == 200 {
                    Region::Custom {
                        region: "Custom".to_string(),
                        endpoint: "".to_string(),
                    }
                } else {
                    Region::Custom {
                        region: format!("Error encountered : {}", e),
                        endpoint: "".to_string(),
                    }
                }
            }
        };
        Ok((region, result.1))
    }

    /// Delete file from an S3 path.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (_, code) = bucket.delete_object("/test.file").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (_, code) = bucket.delete_object("/test.file")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (_, code) = bucket.delete_object_blocking("/test.file")?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn delete_object<S: AsRef<str>>(&self, path: S) -> Result<(Vec<u8>, u16), S3Error> {
        let command = Command::DeleteObject;
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data(false).await
    }

    /// Head object from S3.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (head_object_result, code) = bucket.head_object("/test.png").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (head_object_result, code) = bucket.head_object("/test.png")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (head_object_result, code) = bucket.head_object_blocking("/test.png")?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn head_object<S: AsRef<str>>(
        &self,
        path: S,
    ) -> Result<(HeadObjectResult, u16), S3Error> {
        let command = Command::HeadObject;
        let request = RequestImpl::new(self, path.as_ref(), command);
        let (headers, status) = request.response_header().await?;
        let header_object = HeadObjectResult::from(&headers);
        Ok((header_object, status))
    }

    /// Put into an S3 bucket, with explicit content-type.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    /// let content = "I want to go to S3".as_bytes();
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (_, code) = bucket.put_object_with_content_type("/test.file", content, "text/plain").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (_, code) = bucket.put_object_with_content_type("/test.file", content, "text/plain")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (_, code) = bucket.put_object_with_content_type_blocking("/test.file", content, "text/plain")?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn put_object_with_content_type<S: AsRef<str>>(
        &self,
        path: S,
        content: &[u8],
        content_type: &str,
    ) -> Result<(Vec<u8>, u16), S3Error> {
        let command = Command::PutObject {
            content,
            content_type,
            multipart: None,
        };
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data(true).await
    }

    /// Put into an S3 bucket.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    /// let content = "I want to go to S3".as_bytes();
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (_, code) = bucket.put_object("/test.file", content).await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (_, code) = bucket.put_object("/test.file", content)?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (_, code) = bucket.put_object_blocking("/test.file", content)?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn put_object<S: AsRef<str>>(
        &self,
        path: S,
        content: &[u8],
    ) -> Result<(Vec<u8>, u16), S3Error> {
        self.put_object_with_content_type(path, content, "application/octet-stream")
            .await
    }

    fn _tags_xml<S: AsRef<str>>(&self, tags: &[(S, S)]) -> String {
        let mut s = String::new();
        let content = tags
            .iter()
            .map(|&(ref name, ref value)| {
                format!(
                    "<Tag><Key>{}</Key><Value>{}</Value></Tag>",
                    name.as_ref(),
                    value.as_ref()
                )
            })
            .fold(String::new(), |mut a, b| {
                a.push_str(b.as_str());
                a
            });
        s.push_str("<Tagging><TagSet>");
        s.push_str(&content);
        s.push_str("</TagSet></Tagging>");
        s
    }

    /// Tag an S3 object.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (_, code) = bucket.put_object_tagging("/test.file", &[("Tag1", "Value1"), ("Tag2", "Value2")]).await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (_, code) = bucket.put_object_tagging("/test.file", &[("Tag1", "Value1"), ("Tag2", "Value2")])?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (_, code) = bucket.put_object_tagging_blocking("/test.file", &[("Tag1", "Value1"), ("Tag2", "Value2")])?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn put_object_tagging<S: AsRef<str>>(
        &self,
        path: &str,
        tags: &[(S, S)],
    ) -> Result<(Vec<u8>, u16), S3Error> {
        let content = self._tags_xml(tags);
        let command = Command::PutObjectTagging { tags: &content };
        let request = RequestImpl::new(self, path, command);
        request.response_data(false).await
    }

    /// Delete tags from an S3 object.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (_, code) = bucket.delete_object_tagging("/test.file").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (_, code) = bucket.delete_object_tagging("/test.file")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (_, code) = bucket.delete_object_tagging_blocking("/test.file")?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn delete_object_tagging<S: AsRef<str>>(
        &self,
        path: S,
    ) -> Result<(Vec<u8>, u16), S3Error> {
        let command = Command::DeleteObjectTagging;
        let request = RequestImpl::new(self, path.as_ref(), command);
        request.response_data(false).await
    }

    /// Retrieve an S3 object list of tags.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let (_, code) = bucket.get_object_tagging("/test.file").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let (_, code) = bucket.get_object_tagging("/test.file")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let (_, code) = bucket.get_object_tagging_blocking("/test.file")?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "tags")]
    #[maybe_async::maybe_async]
    pub async fn get_object_tagging<S: AsRef<str>>(
        &self,
        path: S,
    ) -> Result<(Vec<Tag>, u16), S3Error> {
        let command = Command::GetObjectTagging {};
        let request = RequestImpl::new(self, path.as_ref(), command);
        let result = request.response_data(false).await?;

        let mut tags = Vec::new();

        if result.1 == 200 {
            let result_string = String::from_utf8_lossy(&result.0);
            let ns = "http://s3.amazonaws.com/doc/2006-03-01/";
            if let Ok(tagging) = result_string.parse::<Element>() {
                for tag_set in tagging.children() {
                    if tag_set.is("TagSet", ns) {
                        for tag in tag_set.children() {
                            if tag.is("Tag", ns) {
                                let key = if let Some(element) = tag.get_child("Key", ns) {
                                    element.text()
                                } else {
                                    "Could not parse Key from Tag".to_string()
                                };
                                let value = if let Some(element) = tag.get_child("Values", ns) {
                                    element.text()
                                } else {
                                    "Could not parse Values from Tag".to_string()
                                };
                                tags.push(Tag { key, value });
                            }
                        }
                    }
                }
            }
        }

        Ok((tags, result.1))
    }

    #[maybe_async::maybe_async]
    pub async fn list_page(
        &self,
        prefix: String,
        delimiter: Option<String>,
        continuation_token: Option<String>,
        start_after: Option<String>,
        max_keys: Option<usize>,
    ) -> Result<(ListBucketResult, u16), S3Error> {
        let command = if self.listobjects_v2 {
            Command::ListObjectsV2 {
                prefix,
                delimiter,
                continuation_token,
                start_after,
                max_keys,
            }
        } else {
            // In the v1 ListObjects request, there is only one "marker"
            // field that serves as both the initial starting position,
            // and as the continuation token.
            Command::ListObjects {
                prefix,
                delimiter,
                marker: std::cmp::max(continuation_token, start_after),
                max_keys,
            }
        };
        let request = RequestImpl::new(self, "/", command);
        let (response, status_code) = request.response_data(false).await?;
        let list_bucket_result = serde_xml::from_reader(response.as_slice())?;

        Ok((list_bucket_result, status_code))
    }

    /// List the contents of an S3 bucket.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let results = bucket.list("/".to_string(), Some("/".to_string())).await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let results = bucket.list("/".to_string(), Some("/".to_string()))?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let results = bucket.list_blocking("/".to_string(), Some("/".to_string()))?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn list(
        &self,
        prefix: String,
        delimiter: Option<String>,
    ) -> Result<Vec<ListBucketResult>, S3Error> {
        let the_bucket = self.to_owned();
        let mut results = Vec::new();
        let mut continuation_token = None;

        loop {
            let (list_bucket_result, _) = the_bucket
                .list_page(
                    prefix.clone(),
                    delimiter.clone(),
                    continuation_token,
                    None,
                    None,
                )
                .await?;
            continuation_token = list_bucket_result.next_continuation_token.clone();
            results.push(list_bucket_result);
            if continuation_token.is_none() {
                break;
            }
        }

        Ok(results)
    }

    #[maybe_async::maybe_async]
    pub async fn list_multiparts_uploads_page(
        &self,
        prefix: Option<&str>,
        delimiter: Option<&str>,
        key_marker: Option<String>,
        max_uploads: Option<usize>,
    ) -> Result<(ListMultipartUploadsResult, u16), S3Error> {
        let command = Command::ListMultipartUploads {
            prefix,
            delimiter,
            key_marker,
            max_uploads,
        };
        let request = RequestImpl::new(self, "/", command);
        let (response, status_code) = request.response_data(false).await?;
        let list_bucket_result = serde_xml::from_reader(response.as_slice())?;

        Ok((list_bucket_result, status_code))
    }

    /// List the ongoing multipart uploads of an S3 bucket. This may be useful to cleanup failed
    /// uploads, together with [`crate::bucket::Bucket::abort_upload`].
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let results = bucket.list_multiparts_uploads(Some("/"), Some("/")).await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let results = bucket.list_multiparts_uploads(Some("/"), Some("/"))?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let results = bucket.list_multiparts_uploads_blocking(Some("/"), Some("/"))?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn list_multiparts_uploads(
        &self,
        prefix: Option<&str>,
        delimiter: Option<&str>,
    ) -> Result<Vec<ListMultipartUploadsResult>, S3Error> {
        let the_bucket = self.to_owned();
        let mut results = Vec::new();
        let mut next_marker: Option<String> = None;

        loop {
            let (list_multiparts_uploads_result, _) = the_bucket
                .list_multiparts_uploads_page(prefix, delimiter, next_marker, None)
                .await?;

            let is_truncated = list_multiparts_uploads_result.is_truncated;
            next_marker = list_multiparts_uploads_result.next_marker.clone();
            results.push(list_multiparts_uploads_result);

            if !is_truncated {
                break;
            }
        }

        Ok(results)
    }

    /// Abort a running multipart upload.
    ///
    /// # Example:
    ///
    /// ```no_run
    /// use s3::bucket::Bucket;
    /// use s3::creds::Credentials;
    /// use anyhow::Result;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<()> {
    ///
    /// let bucket_name = "rust-s3-test";
    /// let region = "us-east-1".parse()?;
    /// let credentials = Credentials::default()?;
    /// let bucket = Bucket::new(bucket_name, region, credentials)?;
    ///
    /// // Async variant with `tokio` or `async-std` features
    /// let results = bucket.abort_upload("/some/file.txt", "ZDFjM2I0YmEtMzU3ZC00OTQ1LTlkNGUtMTgxZThjYzIwNjA2").await?;
    ///
    /// // `sync` feature will produce an identical method
    /// #[cfg(feature = "sync")]
    /// let results = bucket.abort_upload("/some/file.txt", "ZDFjM2I0YmEtMzU3ZC00OTQ1LTlkNGUtMTgxZThjYzIwNjA2")?;
    ///
    /// // Blocking variant, generated with `blocking` feature in combination
    /// // with `tokio` or `async-std` features.
    /// #[cfg(feature = "blocking")]
    /// let results = bucket.abort_upload_blocking("/some/file.txt", "ZDFjM2I0YmEtMzU3ZC00OTQ1LTlkNGUtMTgxZThjYzIwNjA2")?;
    /// #
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn abort_upload(&self, key: &str, upload_id: &str) -> Result<(), S3Error> {
        let abort = Command::AbortMultipartUpload { upload_id };
        let abort_request = RequestImpl::new(self, key, abort);
        let (content, code) = abort_request.response_data(false).await?;

        if (200..300).contains(&code) {
            Ok(())
        } else {
            let utf8_content = String::from_utf8(content)?;
            Err(S3Error::Http(code, utf8_content))
        }
    }

    /// Get path_style field of the Bucket struct
    pub fn is_path_style(&self) -> bool {
        self.path_style
    }

    /// Get negated path_style field of the Bucket struct
    pub fn is_subdomain_style(&self) -> bool {
        !self.path_style
    }

    /// Configure bucket to use path-style urls and headers
    pub fn set_path_style(&mut self) {
        self.path_style = true;
    }

    /// Configure bucket to use subdomain style urls and headers \[default\]
    pub fn set_subdomain_style(&mut self) {
        self.path_style = false;
    }

    /// Configure bucket to apply this request timeout to all HTTP
    /// requests, or no (infinity) timeout if `None`.  Defaults to
    /// 30 seconds.
    ///
    /// Only the attohttpc and the Reqwest backends obey this option;
    /// async code may instead await with a timeout.
    pub fn set_request_timeout(&mut self, timeout: Option<Duration>) {
        self.request_timeout = timeout;
    }

    /// Configure bucket to use the older ListObjects API
    ///
    /// If your provider doesn't support the ListObjectsV2 interface, set this to
    /// use the v1 ListObjects interface instead. This is currently needed at least
    /// for Google Cloud Storage.
    pub fn set_listobjects_v1(&mut self) {
        self.listobjects_v2 = false;
    }

    /// Configure bucket to use the newer ListObjectsV2 API
    pub fn set_listobjects_v2(&mut self) {
        self.listobjects_v2 = true;
    }

    /// Get a reference to the name of the S3 bucket.
    pub fn name(&self) -> String {
        self.name.to_string()
    }

    // Get a reference to the hostname of the S3 API endpoint.
    pub fn host(&self) -> String {
        if self.path_style {
            self.path_style_host()
        } else {
            self.subdomain_style_host()
        }
    }

    pub fn url(&self) -> String {
        if self.path_style {
            format!(
                "{}://{}/{}",
                self.scheme(),
                self.path_style_host(),
                self.name()
            )
        } else {
            format!("{}://{}", self.scheme(), self.subdomain_style_host())
        }
    }

    /// Get a paths-style reference to the hostname of the S3 API endpoint.
    pub fn path_style_host(&self) -> String {
        self.region.host()
    }

    pub fn subdomain_style_host(&self) -> String {
        format!("{}.{}", self.name, self.region.host())
    }

    // pub fn self_host(&self) -> String {
    //     format!("{}.{}", self.name, self.region.host())
    // }

    pub fn scheme(&self) -> String {
        self.region.scheme()
    }

    /// Get the region this object will connect to.
    pub fn region(&self) -> Region {
        self.region.clone()
    }

    /// Get a reference to the AWS access key.
    pub fn access_key(&self) -> Option<String> {
        self.credentials
            .access_key
            .clone()
            .map(|key| key.replace('\n', ""))
    }

    /// Get a reference to the AWS secret key.
    pub fn secret_key(&self) -> Option<String> {
        self.credentials
            .secret_key
            .clone()
            .map(|key| key.replace('\n', ""))
    }

    /// Get a reference to the AWS security token.
    pub fn security_token(&self) -> Option<&str> {
        self.credentials.security_token.as_deref()
    }

    /// Get a reference to the AWS session token.
    pub fn session_token(&self) -> Option<&str> {
        self.credentials.session_token.as_deref()
    }

    /// Get a reference to the full [`Credentials`](struct.Credentials.html)
    /// object used by this `Bucket`.
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// Change the credentials used by the Bucket, returning the existing
    /// credentials.
    pub fn set_credentials(&mut self, credentials: Credentials) -> Credentials {
        mem::replace(&mut self.credentials, credentials)
    }

    /// Add an extra header to send with requests to S3.
    ///
    /// Add an extra header to send with requests. Note that the library
    /// already sets a number of headers - headers set with this method will be
    /// overridden by the library headers:
    ///   * Host
    ///   * Content-Type
    ///   * Date
    ///   * Content-Length
    ///   * Authorization
    ///   * X-Amz-Content-Sha256
    ///   * X-Amz-Date
    pub fn add_header(&mut self, key: &str, value: &str) {
        self.extra_headers
            .insert(HeaderName::from_str(key).unwrap(), value.parse().unwrap());
    }

    /// Get a reference to the extra headers to be passed to the S3 API.
    pub fn extra_headers(&self) -> &HeaderMap {
        &self.extra_headers
    }

    /// Get a mutable reference to the extra headers to be passed to the S3
    /// API.
    pub fn extra_headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.extra_headers
    }

    /// Add an extra query pair to the URL used for S3 API access.
    pub fn add_query(&mut self, key: &str, value: &str) {
        self.extra_query.insert(key.into(), value.into());
    }

    /// Get a reference to the extra query pairs to be passed to the S3 API.
    pub fn extra_query(&self) -> &Query {
        &self.extra_query
    }

    /// Get a mutable reference to the extra query pairs to be passed to the S3
    /// API.
    pub fn extra_query_mut(&mut self) -> &mut Query {
        &mut self.extra_query
    }

    pub fn request_timeout(&self) -> Option<Duration> {
        self.request_timeout
    }
}

#[cfg(test)]
mod test {

    use crate::creds::Credentials;
    use crate::region::Region;
    use crate::Bucket;
    use crate::BucketConfiguration;
    use crate::Tag;
    use http::header::HeaderName;
    use http::HeaderMap;
    use std::env;
    // use log::info;

    fn init() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    fn test_aws_credentials() -> Credentials {
        Credentials::new(
            Some(&env::var("EU_AWS_ACCESS_KEY_ID").unwrap()),
            Some(&env::var("EU_AWS_SECRET_ACCESS_KEY").unwrap()),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn test_gc_credentials() -> Credentials {
        Credentials::new(
            Some(&env::var("GC_ACCESS_KEY_ID").unwrap()),
            Some(&env::var("GC_SECRET_ACCESS_KEY").unwrap()),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn test_wasabi_credentials() -> Credentials {
        Credentials::new(
            Some(&env::var("WASABI_ACCESS_KEY_ID").unwrap()),
            Some(&env::var("WASABI_SECRET_ACCESS_KEY").unwrap()),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn test_minio_credentials() -> Credentials {
        Credentials::new(Some("test"), Some("test1234"), None, None, None).unwrap()
    }

    fn test_digital_ocean_credentials() -> Credentials {
        Credentials::new(
            Some(&env::var("DIGITAL_OCEAN_ACCESS_KEY_ID").unwrap()),
            Some(&env::var("DIGITAL_OCEAN_SECRET_ACCESS_KEY").unwrap()),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn test_aws_bucket() -> Bucket {
        Bucket::new(
            "rust-s3-test",
            "eu-central-1".parse().unwrap(),
            test_aws_credentials(),
        )
        .unwrap()
    }

    fn test_wasabi_bucket() -> Bucket {
        Bucket::new(
            "rust-s3",
            "wa-eu-central-1".parse().unwrap(),
            test_wasabi_credentials(),
        )
        .unwrap()
    }

    fn test_gc_bucket() -> Bucket {
        let mut bucket = Bucket::new(
            "rust-s3",
            Region::Custom {
                region: "us-east1".to_owned(),
                endpoint: "https://storage.googleapis.com".to_owned(),
            },
            test_gc_credentials(),
        )
        .unwrap();
        bucket.set_listobjects_v1();
        bucket
    }

    fn test_minio_bucket() -> Bucket {
        Bucket::new(
            "rust-s3",
            Region::Custom {
                region: "eu-central-1".to_owned(),
                endpoint: "http://localhost:9000".to_owned(),
            },
            test_minio_credentials(),
        )
        .unwrap()
        .with_path_style()
    }

    fn test_digital_ocean_bucket() -> Bucket {
        Bucket::new("rust-s3", Region::DoFra1, test_digital_ocean_credentials()).unwrap()
    }

    fn object(size: u32) -> Vec<u8> {
        (0..size).map(|_| 33).collect()
    }

    #[maybe_async::maybe_async]
    async fn put_head_get_delete_object(bucket: Bucket) {
        let s3_path = "/+test.file";
        let test: Vec<u8> = object(3072);

        let (_data, code) = bucket.put_object(s3_path, &test).await.unwrap();
        // println!("{}", std::str::from_utf8(&data).unwrap());
        assert_eq!(code, 200);
        let (data, code) = bucket.get_object(s3_path).await.unwrap();
        assert_eq!(code, 200);
        // println!("{}", std::str::from_utf8(&data).unwrap());
        assert_eq!(test, data);

        let (data, code) = bucket
            .get_object_range(s3_path, 100, Some(1000))
            .await
            .unwrap();
        assert_eq!(code, 206);
        // println!("{}", std::str::from_utf8(&data).unwrap());
        assert_eq!(test[100..1001].to_vec(), data);

        let (head_object_result, code) = bucket.head_object(s3_path).await.unwrap();
        assert_eq!(code, 200);
        assert_eq!(
            head_object_result.content_type.unwrap(),
            "application/octet-stream".to_owned()
        );
        // println!("{:?}", head_object_result);
        let (_, code) = bucket.delete_object(s3_path).await.unwrap();
        assert_eq!(code, 204);
    }

    #[ignore]
    #[cfg(feature = "tags")]
    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn test_tagging_aws() {
        let bucket = test_aws_bucket();
        let _target_tags = vec![
            Tag {
                key: "Tag1".to_string(),
                value: "Value1".to_string(),
            },
            Tag {
                key: "Tag2".to_string(),
                value: "Value2".to_string(),
            },
        ];
        let empty_tags: Vec<Tag> = Vec::new();
        let (_data, code) = bucket
            .put_object("tagging_test", b"Gimme tags")
            .await
            .unwrap();
        assert_eq!(code, 200);
        let (tags, _code) = bucket.get_object_tagging("tagging_test").await.unwrap();
        assert_eq!(tags, empty_tags);
        let (_body, code) = bucket
            .put_object_tagging("tagging_test", &[("Tag1", "Value1"), ("Tag2", "Value2")])
            .await
            .unwrap();
        assert_eq!(code, 200);
        // This could be eventually consistent now
        let (_tags, _code) = bucket.get_object_tagging("tagging_test").await.unwrap();
        // assert_eq!(tags, target_tags)
        let (_data, _code) = bucket.delete_object("tagging_test").await.unwrap();
    }

    /// Test multi-part upload
    // #[ignore]
    // #[maybe_async::test(
    //     feature = "sync",
    //     async(
    //         all(
    //             not(feature = "sync"),
    //             not(feature = "tokio-rustls-tls"),
    //             feature = "with-tokio"
    //         ),
    //         tokio::test
    //     ),
    //     async(
    //         all(not(feature = "sync"), feature = "with-async-std"),
    //         async_std::test
    //     )
    // )]
    // async fn streaming_test_put_get_delete_big_object() {
    //     init();
    //     let remote_path = "+stream_test_big";
    //     let local_path = "+stream_test_big";
    //     std::fs::remove_file(remote_path).unwrap_or_else(|_| {});
    //     let bucket = test_aws_bucket();
    //     let content: Vec<u8> = object(10_000_000);

    //     let mut file = File::create(local_path).unwrap();
    //     file.write_all(&content).unwrap();
    //     cfg_if! {
    //         if #[cfg(feature = "with-tokio")] {
    //             let mut reader = tokio::fs::File::open(local_path).await.unwrap();
    //         } else if #[cfg(feature = "with-async-std")] {
    //             let mut reader = async_std::fs::File::open(local_path).await.unwrap();
    //         } else if #[cfg(feature = "sync")] {
    //             let mut reader = File::open(local_path).unwrap();
    //         }
    //     }

    //     let code = bucket
    //         .put_object_stream(&mut reader, remote_path)
    //         .await
    //         .unwrap();
    //     assert_eq!(code, 200);
    //     let mut writer = Vec::new();
    //     let code = bucket
    //         .get_object_stream(remote_path, &mut writer)
    //         .await
    //         .unwrap();
    //     assert_eq!(code, 200);
    //     assert_eq!(content, writer);

    // let (body, code) = bucket.get_object_torrent(remote_path).await.unwrap();
    // assert_eq!(code, 200);
    // assert_eq!(
    //     body,
    //     [
    //         100, 56, 58, 97, 110, 110, 111, 117, 110, 99, 101, 53, 56, 58, 104, 116, 116, 112,
    //         58, 47, 47, 115, 51, 45, 116, 114, 97, 99, 107, 101, 114, 46, 101, 117, 45, 99,
    //         101, 110, 116, 114, 97, 108, 45, 49, 46, 97, 109, 97, 122, 111, 110, 97, 119, 115,
    //         46, 99, 111, 109, 58, 54, 57, 54, 57, 47, 97, 110, 110, 111, 117, 110, 99, 101, 49,
    //         51, 58, 97, 110, 110, 111, 117, 110, 99, 101, 45, 108, 105, 115, 116, 108, 108, 53,
    //         56, 58, 104, 116, 116, 112, 58, 47, 47, 115, 51, 45, 116, 114, 97, 99, 107, 101,
    //         114, 46, 101, 117, 45, 99, 101, 110, 116, 114, 97, 108, 45, 49, 46, 97, 109, 97,
    //         122, 111, 110, 97, 119, 115, 46, 99, 111, 109, 58, 54, 57, 54, 57, 47, 97, 110,
    //         110, 111, 117, 110, 99, 101, 101, 101, 52, 58, 105, 110, 102, 111, 100, 54, 58,
    //         108, 101, 110, 103, 116, 104, 105, 49, 48, 48, 48, 48, 48, 48, 48, 101, 52, 58,
    //         110, 97, 109, 101, 49, 54, 58, 43, 115, 116, 114, 101, 97, 109, 95, 116, 101, 115,
    //         116, 95, 98, 105, 103, 49, 50, 58, 112, 105, 101, 99, 101, 32, 108, 101, 110, 103,
    //         116, 104, 105, 50, 54, 50, 49, 52, 52, 101, 54, 58, 112, 105, 101, 99, 101, 115,
    //         55, 56, 48, 58, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96,
    //         103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1,
    //         96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16,
    //         172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60,
    //         16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136,
    //         60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206,
    //         136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95,
    //         206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85,
    //         95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56,
    //         85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65,
    //         56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67,
    //         65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157,
    //         67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24,
    //         157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103,
    //         24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146,
    //         103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201,
    //         146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134,
    //         201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49,
    //         134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103,
    //         49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96,
    //         103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1,
    //         96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16,
    //         172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60,
    //         16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136,
    //         60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206,
    //         136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95,
    //         206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85,
    //         95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56,
    //         85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67, 65,
    //         56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157, 67,
    //         65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24, 157,
    //         67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103, 24,
    //         157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146, 103,
    //         24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201, 146,
    //         103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134, 201,
    //         146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49, 134,
    //         201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103, 49,
    //         134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96, 103,
    //         49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1, 96,
    //         103, 49, 134, 201, 146, 103, 24, 157, 67, 65, 56, 85, 95, 206, 136, 60, 16, 172, 1,
    //         96, 103, 49, 134, 201, 146, 37, 227, 84, 182, 214, 2, 98, 71, 21, 79, 174, 237,
    //         155, 252, 61, 238, 62, 140, 232, 193, 49, 50, 58, 120, 45, 97, 109, 122, 45, 98,
    //         117, 99, 107, 101, 116, 49, 50, 58, 114, 117, 115, 116, 45, 115, 51, 45, 116, 101,
    //         115, 116, 57, 58, 120, 45, 97, 109, 122, 45, 107, 101, 121, 49, 54, 58, 43, 115,
    //         116, 114, 101, 97, 109, 95, 116, 101, 115, 116, 95, 98, 105, 103, 101, 101
    //     ]
    // );
    //     let (_, code) = bucket.delete_object(remote_path).await.unwrap();
    //     assert_eq!(code, 204);
    //     std::fs::remove_file(local_path).unwrap_or_else(|_| {});
    // }

    /// Test streaming upload, with an object that's smaller than CHUNK_SIZE.
    /// This should be optimized into a plain PUT, not a multi-part upload.
    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(
            all(
                not(feature = "sync"),
                not(feature = "tokio-rustls-tls"),
                feature = "with-tokio"
            ),
            tokio::test
        ),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn streaming_test_put_get_delete_small_object() {
        init();
        let remote_path = "+stream_test_small";
        let bucket = test_gc_bucket();
        let content: Vec<u8> = object(1000);
        let mut reader = std::io::Cursor::new(&content);

        #[cfg(feature = "with-async-std")]
        let mut reader = async_std::io::Cursor::new(&content);

        let code = bucket
            .put_object_stream(&mut reader, remote_path)
            .await
            .unwrap();
        assert_eq!(code, 200);
        let mut writer = Vec::new();
        let code = bucket
            .get_object_stream(remote_path, &mut writer)
            .await
            .unwrap();
        assert_eq!(code, 200);
        assert_eq!(content, writer);

        let (_, code) = bucket.delete_object(remote_path).await.unwrap();
        assert_eq!(code, 204);
    }

    #[cfg(feature = "blocking")]
    fn put_head_get_list_delete_object_blocking(bucket: Bucket) {
        let s3_path = "/test_blocking.file";
        let s3_path_2 = "/test_blocking.file2";
        let s3_path_3 = "/test_blocking.file3";
        let test: Vec<u8> = object(3072);

        // Test PutObject
        let (_data, code) = bucket.put_object_blocking(s3_path, &test).unwrap();
        // println!("{}", std::str::from_utf8(&data).unwrap());
        assert_eq!(code, 200);

        // Test GetObject
        let (data, code) = bucket.get_object_blocking(s3_path).unwrap();
        assert_eq!(code, 200);
        // println!("{}", std::str::from_utf8(&data).unwrap());
        assert_eq!(test, data);

        // Test GetObject with a range
        let (data, code) = bucket
            .get_object_range_blocking(s3_path, 100, Some(1000))
            .unwrap();
        assert_eq!(code, 206);
        // println!("{}", std::str::from_utf8(&data).unwrap());
        assert_eq!(test[100..1001].to_vec(), data);

        // Test HeadObject
        let (head_object_result, code) = bucket.head_object_blocking(s3_path).unwrap();
        assert_eq!(code, 200);
        assert_eq!(
            head_object_result.content_type.unwrap(),
            "application/octet-stream".to_owned()
        );
        // println!("{:?}", head_object_result);

        // Put some additional objects, so that we can test ListObjects
        let (_data, code) = bucket.put_object_blocking(s3_path_2, &test).unwrap();
        assert_eq!(code, 200);
        let (_data, code) = bucket.put_object_blocking(s3_path_3, &test).unwrap();
        assert_eq!(code, 200);

        // Test ListObjects, with continuation
        let (result, code) = bucket
            .list_page_blocking(
                "test_blocking.".to_string(),
                Some("/".to_string()),
                None,
                None,
                Some(2),
            )
            .unwrap();
        assert_eq!(code, 200);
        assert_eq!(result.contents.len(), 2);
        assert_eq!(result.contents[0].key, s3_path[1..]);
        assert_eq!(result.contents[1].key, s3_path_2[1..]);

        let cont_token = result.next_continuation_token.unwrap();

        let (result, code) = bucket
            .list_page_blocking(
                "test_blocking.".to_string(),
                Some("/".to_string()),
                Some(cont_token),
                None,
                Some(2),
            )
            .unwrap();
        assert_eq!(code, 200);
        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].key, s3_path_3[1..]);
        assert!(result.next_continuation_token.is_none());

        // cleanup (and test Delete)
        let (_, code) = bucket.delete_object_blocking(s3_path).unwrap();
        assert_eq!(code, 204);
        let (_, code) = bucket.delete_object_blocking(s3_path_2).unwrap();
        assert_eq!(code, 204);
        let (_, code) = bucket.delete_object_blocking(s3_path_3).unwrap();
        assert_eq!(code, 204);
    }

    #[ignore]
    #[cfg(all(
        any(feature = "with-tokio", feature = "with-async-std"),
        feature = "blocking"
    ))]
    #[test]
    fn aws_put_head_get_delete_object_blocking() {
        put_head_get_list_delete_object_blocking(test_aws_bucket())
    }

    #[ignore]
    #[cfg(all(
        any(feature = "with-tokio", feature = "with-async-std"),
        feature = "blocking"
    ))]
    #[test]
    fn gc_put_head_get_delete_object_blocking() {
        put_head_get_list_delete_object_blocking(test_gc_bucket())
    }

    #[ignore]
    #[cfg(all(
        any(feature = "with-tokio", feature = "with-async-std"),
        feature = "blocking"
    ))]
    #[test]
    fn wasabi_put_head_get_delete_object_blocking() {
        put_head_get_list_delete_object_blocking(test_wasabi_bucket())
    }

    #[ignore]
    #[cfg(all(
        any(feature = "with-tokio", feature = "with-async-std"),
        feature = "blocking"
    ))]
    #[test]
    fn minio_put_head_get_delete_object_blocking() {
        Bucket::create_with_path_style_blocking(
            "rust-s3",
            Region::Custom {
                region: "eu-central-1".to_owned(),
                endpoint: "http://localhost:9000".to_owned(),
            },
            test_minio_credentials(),
            BucketConfiguration::default(),
        )
        .unwrap();
        put_head_get_list_delete_object_blocking(test_minio_bucket())
    }

    #[ignore]
    #[cfg(all(
        any(feature = "with-tokio", feature = "with-async-std"),
        feature = "blocking"
    ))]
    #[test]
    fn digital_ocean_put_head_get_delete_object_blocking() {
        put_head_get_list_delete_object_blocking(test_digital_ocean_bucket())
    }

    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn aws_put_head_get_delete_object() {
        put_head_get_delete_object(test_aws_bucket()).await;
    }

    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(
            all(
                not(any(feature = "sync", feature = "tokio-rustls-tls")),
                feature = "with-tokio"
            ),
            tokio::test
        ),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn gc_test_put_head_get_delete_object() {
        put_head_get_delete_object(test_gc_bucket()).await;
    }

    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn wasabi_test_put_head_get_delete_object() {
        put_head_get_delete_object(test_wasabi_bucket()).await;
    }

    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn minio_test_put_head_get_delete_object() {
        Bucket::create_with_path_style(
            "rust-s3",
            Region::Custom {
                region: "eu-central-1".to_owned(),
                endpoint: "http://localhost:9000".to_owned(),
            },
            test_minio_credentials(),
            BucketConfiguration::default(),
        )
        .await
        .unwrap();
        put_head_get_delete_object(test_minio_bucket()).await;
    }

    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn digital_ocean_test_put_head_get_delete_object() {
        put_head_get_delete_object(test_digital_ocean_bucket()).await;
    }

    #[test]
    #[ignore]
    fn test_presign_put() {
        let s3_path = "/test/test.file";
        let bucket = test_aws_bucket();

        let mut custom_headers = HeaderMap::new();
        custom_headers.insert(
            HeaderName::from_static("custom_header"),
            "custom_value".parse().unwrap(),
        );

        let url = bucket
            .presign_put(s3_path, 86400, Some(custom_headers))
            .unwrap();

        assert!(url.contains("host%3Bcustom_header"));
        assert!(url.contains("/test/test.file"))
    }

    #[test]
    #[ignore]
    fn test_presign_get() {
        let s3_path = "/test/test.file";
        let bucket = test_aws_bucket();

        let url = bucket.presign_get(s3_path, 86400, None).unwrap();
        assert!(url.contains("/test/test.file?"))
    }

    #[test]
    #[ignore]
    fn test_presign_delete() {
        let s3_path = "/test/test.file";
        let bucket = test_aws_bucket();

        let url = bucket.presign_delete(s3_path, 86400).unwrap();
        assert!(url.contains("/test/test.file?"))
    }

    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    #[ignore]
    async fn test_bucket_create_delete_default_region() {
        let config = BucketConfiguration::default();
        let response = Bucket::create(
            &uuid::Uuid::new_v4().to_string(),
            "us-east-1".parse().unwrap(),
            test_aws_credentials(),
            config,
        )
        .await
        .unwrap();

        assert_eq!(&response.response_text, "");

        assert_eq!(response.response_code, 200);

        let response_code = response.bucket.delete().await.unwrap();
        assert!(response_code < 300);
    }

    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn test_bucket_create_delete_non_default_region() {
        let config = BucketConfiguration::default();
        let response = Bucket::create(
            &uuid::Uuid::new_v4().to_string(),
            "eu-central-1".parse().unwrap(),
            test_aws_credentials(),
            config,
        )
        .await
        .unwrap();

        assert_eq!(&response.response_text, "");

        assert_eq!(response.response_code, 200);

        let response_code = response.bucket.delete().await.unwrap();
        assert!(response_code < 300);
    }

    #[ignore]
    #[maybe_async::test(
        feature = "sync",
        async(all(not(feature = "sync"), feature = "with-tokio"), tokio::test),
        async(
            all(not(feature = "sync"), feature = "with-async-std"),
            async_std::test
        )
    )]
    async fn test_bucket_create_delete_non_default_region_public() {
        let config = BucketConfiguration::public();
        let response = Bucket::create(
            &uuid::Uuid::new_v4().to_string(),
            "eu-central-1".parse().unwrap(),
            test_aws_credentials(),
            config,
        )
        .await
        .unwrap();

        assert_eq!(&response.response_text, "");

        assert_eq!(response.response_code, 200);

        let response_code = response.bucket.delete().await.unwrap();
        assert!(response_code < 300);
    }

    #[test]
    fn test_tag_has_key_and_value_functions() {
        let key = "key".to_owned();
        let value = "value".to_owned();
        let tag = Tag { key, value };
        assert_eq!["key", tag.key()];
        assert_eq!["value", tag.value()];
    }

    #[test]
    fn test_builder_composition() {
        use std::time::Duration;

        let bucket = Bucket::new(
            "test-bucket",
            "eu-central-1".parse().unwrap(),
            test_aws_credentials(),
        )
        .unwrap()
        .with_request_timeout(Duration::from_secs(10));

        assert_eq!(bucket.request_timeout(), Some(Duration::from_secs(10)));
    }
}
