use flate2::read::GzDecoder;
use std::{env, fs, io, path::PathBuf};

struct Library(&'static str, &'static str);

const fn static_lib() -> &'static str {
    if cfg!(feature = "static-link") {
        "static"
    } else {
        "dylib"
    }
}

const fn build_zlib() -> bool {
    cfg!(not(feature = "nozlib"))
}

// Compiler specific compiler flags for CMake
fn compiler_flags() -> Vec<&'static str> {
    let mut flags = Vec::new();

    if cfg!(target_env = "msvc") {
        flags.push("/EHsc");

        // Find Ninja
        if which::which("ninja").is_ok() {
            env::set_var("CMAKE_GENERATOR", "Ninja");
        }
    }

    flags
}

fn lib_names() -> Vec<Library> {
    let mut names = Vec::new();

    if cfg!(target_os = "windows") && cfg!(target_env = "gnu") {
        panic!("Windows GNU is not supported, assimp fails to build for some reason.\nSee https://github.com/assimp/assimp/issues/4868");
    } else {
        names.push(Library("assimp", static_lib()));
    }

    if cfg!(feature = "nozlib") {
        names.push(Library("z", "dylib"));
    } else {
        names.push(Library("zlibstatic", "static"));
    }

    if cfg!(target_os = "linux") {
        names.push(Library("stdc++", "dylib"));
    }

    if cfg!(target_os = "macos") {
        names.push(Library("c++", "dylib"));
    }

    names
}

fn build_from_source() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Build Zlib from source?
    let build_zlib = if build_zlib() { "OFF" } else { "ON" };

    // Build static libs?
    let build_static = if static_lib() == "static" {
        "OFF"
    } else {
        "ON"
    };

    // CMake
    let mut cmake = cmake::Config::new("assimp");
    cmake
        .profile("Release")
        .static_crt(true)
        .out_dir(out_dir.join(static_lib()))
        .define("BUILD_SHARED_LIBS", build_static)
        .define("ASSIMP_BUILD_ASSIMP_TOOLS", "OFF")
        .define("ASSIMP_BUILD_TESTS", "OFF")
        .define("ASSIMP_BUILD_ZLIB", build_zlib)
        .define("LIBRARY_SUFFIX", "");

    // Add compiler flags
    for flag in compiler_flags().iter() {
        cmake.cflag(flag);
        cmake.cxxflag(flag);
    }

    let cmake_dir = cmake.build();

    println!(
        "cargo:rustc-link-search=native={}",
        cmake_dir.join("lib").display()
    );

    println!(
        "cargo:rustc-link-search=native={}",
        cmake_dir.join("bin").display()
    );
}

fn link_from_package() {
    let out_dir;
    let target = env::var("TARGET").unwrap();
    let crate_version = env::var("CARGO_PKG_VERSION").unwrap();
    let archive_name = format!(
        "russimp-{}-{}-{}.tar.gz",
        crate_version,
        target,
        static_lib()
    );

    if option_env!("RUSSIMP_PACKAGE_DIR").is_some() {
        out_dir = PathBuf::from(env::var("RUSSIMP_PACKAGE_DIR").unwrap());
    } else {
        out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let dl_link = format!(
            "https://github.com/jkvargas/russimp-sys/releases/download/v{}/{}",
            crate_version, archive_name
        );

        match fs::File::open(out_dir.join(&archive_name)) {
            Ok(_) => {}
            Err(_) => {
                let resp = reqwest::blocking::get(dl_link).unwrap();
                let mut bytes = io::Cursor::new(resp.bytes().unwrap());

                let mut file = fs::File::create(out_dir.join(&archive_name)).unwrap();
                io::copy(&mut bytes, &mut file).unwrap();
            }
        }
    }

    let file = fs::File::open(out_dir.join(&archive_name)).unwrap();
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    archive.unpack(out_dir).unwrap();
}

fn main() {
    println!("cargo:rerun-if-changed=bin/package/main.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    if cfg!(feature = "build-assimp") {
        build_from_source();
    } else if cfg!(feature = "prebuilt") {
        link_from_package();
    }

    bindgen::builder()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", out_dir.join(static_lib()).join("include").display()))
        .clang_arg(format!("-I{}", "assimp/include"))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .allowlist_type("ai.*")
        .allowlist_function("ai.*")
        .allowlist_var("ai.*")
        .allowlist_var("AI_.*")
        .derive_partialeq(true)
        .derive_eq(true)
        .derive_hash(true)
        .derive_debug(true)
        .generate()
        .unwrap()
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Could not generate russimp bindings, for details see https://github.com/jkvargas/russimp-sys");

    let mut built_opts = built::Options::default();
    built_opts
        .set_dependencies(false)
        .set_compiler(false)
        .set_ci(false)
        .set_cfg(false);
    built::write_built_file_with_opts(&built_opts, &manifest_dir, &out_dir.join("built.rs"))
        .unwrap();

    for n in lib_names().iter() {
        println!("cargo:rustc-link-lib={}={}", n.1, n.0);
    }
}
