use std::sync::atomic::{AtomicU32, Ordering};

static GPU_FAILURE_COUNT: AtomicU32 = AtomicU32::new(0);
const GPU_FAILURE_THRESHOLD: u32 = 3;

/// Record a GPU initialization failure. Returns the new failure count.
pub fn record_gpu_failure() -> u32 {
    let prev = GPU_FAILURE_COUNT.fetch_add(1, Ordering::SeqCst);
    prev + 1
}

/// Reset the GPU failure counter (e.g. after a successful GPU init).
pub fn reset_gpu_failures() {
    GPU_FAILURE_COUNT.store(0, Ordering::SeqCst);
}

/// Feed the auto-disable counter from an engine load. Only a GPU build with the GPU
/// enabled counts, so the default CPU build is unaffected (the GPU backend is wired
/// into transcribe-rs in a later lot; this keeps the mechanism live).
pub fn note_engine_load(use_gpu: bool, succeeded: bool) {
    const GPU_BUILD: bool = cfg!(any(feature = "gpu-vulkan", feature = "gpu-metal"));
    if !(GPU_BUILD && use_gpu) {
        return;
    }
    if succeeded {
        reset_gpu_failures();
    } else {
        let count = record_gpu_failure();
        crate::dev_log!("[GPU] Engine load failed with GPU enabled (failure #{count})");
    }
}

pub fn get_gpu_failure_count() -> u32 {
    GPU_FAILURE_COUNT.load(Ordering::SeqCst)
}

pub fn is_gpu_disabled_by_failures() -> bool {
    get_gpu_failure_count() >= GPU_FAILURE_THRESHOLD
}
