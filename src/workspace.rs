//! The data folder: the SQLite database plus the image and report files that
//! belong to each patient. All file handling behind the UI lives here so it
//! can be tested without a window.

use crate::image::{decode, Image};
use crate::patients::{Patient, PatientStore};
use crate::reports::{encode_bmp, Clinic, Draft, ReportDoc};
use crate::studies::{folder_name, Report, Study, INTRAORAL};
use crate::users::UserStore;
use std::path::{Path, PathBuf};

pub const ALL_DATES: (&str, &str) = ("", "9999-12-31 23:59:59");
pub const IMPORT_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "bmp", "dcm"];

pub struct Workspace {
    pub store: PatientStore,
    /// The reference's separate user database: products, sign-in users, cloud accounts.
    pub users: UserStore,
    pub data: PathBuf,
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

/// Removes a folder, treating one that is already gone as removed.
fn remove_tree(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

impl Workspace {
    pub fn open(data: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&data).map_err(io)?;
        let store = PatientStore::open(&data.join("patients.sqlite3"))?;
        let users = UserStore::open(&data.join("users.sqlite3"))?;
        Ok(Self { store, users, data })
    }

    pub fn image_dir(&self, patient_id: &str) -> PathBuf {
        self.data.join("images").join(folder_name(patient_id))
    }

    pub fn reports_dir(&self, patient_id: &str) -> PathBuf {
        self.data.join("reports").join(folder_name(patient_id))
    }

    fn clinic_path(&self) -> PathBuf {
        self.data.join("clinic.txt")
    }

    pub fn load_clinic(&self) -> Clinic {
        Clinic::load(&self.clinic_path())
    }

    pub fn save_clinic(&self, clinic: &Clinic) -> Result<(), String> {
        std::fs::write(self.clinic_path(), clinic.to_text()).map_err(io)
    }

    /// Deletes the patient row (Study and Report rows cascade) and the patient's files.
    pub fn delete_patient(&self, patient: &Patient) -> Result<(), String> {
        self.store.remove(patient.pk)?;
        for folder in [self.image_dir(&patient.id), self.reports_dir(&patient.id)] {
            remove_tree(&folder).map_err(|e| format!("Fail to delete the patient's files: {e}"))?;
        }
        Ok(())
    }

    // ---- studies --------------------------------------------------------

    pub fn studies(&self, patient: &Patient) -> Result<Vec<Study>, String> {
        self.store.studies(&patient.id, INTRAORAL, ALL_DATES.0, ALL_DATES.1)
    }

    /// Copies an image into the patient's folder and records it as a Study.
    /// The reference application names images with the pattern `%s/%s_%08X`.
    pub fn import_image(&self, patient: &Patient, source: &Path) -> Result<Study, String> {
        let kind = source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|e| IMPORT_EXTENSIONS.contains(&e.as_str()))
            .ok_or("Only .jpg, .jpeg, .png, .bmp and .dcm images can be imported.")?;
        let bytes = std::fs::read(source).map_err(io)?;
        decode(&bytes)?;
        let tags = crate::dicom::is_dicom(&bytes).then(|| crate::dicom::parse(&bytes).map(|d| d.tags)).transpose()?.unwrap_or_default();
        let dir = self.image_dir(&patient.id);
        std::fs::create_dir_all(&dir).map_err(io)?;
        let seed = nanos() as u32;
        let name = (0..u32::MAX)
            .map(|k| format!("{}_{:08X}.{kind}", folder_name(&patient.id), seed.wrapping_add(k)))
            .find(|name| !dir.join(name).exists())
            .ok_or("No free image file name")?;
        let path = dir.join(&name);
        std::fs::write(&path, &bytes).map_err(io)?;
        let mut study = Study {
            patient_id: patient.id.clone(),
            sop_instance_uid: if tags.sop_instance_uid.is_empty() { crate::dicom::new_uid() } else { tags.sop_instance_uid },
            modality: if tags.modality.is_empty() { INTRAORAL.into() } else { tags.modality },
            accession_number: tags.accession_number,
            body_part_examined: tags.body_part,
            description: tags.description,
            image_name: name,
            image_type: kind,
            datetime: if tags.datetime.is_empty() { crate::dates::local_timestamp() } else { tags.datetime },
            ..Default::default()
        };
        match self.store.add_study(&study) {
            Ok(pk) => {
                study.pk = pk;
                Ok(study)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        }
    }

    pub fn load_image(&self, study: &Study) -> Result<Image, String> {
        self.load_study_image(study).map(|(image, _)| image)
    }

    /// The image and, for DICOM files that state it, millimetres per pixel.
    pub fn load_study_image(&self, study: &Study) -> Result<(Image, Option<f64>), String> {
        let path = self.image_dir(&study.patient_id).join(&study.image_name);
        let bytes = std::fs::read(&path).map_err(|e| format!("Cannot open {}: {e}", study.image_name))?;
        if crate::dicom::is_dicom(&bytes) {
            let d = crate::dicom::parse(&bytes)?;
            return Ok((d.image, d.tags.spacing_mm()));
        }
        decode(&bytes).map(|image| (image, None))
    }

    pub fn remove_study(&self, study: &Study) -> Result<(), String> {
        self.store.remove_study(study.pk)?;
        let path = self.image_dir(&study.patient_id).join(&study.image_name);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("Fail to delete the image file {}: {e}", path.display()))
            }
            _ => Ok(()),
        }
    }

    // ---- reports --------------------------------------------------------

    pub fn reports(&self, patient: &Patient) -> Result<Vec<Report>, String> {
        self.store.reports(&patient.id, ALL_DATES.0, ALL_DATES.1)
    }

    /// Writes the draft's project folder and records it. A saved draft keeps
    /// its folder and only refreshes `updated_datetime`.
    pub fn save_report(&self, draft: &mut Draft) -> Result<i64, String> {
        if draft.is_empty() {
            return Err("Unable to save an empty report.".into());
        }
        let patient = draft.patient.clone().ok_or("Please select a patient.")?;
        let root = self.reports_dir(&patient.id);
        let project = draft.project.clone().unwrap_or_else(|| {
            let stamp: String = crate::dates::local_timestamp().chars().filter(char::is_ascii_digit).collect();
            let base = format!("{}_{stamp}", folder_name(&patient.id));
            (0..)
                .map(|k| if k == 0 { base.clone() } else { format!("{base}_{k}") })
                .find(|name| !root.join(name).exists())
                .unwrap()
        });
        let folder = root.join(&project);
        let failed = |_| "Save report error: create project failed".to_string();
        std::fs::create_dir_all(&folder).map_err(failed)?;
        // Rewrite the images so ones removed from the draft do not linger.
        for entry in std::fs::read_dir(&folder).map_err(failed)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("img_") && name.ends_with(".bmp") {
                std::fs::remove_file(entry.path()).map_err(failed)?;
            }
        }
        draft.doc.images.clear();
        for (i, image) in draft.images.iter().enumerate() {
            let name = format!("img_{:02}.bmp", i + 1);
            std::fs::write(folder.join(&name), encode_bmp(image)).map_err(failed)?;
            draft.doc.images.push(name);
        }
        std::fs::write(folder.join("report.txt"), draft.doc.to_text()).map_err(failed)?;
        let pk = match draft.pk {
            Some(pk) => {
                self.store
                    .touch_report(pk)
                    .map_err(|_| "Update report error: write to database failed".to_string())?;
                pk
            }
            None => self
                .store
                .add_report(&patient.id, &project)
                .map_err(|_| "Save report error: write to database failed".to_string())?,
        };
        if draft.created.is_empty() {
            draft.created = crate::dates::local_timestamp();
        }
        draft.pk = Some(pk);
        draft.project = Some(project);
        draft.dirty = false;
        Ok(pk)
    }

    pub fn load_report(&self, patient: &Patient, row: &Report) -> Result<Draft, String> {
        let folder = self.reports_dir(&row.patient_id).join(&row.project_name);
        let failed = |e: String| format!("Load Report failed: {e}");
        let text = std::fs::read_to_string(folder.join("report.txt")).map_err(|e| failed(e.to_string()))?;
        let doc = ReportDoc::from_text(&text).map_err(failed)?;
        let images = doc
            .images
            .iter()
            .map(|name| std::fs::read(folder.join(name)).map_err(|e| format!("{name}: {e}")).and_then(|b| decode(&b)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?;
        Ok(Draft {
            patient: Some(patient.clone()),
            pk: Some(row.pk),
            project: Some(row.project_name.clone()),
            created: row.created_datetime.clone(),
            doc,
            images,
            dirty: false,
        })
    }

    pub fn delete_report(&self, row: &Report) -> Result<(), String> {
        self.store
            .remove_report(row.pk)
            .map_err(|_| "Delete report error: delete from database failed".to_string())?;
        remove_tree(&self.reports_dir(&row.patient_id).join(&row.project_name))
            .map_err(|e| format!("Delete report error, remove report file failed: {e}"))
    }
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::patients::PatientDetails;

    /// A workspace in a fresh temporary folder, removed on drop.
    pub(crate) struct Temp(pub Workspace);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0.data);
        }
    }
    pub(crate) fn temp_workspace(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!("lumodent-{tag}-{}-{}", std::process::id(), nanos()));
        Temp(Workspace::open(dir).unwrap())
    }

    fn patient(ws: &Workspace, id: &str) -> Patient {
        let details = PatientDetails { last_name: "Wijaya".into(), first_name: "Sari".into(), ..Default::default() };
        let pk = ws.store.add(id, &details).unwrap();
        Patient { pk, id: id.into(), details }
    }

    fn fixture(ws: &Workspace, name: &str, bytes: &[u8]) -> PathBuf {
        let path = ws.data.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn import_open_and_remove_images() {
        let t = temp_workspace("images");
        let ws = &t.0;
        let p = patient(ws, "P/01");
        let png = fixture(ws, "a.png", include_bytes!("../tests/fixtures/rgb-3x2.png"));
        let jpg = fixture(ws, "b.JPG", include_bytes!("../tests/fixtures/halves-32x16.jpg"));
        let bad = fixture(ws, "c.png", b"not a png");
        let txt = fixture(ws, "d.txt", b"");

        let first = ws.import_image(&p, &png).unwrap();
        let second = ws.import_image(&p, &jpg).unwrap();
        assert!(first.image_name.starts_with("P_01_") && first.image_name.ends_with(".png"));
        assert_eq!(second.image_type, "jpg");
        assert!(ws.import_image(&p, &bad).is_err());
        assert!(ws.import_image(&p, &txt).is_err());
        let pks: Vec<i64> = ws.studies(&p).unwrap().iter().map(|s| s.pk).collect();
        assert_eq!(pks, [first.pk, second.pk]);
        assert_eq!(std::fs::read_dir(ws.image_dir(&p.id)).unwrap().count(), 2);
        assert_eq!(ws.load_image(&first).unwrap().width, 3);

        ws.remove_study(&first).unwrap();
        assert_eq!(ws.studies(&p).unwrap().len(), 1);
        assert_eq!(std::fs::read_dir(ws.image_dir(&p.id)).unwrap().count(), 1);
        assert!(ws.load_image(&first).is_err());
    }

    #[test]
    fn report_save_resave_reload_delete() {
        let t = temp_workspace("reports");
        let ws = &t.0;
        let p = patient(ws, "P-02");
        let image = Image { width: 2, height: 1, pixels: vec![0x112233, 0xffffff] };

        let mut draft = Draft::for_patient(&p);
        assert_eq!(ws.save_report(&mut draft).unwrap_err(), "Unable to save an empty report.");
        draft.images = vec![image.clone(), image.clone()];
        draft.doc.note = "Recall in 6 months".into();
        draft.doc.landscape = true;
        let pk = ws.save_report(&mut draft).unwrap();
        let folder = ws.reports_dir(&p.id).join(draft.project.as_ref().unwrap());
        assert!(folder.join("img_02.bmp").exists());

        draft.images.pop();
        assert_eq!(ws.save_report(&mut draft).unwrap(), pk);
        assert!(!folder.join("img_02.bmp").exists());
        let rows = ws.reports(&p).unwrap();
        assert_eq!(rows.len(), 1);

        let loaded = ws.load_report(&p, &rows[0]).unwrap();
        assert_eq!((loaded.doc.note.as_str(), loaded.doc.landscape), ("Recall in 6 months", true));
        assert_eq!(loaded.images, vec![image]);
        assert_eq!(loaded.pk, Some(pk));

        let clinic = Clinic { name: "Klinik".into(), ..Default::default() };
        ws.save_clinic(&clinic).unwrap();
        assert_eq!(ws.load_clinic(), clinic);

        ws.delete_report(&rows[0]).unwrap();
        assert!(!folder.exists());
        assert!(ws.reports(&p).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_patient_removes_rows_and_folders() {
        let t = temp_workspace("delete");
        let ws = &t.0;
        let p = patient(ws, "P-03");
        let png = fixture(ws, "a.png", include_bytes!("../tests/fixtures/rgb-3x2.png"));
        ws.import_image(&p, &png).unwrap();
        let mut draft = Draft::for_patient(&p);
        draft.doc.note = "x".into();
        ws.save_report(&mut draft).unwrap();
        ws.delete_patient(&p).unwrap();
        assert!(!ws.image_dir(&p.id).exists() && !ws.reports_dir(&p.id).exists());
        assert!(ws.studies(&p).unwrap().is_empty() && ws.reports(&p).unwrap().is_empty());
    }
}
