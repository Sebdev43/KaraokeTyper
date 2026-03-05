// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Use jemalloc to prevent glibc malloc virtual memory fragmentation
// with many threads (whisper, evdev, tokio, WebKitGTK)
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() {
    karaoketyper_lib::run()
}
