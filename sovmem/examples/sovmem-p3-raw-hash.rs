use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(path) = arguments.next() else {
        return invalid();
    };
    if arguments.next().is_some() {
        return invalid();
    }
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => metadata,
        _ => return invalid(),
    };
    if !metadata.is_file() {
        return invalid();
    }
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(_) => return invalid(),
    };
    let opened = match file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return invalid(),
    };
    if !same_file(&opened, &metadata) {
        return invalid();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return invalid();
    }
    let current = match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => metadata,
        _ => return invalid(),
    };
    if !same_file(&opened, &current) {
        return invalid();
    }
    println!("{}", blake3::hash(&bytes).to_hex());
    ExitCode::SUCCESS
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    false
}

fn invalid() -> ExitCode {
    eprintln!("safe-clone hash: invalid input");
    ExitCode::from(2)
}
