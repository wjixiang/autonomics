use crate::errors::AnthropicError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileObject {
    pub id: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub filename: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub purpose: FilePurpose,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub status: FileStatus,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FilePurpose {
    BatchInput,
    BatchOutput,
    Vision,
    Document,
    Upload,
}

impl FilePurpose {
    pub fn all() -> Vec<FilePurpose> {
        vec![
            FilePurpose::BatchInput,
            FilePurpose::BatchOutput,
            FilePurpose::Vision,
            FilePurpose::Document,
            FilePurpose::Upload,
        ]
    }
    
    pub fn supports_mime_type(&self, mime_type: &str) -> bool {
        match self {
            FilePurpose::BatchInput => {
                mime_type == "application/json" || mime_type == "text/plain"
            }
            FilePurpose::BatchOutput => {
                mime_type == "application/json" || mime_type == "text/plain"
            }
            FilePurpose::Vision => {
                mime_type.starts_with("image/")
            }
            FilePurpose::Document => {
                mime_type == "application/pdf" 
                || mime_type == "text/plain" 
                || mime_type == "application/msword"
                || mime_type == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            FilePurpose::Upload => true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Processing,
    Processed,
    Error,
    Deleted,
}

impl FileStatus {
    pub fn is_ready(&self) -> bool {
        *self == FileStatus::Processed
    }
    
    pub fn has_error(&self) -> bool {
        *self == FileStatus::Error
    }
    
    pub fn is_deleted(&self) -> bool {
        *self == FileStatus::Deleted
    }
}

#[derive(Debug, Clone)]
pub struct FileUploadParams {
    pub content: Vec<u8>,
    pub filename: String,
    pub content_type: String,
    pub purpose: FilePurpose,
    pub metadata: HashMap<String, String>,
}

impl FileUploadParams {
    pub fn new(
        content: Vec<u8>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        purpose: FilePurpose,
    ) -> Self {
        Self {
            content,
            filename: filename.into(),
            content_type: content_type.into(),
            purpose,
            metadata: HashMap::new(),
        }
    }
    
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }
    
    pub fn with_meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
    
    pub fn validate(&self) -> Result<(), AnthropicError> {
        const MAX_SIZE: u64 = 100 * 1024 * 1024;
        if self.content.len() as u64 > MAX_SIZE {
            return Err(AnthropicError::Other(format!(
                "File size {} bytes exceeds maximum of {} bytes",
                self.content.len(),
                MAX_SIZE
            )));
        }
        
        if self.filename.is_empty() {
            return Err(AnthropicError::Other(
                "Filename cannot be empty".to_string()
            ));
        }
        
        if !self.purpose.supports_mime_type(&self.content_type) {
            return Err(AnthropicError::Other(format!(
                "MIME type '{}' not supported for purpose '{:?}'",
                self.content_type, self.purpose
            )));
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<FilePurpose>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<FileOrder>,
}

impl FileListParams {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn purpose(mut self, purpose: FilePurpose) -> Self {
        self.purpose = Some(purpose);
        self
    }
    
    pub fn after(mut self, after: impl Into<String>) -> Self {
        self.after = Some(after.into());
        self
    }
    
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit.clamp(1, 100));
        self
    }
    
    pub fn order(mut self, order: FileOrder) -> Self {
        self.order = Some(order);
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileOrder {
    NewestFirst,
    OldestFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileList {
    pub data: Vec<FileObject>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadProgress {
    pub bytes_uploaded: u64,
    pub total_bytes: u64,
    pub percentage: f64,
    pub speed_bps: Option<f64>,
    pub eta_seconds: Option<f64>,
}

impl UploadProgress {
    pub fn new(bytes_uploaded: u64, total_bytes: u64) -> Self {
        let percentage = if total_bytes > 0 {
            (bytes_uploaded as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };
        
        Self {
            bytes_uploaded,
            total_bytes,
            percentage,
            speed_bps: None,
            eta_seconds: None,
        }
    }
    
    pub fn with_speed(mut self, speed_bps: f64) -> Self {
        self.speed_bps = Some(speed_bps);
        
        if speed_bps > 0.0 {
            let remaining_bytes = self.total_bytes - self.bytes_uploaded;
            self.eta_seconds = Some(remaining_bytes as f64 / speed_bps);
        }
        
        self
    }
    
    pub fn is_complete(&self) -> bool {
        self.bytes_uploaded >= self.total_bytes
    }
    
    pub fn percentage_string(&self) -> String {
        format!("{:.1}%", self.percentage)
    }
    
    pub fn size_string(&self) -> String {
        format!("{} / {}", 
            format_bytes(self.bytes_uploaded),
            format_bytes(self.total_bytes)
        )
    }
    
    pub fn speed_string(&self) -> Option<String> {
        self.speed_bps.map(|speed| format!("{}/s", format_bytes(speed as u64)))
    }
    
    pub fn eta_string(&self) -> Option<String> {
        self.eta_seconds.map(|eta| {
            if eta < 60.0 {
                format!("{:.0}s", eta)
            } else if eta < 3600.0 {
                format!("{:.0}m {:.0}s", eta / 60.0, eta % 60.0)
            } else {
                format!("{:.0}h {:.0}m", eta / 3600.0, (eta % 3600.0) / 60.0)
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub quota_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub file_count: u32,
    pub usage_by_purpose: HashMap<String, u64>,
}

impl StorageInfo {
    pub fn usage_percentage(&self) -> f64 {
        if self.quota_bytes > 0 {
            (self.used_bytes as f64 / self.quota_bytes as f64) * 100.0
        } else {
            0.0
        }
    }
    
    pub fn is_nearly_full(&self) -> bool {
        self.usage_percentage() > 90.0
    }
    
    pub fn is_full(&self) -> bool {
        self.used_bytes >= self.quota_bytes
    }
    
    pub fn quota_string(&self) -> String {
        format_bytes(self.quota_bytes)
    }
    
    pub fn usage_string(&self) -> String {
        format!("{} / {} ({:.1}%)",
            format_bytes(self.used_bytes),
            format_bytes(self.quota_bytes),
            self.usage_percentage()
        )
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    
    if bytes == 0 {
        return "0 B".to_string();
    }
    
    let mut size = bytes as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", size as u64, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

#[derive(Debug, Clone)]
pub struct FileDownload {
    pub content: Vec<u8>,
    pub content_type: String,
    pub filename: String,
    pub size: u64,
}

impl FileDownload {
    pub fn as_string(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.content.clone())
    }
    
    pub fn as_json<T>(&self) -> Result<T, serde_json::Error> 
    where
        T: for<'de> Deserialize<'de>,
    {
        serde_json::from_slice(&self.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_purpose_mime_type_support() {
        assert!(FilePurpose::Vision.supports_mime_type("image/jpeg"));
        assert!(FilePurpose::Vision.supports_mime_type("image/png"));
        assert!(!FilePurpose::Vision.supports_mime_type("application/pdf"));
        
        assert!(FilePurpose::Document.supports_mime_type("application/pdf"));
        assert!(FilePurpose::Document.supports_mime_type("text/plain"));
        assert!(!FilePurpose::Document.supports_mime_type("image/jpeg"));
        
        assert!(FilePurpose::BatchInput.supports_mime_type("application/json"));
        assert!(FilePurpose::BatchInput.supports_mime_type("text/plain"));
        assert!(!FilePurpose::BatchInput.supports_mime_type("image/jpeg"));
    }

    #[test]
    fn test_upload_params_validation() {
        let params = FileUploadParams::new(
            b"test content".to_vec(),
            "test.txt",
            "text/plain",
            FilePurpose::Document,
        );
        assert!(params.validate().is_ok());
        
        let params = FileUploadParams::new(
            b"test content".to_vec(),
            "test.txt",
            "image/jpeg",
            FilePurpose::BatchInput,
        );
        assert!(params.validate().is_err());
        
        let params = FileUploadParams::new(
            b"test content".to_vec(),
            "",
            "text/plain",
            FilePurpose::Document,
        );
        assert!(params.validate().is_err());
    }

    #[test]
    fn test_upload_progress() {
        let progress = UploadProgress::new(512, 1024);
        assert_eq!(progress.percentage, 50.0);
        assert!(!progress.is_complete());
        
        let progress = UploadProgress::new(1024, 1024);
        assert_eq!(progress.percentage, 100.0);
        assert!(progress.is_complete());
        
        let progress = UploadProgress::new(512, 1024).with_speed(1024.0);
        assert!(progress.speed_bps.is_some());
        assert!(progress.eta_seconds.is_some());
    }

    #[test]
    fn test_storage_info() {
        let storage = StorageInfo {
            quota_bytes: 1000,
            used_bytes: 910,
            available_bytes: 90,
            file_count: 10,
            usage_by_purpose: HashMap::new(),
        };
        
        assert_eq!(storage.usage_percentage(), 91.0);
        assert!(storage.is_nearly_full());
        assert!(!storage.is_full());
        
        let storage = StorageInfo {
            quota_bytes: 1000,
            used_bytes: 1000,
            available_bytes: 0,
            file_count: 10,
            usage_by_purpose: HashMap::new(),
        };
        
        assert!(storage.is_full());
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_file_status() {
        assert!(FileStatus::Processed.is_ready());
        assert!(!FileStatus::Processing.is_ready());
        assert!(FileStatus::Error.has_error());
        assert!(FileStatus::Deleted.is_deleted());
    }
}
