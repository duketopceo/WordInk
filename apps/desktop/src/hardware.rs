//! What this computer can run, and what to recommend for it (KTD8).
//!
//! [`probe`] reads the GPU devices ggml reports (the same list Hex's Auto
//! device logic uses, minus any GPU that hung before: `gpu_health`), the
//! operating system's own view of the GPU and its Vulkan loader ([`gpu`]),
//! the memory, and the CPU features. [`recommend`] is a pure function from
//! that to one primary choice and one alternative, for onboarding to show.

use std::sync::Mutex;

pub mod gpu;
pub mod tools;

use crate::transcription::cloud::Provider;

/// The model local dictation recommends: its wire name in the catalogue.
pub const PARAKEET_V3: &str = "parakeet_v3";
/// The same model on the Apple Neural Engine (Linux on Apple silicon), which
/// local dictation recommends where its runtime answers.
pub const PARAKEET_V3_ANE: &str = "parakeet_v3_ane";
/// Local dictation wants at least this much RAM (GB).
pub const MIN_LOCAL_RAM_GB: f64 = 8.0;

/// The GPU API the speech model could run on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuApi {
    /// A hardware Vulkan device (or any other GPU backend ggml found).
    Vulkan,
    Metal,
    /// No GPU device at all.
    None,
    /// Only a software rasteriser (Mesa lavapipe / llvmpipe, SwiftShader).
    Software,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuFeatures {
    pub x86: bool,
    /// Only meaningful on x86.
    pub avx2: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub gpu_api: GpuApi,
    /// The GPU's name and whether it is ready, for onboarding's line.
    pub gpu: gpu::GpuSummary,
    /// The best GPU's memory: VRAM, or the unified pool on Apple silicon.
    /// 0 when unreported.
    pub vram_or_unified_gb: f64,
    /// System RAM. 0 when it could not be read.
    pub ram_gb: f64,
    pub cpu_features: CpuFeatures,
    /// The Apple Neural Engine and its runtime are ready here (Linux on
    /// Apple silicon only; see `transcription::ane::probe`).
    pub neural_engine: bool,
}

