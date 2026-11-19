//! Headless UI tests: they click and type through egui's accessibility tree,
//! the same way on every operating system.

use super::*;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumodent::image::Image;
use lumodent::patients::PatientDetails;
use lumodent::reports::encode_bmp;
use std::cell::RefCell;
use std::rc::Rc;

const PNG: &[u8] = include_bytes!("../../tests/fixtures/rgb-3x2.png");
const JPG: &[u8] = include_bytes!("../../tests/fixtures/halves-32x16.jpg");

struct Env {
    dir: PathBuf,
    pick: Rc<RefCell<Option<PathBuf>>>,
    save: Rc<RefCell<Option<PathBuf>>>,
    opened: Rc<RefCell<Vec<PathBuf>>>,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(tag: &str) -> (Harness<'static, App>, Env) {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("lumodent-ui-{tag}-{}-{stamp}", std::process::id()));
    let env = Env {
        dir: dir.clone(),
        pick: Rc::default(),
        save: Rc::default(),
        opened: Rc::default(),
    };
    let (pick, save, opened) = (env.pick.clone(), env.save.clone(), env.opened.clone());
    let hooks = Hooks {
        pick_image: Box::new(move || pick.borrow().clone()),
        save_pdf: Box::new(move |_| save.borrow().clone()),
        open_file: Box::new(move |path| {
            opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        }),
    };
    let app = App::new(Workspace::open(dir.join("data")).unwrap(), hooks);
    let harness = Harness::builder()
        .with_size([1280.0, 860.0])
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    (harness, env)
}

fn file(env: &Env, name: &str, bytes: &[u8]) -> PathBuf {
    let path = env.dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

/// The text field captioned `label` (a caption and its field share the name).
fn field<'h>(h: &'h Harness<'static, App>, label: &'h str) -> egui_kittest::Node<'h> {
    use egui::accesskit::Role;
    h.query_by_role_and_label(Role::TextInput, label)
        .unwrap_or_else(|| h.get_by_role_and_label(Role::MultilineTextInput, label))
}

fn type_into(h: &mut Harness<'static, App>, label: &str, text: &str) {
    field(h, label).focus();
    h.run();
    field(h, label).type_text(text);
    h.run();
}

/// Reads and dismisses the notice dialog.
fn notice(h: &mut Harness<'static, App>) -> String {
    let text = h.state().notice.clone().expect("a notice is open");
    click(h, "OK");
    assert!(h.state().notice.is_none());
    text
}

fn add_patient(h: &mut Harness<'static, App>, id: &str, last: &str, first: &str) {
    click(h, "New Patient");
    type_into(h, "Chart No.", id);
    type_into(h, "Last Name", last);
    type_into(h, "First Name", first);
    click(h, "Ok");
}

#[test]
fn patient_create_search_edit_delete() {
    let (mut h, _env) = start("patients");
    h.run();
    assert!(h.query_by_label("Edit Patient").is_some());

    add_patient(&mut h, "P-01", "Wijaya", "Sari");
    assert!(h.state().patients.editor.is_none());
    assert_eq!(h.state().patients.rows.len(), 1);
    assert_eq!(h.state().patients.selected_patient().unwrap().details.first_name, "Sari");

    // A duplicate chart number is refused with a message and the form stays open.
    add_patient(&mut h, "P-01", "Other", "Person");
    assert_eq!(notice(&mut h), "Chart No. P-01 already exists.");
    assert!(h.state().patients.editor.is_some());
    click(&mut h, "Cancel");
    // Empty name is refused too.
    click(&mut h, "New Patient");
    type_into(&mut h, "Chart No.", "P-02");
    click(&mut h, "Ok");
    assert_eq!(notice(&mut h), "Patient's name can't be empty, please input one.");
    click(&mut h, "Cancel");

    type_into(&mut h, "Search patients", "nobody");
    click(&mut h, "Search");
    assert!(h.state().patients.rows.is_empty());
    h.state_mut().patients.query.clear();
    click(&mut h, "Search");
    assert_eq!(h.state().patients.rows.len(), 1);

    click(&mut h, "P-01 Wijaya Sari");
    click(&mut h, "Edit Patient");
    type_into(&mut h, "Phone", "0812");
    click(&mut h, "Ok");
    assert_eq!(h.state().ws.store.list("0812").unwrap().len(), 1);

    click(&mut h, "Show Detail");
    assert!(h.state().patients.detail);
    click(&mut h, "Close");

    click(&mut h, "Delete Patient");
    click(&mut h, "No");
    assert_eq!(h.state().patients.rows.len(), 1);
    click(&mut h, "Delete Patient");
    click(&mut h, "Yes");
    assert!(h.state().patients.rows.is_empty());
}

