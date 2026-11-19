use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;
use std::ptr;

// Windows ships SQLite as an OS component (winsqlite3, x64 and ARM64); raw-dylib
// generates its import library at build time. Linux and macOS link the system
// libsqlite3.
#[cfg_attr(windows, link(name = "winsqlite3", kind = "raw-dylib"))]
#[cfg_attr(not(windows), link(name = "sqlite3"))]
unsafe extern "C" {
    fn sqlite3_open(filename: *const c_char, db: *mut *mut c_void) -> c_int;
    fn sqlite3_close(db: *mut c_void) -> c_int;
    fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
    fn sqlite3_prepare_v2(
        db: *mut c_void,
        sql: *const c_char,
        len: c_int,
        stmt: *mut *mut c_void,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut c_void,
        index: c_int,
        value: *const c_char,
        len: c_int,
        destructor: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut c_void, index: c_int, value: i64) -> c_int;
    fn sqlite3_step(stmt: *mut c_void) -> c_int;
    fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
    fn sqlite3_column_int64(stmt: *mut c_void, col: c_int) -> i64;
    fn sqlite3_column_text(stmt: *mut c_void, col: c_int) -> *const u8;
    fn sqlite3_last_insert_rowid(db: *mut c_void) -> i64;
    fn sqlite3_changes(db: *mut c_void) -> c_int;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PatientDetails {
    pub last_name: String,
    pub first_name: String,
    pub sex: String,
    pub birthdate: String,
    pub doctor: String,
    pub social_id: String,
    pub phone: String,
    pub email: String,
    pub zipcode: String,
    pub address: String,
}

impl PatientDetails {
    /// First name first, as shown everywhere in the interface.
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name.trim(), self.last_name.trim()).trim().to_string()
    }

    fn values(&self) -> [&str; 10] {
        [
            &self.last_name,
            &self.first_name,
            &self.sex,
            &self.birthdate,
            &self.doctor,
            &self.social_id,
            &self.phone,
            &self.email,
            &self.zipcode,
            &self.address,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patient {
    pub pk: i64,
    pub id: String,
    pub details: PatientDetails,
}

// Column order follows the CREATE TABLE literals in CoreLibrary.dll.
const SCHEMA: [&str; 3] = ["CREATE TABLE IF NOT EXISTS Patient (
    pk INTEGER PRIMARY KEY, id VARCHAR(250) NOT NULL UNIQUE,
    last_name VARCHAR(250) NOT NULL, first_name VARCHAR(250), sex VARCHAR(250),
    birthdate DATE, doctor VARCHAR(250), social_id VARCHAR(250), phone VARCHAR(250),
    email VARCHAR(250), zipcode VARCHAR(250), address VARCHAR(250),
    created_datetime DATETIME, updated_datetime DATETIME)",
    "CREATE TABLE IF NOT EXISTS Study (pk INTEGER PRIMARY KEY,
    patient_id VARCHAR REFERENCES Patient(id) ON UPDATE CASCADE ON DELETE CASCADE,
    accession_number VARCHAR(250) DEFAULT 0, sop_instance_uid VARCHAR(250),
    patient_age VARCHAR(250), modality VARCHAR(250), body_part_examined VARCHAR(250),
    image_name VARCHAR(250), image_type VARCHAR(250), description VARCHAR(250),
    datetime DATETIME, created_datetime DATETIME, updated_datetime DATETIME)",
    "CREATE TABLE IF NOT EXISTS Report (pk INTEGER PRIMARY KEY,
    patient_id VARCHAR REFERENCES Patient(id) ON UPDATE CASCADE ON DELETE CASCADE,
    project_name VARCHAR(250), created_datetime DATETIME, updated_datetime DATETIME)",
];

pub struct PatientStore(*mut c_void);

const SELECT_PATIENTS: &str = "SELECT pk,id,last_name,first_name,sex,birthdate,doctor,social_id,
    phone,email,zipcode,address FROM Patient";

impl Drop for PatientStore {
    fn drop(&mut self) {
        unsafe {
            sqlite3_close(self.0);
        }
    }
}

