fn main() {
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-changed=cdm-api/content_decryption_module.h");
    cc::Build::new().cpp(true).std("c++17").file("src/shim.cc").warnings(true).compile("cdmshim");
    println!("cargo:rustc-link-lib=dl");
}
