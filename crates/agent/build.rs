fn main() {
    // Rebuild embedded assets even when a Vite rebuild changes hashed filenames.
    println!("cargo:rerun-if-changed=../../web/dist");
    println!("cargo:rerun-if-changed=build.rs");
}
