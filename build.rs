extern crate gl_generator;

use gl_generator::{Api, Fallbacks, GlobalGenerator, Profile, Registry};

use std::env;
use std::fs::File;
use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::unix::fs::FileExt;

#[cfg(target_os = "windows")]
use std::os::windows::fs::FileExt;

fn main() {
    let dst = env::var("OUT_DIR").unwrap();
    let mut file = File::create(&Path::new(&dst).join("gl_bindings.rs")).unwrap();

    file.write_at(b"#![allow(clippy::all)]", 0).unwrap();

    Registry::new(Api::Gl, (4, 6), Profile::Core, Fallbacks::All, [])
        .write_bindings(GlobalGenerator, &mut file)
        .unwrap();
}
