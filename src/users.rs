//! The second database of the reference (CreateUserDB / CheckUserDB in
//! CoreLibrary.dll): Product, User and CloudUser, kept apart from patient data.
//!
//! Table layouts and statements follow the literals in CoreLibrary.dll. One
//! deliberate difference: the reference stores `user_password` as plain text (its
//! first-run admin is "Admin" / "123456"). LumoDent writes a salted, iterated
//! SHA-256 instead and still accepts a plain-text password from an old database,
//! upgrading it on the first successful sign-in.

use crate::patients::PatientStore;
use crate::sha256;
use std::path::Path;

/// `PRAGMA user_version` that CreateUserDB writes.
pub const USER_DB_VERSION: i64 = 0;

/// The account NanoPix creates on first run.
pub const DEFAULT_ADMIN: (&str, &str) = ("Admin", "123456");

const SCHEMA: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS Product (pk INTEGER PRIMARY KEY,
    sensor_sn VARCHAR(250), product_sn VARCHAR(250), product_key VARCHAR(250),
    software_version VARCHAR(250), cpu_id VARCHAR(250), mac_address VARCHAR(250),
    disk_no VARCHAR(250), created_datetime DATETIME, updated_datetime DATETIME)",
    "CREATE TABLE IF NOT EXISTS User (pk INTEGER PRIMARY KEY,
    user_name VARCHAR(250), user_password VARCHAR(250), user_permission VARCHAR(3000),
    created_datetime DATETIME, updated_datetime DATETIME)",
    "CREATE TABLE IF NOT EXISTS CloudUser (pk INTEGER PRIMARY KEY,
    product_pk INTEGER REFERENCES Product(pk) ON UPDATE CASCADE ON DELETE CASCADE,
    uuid VARCHAR(250), secret VARCHAR(250), sessionid VARCHAR(250), expiresAt INTEGER,
    created_datetime DATETIME, updated_datetime DATETIME)",
];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Product {
    pub pk: i64,
    pub sensor_sn: String,
    pub product_sn: String,
    pub product_key: String,
    pub software_version: String,
    pub cpu_id: String,
    pub mac_address: String,
    pub disk_no: String,
}

