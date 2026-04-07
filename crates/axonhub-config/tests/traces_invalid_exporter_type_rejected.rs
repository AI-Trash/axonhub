use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("axonhub-config-{name}-{unique}"))
}

struct TestFixture {
    root: PathBuf,
    workspace: PathBuf,
    original_dir: PathBuf,
    original_home: Option<OsString>,
}

impl TestFixture {
    fn new(name: &str) -> Self {
        let root = temp_dir(name);
        let workspace = root.join("workspace");
        let home = root.join("home");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(home.join(".config/axonhub")).unwrap();

        let original_dir = env::current_dir().unwrap();
        let original_home = env::var_os("HOME");

        env::set_var("HOME", &home);
        env::set_current_dir(&workspace).unwrap();

        Self {
            root,
            workspace,
            original_dir,
            original_home,
        }
    }

    fn write_workspace_file(&self, relative_path: &str, contents: &str) {
        let path = self.workspace.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        env::set_current_dir(&self.original_dir).unwrap();

        match &self.original_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }

        fs::remove_dir_all(&self.root).ok();
    }
}

#[test]
fn traces_invalid_exporter_type_rejected() {
    let fixture = TestFixture::new("traces-invalid-exporter-type");
    fixture.write_workspace_file(
        "config.yml",
        r#"
traces:
  enabled: true
  exporter:
    type: "bogus"
"#,
    );

    let error = axonhub_config::load().unwrap_err().to_string();

    assert_eq!(error, "invalid traces exporter type 'bogus'");
}