#[test]
fn navigation_needs_a_patient() {
    let (mut h, _env) = start("nav");
    h.run();
    click(&mut h, "Viewer");
    assert_eq!(notice(&mut h), "Please select a patient.");
    click(&mut h, "Acquisition");
    assert!(notice(&mut h).contains("not available"));
    assert_eq!(h.state().screen, Screen::Patient);
}

#[test]
fn viewer_send_to_report_save_export_print_and_delete() {
    let (mut h, env) = start("flow");
    h.run();
    add_patient(&mut h, "P-07", "Santoso", "Budi");

    click(&mut h, "Viewer");
    assert_eq!(h.state().screen, Screen::Viewer);
    *env.pick.borrow_mut() = Some(file(&env, "a.png", PNG));
    click(&mut h, "Import Image");
    *env.pick.borrow_mut() = Some(file(&env, "b.jpg", JPG));
    click(&mut h, "Import Image");
    *env.pick.borrow_mut() = Some(file(&env, "c.txt", b"x"));
    click(&mut h, "Import Image");
    assert!(notice(&mut h).contains("Only .jpg"));
    assert_eq!(h.state().viewer.studies.len(), 2);
    assert_eq!(h.state().viewer.current, Some(1));

    let size = |h: &Harness<'static, App>| h.state().viewer.image.as_ref().map(|i| (i.width, i.height));
    assert_eq!(size(&h), Some((32, 16)));
    click(&mut h, "Rotate");
    assert_eq!(size(&h), Some((16, 32)));
    click(&mut h, "Invert");
    click(&mut h, "Send to Report");
    assert_eq!(notice(&mut h), "Image sent to report (1 in the current report).");

    // Choose the PNG by its thumbnail row and send it as well.
    h.get_by_label_contains("1.").click();
    h.run();
    assert_eq!(size(&h), Some((3, 2)));
    click(&mut h, "Send to Report");
    notice(&mut h);

    click(&mut h, "Report");
    assert_eq!(h.state().screen, Screen::Report);
    assert_eq!(h.state().report.draft.images.len(), 2);
    type_into(&mut h, "Clinic Name", "Klinik Gigi Sehat");
    type_into(&mut h, "Note", "Caries on 36.");
    click(&mut h, "Save Report");
    assert!(notice(&mut h).starts_with("Save report successful"));
    assert_eq!(h.state().report.rows.len(), 1);
    assert_eq!(h.state().ws.load_clinic().name, "Klinik Gigi Sehat");

    let pdf = env.dir.join("out.pdf");
    *env.save.borrow_mut() = Some(pdf.clone());
    click(&mut h, "Export PDF");
    assert!(notice(&mut h).starts_with("Report exported to"));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
    click(&mut h, "Print");
    let opened = env.opened.borrow().clone();
    assert_eq!(opened.len(), 1);
    assert!(std::fs::read(&opened[0]).unwrap().starts_with(b"%PDF-"));

    // Start a new report, then reopen the saved one from the list.
    click(&mut h, "New Report");
    assert!(h.state().report.draft.images.is_empty());
    h.get_by_label_contains("1.  ").click();
    h.run();
    assert_eq!(h.state().report.draft.doc.note, "Caries on 36.");
    assert_eq!(h.state().report.draft.images.len(), 2);

    // Unsaved edits ask before switching away.
    click(&mut h, "Remove Image");
    h.state_mut().report.selected_image = Some(0);
    click(&mut h, "Remove Image");
    assert_eq!(h.state().report.draft.images.len(), 1);
    click(&mut h, "New Report");
    click(&mut h, "No");
    assert_eq!(h.state().report.draft.images.len(), 1);

    click(&mut h, "Delete Report");
    click(&mut h, "Yes");
    assert!(h.state().report.rows.is_empty());

    click(&mut h, "Patient");
    let patient = h.state().patients.selected_patient().cloned().unwrap();
    click(&mut h, "Delete Patient");
    click(&mut h, "Yes");
    assert!(!h.state().ws.image_dir(&patient.id).exists());
    assert!(!h.state().ws.reports_dir(&patient.id).exists());
}

