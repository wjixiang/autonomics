use std::fmt::{Debug, Display};
use std::time::SystemTime;

use bytes::Bytes;
use chrono::{TimeZone, Utc};
use datafusion::object_store::Error as ObjectStoreError;
use datafusion::object_store::{
    Attributes, GetOptions, GetRange, GetResult, GetResultPayload, ListResult, MultipartUpload,
    ObjectMeta, ObjectStore, PutMultipartOptions, PutOptions, PutPayload, PutResult, path::Path,
};
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use opendal::{Operator, services::Memory};

pub struct OpendalFileStorage {
    pub op: Operator,
}

impl OpendalFileStorage {
    /// TODO: change to use fs
    pub fn new() -> Self {
        let op = Operator::new(Memory::default()).unwrap().finish();
        Self { op }
    }

    pub fn new_in_memory() -> Self {
        let op = Operator::new(Memory::default()).unwrap().finish();
        Self { op }
    }
}

impl Debug for OpendalFileStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpendalFileStorage")
            .field("op", &self.op)
            .finish()
    }
}

impl Display for OpendalFileStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OpendalFileStorage({})", self.op.info().name())
    }
}

impl ObjectStore for OpendalFileStorage {
    fn put_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        location: &'life1 Path,
        payload: PutPayload,
        _opts: PutOptions,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<PutResult, ObjectStoreError>>
                + std::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let path = location.to_string();
        let op = self.op.clone();
        Box::pin(async move {
            // Collect all Bytes chunks from PutPayload into a single contiguous buffer
            let total_len = payload.content_length();
            let mut buf = Vec::with_capacity(total_len);
            for chunk in payload.iter() {
                buf.extend_from_slice(&chunk);
            }
            let buffer = opendal::Buffer::from(buf);
            op.write(&path, buffer)
                .await
                .map_err(opendal_to_object_store_error)?;
            let e_tag = op
                .stat(&path)
                .await
                .ok()
                .and_then(|m| m.etag().map(String::from));
            Ok(PutResult {
                e_tag,
                version: None,
            })
        })
    }

    fn put_multipart_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _location: &'life1 Path,
        _opts: PutMultipartOptions,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Box<dyn MultipartUpload>, ObjectStoreError>>
                + std::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async {
            Err(ObjectStoreError::NotSupported {
                source: "multipart upload is not supported".into(),
            })
        })
    }

    fn get_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        location: &'life1 Path,
        options: GetOptions,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<GetResult, ObjectStoreError>>
                + std::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let path = location.to_string();
        let op = self.op.clone();
        Box::pin(async move {
            let meta = op
                .stat(&path)
                .await
                .map_err(opendal_to_object_store_error)?;
            let object_meta = opendal_meta_to_object_meta(&path, &meta);
            let size = meta.content_length();

            if options.head {
                return Ok(GetResult {
                    payload: GetResultPayload::Stream(futures::stream::empty().boxed()),
                    meta: object_meta,
                    range: 0..size,
                    attributes: Attributes::default(),
                });
            }

            let range = match options.range {
                Some(r) => r,
                None => GetRange::Bounded(0..size),
            };

            let reader = op
                .reader(&path)
                .await
                .map_err(opendal_to_object_store_error)?;

            let byte_range = match range {
                GetRange::Bounded(r) => r,
                GetRange::Offset(start) => start..size,
                GetRange::Suffix(suffix) => size.saturating_sub(suffix)..size,
            };

            let byte_stream = reader
                .into_bytes_stream(byte_range.clone())
                .await
                .map_err(opendal_to_object_store_error)?;
            let stream: BoxStream<'static, Result<Bytes, ObjectStoreError>> =
                Box::pin(byte_stream.map_err(|e| ObjectStoreError::Generic {
                    store: "opendal",
                    source: e.into(),
                }));

            Ok(GetResult {
                payload: GetResultPayload::Stream(stream),
                meta: object_meta,
                range: byte_range,
                attributes: Attributes::default(),
            })
        })
    }

    fn delete<'life0, 'life1, 'async_trait>(
        &'life0 self,
        location: &'life1 Path,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), ObjectStoreError>>
                + std::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let path = location.to_string();
        let op = self.op.clone();
        Box::pin(async move {
            op.delete(&path)
                .await
                .map_err(opendal_to_object_store_error)
        })
    }

    fn list(
        &self,
        prefix: Option<&Path>,
    ) -> BoxStream<'static, Result<ObjectMeta, ObjectStoreError>> {
        let scan_path = prefix.map(|p| p.to_string()).unwrap_or_default();
        let op = self.op.clone();

        let stream = async_stream::stream! {
            let lister = match op.lister_with(&scan_path).recursive(true).await {
                Ok(l) => l,
                Err(e) => {
                    yield Err(opendal_to_object_store_error(e));
                    return;
                }
            };
            let mut entries = lister;
            while let Some(entry) = entries.next().await {
                match entry {
                    Ok(e) => {
                        if e.metadata().is_file() {
                            if let Some(meta) = entry_to_meta(&e) {
                                yield Ok(meta);
                            }
                        }
                    }
                    Err(e) => {
                        yield Err(opendal_to_object_store_error(e));
                    }
                }
            }
        };
        Box::pin(stream.boxed())
    }

    fn list_with_delimiter<'life0, 'life1, 'async_trait>(
        &'life0 self,
        prefix: Option<&'life1 Path>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<ListResult, ObjectStoreError>>
                + std::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let scan_path = prefix.map(|p| p.to_string()).unwrap_or_default();
        let op = self.op.clone();
        Box::pin(async move {
            let entries = op
                .list(&scan_path)
                .await
                .map_err(opendal_to_object_store_error)?;

            let mut objects = Vec::new();
            let mut common_prefixes = Vec::new();

            for entry in &entries {
                if entry.metadata().is_file() {
                    if let Some(meta) = entry_to_meta(entry) {
                        objects.push(meta);
                    }
                } else if entry.metadata().is_dir() {
                    if let Ok(p) = Path::parse(entry.path()) {
                        common_prefixes.push(p);
                    }
                }
            }

            Ok(ListResult {
                common_prefixes,
                objects,
            })
        })
    }

    fn copy<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        from: &'life1 Path,
        to: &'life2 Path,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), ObjectStoreError>>
                + std::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        let from_path = from.to_string();
        let to_path = to.to_string();
        let op = self.op.clone();
        Box::pin(async move {
            op.copy(&from_path, &to_path)
                .await
                .map_err(opendal_to_object_store_error)?;
            Ok(())
        })
    }

    fn copy_if_not_exists<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        from: &'life1 Path,
        to: &'life2 Path,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), ObjectStoreError>>
                + std::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        let from_path = from.to_string();
        let to_path = to.to_string();
        let op = self.op.clone();
        Box::pin(async move {
            op.copy_with(&from_path, &to_path)
                .if_not_exists(true)
                .await
                .map_err(opendal_to_object_store_error)?;
            Ok(())
        })
    }

    fn rename<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        from: &'life1 Path,
        to: &'life2 Path,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), ObjectStoreError>> + Send + 'async_trait>,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        let from_path = from.to_string();
        let to_path = to.to_string();
        let op = self.op.clone();
        Box::pin(async move {
            op.rename(&from_path, &to_path)
                .await
                .map_err(opendal_to_object_store_error)
        })
    }
}