impl PatientStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::connect(path.to_str().ok_or("Invalid UTF-8 database path")?)
    }

    pub fn in_memory() -> Result<Self, String> {
        Self::connect(":memory:")
    }

    fn connect(filename: &str) -> Result<Self, String> {
        let filename = CString::new(filename).map_err(|e| e.to_string())?;
        let mut db = ptr::null_mut();
        let status = unsafe { sqlite3_open(filename.as_ptr(), &mut db) };
        if db.is_null() {
            return Err("Could not allocate a SQLite connection".into());
        }
        let store = Self(db);
        store.check(status)?;
        store.prepare("PRAGMA foreign_keys=ON")?.finish()?;
        for statement in SCHEMA {
            store.prepare(statement)?.finish()?;
        }
        Ok(store)
    }

    fn check(&self, status: c_int) -> Result<(), String> {
        if status == 0 {
            return Ok(());
        }
        Err(unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }
            .to_string_lossy()
            .into_owned())
    }

    pub(crate) fn prepare(&self, sql: &str) -> Result<Statement<'_>, String> {
        let mut raw = ptr::null_mut();
        let sql = CString::new(sql).map_err(|e| e.to_string())?;
        self.check(unsafe {
            sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut raw, ptr::null_mut())
        })?;
        Ok(Statement {
            db: self,
            raw,
            parameters: Vec::new(),
        })
    }

    pub fn add(&self, id: &str, details: &PatientDetails) -> Result<i64, String> {
        let mut stmt = self.prepare("INSERT INTO Patient
            (id,last_name,first_name,sex,birthdate,doctor,social_id,phone,email,zipcode,address,created_datetime,updated_datetime)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)")?;
        stmt.bind_text(1, id)?;
        stmt.bind_details(2, details)?;
        stmt.bind_text(12, &self.local_timestamp()?)?;
        stmt.finish()?;
        Ok(unsafe { sqlite3_last_insert_rowid(self.0) })
    }

    pub fn update(&self, pk: i64, details: &PatientDetails) -> Result<(), String> {
        let mut stmt = self.prepare(
            "UPDATE Patient SET last_name=?2,first_name=?3,sex=?4,birthdate=?5,
            doctor=?6,social_id=?7,phone=?8,email=?9,zipcode=?10,address=?11,
            updated_datetime=?12 WHERE pk=?1",
        )?;
        stmt.bind_pk(pk)?;
        stmt.bind_details(2, details)?;
        stmt.bind_text(12, &self.local_timestamp()?)?;
        stmt.finish()?;
        self.require_change("Patient")
    }

    // The system SQLite on some hosts returns NULL for datetime('now','localtime'),
    // so local time comes from Rust instead.
    pub(crate) fn local_timestamp(&self) -> Result<String, String> {
        Ok(crate::dates::local_timestamp())
    }

    pub fn remove(&self, pk: i64) -> Result<(), String> {
        let mut stmt = self.prepare("DELETE FROM Patient WHERE pk=?1")?;
        stmt.bind_pk(pk)?;
        stmt.finish()?;
        self.require_change("Patient")
    }

    pub(crate) fn require_change(&self, what: &str) -> Result<(), String> {
        match unsafe { sqlite3_changes(self.0) } {
            0 => Err(format!("{what} no longer exists")),
            _ => Ok(()),
        }
    }

    pub fn list(&self, search: &str) -> Result<Vec<Patient>, String> {
        self.list_created_between(search, "", "9999-12-31 23:59:59")
    }

    /// Bounds use the local SQLite timestamp format YYYY-MM-DD HH:MM:SS.
    pub fn list_created_between(
        &self,
        search: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<Patient>, String> {
        let mut stmt = self.prepare(&format!(
            "{SELECT_PATIENTS} WHERE
            (id LIKE ?1 OR last_name LIKE ?1 OR first_name LIKE ?1 OR phone LIKE ?1)
            AND created_datetime>=?2 AND created_datetime<=?3 ORDER BY pk",
        ))?;
        stmt.bind_text(1, &format!("%{search}%"))?;
        stmt.bind_text(2, start)?;
        stmt.bind_text(3, end)?;
        stmt.patients()
    }

    pub fn recently_acquired(&self, start: &str, end: &str) -> Result<Vec<Patient>, String> {
        self.with_recent_activity("Study", start, end)
    }

    pub fn recently_printed(&self, start: &str, end: &str) -> Result<Vec<Patient>, String> {
        self.with_recent_activity("Report", start, end)
    }

    // Only the two fixed table names above can become SQL identifiers here.
    fn with_recent_activity(
        &self,
        table: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<Patient>, String> {
        let mut stmt = self.prepare(&format!("{SELECT_PATIENTS} WHERE id IN
            (SELECT DISTINCT patient_id FROM {table} WHERE created_datetime>=?1 AND created_datetime<=?2)
            ORDER BY pk"))?;
        stmt.bind_text(1, start)?;
        stmt.bind_text(2, end)?;
        stmt.patients()
    }
}

impl Statement<'_> {
    fn patients(mut self) -> Result<Vec<Patient>, String> {
        let mut result = Vec::new();
        while self.row()? {
            result.push(Patient {
                pk: unsafe { sqlite3_column_int64(self.raw, 0) },
                id: self.text(1),
                details: PatientDetails {
                    last_name: self.text(2),
                    first_name: self.text(3),
                    sex: self.text(4),
                    birthdate: self.text(5),
                    doctor: self.text(6),
                    social_id: self.text(7),
                    phone: self.text(8),
                    email: self.text(9),
                    zipcode: self.text(10),
                    address: self.text(11),
                },
            });
        }
        Ok(result)
    }
}

pub(crate) struct Statement<'a> {
    db: &'a PatientStore,
    raw: *mut c_void,
    // SQLITE_STATIC borrows the heap buffers until finalization. Moving a
    // CString as the vector grows leaves its heap allocation in place.
    parameters: Vec<CString>,
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        unsafe {
            sqlite3_finalize(self.raw);
        }
    }
}

