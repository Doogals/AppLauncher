fn main() {
    // Tell Cargo to recompile whenever any icon file changes.
    println!("cargo:rerun-if-changed=icons/32x32.png");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    println!("cargo:rerun-if-changed=icons/128x128@2x.png");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");

    // NOTE ON `cargo test`: the lib's unit tests currently cannot be executed on
    // Windows. tauri-plugin-dialog imports TaskDialogIndirect, which only exists
    // in comctl32 v6, and Windows loads v6 only for executables whose manifest
    // declares that assembly dependency. tauri_build::build() embeds such a
    // manifest into the application binary, but the unittest harness is a
    // separate link target that receives none — so it resolves against
    // comctl32 v5 and aborts at load with STATUS_ENTRYPOINT_NOT_FOUND
    // (0xC0000139) before running anything.
    //
    // There is no cargo directive that targets the lib's unittest binary:
    // rustc-link-arg-tests covers only [[test]] targets (this package has none),
    // and the generic rustc-link-arg would also apply to the app binary, whose
    // manifest is already supplied by a resource script — embedding a second one
    // risks breaking the release build. Tests therefore compile (and are
    // type-checked by `cargo check --tests`) but are not run.
    tauri_build::build()
}
