fn main() {
    tauri_build::build();

    #[cfg(target_os = "ios")]
    {
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}