impl Product {
    fn values(&self) -> [&str; 7] {
        [&self.sensor_sn, &self.product_sn, &self.product_key, &self.software_version, &self.cpu_id, &self.mac_address, &self.disk_no]
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct User {
    pub pk: i64,
    pub name: String,
    /// Stored form: `sha256$<salt>$<rounds>$<hash>`, or plain text from NanoPix.
    pub password: String,
    pub permission: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CloudUser {
    pub pk: i64,
    pub product_pk: i64,
    pub uuid: String,
    pub secret: String,
    pub session_id: String,
    pub expires_at: i64,
}

const ROUNDS: u32 = 20_000;

/// A salted, iterated SHA-256 of `password`.
pub fn hash_password(password: &str) -> String {
    hash_with(password, &new_salt(), ROUNDS)
}

fn hash_with(password: &str, salt: &str, rounds: u32) -> String {
    let mut state = sha256::digest(format!("{salt}\0{password}").as_bytes());
    for _ in 1..rounds {
        let mut next = state.to_vec();
        next.extend_from_slice(password.as_bytes());
        state = sha256::digest(&next);
    }
    format!("sha256${salt}${rounds}${}", sha256::hex(&state))
}

fn new_salt() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seed = format!(
        "{:?}{}{}{:p}",
        std::time::SystemTime::now(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        &COUNTER
    );
    sha256::hex(&sha256::digest(seed.as_bytes())[..16])
}

/// Whether `password` matches a stored value, hashed or legacy plain text.
pub fn password_matches(stored: &str, password: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    match parts.as_slice() {
        ["sha256", salt, rounds, _] => match rounds.parse() {
            Ok(rounds) => constant_time_eq(hash_with(password, salt, rounds).as_bytes(), stored.as_bytes()),
            Err(_) => false,
        },
        _ => constant_time_eq(stored.as_bytes(), password.as_bytes()),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub struct UserStore(PatientStore);

impl UserStore {
    /// CheckUserDB: opens the file, creating any missing table, and adds the
    /// default admin when there is no user at all.
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::connect(path.to_str().ok_or("Invalid UTF-8 database path")?)
    }

    pub fn in_memory() -> Result<Self, String> {
        Self::connect(":memory:")
    }

    fn connect(filename: &str) -> Result<Self, String> {
        let store = Self(PatientStore::connect(filename, &SCHEMA, USER_DB_VERSION)?);
        if store.users()?.is_empty() {
            store.add_user(DEFAULT_ADMIN.0, DEFAULT_ADMIN.1, "")?;
        }
        Ok(store)
    }

    fn now(&self) -> Result<String, String> {
        self.0.local_timestamp()
    }

    // Product

    /// AddProduct.
    pub fn add_product(&self, product: &Product) -> Result<i64, String> {
        let mut stmt = self.0.prepare("INSERT INTO Product VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)")?;
        for (i, v) in product.values().into_iter().enumerate() {
            stmt.bind_text(i as i32 + 1, v)?;
        }
        stmt.bind_text(8, &self.now()?)?;
        stmt.finish()?;
        Ok(self.0.last_insert())
    }

    /// UpdateProduct.
    pub fn update_product(&self, product: &Product) -> Result<(), String> {
        let mut stmt = self.0.prepare(
            "UPDATE Product SET sensor_sn=?1, product_sn=?2, product_key=?3, software_version=?4,
            cpu_id=?5, mac_address=?6, disk_no=?7, updated_datetime=?8 WHERE pk=?9",
        )?;
        for (i, v) in product.values().into_iter().enumerate() {
            stmt.bind_text(i as i32 + 1, v)?;
        }
        stmt.bind_text(8, &self.now()?)?;
        stmt.bind_int(9, product.pk)?;
        stmt.finish()?;
        self.0.require_change("Product")
    }

    /// DeleteProduct; its CloudUser rows go with it (ON DELETE CASCADE).
    pub fn delete_product(&self, pk: i64) -> Result<(), String> {
        let mut stmt = self.0.prepare("DELETE FROM Product WHERE pk=?1")?;
        stmt.bind_int(1, pk)?;
        stmt.finish()?;
        self.0.require_change("Product")
    }

    /// GetProducts.
    pub fn products(&self) -> Result<Vec<Product>, String> {
        let mut stmt = self.0.prepare(
            "SELECT pk,sensor_sn,product_sn,product_key,software_version,cpu_id,mac_address,disk_no FROM Product ORDER BY pk",
        )?;
        let mut rows = Vec::new();
        while stmt.row()? {
            rows.push(Product {
                pk: stmt.int(0),
                sensor_sn: stmt.text(1),
                product_sn: stmt.text(2),
                product_key: stmt.text(3),
                software_version: stmt.text(4),
                cpu_id: stmt.text(5),
                mac_address: stmt.text(6),
                disk_no: stmt.text(7),
            });
        }
        Ok(rows)
    }

    // User

    /// AddUser, hashing the password.
    pub fn add_user(&self, name: &str, password: &str, permission: &str) -> Result<i64, String> {
        let mut stmt = self.0.prepare("INSERT INTO User VALUES (NULL, ?1, ?2, ?3, ?4, ?4)")?;
        stmt.bind_text(1, name)?;
        stmt.bind_text(2, &hash_password(password))?;
        stmt.bind_text(3, permission)?;
        stmt.bind_text(4, &self.now()?)?;
        stmt.finish()?;
        Ok(self.0.last_insert())
    }

    /// UpdateUser: password and permission, like the reference (the name is fixed).
    pub fn update_user(&self, pk: i64, password: &str, permission: &str) -> Result<(), String> {
        self.write_user(pk, &hash_password(password), permission)
    }

    fn write_user(&self, pk: i64, stored_password: &str, permission: &str) -> Result<(), String> {
        let mut stmt = self.0.prepare("UPDATE User SET user_password=?1, user_permission=?2, updated_datetime=?3 WHERE pk=?4")?;
        stmt.bind_text(1, stored_password)?;
        stmt.bind_text(2, permission)?;
        stmt.bind_text(3, &self.now()?)?;
        stmt.bind_int(4, pk)?;
        stmt.finish()?;
        self.0.require_change("User")
    }

    /// DeleteUser.
    pub fn delete_user(&self, pk: i64) -> Result<(), String> {
        let mut stmt = self.0.prepare("DELETE FROM User WHERE pk=?1")?;
        stmt.bind_int(1, pk)?;
        stmt.finish()?;
        self.0.require_change("User")
    }

    /// GetUsers.
    pub fn users(&self) -> Result<Vec<User>, String> {
        let mut stmt = self.0.prepare("SELECT pk,user_name,user_password,user_permission FROM User ORDER BY pk")?;
        let mut rows = Vec::new();
        while stmt.row()? {
            rows.push(User { pk: stmt.int(0), name: stmt.text(1), password: stmt.text(2), permission: stmt.text(3) });
        }
        Ok(rows)
    }

    /// Sign-in check ("User id or password error" in the reference). A matching
    /// plain-text password from NanoPix is replaced by its hash.
    pub fn sign_in(&self, name: &str, password: &str) -> Result<Option<User>, String> {
        let Some(user) = self.users()?.into_iter().find(|u| u.name == name) else { return Ok(None) };
        if !password_matches(&user.password, password) {
            return Ok(None);
        }
        if !user.password.starts_with("sha256$") {
            let hashed = hash_password(password);
            self.write_user(user.pk, &hashed, &user.permission)?;
            return Ok(Some(User { password: hashed, ..user }));
        }
        Ok(Some(user))
    }

    // CloudUser

    /// AddCloudUser.
    pub fn add_cloud_user(&self, cloud: &CloudUser) -> Result<i64, String> {
        let mut stmt = self.0.prepare("INSERT INTO CloudUser VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?6)")?;
        stmt.bind_int(1, cloud.product_pk)?;
        stmt.bind_text(2, &cloud.uuid)?;
        stmt.bind_text(3, &cloud.secret)?;
        stmt.bind_text(4, &cloud.session_id)?;
        stmt.bind_int(5, cloud.expires_at)?;
        stmt.bind_text(6, &self.now()?)?;
        stmt.finish()?;
        Ok(self.0.last_insert())
    }

    /// UpdateCloudUser: keyed by product, as in the reference.
    pub fn update_cloud_user(&self, cloud: &CloudUser) -> Result<(), String> {
        let mut stmt = self.0.prepare(
            "UPDATE CloudUser SET uuid=?1, secret=?2, sessionid=?3, expiresAt=?4, updated_datetime=?5 WHERE product_pk=?6",
        )?;
        stmt.bind_text(1, &cloud.uuid)?;
        stmt.bind_text(2, &cloud.secret)?;
        stmt.bind_text(3, &cloud.session_id)?;
        stmt.bind_int(4, cloud.expires_at)?;
        stmt.bind_text(5, &self.now()?)?;
        stmt.bind_int(6, cloud.product_pk)?;
        stmt.finish()?;
        self.0.require_change("CloudUser")
    }

    /// DeleteCloudUser.
    pub fn delete_cloud_user(&self, product_pk: i64) -> Result<(), String> {
        let mut stmt = self.0.prepare("DELETE FROM CloudUser WHERE product_pk=?1")?;
        stmt.bind_int(1, product_pk)?;
        stmt.finish()?;
        self.0.require_change("CloudUser")
    }

    /// GetCloudUser: the account of one product, if any.
    pub fn cloud_user(&self, product_pk: i64) -> Result<Option<CloudUser>, String> {
        let mut stmt = self.0.prepare("SELECT pk,product_pk,uuid,secret,sessionid,expiresAt FROM CloudUser WHERE product_pk=?1 ORDER BY pk")?;
        stmt.bind_int(1, product_pk)?;
        if !stmt.row()? {
            return Ok(None);
        }
        Ok(Some(CloudUser {
            pk: stmt.int(0),
            product_pk: stmt.int(1),
            uuid: stmt.text(2),
            secret: stmt.text(3),
            session_id: stmt.text(4),
            expires_at: stmt.int(5),
        }))
    }

    pub fn user_version(&self) -> Result<i64, String> {
        self.0.user_version()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_hashed_salted_and_verified() {
        let a = hash_password("123456");
        let b = hash_password("123456");
        assert_ne!(a, b, "salt differs");
        assert!(a.starts_with("sha256$") && !a.contains("123456"));
        assert!(password_matches(&a, "123456") && !password_matches(&a, "1234567"));
        // Legacy NanoPix rows are plain text.
        assert!(password_matches("123456", "123456") && !password_matches("123456", "x"));
    }

    #[test]
    fn first_open_adds_the_default_admin() {
        let db = UserStore::in_memory().unwrap();
        let users = db.users().unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "Admin");
        assert!(db.sign_in("Admin", "123456").unwrap().is_some());
        assert!(db.sign_in("Admin", "wrong").unwrap().is_none());
        assert!(db.sign_in("nobody", "123456").unwrap().is_none());
        assert_eq!(db.user_version().unwrap(), USER_DB_VERSION);
    }

    #[test]
    fn legacy_plain_password_is_upgraded_on_sign_in() {
        let db = UserStore::in_memory().unwrap();
        let pk = db.add_user("dr", "pw", "view").unwrap();
        db.write_user(pk, "pw", "view").unwrap(); // as NanoPix stores it
        let user = db.sign_in("dr", "pw").unwrap().unwrap();
        assert!(user.password.starts_with("sha256$"));
        let stored = db.users().unwrap().into_iter().find(|u| u.pk == pk).unwrap();
        assert_eq!(stored.password, user.password);
        assert_eq!(stored.permission, "view");
        assert!(db.sign_in("dr", "pw").unwrap().is_some());
    }

    #[test]
    fn user_update_and_delete() {
        let db = UserStore::in_memory().unwrap();
        let pk = db.add_user("nurse", "a", "").unwrap();
        db.update_user(pk, "b", "report").unwrap();
        assert!(db.sign_in("nurse", "a").unwrap().is_none());
        assert_eq!(db.sign_in("nurse", "b").unwrap().unwrap().permission, "report");
        db.delete_user(pk).unwrap();
        assert!(db.delete_user(pk).is_err());
    }

    #[test]
    fn products_and_cloud_users_cascade() {
        let db = UserStore::in_memory().unwrap();
        let mut product = Product { sensor_sn: "S1".into(), product_sn: "P1".into(), mac_address: "00:11".into(), ..Default::default() };
        product.pk = db.add_product(&product).unwrap();
        assert_eq!(db.products().unwrap(), [product.clone()]);
        product.product_key = "KEY".into();
        db.update_product(&product).unwrap();
        assert_eq!(db.products().unwrap()[0].product_key, "KEY");

        assert_eq!(db.cloud_user(product.pk).unwrap(), None);
        let mut cloud = CloudUser { product_pk: product.pk, uuid: "u".into(), secret: "s".into(), session_id: "x".into(), expires_at: 42, ..Default::default() };
        cloud.pk = db.add_cloud_user(&cloud).unwrap();
        assert_eq!(db.cloud_user(product.pk).unwrap(), Some(cloud.clone()));
        cloud.expires_at = 99;
        db.update_cloud_user(&cloud).unwrap();
        assert_eq!(db.cloud_user(product.pk).unwrap().unwrap().expires_at, 99);

        // Deleting the product removes its cloud account (foreign key cascade).
        db.delete_product(product.pk).unwrap();
        assert_eq!(db.cloud_user(product.pk).unwrap(), None);
        assert!(db.delete_cloud_user(product.pk).is_err());
    }
}