/// Convert an opendal error into an object_store error.
fn opendal_to_object_store_error(err: opendal::Error) -> ObjectStoreError {
    let msg = err.message().to_string();
    match err.kind() {
        opendal::ErrorKind::NotFound => ObjectStoreError::NotFound {
            path: msg.into(),
            source: err.into(),
        },
        opendal::ErrorKind::AlreadyExists => ObjectStoreError::AlreadyExists {
            path: msg.into(),
            source: err.into(),
        },
        opendal::ErrorKind::PermissionDenied => {
            ObjectStoreError::NotSupported { source: err.into() }
        }
        _ => ObjectStoreError::Generic {
            store: "opendal",
            source: err.into(),
        },
    }
}

fn opendal_meta_to_object_meta(path: &str, meta: &opendal::Metadata) -> ObjectMeta {
    let last_modified = meta
        .last_modified()
        .map(|ts| {
            let sys_time: SystemTime = ts.into();
            Utc.timestamp_opt(
                sys_time
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0) as i64,
                0,
            )
            .single()
            .unwrap_or_else(Utc::now)
        })
        .unwrap_or_else(Utc::now);
    ObjectMeta {
        location: Path::parse(path).unwrap_or_else(|_| Path::from(path)),
        size: meta.content_length(),
        last_modified,
        e_tag: meta.etag().map(String::from),
        version: None,
    }
}

fn entry_to_meta(entry: &opendal::Entry) -> Option<ObjectMeta> {
    let meta = entry.metadata();
    if !meta.is_file() {
        return None;
    }
    opendal_meta_to_object_meta(entry.path(), meta).into()
}
