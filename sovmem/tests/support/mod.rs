use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod p2;
pub mod p3;

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

pub struct GitFixture {
    pub root: PathBuf,
}

impl GitFixture {
    pub fn new(name: &str) -> Self {
        let safe_name: String = name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-{safe_name}-{}-{timestamp}-{fixture_id}",
            std::process::id()
        ));

        fs::create_dir(&root).expect("create Git fixture directory");
        let fixture = Self { root };
        fixture.git(&["init", "--quiet"]);
        fixture.git(&["config", "user.name", "SovMem Test"]);
        fixture.git(&["config", "user.email", "sovmem@example.invalid"]);
        fixture
    }

    pub fn write(&self, path: &str, body: &str) {
        let destination = self.root.join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).expect("create fixture file parent directories");
        }
        fs::write(destination, body).expect("write fixture file");
    }

    pub fn commit_all(&self, message: &str, date: &str) {
        self.git(&["add", "--all"]);

        let mut command = self.git_command();
        command
            .args(["commit", "--quiet", "--message", message])
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
        Self::expect_success(command.output().expect("run Git commit"), "git commit");
    }

    pub fn head(&self) -> String {
        let output = self.git(&["rev-parse", "HEAD"]);
        String::from_utf8(output.stdout)
            .expect("Git HEAD must be UTF-8")
            .trim()
            .to_owned()
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).expect("read fixture file")
    }

    pub fn hash_worktree(&self) -> BTreeMap<String, String> {
        let mut hashes = BTreeMap::new();
        self.hash_directory(&self.root, &mut hashes);
        hashes
    }

    fn git_command(&self) -> Command {
        let mut command = Command::new("git");
        command.args(["-C"]).arg(&self.root);
        command
    }

    fn git(&self, arguments: &[&str]) -> Output {
        let mut command = self.git_command();
        command.args(arguments);
        let output = command.output().expect("run Git command");
        Self::expect_success(output, &format!("git {}", arguments.join(" ")))
    }

    fn expect_success(output: Output, operation: &str) -> Output {
        assert!(
            output.status.success(),
            "{operation} failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn hash_directory(&self, directory: &Path, hashes: &mut BTreeMap<String, String>) {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .expect("read fixture directory")
            .map(|entry| entry.expect("read fixture directory entry"))
            .collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            if path == self.root.join(".git") {
                continue;
            }
            if entry.file_type().expect("read fixture entry type").is_dir() {
                self.hash_directory(&path, hashes);
                continue;
            }

            let relative_path = path
                .strip_prefix(&self.root)
                .expect("fixture path must remain beneath root")
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path).expect("read fixture file for hashing");
            hashes.insert(relative_path, blake3::hash(&bytes).to_hex().to_string());
        }
    }
}

impl Drop for GitFixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove Git fixture directory: {error}");
            }
        }
    }
}
