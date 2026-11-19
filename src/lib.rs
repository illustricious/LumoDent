//! LumoDent core: patient database, studies, images and reports. Everything
//! here is platform-neutral; the window lives in the `lumodent` binary.

pub const APP_NAME: &str = "LumoDent";
pub const DEVELOPER: &str = "illustricious";

pub mod dates;
pub mod dicom;
pub mod hdr;
pub mod image;
pub mod patients;
pub mod pdf;
pub mod processing;
pub mod reports;
pub mod sensor;
pub mod sha256;
pub mod studies;
pub mod users;
pub mod workspace;
