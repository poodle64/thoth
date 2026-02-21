fn main() {
    // whisper-rs Metal code uses @available() checks that require the Clang runtime
    // (___isPlatformVersionAtLeast). When sherpa-rs is excluded (no parakeet feature),
    // this runtime isn't linked transitively, so we must link it explicitly.
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("clang")
            .arg("--print-runtime-dir")
            .output()
        {
            let runtime_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !runtime_dir.is_empty() {
                println!("cargo:rustc-link-search={runtime_dir}");
                println!("cargo:rustc-link-lib=dylib=clang_rt.osx");
            }
        }

        // FluidAudio links Swift runtime (swiftCore) dynamically. On deployment targets
        // < macOS 15.0, libswift_Concurrency.dylib resolves via @rpath, so the binary
        // needs rpath entries pointing to Swift runtime library directories.
        #[cfg(feature = "fluidaudio")]
        {
            if let Ok(output) = std::process::Command::new("swift")
                .args(["-print-target-info"])
                .output()
            {
                if output.status.success() {
                    if let Ok(json_str) = String::from_utf8(output.stdout) {
                        if let Some(paths_start) = json_str.find("\"runtimeLibraryPaths\"") {
                            if let Some(arr_start) = json_str[paths_start..].find('[') {
                                let arr_offset = paths_start + arr_start;
                                if let Some(arr_end) = json_str[arr_offset..].find(']') {
                                    let arr_str = &json_str[arr_offset + 1..arr_offset + arr_end];
                                    for item in arr_str.split(',') {
                                        let path = item.trim().trim_matches('"').trim();
                                        if !path.is_empty() {
                                            println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Fallback: always include /usr/lib/swift
            println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        }
    }

    tauri_build::build();
}
