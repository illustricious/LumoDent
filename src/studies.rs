//! Study and Report rows, following the DataAccessor SQL in CoreLibrary.dll.

use crate::patients::{PatientStore, Statement};

/// DICOM modality for intraoral radiographs. The literal appears in the reference executable.
pub const INTRAORAL: &str = "IO";
/// Pass as the modality filter to list studies of every modality.
pub const ANY_MODALITY: &str = "";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Study {
    pub pk: i64,
    pub patient_id: String,
    pub accession_number: String,
    pub sop_instance_uid: String,
    pub patient_age: String,
    pub modality: String,
    pub body_part_examined: String,
    pub image_name: String,
    pub image_type: String,
    pub description: String,
    /// Acquisition time supplied by the caller (the `:datetime` parameter).
    pub datetime: String,
    pub created_datetime: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub pk: i64,
    pub patient_id: String,
    pub project_name: String,
    pub created_datetime: String,
}

impl PatientStore {
    pub fn add_study(&self, study: &Study) -> Result<i64, String> {
        let mut stmt = self.prepare(
            "INSERT INTO Study VALUES (NULL,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
        )?;
        for (i, value) in [
            &study.patient_id,
            &study.accession_number,
            &study.sop_instance_uid,
            &study.patient_age,
            &study.modality,
            &study.body_part_examined,
            &study.image_name,
            &study.image_type,
            &study.description,
            &study.datetime,
        ]
        .into_iter()
        .enumerate()
        {
            stmt.bind_text(i as i32 + 1, value)?;
        }
        stmt.bind_text(11, &self.local_timestamp()?)?;
        stmt.finish()?;
        self.last_rowid()
    }

    pub fn update_study(
        &self,
        pk: i64,
        image_name: &str,
        image_type: &str,
        description: &str,
    ) -> Result<(), String> {
        let mut stmt = self.prepare(
            "UPDATE Study SET image_name=?2,image_type=?3,description=?4,updated_datetime=?5 WHERE pk=?1",
        )?;
        stmt.bind_int(1, pk)?;
        stmt.bind_text(2, image_name)?;
        stmt.bind_text(3, image_type)?;
        stmt.bind_text(4, description)?;
        stmt.bind_text(5, &self.local_timestamp()?)?;
        stmt.finish()?;
        self.require_change("Study")
    }

    pub fn remove_study(&self, pk: i64) -> Result<(), String> {
        self.delete_row("DELETE FROM Study WHERE pk=?1", pk, "Study")
    }

