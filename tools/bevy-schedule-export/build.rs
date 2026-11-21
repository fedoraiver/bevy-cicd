fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!(
        "cargo::rustc-env=BEVY_SCHEDULE_EXPORT_TARGET={}",
        std::env::var("TARGET").expect("Cargo provides the target triple")
    );
}