/// One device, as the probe classifies it.
#[derive(Clone, Debug, PartialEq)]
pub struct RawDevice {
    pub description: String,
    /// ggml's kind: "vulkan", "metal", "cuda", "cpu", ...
    pub kind: String,
    /// A GPU or integrated GPU, as opposed to the CPU or an accelerator.
    pub gpu: bool,
    pub memory_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    Cloud(Provider),
    /// A catalogue model id, e.g. [`PARAKEET_V3`].
    Local(&'static str),
}

/// Why `primary` was picked, for the line under it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    HasKey,
    NoAvx2,
    NoGpu,
    /// A hardware GPU without a Vulkan driver (`gpu::GpuStatus::NeedsDriver`).
    NeedsDriver,
    SoftwareGpu,
    LowMemory,
    FitsLocal,
    /// The Apple Neural Engine runs the local model.
    NeuralEngine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Recommendation {
    pub primary: Choice,
    pub alternative: Choice,
    pub reason: Reason,
}

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Software rasterisers report themselves by name.
pub fn is_software(description: &str) -> bool {
    let name = description.to_ascii_lowercase();
    [
        "llvmpipe",
        "lavapipe",
        "softpipe",
        "swiftshader",
        "software",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

/// The GPU API and memory (GB) of a device list: the best hardware GPU
/// (most memory) decides, a list of only software rasterisers is
/// [`GpuApi::Software`], and no GPU at all is [`GpuApi::None`]. Any GPU
/// backend other than Metal counts as Vulkan, the API the window needs.
pub fn classify(devices: &[RawDevice]) -> (GpuApi, f64) {
    let gpus = devices.iter().filter(|device| device.gpu);
    let best = gpus
        .clone()
        .filter(|device| !is_software(&device.description))
        .max_by_key(|device| device.memory_bytes);
    match best {
        Some(device) => (
            if device.kind == "metal" {
                GpuApi::Metal
            } else {
                GpuApi::Vulkan
            },
            device.memory_bytes as f64 / GB,
        ),
        None if gpus.count() > 0 => (GpuApi::Software, 0.0),
        None => (GpuApi::None, 0.0),
    }
}

/// `MemTotal` of a `/proc/meminfo`, in GB.
#[cfg(any(target_os = "linux", test))]
pub fn parse_meminfo(text: &str) -> Option<f64> {
    let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kilobytes: f64 = line
        .trim_start_matches("MemTotal:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()?;
    Some(kilobytes * 1024.0 / GB)
}

/// The recommendation for `probe`, in this order: a key already set means
/// Groq; x86 without AVX2 can't run the local model well; no GPU, or only a
/// software driver, means Groq; known RAM under [`MIN_LOCAL_RAM_GB`] means
/// Groq; otherwise Parakeet v3 on this computer, on the Neural Engine where
/// that is ready. Each names the other as the alternative. Unknown RAM (0)
/// does not rule local out.
pub fn recommend(probe: &Probe, has_groq_key: bool) -> Recommendation {
    const GROQ: Choice = Choice::Cloud(Provider::Groq);
    const LOCAL: Choice = Choice::Local(PARAKEET_V3);
    let cloud = |reason| Recommendation {
        primary: GROQ,
        alternative: LOCAL,
        reason,
    };
    if has_groq_key {
        return cloud(Reason::HasKey);
    }
    if probe.cpu_features.x86 && !probe.cpu_features.avx2 {
        return cloud(Reason::NoAvx2);
    }
    // The Neural Engine needs no GPU driver: its own runtime drives it.
    if probe.neural_engine && !(probe.ram_gb > 0.0 && probe.ram_gb < MIN_LOCAL_RAM_GB) {
        return Recommendation {
            primary: Choice::Local(PARAKEET_V3_ANE),
            alternative: GROQ,
            reason: Reason::NeuralEngine,
        };
    }
    if probe.gpu.status == gpu::GpuStatus::NeedsDriver {
        return cloud(Reason::NeedsDriver);
    }
    match probe.gpu_api {
        GpuApi::None => return cloud(Reason::NoGpu),
        GpuApi::Software => return cloud(Reason::SoftwareGpu),
        GpuApi::Vulkan | GpuApi::Metal => {}
    }
    if probe.ram_gb > 0.0 && probe.ram_gb < MIN_LOCAL_RAM_GB {
        return cloud(Reason::LowMemory);
    }
    Recommendation {
        primary: LOCAL,
        alternative: GROQ,
        reason: Reason::FitsLocal,
    }
}

/// What the probe found, with whether a Groq key is already set and which
/// other dictation tools are installed.
#[derive(Clone, Debug, PartialEq)]
pub struct Probed {
    pub probe: Probe,
    pub has_groq_key: bool,
    pub other_tools: Vec<tools::OtherTool>,
}

impl Probed {
    pub fn recommendation(&self) -> Recommendation {
        recommend(&self.probe, self.has_groq_key)
    }
}

/// Reads this computer. Initialising Vulkan and the keyring can take a
/// moment (the keyring up to its own deadline): call it from [`start`]'s
/// thread, never the UI's.
pub fn probe() -> Probe {
    // ggml's own device list, the one Auto uses, minus a GPU that hung or
    // ran slower than the CPU under this driver (`gpu_health`). The backends
    // start the way a model load starts them (on Windows inside the GPU
    // crash guard), so the probe never sees a different list than the load.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let devices = match crate::transcription::local_transcriber::init_backends() {
        Ok(_) => crate::transcription::gpu_health::gpus()
            .into_iter()
            .filter(|device| !crate::transcription::gpu_health::failed(device))
            .map(|device| RawDevice {
                description: device.description.clone(),
                kind: device.kind.clone(),
                gpu: true,
                memory_bytes: device.memory_total,
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not start the GPU backends for the hardware probe"
            );
            Vec::new()
        }
    };
    // macOS runs on Apple silicon only: Metal with unified memory.
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let devices = vec![RawDevice {
        description: String::new(),
        kind: "metal".into(),
        gpu: true,
        memory_bytes: 0,
    }];
    let observed = gpu::Observed {
        os: gpu::Os::current(),
        gpu_build: cfg!(any(
            target_os = "macos",
            target_os = "linux",
            all(target_os = "windows", target_arch = "x86_64")
        )),
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        loader: gpu::loader_present(),
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        loader: false,
        #[cfg(target_os = "linux")]
        icds: gpu::icds_installed(),
        #[cfg(not(target_os = "linux"))]
        icds: true,
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        adapters: gpu::adapters(),
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        adapters: Vec::new(),
        ggml: devices,
    };
    tracing::info!(?observed, "GPU observation");
    let (gpu, gpu_api, vram_or_unified_gb) = gpu::assess(&observed);
    Probe {
        gpu_api,
        gpu,
        vram_or_unified_gb,
        ram_gb: system_ram_gb().unwrap_or(0.0),
        cpu_features: CpuFeatures {
            x86: cfg!(target_arch = "x86_64"),
            #[cfg(target_arch = "x86_64")]
            avx2: std::arch::is_x86_feature_detected!("avx2"),
            #[cfg(not(target_arch = "x86_64"))]
            avx2: false,
        },
        neural_engine: neural_engine_ready(),
    }
}

/// Whether the Neural Engine engine could run now. Only Linux on Apple
/// silicon asks the runtime (a short subprocess, on the probe's thread);
/// everywhere else the answer is no without touching anything.
fn neural_engine_ready() -> bool {
    if !cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        return false;
    }
    let ane = crate::transcription::ane::probe::current();
    tracing::info!(verdict = ?ane.verdict, "Neural Engine probe");
    ane.verdict == crate::transcription::ane::probe::Verdict::Available
}

#[cfg(target_os = "linux")]
fn system_ram_gb() -> Option<f64> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(target_os = "macos")]
fn system_ram_gb() -> Option<f64> {
    let output = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    let bytes: f64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;
    Some(bytes / GB)
}

/// Not read on Windows: 0 means unknown, and [`recommend`] then trusts the
/// GPU's own memory.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn system_ram_gb() -> Option<f64> {
    None
}