    /// The original query has no ORDER BY; PK order keeps acquisition order stable.
    pub fn studies(
        &self,
        patient_id: &str,
        modality: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<Study>, String> {
        let mut stmt = self.prepare(
            "SELECT pk,patient_id,accession_number,sop_instance_uid,patient_age,modality,
            body_part_examined,image_name,image_type,description,datetime,created_datetime
            FROM Study WHERE (?1='' OR modality=?1) AND patient_id=?2
            AND created_datetime>=?3 AND created_datetime<=?4 ORDER BY pk",
        )?;
        stmt.bind_text(1, modality)?;
        stmt.bind_text(2, patient_id)?;
        stmt.bind_text(3, start)?;
        stmt.bind_text(4, end)?;
        let mut rows = Vec::new();
        while stmt.row()? {
            rows.push(Study {
                pk: stmt.int(0),
                patient_id: stmt.text(1),
                accession_number: stmt.text(2),
                sop_instance_uid: stmt.text(3),
                patient_age: stmt.text(4),
                modality: stmt.text(5),
                body_part_examined: stmt.text(6),
                image_name: stmt.text(7),
                image_type: stmt.text(8),
                description: stmt.text(9),
                datetime: stmt.text(10),
                created_datetime: stmt.text(11),
            });
        }
        Ok(rows)
    }

    pub fn add_report(&self, patient_id: &str, project_name: &str) -> Result<i64, String> {
        let mut stmt = self.prepare("INSERT INTO Report VALUES (NULL,?1,?2,?3,?3)")?;
        stmt.bind_text(1, patient_id)?;
        stmt.bind_text(2, project_name)?;
        stmt.bind_text(3, &self.local_timestamp()?)?;
        stmt.finish()?;
        self.last_rowid()
    }

    /// Matches `UPDATE Report SET updated_datetime=...`, the only Report update in the DLL.
    pub fn touch_report(&self, pk: i64) -> Result<(), String> {
        let mut stmt = self.prepare("UPDATE Report SET updated_datetime=?2 WHERE pk=?1")?;
        stmt.bind_int(1, pk)?;
        stmt.bind_text(2, &self.local_timestamp()?)?;
        stmt.finish()?;
        self.require_change("Report")
    }

    pub fn remove_report(&self, pk: i64) -> Result<(), String> {
        self.delete_row("DELETE FROM Report WHERE pk=?1", pk, "Report")
    }

    pub fn reports(&self, patient_id: &str, start: &str, end: &str) -> Result<Vec<Report>, String> {
        let mut stmt = self.prepare(
            "SELECT pk,patient_id,project_name,created_datetime FROM Report
            WHERE patient_id=?1 AND created_datetime>=?2 AND created_datetime<=?3 ORDER BY pk",
        )?;
        stmt.bind_text(1, patient_id)?;
        stmt.bind_text(2, start)?;
        stmt.bind_text(3, end)?;
        let mut rows = Vec::new();
        while stmt.row()? {
            rows.push(Report {
                pk: stmt.int(0),
                patient_id: stmt.text(1),
                project_name: stmt.text(2),
                created_datetime: stmt.text(3),
            });
        }
        Ok(rows)
    }

    fn delete_row(&self, sql: &str, pk: i64, what: &str) -> Result<(), String> {
        let mut stmt = self.prepare(sql)?;
        stmt.bind_int(1, pk)?;
        stmt.finish()?;
        self.require_change(what)
    }

    fn last_rowid(&self) -> Result<i64, String> {
        let mut stmt: Statement<'_> = self.prepare("SELECT last_insert_rowid()")?;
        stmt.row()?;
        Ok(stmt.int(0))
    }
}

/// Turns a chart number into a single Windows path component for the image folder.
pub fn folder_name(id: &str) -> String {
    let mut name: String = id
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while name.ends_with(['.', ' ']) {
        name.pop();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if name.is_empty() || reserved {
        name.insert(0, '_');
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patients::PatientDetails;

    const ALL: (&str, &str) = ("", "9999-12-31 23:59:59");

    fn study(patient: &str, name: &str) -> Study {
        Study {
            patient_id: patient.into(),
            modality: INTRAORAL.into(),
            image_name: name.into(),
            image_type: "bmp".into(),
            datetime: "2024-05-16 10:00:00".into(),
            ..Default::default()
        }
    }

    #[test]
    fn studies_filter_by_patient_and_modality_and_cascade_on_patient_delete() {
        let db = PatientStore::in_memory().unwrap();
        let a = db.add("A", &PatientDetails::default()).unwrap();
        db.add("B", &PatientDetails::default()).unwrap();
        assert!(db.add_study(&study("missing", "x.bmp")).is_err(), "FK must reject unknown patient");

        let first = db.add_study(&study("A", "A_00000001.bmp")).unwrap();
        db.add_study(&study("A", "A_00000002.bmp")).unwrap();
        db.add_study(&Study { modality: "PX".into(), ..study("A", "pano.bmp") }).unwrap();
        db.add_study(&study("B", "B_00000001.bmp")).unwrap();

        assert_eq!(db.studies("A", ANY_MODALITY, ALL.0, ALL.1).unwrap().len(), 3);
        let rows = db.studies("A", INTRAORAL, ALL.0, ALL.1).unwrap();
        let names: Vec<_> = rows.iter().map(|s| s.image_name.as_str()).collect();
        assert_eq!(names, ["A_00000001.bmp", "A_00000002.bmp"]);
        assert_eq!(rows[0].pk, first);
        assert_eq!(rows[0].accession_number, "");
        assert!(!rows[0].created_datetime.is_empty());
        assert!(db.studies("A", INTRAORAL, "1970-01-01", "1971-01-01").unwrap().is_empty());

        db.update_study(first, "renamed.bmp", "bmp", "upper molar").unwrap();
        let row = &db.studies("A", INTRAORAL, ALL.0, ALL.1).unwrap()[0];
        assert_eq!((row.image_name.as_str(), row.description.as_str()), ("renamed.bmp", "upper molar"));

        db.remove_study(first).unwrap();
        assert!(db.remove_study(first).is_err());
        assert_eq!(db.studies("A", INTRAORAL, ALL.0, ALL.1).unwrap().len(), 1);

        let report = db.add_report("A", "Full mouth").unwrap();
        db.touch_report(report).unwrap();
        assert_eq!(db.reports("A", ALL.0, ALL.1).unwrap()[0].project_name, "Full mouth");

        db.remove(a).unwrap();
        assert!(db.studies("A", "PX", ALL.0, ALL.1).unwrap().is_empty());
        assert!(db.reports("A", ALL.0, ALL.1).unwrap().is_empty());
        assert_eq!(db.studies("B", INTRAORAL, ALL.0, ALL.1).unwrap().len(), 1);

        let b_report = db.add_report("B", "").unwrap();
        db.remove_report(b_report).unwrap();
        assert!(db.touch_report(b_report).is_err());
    }

    #[test]
    fn folder_names_are_single_safe_components() {
        assert_eq!(folder_name("P-01"), "P-01");
        assert_eq!(folder_name("a/b\\c:d*?"), "a_b_c_d__");
        assert_eq!(folder_name("trail. "), "trail");
        assert_eq!(folder_name("con"), "_con");
        assert_eq!(folder_name("COM1.txt"), "_COM1.txt");
        assert_eq!(folder_name("COMX"), "COMX");
        assert_eq!(folder_name(".."), "_");
        assert_eq!(folder_name(""), "_");
    }
}