impl Statement<'_> {
    pub(crate) fn bind_text(&mut self, position: c_int, value: &str) -> Result<(), String> {
        let value = CString::new(value).map_err(|e| e.to_string())?;
        self.db
            .check(unsafe { sqlite3_bind_text(self.raw, position, value.as_ptr(), -1, None) })?;
        self.parameters.push(value);
        Ok(())
    }

    fn bind_details(&mut self, first: c_int, details: &PatientDetails) -> Result<(), String> {
        for (i, text) in details.values().into_iter().enumerate() {
            self.bind_text(first + i as c_int, text)?;
        }
        Ok(())
    }

    fn bind_pk(&mut self, pk: i64) -> Result<(), String> {
        self.bind_int(1, pk)
    }

    pub(crate) fn bind_int(&mut self, position: c_int, value: i64) -> Result<(), String> {
        self.db
            .check(unsafe { sqlite3_bind_int64(self.raw, position, value) })
    }

    pub(crate) fn int(&self, col: c_int) -> i64 {
        unsafe { sqlite3_column_int64(self.raw, col) }
    }

    pub(crate) fn row(&mut self) -> Result<bool, String> {
        match unsafe { sqlite3_step(self.raw) } {
            100 => Ok(true),
            101 => Ok(false),
            status => self.db.check(status).map(|_| false),
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), String> {
        if self.row()? {
            return Err("Unexpected result row".into());
        }
        Ok(())
    }

    pub(crate) fn text(&self, col: c_int) -> String {
        let raw = unsafe { sqlite3_column_text(self.raw, col) };
        if raw.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(raw.cast()) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_activity_uses_study_and_report_dates_independently() {
        let db = PatientStore::in_memory().unwrap();
        let start = "2020-01-01 00:00:00";
        let end = "2020-01-31 23:59:59";
        for id in ["A", "B", "C"] {
            db.add(id, &PatientDetails::default()).unwrap();
        }
        assert!(db.recently_acquired(start, end).unwrap().is_empty());
        assert!(db.recently_printed(start, end).unwrap().is_empty());
        for statement in [
            "INSERT INTO Study (patient_id,created_datetime) VALUES ('A','2020-01-01 00:00:00'),('A','2020-01-02 12:00:00'),('B','2019-12-31 23:59:59')",
            "INSERT INTO Report (patient_id,created_datetime) VALUES ('B','2020-01-31 23:59:59'),('C','2020-02-01 00:00:00')",
        ] { db.prepare(statement).unwrap().finish().unwrap(); }
        let ids = |rows: Vec<Patient>| rows.into_iter().map(|row| row.id).collect::<Vec<_>>();
        assert_eq!(ids(db.recently_acquired(start, end).unwrap()), ["A"]);
        assert_eq!(ids(db.recently_printed(start, end).unwrap()), ["B"]);
    }

    #[test]
    fn add_search_edit_delete_and_reopen() {
        let path = std::env::temp_dir().join(format!(
            "lumodent-patients-test-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut details = PatientDetails {
            last_name: "O'Brien".into(),
            first_name: "Sari".into(),
            phone: "081234".into(),
            ..Default::default()
        };
        let pk = {
            let db = PatientStore::open(&path).unwrap();
            let pk = db.add("P-01", &details).unwrap();
            assert!(db.add("P-01", &details).is_err());
            for query in ["-01", "Bri", "ari", "812"] {
                assert_eq!(db.list(query).unwrap()[0].details, details);
            }
            assert!(db
                .list_created_between("", "1970-01-01", "1971-01-01")
                .unwrap()
                .is_empty());
            details.phone = "02199".into();
            db.update(pk, &details).unwrap();
            assert!(db.list("812").unwrap().is_empty());
            pk
        };
        let reopened = PatientStore::open(&path).unwrap();
        assert_eq!(
            reopened.list("").unwrap(),
            vec![Patient {
                pk,
                id: "P-01".into(),
                details
            }]
        );
        reopened.remove(pk).unwrap();
        assert!(reopened.remove(pk).is_err());
        assert!(reopened.list("").unwrap().is_empty());
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
}
