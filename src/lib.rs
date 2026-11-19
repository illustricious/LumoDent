//! LumoDent core: patient database, studies, images and reports. Everything
//! here is platform-neutral; the window lives in the `lumodent` binary.

pub const APP_NAME: &str = "LumoDent";
pub const DEVELOPER: &str = "illustricious";

pub mod dates;
pub mod dicom;
pub mod image;
pub mod patients;
pub mod pdf;
pub mod reports;
pub mod studies;
pub mod workspace;
