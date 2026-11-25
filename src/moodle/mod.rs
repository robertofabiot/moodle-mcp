pub mod auth;
pub mod breaker;
pub mod classify;
pub mod client;
pub mod course_tree;
pub mod courses;
pub mod envelope;
pub mod error;
pub mod files;
pub mod gate;
pub mod ids;
pub mod metrics;
pub mod params;
pub mod raw;
pub mod submissions;

pub use auth::{LoginInfo, SiteInfo, SiteInfoFunction};
pub use breaker::{CircuitBreaker, CircuitState};
pub use classify::is_read_only;
pub use client::{MoodleClient, MoodleClientBuilder, DEFAULT_BASE_URL, DEFAULT_USER_AGENT};
pub use course_tree::{ContentEntry, Module, Section};
pub use courses::{ModuleInfo, MoodleAssignment, MoodleCourse};
pub use envelope::decode_envelope;
pub use error::{MoodleError, Result};
pub use files::{
    authorize_file_url, normalize_mime, resolve_mime, sanitize_filename, DownloadedFile,
    FileClient, UploadResult,
};
pub use gate::{GatePermit, MoodleGate, Priority};
pub use ids::{resolve_cmid, resolve_instance, resolve_module, CourseModuleRef};
pub use metrics::{RequestCounters, RequestCounts};
pub use params::flatten_params;
pub use submissions::{strip_html_to_plain_text, SubmissionStatus};

/// Comprueba el origen y el subdirectorio de la instalación de Moodle.
pub(crate) fn is_moodle_site_url(base: &url::Url, url: &url::Url) -> bool {
    let base_path = base.path().trim_end_matches('/');
    base.scheme() == url.scheme()
        && base.origin() == url.origin()
        && (base_path.is_empty()
            || url.path() == base_path
            || url
                .path()
                .strip_prefix(base_path)
                .is_some_and(|suffix| suffix.starts_with('/')))
}