static PROBED: Mutex<Option<Probed>> = Mutex::new(None);
static STARTED: std::sync::Once = std::sync::Once::new();

/// Starts the probe on its own thread, once.
pub fn start() {
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("hardware-probe".into())
            .spawn(|| {
                let probe = probe();
                let has_groq_key =
                    crate::keys::status(Provider::Groq.id(), Provider::Groq.key_env()).is_some();
                let other_tools = tools::assess(&tools::observe());
                tracing::info!(?probe, has_groq_key, ?other_tools, "hardware probe");
                *PROBED
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Probed {
                    probe,
                    has_groq_key,
                    other_tools,
                });
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the hardware probe");
        }
    });
}

/// The probe's answer, once [`start`]'s thread has it.
pub fn cached() -> Option<Probed> {
    PROBED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROQ: Choice = Choice::Cloud(Provider::Groq);
    const LOCAL: Choice = Choice::Local(PARAKEET_V3);

    fn probe(gpu_api: GpuApi, vram: f64, ram: f64, x86: bool, avx2: bool) -> Probe {
        let status = match gpu_api {
            GpuApi::Vulkan | GpuApi::Metal => gpu::GpuStatus::Ready,
            GpuApi::Software => gpu::GpuStatus::Software,
            GpuApi::None => gpu::GpuStatus::None,
        };
        Probe {
            gpu_api,
            gpu: gpu::GpuSummary {
                status,
                name: None,
                vendor: gpu::Vendor::Other,
            },
            vram_or_unified_gb: vram,
            ram_gb: ram,
            cpu_features: CpuFeatures { x86, avx2 },
            neural_engine: false,
        }
    }

    #[test]
    fn recommend_table() {
        use GpuApi::{Metal, None as NoGpu, Software, Vulkan};
        // (name, probe, has key, primary, alternative, reason)
        let cases = [
            (
                "no GPU",
                probe(NoGpu, 0.0, 32.0, true, true),
                false,
                GROQ,
                LOCAL,
                Reason::NoGpu,
            ),
            (
                "software Vulkan",
                probe(Software, 0.0, 32.0, true, true),
                false,
                GROQ,
                LOCAL,
                Reason::SoftwareGpu,
            ),
            (
                "Vulkan with 16 GB",
                probe(Vulkan, 8.0, 16.0, true, true),
                false,
                LOCAL,
                GROQ,
                Reason::FitsLocal,
            ),
            (
                "Vulkan with exactly 8 GB",
                probe(Vulkan, 4.0, 8.0, true, true),
                false,
                LOCAL,
                GROQ,
                Reason::FitsLocal,
            ),
            (
                "Vulkan with 4 GB",
                probe(Vulkan, 2.0, 4.0, true, true),
                false,
                GROQ,
                LOCAL,
                Reason::LowMemory,
            ),
            (
                "Metal, 64 GB unified",
                probe(Metal, 64.0, 64.0, false, false),
                false,
                LOCAL,
                GROQ,
                Reason::FitsLocal,
            ),
            (
                "x86 without AVX2",
                probe(Vulkan, 8.0, 32.0, true, false),
                false,
                GROQ,
                LOCAL,
                Reason::NoAvx2,
            ),
            (
                "arm64 needs no AVX2",
                probe(Vulkan, 8.0, 32.0, false, false),
                false,
                LOCAL,
                GROQ,
                Reason::FitsLocal,
            ),
            (
                "a key beats a fitting GPU",
                probe(Vulkan, 8.0, 32.0, true, true),
                true,
                GROQ,
                LOCAL,
                Reason::HasKey,
            ),
            (
                "a key on a machine with no GPU",
                probe(NoGpu, 0.0, 32.0, true, true),
                true,
                GROQ,
                LOCAL,
                Reason::HasKey,
            ),
            (
                "unknown RAM does not rule local out",
                probe(Vulkan, 8.0, 0.0, true, true),
                false,
                LOCAL,
                GROQ,
                Reason::FitsLocal,
            ),
        ];
        let mut needs_driver = probe(NoGpu, 0.0, 32.0, true, true);
        needs_driver.gpu.status = gpu::GpuStatus::NeedsDriver;
        let mut driver_found = probe(Vulkan, 0.0, 16.0, true, true);
        driver_found.gpu.status = gpu::GpuStatus::DriverFound;
        let cases = cases.into_iter().chain([
            (
                "a GPU without a Vulkan driver",
                needs_driver,
                false,
                GROQ,
                LOCAL,
                Reason::NeedsDriver,
            ),
            (
                "a GPU whose driver is installed but ggml did not list",
                driver_found,
                false,
                LOCAL,
                GROQ,
                Reason::FitsLocal,
            ),
        ]);
        for (name, probe, key, primary, alternative, reason) in cases {
            let got = recommend(&probe, key);
            assert_eq!(
                (got.primary, got.alternative, got.reason),
                (primary, alternative, reason),
                "{name}"
            );
        }
    }

    #[test]
    fn the_recommended_model_is_in_the_catalogue() {
        assert_eq!(
            PARAKEET_V3
                .parse::<crate::transcription::models::TranscriptionModelId>()
                .ok(),
            Some(crate::transcription::models::TranscriptionModelId::ParakeetV3)
        );
    }

    fn device(description: &str, kind: &str, gpu: bool, gb: f64) -> RawDevice {
        RawDevice {
            description: description.into(),
            kind: kind.into(),
            gpu,
            memory_bytes: (gb * GB) as u64,
        }
    }

    #[test]
    fn devices_classify_from_what_ggml_reports() {
        // An Asahi laptop: Honeykrisp plus the lavapipe fallback Mesa also lists.
        let asahi = [
            device("Apple M1 Max (G13C C0)", "vulkan", true, 48.0),
            device("llvmpipe (LLVM 23.1.4, 128 bits)", "vulkan", true, 61.0),
        ];
        assert_eq!(classify(&asahi), (GpuApi::Vulkan, 48.0));
        // A Mac.
        let mac = [device("Apple M4 Max", "metal", true, 36.0)];
        assert_eq!(classify(&mac), (GpuApi::Metal, 36.0));
        // Only lavapipe: a software driver counts as no GPU.
        let lavapipe = [device(
            "llvmpipe (LLVM 19.1.7, 256 bits)",
            "vulkan",
            true,
            30.0,
        )];
        assert_eq!(classify(&lavapipe), (GpuApi::Software, 0.0));
        // Only the CPU.
        assert_eq!(
            classify(&[device("CPU", "cpu", false, 0.0)]),
            (GpuApi::None, 0.0)
        );
        assert_eq!(classify(&[]), (GpuApi::None, 0.0));
        // The best of two hardware GPUs.
        let two = [
            device("Intel UHD", "vulkan", true, 2.0),
            device("NVIDIA RTX 4070", "vulkan", true, 12.0),
        ];
        assert_eq!(classify(&two), (GpuApi::Vulkan, 12.0));
    }

    #[test]
    fn software_drivers_are_recognised_by_name() {
        for name in [
            "llvmpipe (LLVM 23.1.4, 128 bits)",
            "Lavapipe",
            "SwiftShader Device (Subzero)",
            "softpipe",
        ] {
            assert!(is_software(name), "{name}");
        }
        for name in [
            "Apple M1 Max (G13C C0)",
            "AMD Radeon RX 7800 XT",
            "Intel(R) Arc(tm) A770",
        ] {
            assert!(!is_software(name), "{name}");
        }
    }

    #[test]
    fn a_ready_neural_engine_is_recommended_ahead_of_the_gpu() {
        use GpuApi::{None as NoGpu, Vulkan};
        const ANE: Choice = Choice::Local(PARAKEET_V3_ANE);
        let with_ane = |mut probe: Probe| {
            probe.neural_engine = true;
            probe
        };
        let ready = recommend(&with_ane(probe(Vulkan, 0.0, 64.0, false, false)), false);
        assert_eq!(ready.primary, ANE);
        assert_eq!(ready.alternative, GROQ);
        assert_eq!(ready.reason, Reason::NeuralEngine);
        // The Neural Engine needs no Vulkan driver of its own.
        let no_gpu = recommend(&with_ane(probe(NoGpu, 0.0, 16.0, false, false)), false);
        assert_eq!(no_gpu.primary, ANE);
        // A key already set and too little memory still win.
        assert_eq!(
            recommend(&with_ane(probe(Vulkan, 0.0, 64.0, false, false)), true).reason,
            Reason::HasKey
        );
        assert_eq!(
            recommend(&with_ane(probe(Vulkan, 0.0, 4.0, false, false)), false).reason,
            Reason::LowMemory
        );
        // Without it, the same machine gets Parakeet v3 on the GPU.
        assert_eq!(
            recommend(&probe(Vulkan, 0.0, 64.0, false, false), false).primary,
            LOCAL
        );
    }

    #[test]
    fn meminfo_parses_the_total() {
        let fixture =
            "MemTotal:       65228676 kB\nMemFree:         1234 kB\nMemAvailable:   3000 kB\n";
        let gb = parse_meminfo(fixture).unwrap();
        assert!((gb - 62.2).abs() < 0.1, "{gb}");
        assert_eq!(parse_meminfo("MemFree: 12 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: lots kB\n"), None);
    }
}
