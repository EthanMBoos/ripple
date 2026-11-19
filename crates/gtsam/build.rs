//! Compiles the C++ shim (src/smoother.cc) against an installed GTSAM and
//! links GTSAM's static libraries into whatever uses this crate. The install
//! is the one the book's GTSAM guide describes; `GTSAM_PREFIX` points at it.
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=GTSAM_PREFIX");
    for file in ["src/ffi.rs", "src/smoother.h", "src/smoother.cc"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let prefix = match env::var_os("GTSAM_PREFIX") {
        Some(prefix) => PathBuf::from(prefix),
        None => panic!(
            "GTSAM_PREFIX is not set. Point it at a GTSAM 4.3 install built as described in \
             book/src/guides/gtsam.md, such as $HOME/.local/gtsam-4.3.0"
        ),
    };
    let include = prefix.join("include");
    if !include.join("gtsam/config.h").is_file() {
        panic!(
            "no GTSAM headers under {}: GTSAM_PREFIX must be the install prefix that holds \
             include/gtsam and lib/libgtsam.a (book/src/guides/gtsam.md)",
            include.display()
        );
    }
    cxx_build::bridge("src/ffi.rs")
        .file("src/smoother.cc")
        .std("c++17")
        // GTSAM's headers trip the default warning set (unused parameters); they are not ours.
        .warnings(false)
        .include(&include)
        // GTSAM installs the Eigen it was built with; the shim must use the same one.
        .include(include.join("gtsam/3rdparty/Eigen"))
        .compile("ripple-gtsam-smoother");
    println!(
        "cargo:rustc-link-search=native={}",
        prefix.join("lib").display()
    );
    // Static, so `ripple` carries GTSAM and needs no library path at run time.
    println!("cargo:rustc-link-lib=static=gtsam");
    println!("cargo:rustc-link-lib=static=cephes-gtsam");
}
