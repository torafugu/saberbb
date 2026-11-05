use sha2::{Digest,Sha256};
use std::{fs,path::{Path,PathBuf}};
fn collect(path:&Path,files:&mut Vec<PathBuf>) {
    for entry in fs::read_dir(path).expect("read source directory") {
        let path=entry.expect("source entry").path();
        if path.is_dir() { collect(&path,files); }
        else if path.extension().is_some_and(|x|x=="rs") { files.push(path); }
    }
}
fn main() {
    let mut files=vec![PathBuf::from("Cargo.toml"),PathBuf::from("Cargo.lock"),PathBuf::from("build.rs")];
    collect(Path::new("src"),&mut files);files.sort();
    let mut hash=Sha256::new();
    for path in files {
        let name=path.to_string_lossy().replace('\\',"/");
        let bytes=fs::read(&path).expect("read build input");
        hash.update((name.len() as u64).to_le_bytes());hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());hash.update(bytes);
        println!("cargo:rerun-if-changed={name}");
    }
    println!("cargo:rerun-if-changed=src");
    let source=format!("{:x}",hash.finalize());
    let target=std::env::var("TARGET").unwrap();let profile=std::env::var("PROFILE").unwrap();
    let compiler=std::process::Command::new(std::env::var("RUSTC").unwrap()).arg("--version").output().expect("rustc version");
    let compiler=String::from_utf8(compiler.stdout).expect("rustc UTF8").trim().to_string();
    let mut build=Sha256::new();for value in [&source,&target,&profile,&compiler] { build.update((value.len() as u64).to_le_bytes());build.update(value.as_bytes()); }
    println!("cargo:rustc-env=SABERBB_ENGINE_SOURCE_SHA256={source}");
    println!("cargo:rustc-env=SABERBB_BUILD_SHA256={:x}",build.finalize());
    println!("cargo:rustc-env=SABERBB_BUILD_TARGET={target}");
    println!("cargo:rustc-env=SABERBB_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=SABERBB_BUILD_COMPILER={compiler}");
    let git=|args:&[&str]| std::process::Command::new("git").args(args).output().ok()
        .filter(|o|o.status.success()).and_then(|o|String::from_utf8(o.stdout).ok()).map(|s|s.trim().to_string());
    println!("cargo:rustc-env=SABERBB_GIT_COMMIT={}",git(&["rev-parse","HEAD"]).unwrap_or_else(||"unknown".into()));
    println!("cargo:rustc-env=SABERBB_GIT_DIRTY={}",git(&["status","--porcelain"]).map(|s|!s.is_empty()).unwrap_or(true));
    if let Some(path)=git(&["rev-parse","--git-path","HEAD"]) { println!("cargo:rerun-if-changed={path}"); }
    if let Some(reference)=git(&["symbolic-ref","--quiet","HEAD"]) {
        if let Some(path)=git(&["rev-parse","--git-path",&reference]) { println!("cargo:rerun-if-changed={path}"); }
    }
}