/// Renders each screen to PNG with demo data:
/// `cargo test --release render_screens -- --ignored` (output in `target/screens`).
#[test]
#[ignore]
fn render_screens() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    let _ = std::fs::remove_dir_all(&out);
    seed_demo(&out.join("data"));
    let app = App::new(Workspace::open(out.join("data")).unwrap(), Hooks::native());
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    theme::apply(&h.ctx);
    h.run();
    let shot = |h: &mut Harness<'static, App>, name: &str| {
        h.run();
        h.render().unwrap().save(out.join(name)).unwrap();
    };
    click(&mut h, "P-0001 Wijaya Sari");
    shot(&mut h, "1-patient.png");
    click(&mut h, "New Patient");
    shot(&mut h, "2-new-patient.png");
    click(&mut h, "Cancel");
    click(&mut h, "Viewer");
    shot(&mut h, "3-viewer.png");
    click(&mut h, "Send to Report");
    click(&mut h, "OK");
    h.get_by_label_contains("1.").click();
    h.run();
    click(&mut h, "Send to Report");
    click(&mut h, "OK");
    click(&mut h, "Report");
    type_into(&mut h, "Note", "Caries on 36 and 37. Recall in six months for a follow-up bitewing.");
    shot(&mut h, "4-report.png");
    h.state_mut().report.draft.doc.landscape = true;
    shot(&mut h, "5-report-landscape.png");
}

fn seed_demo(dir: &Path) {
    let dir = dir.to_path_buf();
    let ws = Workspace::open(dir.clone()).unwrap();
    let people = [("P-0001", "Wijaya", "Sari", "F", "1990-04-12", "0812-3456-7788"), ("P-0002", "Santoso", "Budi", "M", "1984-11-02", "0813-2222-1010"), ("P-0003", "Lestari", "Dewi", "F", "2001-07-30", "0857-9090-1212")];
    for (id, last, first, sex, birthdate, phone) in people {
        let details = PatientDetails { last_name: last.into(), first_name: first.into(), sex: sex.into(), birthdate: birthdate.into(), phone: phone.into(), doctor: "drg. Rina".into(), ..Default::default() };
        let pk = ws.store.add(id, &details).unwrap();
        if id != "P-0001" {
            continue;
        }
        let patient = Patient { pk, id: id.into(), details };
        for seed in 0..3 {
            let image = synthetic_radiograph(640, 480, seed);
            let path = dir.join(format!("demo{seed}.bmp"));
            std::fs::write(&path, encode_bmp(&image)).unwrap();
            ws.import_image(&patient, &path).unwrap();
            std::fs::remove_file(path).unwrap();
        }
    }
    let clinic = lumodent::reports::Clinic { name: "Klinik Gigi Sehat".into(), address: "Jl. Merdeka 1, Bandung".into(), phone: "022-123456".into() };
    ws.save_clinic(&clinic).unwrap();
}

/// A grey bitewing-like picture: two crowns over a darker background with noise.
fn synthetic_radiograph(w: usize, h: usize, seed: u32) -> Image {
    let mut pixels = Vec::with_capacity(w * h);
    let mut state = 0x9e37_79b9u32 ^ seed.wrapping_mul(7919);
    for y in 0..h {
        for x in 0..w {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let mut v = 40.0 + 30.0 * fy;
            for (cx, cy) in [(0.32 + seed as f32 * 0.03, 0.45), (0.68 - seed as f32 * 0.02, 0.48)] {
                let d = ((fx - cx) / 0.15).powi(2) + ((fy - cy) / 0.32).powi(2);
                if d < 1.0 {
                    v = 110.0 + 90.0 * (1.0 - d);
                    if ((fx - cx) / 0.07).powi(2) + ((fy - cy + 0.08) / 0.12).powi(2) < 1.0 {
                        v -= 55.0;
                    }
                }
            }
            let g = (v + (state % 24) as f32 - 12.0).clamp(0.0, 255.0) as u32;
            pixels.push(g << 16 | g << 8 | g);
        }
    }
    Image { width: w, height: h, pixels }
}
