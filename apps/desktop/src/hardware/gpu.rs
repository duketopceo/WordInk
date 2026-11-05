//! Which GPU this computer has and whether WordInk can transcribe on it.
//!
//! ggml's device list (what [`super::classify`] reads) only says whether
//! ggml's GPU backend started a device. On its own it can't tell "no Vulkan
//! driver" from "a GPU ggml didn't start", and it has no idea what to call the
//! GPU when it lists nothing. So the probe also asks the operating system,
//! cheaply and without creating a Vulkan instance:
//!
//! - the Vulkan loader: `vulkan-1.dll` in the System32 directory on Windows
//!   (NVIDIA, AMD, and Intel drivers install it there), `libvulkan.so.1`
//!   loadable plus at least one ICD manifest on Linux. macOS uses Metal.
//! - the display adapters: the display class in the registry on Windows,
//!   `/sys/class/drm/card*/device` on Linux, the Apple chip on macOS.
//!
//! [`assess`] is the pure decision over what was observed; the readers below
//! it are the only code that touches the system, and tests never call them.

#[cfg(any(target_os = "linux", test))]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "windows", test))]
use std::path::PathBuf;

use super::{GpuApi, RawDevice, is_software};

/// The operating system the observation came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Other,
}

impl Vendor {
    /// The PCI vendor id's vendor.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn from_pci(id: u16) -> Self {
        match id {
            0x10de => Vendor::Nvidia,
            0x1002 | 0x1022 => Vendor::Amd,
            0x8086 => Vendor::Intel,
            0x106b => Vendor::Apple,
            _ => Vendor::Other,
        }
    }

    /// A vendor read from a device name, for names ggml reports.
    pub fn from_name(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        if ["nvidia", "geforce", "quadro", "tesla"]
            .iter()
            .any(|marker| name.contains(marker))
        {
            Vendor::Nvidia
        } else if ["amd", "radeon", "ati "].iter().any(|m| name.contains(m)) {
            Vendor::Amd
        } else if name.contains("intel") {
            Vendor::Intel
        } else if name.contains("apple") {
            Vendor::Apple
        } else {
            Vendor::Other
        }
    }

    pub fn name(self) -> Option<&'static str> {
        match self {
            Vendor::Nvidia => Some("NVIDIA"),
            Vendor::Amd => Some("AMD"),
            Vendor::Intel => Some("Intel"),
            Vendor::Apple => Some("Apple"),
            Vendor::Other => None,
        }
    }
}

/// One display adapter as the operating system lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Adapter {
    /// What the driver calls it; empty when unknown.
    pub name: String,
    pub vendor: Vendor,
    /// A software renderer (Microsoft Basic Render Driver, llvmpipe) or a
    /// display-only device with no GPU behind it.
    pub software: bool,
}

/// Everything [`assess`] decides from.
#[derive(Clone, Debug, PartialEq)]
pub struct Observed {
    pub os: Os,
    /// This build transcribes on a GPU at all: Metal on macOS, Vulkan on
    /// Linux and x64 Windows. Windows on ARM is CPU-only.
    pub gpu_build: bool,
    /// The Vulkan loader is installed (not used on macOS).
    pub loader: bool,
    /// At least one Vulkan driver manifest (ICD) is installed. Linux only;
    /// Windows drivers register theirs in the registry, so it counts as
    /// found there.
    pub icds: bool,
    /// The operating system's display adapters.
    pub adapters: Vec<Adapter>,
    /// The devices ggml's backends started.
    pub ggml: Vec<RawDevice>,
}

/// What onboarding says about the GPU.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuStatus {
    /// ggml runs on a hardware GPU: transcription is accelerated.
    Ready,
    /// A hardware GPU and its Vulkan driver are installed, but ggml did not
    /// list it yet. A model load tries the GPU and falls back to the CPU, so
    /// nothing needs installing.
    DriverFound,
    /// A hardware GPU with no Vulkan loader or driver: the one case where
    /// onboarding asks for a driver install or update.
    NeedsDriver,
    /// This build transcribes on the CPU (Windows on ARM).
    CpuBuild,
    /// Only a software renderer.
    Software,
    /// No GPU at all.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuSummary {
    pub status: GpuStatus,
    /// The GPU's name, when anything reported one.
    pub name: Option<String>,
    pub vendor: Vendor,
}

/// The adapter worth naming: hardware before software, then a discrete
/// vendor (NVIDIA, AMD, other) before Intel's usually integrated one.
fn best_adapter(adapters: &[Adapter]) -> Option<&Adapter> {
    adapters
        .iter()
        .filter(|adapter| !adapter.software)
        .min_by_key(|adapter| match adapter.vendor {
            Vendor::Nvidia => 0,
            Vendor::Amd => 1,
            Vendor::Apple => 2,
            Vendor::Other => 3,
            Vendor::Intel => 4,
        })
}

fn non_empty(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The decision: from what was observed to what onboarding says, and the
/// GPU API and memory (GB) the recommendation uses.
///
/// - ggml started a hardware GPU -> Ready, named by ggml.
/// - A CPU-only build -> CpuBuild, whatever the hardware.
/// - A hardware adapter with the loader (and on Linux an ICD) -> DriverFound,
///   recommended as a Vulkan GPU: the model load tries it.
/// - A hardware adapter without them -> NeedsDriver.
/// - Only software renderers -> Software; nothing -> None.
pub fn assess(observed: &Observed) -> (GpuSummary, GpuApi, f64) {
    let (api, memory) = super::classify(&observed.ggml);
    let adapter = best_adapter(&observed.adapters);
    let summary = |status, name: Option<String>, vendor| GpuSummary {
        status,
        name,
        vendor,
    };
    if matches!(api, GpuApi::Vulkan | GpuApi::Metal) {
        let device = observed
            .ggml
            .iter()
            .filter(|device| device.gpu && !is_software(&device.description))
            .max_by_key(|device| device.memory_bytes);
        let name = device
            .and_then(|device| non_empty(&device.description))
            .or_else(|| adapter.and_then(|adapter| non_empty(&adapter.name)));
        let vendor = match (&name, adapter) {
            (Some(name), _) if Vendor::from_name(name) != Vendor::Other => Vendor::from_name(name),
            (_, Some(adapter)) => adapter.vendor,
            _ if observed.os == Os::Macos => Vendor::Apple,
            _ => Vendor::Other,
        };
        return (summary(GpuStatus::Ready, name, vendor), api, memory);
    }
    if !observed.gpu_build {
        let (name, vendor) = adapter.map_or((None, Vendor::Other), |adapter| {
            (non_empty(&adapter.name), adapter.vendor)
        });
        return (
            summary(GpuStatus::CpuBuild, name, vendor),
            GpuApi::None,
            0.0,
        );
    }
    if let Some(adapter) = adapter {
        let name = non_empty(&adapter.name);
        let driver = match observed.os {
            Os::Linux => observed.loader && observed.icds,
            Os::Windows => observed.loader,
            // macOS always has Metal; ggml not listing it is not a driver
            // the person can install.
            Os::Macos => true,
        };
        return if driver {
            (
                summary(GpuStatus::DriverFound, name, adapter.vendor),
                GpuApi::Vulkan,
                0.0,
            )
        } else {
            (
                summary(GpuStatus::NeedsDriver, name, adapter.vendor),
                GpuApi::None,
                0.0,
            )
        };
    }
    let software = api == GpuApi::Software || observed.adapters.iter().any(|a| a.software);
    if software {
        (
            summary(GpuStatus::Software, None, Vendor::Other),
            GpuApi::Software,
            0.0,
        )
    } else {
        (
            summary(GpuStatus::None, None, Vendor::Other),
            GpuApi::None,
            0.0,
        )
    }
}

// ---------------------------------------------------------------------------
// Parsing, pure

/// A sysfs `vendor` file's id, such as `0x10de\n`.
#[cfg(any(target_os = "linux", test))]
pub fn parse_pci_id(text: &str) -> Option<u16> {
    let text = text.trim();
    let hex = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u16::from_str_radix(hex, 16).ok()
}

/// The vendor in a Windows `MatchingDeviceId`, such as
/// `pci\ven_10de&dev_28e0&subsys_...`.
#[cfg(any(target_os = "windows", test))]
pub fn vendor_from_device_id(device_id: &str) -> Vendor {
    let lower = device_id.to_ascii_lowercase();
    lower
        .split_once("ven_")
        .and_then(|(_, rest)| rest.get(..4))
        .and_then(|id| u16::from_str_radix(id, 16).ok())
        .map_or(Vendor::Other, Vendor::from_pci)
}

/// A Windows display adapter that draws in software or only scans out:
/// Microsoft Basic Render Driver / Basic Display Adapter, remote display,
/// and virtual display drivers.
#[cfg(any(target_os = "windows", test))]
pub fn windows_software_adapter(name: &str, device_id: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let id = device_id.to_ascii_lowercase();
    id.starts_with("root\\")
        || id.is_empty()
        || name.contains("basic render")
        || name.contains("basic display")
        || name.contains("remote display")
        || is_software(&name)
}

/// Linux DRM drivers with no GPU behind them (scan-out, virtual, software).
#[cfg(any(target_os = "linux", test))]
pub fn linux_software_driver(driver: &str) -> bool {
    matches!(
        driver,
        "simpledrm"
            | "simple-framebuffer"
            | "apple-drm"
            | "vkms"
            | "vgem"
            | "efifb"
            | "bochs"
            | "cirrus"
    )
}

/// The Linux display adapter's name: its vendor's, since the kernel has no
/// marketing name (ggml supplies the real one once it starts the GPU).
#[cfg(any(target_os = "linux", test))]
pub fn linux_adapter(vendor_id: Option<u16>, driver: &str) -> Adapter {
    let vendor = match vendor_id {
        Some(id) => Vendor::from_pci(id),
        // Platform GPUs have no PCI ids: Asahi's is Apple's.
        None if driver == "asahi" => Vendor::Apple,
        None => Vendor::Other,
    };
    let name = match vendor.name() {
        Some(vendor) => format!("{vendor} GPU"),
        None => String::new(),
    };
    Adapter {
        name,
        vendor,
        software: linux_software_driver(driver),
    }
}

/// Where the Vulkan loader looks for driver manifests on Linux, in its
/// order. `VK_DRIVER_FILES` (or the older `VK_ICD_FILENAMES`) replaces the
/// search with its own files and directories; `VK_ADD_DRIVER_FILES` adds to
/// it. `env` reads one variable.
#[cfg(any(target_os = "linux", test))]
pub fn icd_search_paths(env: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let list = |value: String| -> Vec<PathBuf> {
        value
            .split(':')
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect()
    };
    let set = |name: &str| env(name).filter(|value| !value.is_empty());
    let mut paths = set("VK_ADD_DRIVER_FILES").map(list).unwrap_or_default();
    if let Some(files) = set("VK_DRIVER_FILES").or_else(|| set("VK_ICD_FILENAMES")) {
        paths.extend(list(files));
        return paths;
    }
    let config = set("XDG_CONFIG_DIRS").unwrap_or_else(|| "/etc/xdg".into());
    paths.extend(list(config).into_iter().map(|dir| dir.join("vulkan/icd.d")));
    paths.push(PathBuf::from("/etc/vulkan/icd.d"));
    let data_home = set("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    if let Some(data_home) = data_home {
        paths.push(data_home.join("vulkan/icd.d"));
    }
    let data = set("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    paths.extend(list(data).into_iter().map(|dir| dir.join("vulkan/icd.d")));
    // The packaged locations, even when XDG_DATA_DIRS leaves them out.
    for dir in ["/usr/local/share/vulkan/icd.d", "/usr/share/vulkan/icd.d"] {
        let dir = PathBuf::from(dir);
        if !paths.contains(&dir) {
            paths.push(dir);
        }
    }
    paths
}

/// Whether `path` is a manifest or a directory holding one.
#[cfg(any(target_os = "linux", test))]
fn has_manifest(path: &Path) -> bool {
    let is_json = |path: &Path| path.extension().is_some_and(|ext| ext == "json");
    if path.is_file() {
        return is_json(path);
    }
    std::fs::read_dir(path).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| is_json(&entry.path()) && entry.path().is_file())
    })
}

// ---------------------------------------------------------------------------
// Reading the system

/// Whether any Vulkan driver manifest is installed (Linux).
#[cfg(target_os = "linux")]
pub fn icds_installed() -> bool {
    icd_search_paths(|name| std::env::var(name).ok())
        .iter()
        .any(|path| has_manifest(path))
}

/// Whether the Vulkan loader is installed.
#[cfg(target_os = "linux")]
pub fn loader_present() -> bool {
    // dlopen, then close: the same lookup ggml's and wgpu's loads make.
    let name = c"libvulkan.so.1";
    // SAFETY: a NUL-terminated name; the handle is closed before returning.
    unsafe {
        let handle = libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if handle.is_null() {
            return false;
        }
        libc::dlclose(handle);
    }
    true
}

/// Whether the Vulkan loader is installed: `vulkan-1.dll` in the System32
/// directory, where GPU drivers install it, or loadable from System32.
#[cfg(target_os = "windows")]
pub fn loader_present() -> bool {
    system_directory().is_some_and(|dir| dir.join("vulkan-1.dll").is_file()) || loads_vulkan_1()
}

#[cfg(target_os = "windows")]
fn system_directory() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = [0u16; 520];
    // SAFETY: the buffer and its length in UTF-16 units.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    (length > 0 && length < buffer.len())
        .then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])))
}

/// A second signal: System32's `vulkan-1.dll` loads.
#[cfg(target_os = "windows")]
fn loads_vulkan_1() -> bool {
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
    let name: Vec<u16> = "vulkan-1.dll\0".encode_utf16().collect();
    // SAFETY: a NUL-terminated name; the module is released at once.
    unsafe {
        let module = LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if module.is_null() {
            return false;
        }
        FreeLibrary(module);
    }
    true
}

/// The display adapters, from `/sys/class/drm/card*/device`.
#[cfg(target_os = "linux")]
pub fn adapters() -> Vec<Adapter> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut adapters = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // `card0`, not its connectors (`card0-DP-1`).
        let Some(index) = name.strip_prefix("card") else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let device = entry.path().join("device");
        let vendor = std::fs::read_to_string(device.join("vendor"))
            .ok()
            .and_then(|text| parse_pci_id(&text));
        let driver = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        adapters.push(linux_adapter(vendor, &driver));
    }
    adapters
}

/// The display adapters, from the display class in the registry
/// (`DriverDesc` and `MatchingDeviceId` of each installed adapter driver),
/// readable without elevation and without DXGI.
#[cfg(target_os = "windows")]
pub fn adapters() -> Vec<Adapter> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    const DISPLAY_CLASS: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let Ok(class) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(DISPLAY_CLASS, KEY_READ)
    else {
        return Vec::new();
    };
    class
        .enum_keys()
        .flatten()
        .filter(|key| key.len() == 4 && key.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|key| class.open_subkey_with_flags(key, KEY_READ).ok())
        .filter_map(|driver| {
            let name: String = driver.get_value("DriverDesc").ok()?;
            let device_id: String = driver.get_value("MatchingDeviceId").unwrap_or_default();
            Some(Adapter {
                vendor: match vendor_from_device_id(&device_id) {
                    Vendor::Other => Vendor::from_name(&name),
                    vendor => vendor,
                },
                software: windows_software_adapter(&name, &device_id),
                name,
            })
        })
        .collect()
}

/// The Apple chip, such as "Apple M4 Max".
#[cfg(target_os = "macos")]
pub fn adapters() -> Vec<Adapter> {
    let name = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    vec![Adapter {
        name,
        vendor: Vendor::Apple,
        software: false,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn ggml(description: &str, kind: &str, gb: u64) -> RawDevice {
        RawDevice {
            description: description.into(),
            kind: kind.into(),
            gpu: true,
            memory_bytes: gb * GB,
        }
    }

    fn adapter(name: &str, vendor: Vendor, software: bool) -> Adapter {
        Adapter {
            name: name.into(),
            vendor,
            software,
        }
    }

    fn observed(os: Os, loader: bool, icds: bool, adapters: Vec<Adapter>) -> Observed {
        Observed {
            os,
            gpu_build: true,
            loader,
            icds,
            adapters,
            ggml: Vec::new(),
        }
    }

    fn rtx_laptop() -> Vec<Adapter> {
        vec![
            adapter("Intel(R) UHD Graphics", Vendor::Intel, false),
            adapter("NVIDIA GeForce RTX 4060 Laptop GPU", Vendor::Nvidia, false),
            adapter("Microsoft Basic Render Driver", Vendor::Other, true),
        ]
    }

    #[test]
    fn a_gpu_ggml_runs_on_is_ready_and_named_by_ggml() {
        let mut seen = observed(Os::Windows, true, true, rtx_laptop());
        seen.ggml = vec![
            ggml("Intel(R) UHD Graphics", "vulkan", 1),
            ggml("NVIDIA GeForce RTX 4060 Laptop GPU", "vulkan", 8),
        ];
        let (summary, api, memory) = assess(&seen);
        assert_eq!(summary.status, GpuStatus::Ready);
        assert_eq!(
            summary.name.as_deref(),
            Some("NVIDIA GeForce RTX 4060 Laptop GPU")
        );
        assert_eq!(summary.vendor, Vendor::Nvidia);
        assert_eq!((api, memory), (GpuApi::Vulkan, 8.0));
    }

    #[test]
    fn an_nvidia_laptop_with_its_loader_never_needs_vulkan_installed() {
        // ggml listed nothing (it had not started its backend, or a guard
        // kept it on the CPU), but the driver's loader is in System32.
        let (summary, api, _) = assess(&observed(Os::Windows, true, false, rtx_laptop()));
        assert_eq!(summary.status, GpuStatus::DriverFound);
        assert_eq!(
            summary.name.as_deref(),
            Some("NVIDIA GeForce RTX 4060 Laptop GPU")
        );
        assert_eq!(api, GpuApi::Vulkan);
    }

    #[test]
    fn only_a_missing_loader_or_icd_asks_for_a_driver() {
        let (windows, api, _) = assess(&observed(Os::Windows, false, false, rtx_laptop()));
        assert_eq!(
            (windows.status, api),
            (GpuStatus::NeedsDriver, GpuApi::None)
        );
        let amd = vec![adapter("AMD GPU", Vendor::Amd, false)];
        let (no_icd, _, _) = assess(&observed(Os::Linux, true, false, amd.clone()));
        assert_eq!(no_icd.status, GpuStatus::NeedsDriver);
        let (no_loader, _, _) = assess(&observed(Os::Linux, false, true, amd.clone()));
        assert_eq!(no_loader.status, GpuStatus::NeedsDriver);
        let (both, api, _) = assess(&observed(Os::Linux, true, true, amd));
        assert_eq!((both.status, api), (GpuStatus::DriverFound, GpuApi::Vulkan));
    }

    #[test]
    fn software_and_missing_adapters_fall_back_to_the_cpu() {
        let basic = vec![adapter(
            "Microsoft Basic Render Driver",
            Vendor::Other,
            true,
        )];
        let (summary, api, _) = assess(&observed(Os::Windows, true, true, basic));
        assert_eq!(
            (summary.status, api),
            (GpuStatus::Software, GpuApi::Software)
        );
        let mut lavapipe = observed(Os::Linux, true, true, Vec::new());
        lavapipe.ggml = vec![ggml("llvmpipe (LLVM 19.1.7, 256 bits)", "vulkan", 30)];
        assert_eq!(assess(&lavapipe).0.status, GpuStatus::Software);
        let (nothing, api, _) = assess(&observed(Os::Linux, false, false, Vec::new()));
        assert_eq!((nothing.status, api), (GpuStatus::None, GpuApi::None));
        assert_eq!(nothing.name, None);
    }

    #[test]
    fn a_cpu_only_build_says_so_whatever_the_gpu() {
        let mut arm = observed(Os::Windows, true, true, rtx_laptop());
        arm.gpu_build = false;
        let (summary, api, _) = assess(&arm);
        assert_eq!((summary.status, api), (GpuStatus::CpuBuild, GpuApi::None));
    }

    #[test]
    fn a_mac_is_ready_on_metal_with_its_chip_name() {
        let mut mac = observed(
            Os::Macos,
            false,
            false,
            vec![adapter("Apple M4 Max", Vendor::Apple, false)],
        );
        mac.ggml = vec![ggml("Apple GPU", "metal", 0)];
        let (summary, api, _) = assess(&mac);
        assert_eq!((summary.status, api), (GpuStatus::Ready, GpuApi::Metal));
        assert_eq!(summary.name.as_deref(), Some("Apple GPU"));
        assert_eq!(summary.vendor, Vendor::Apple);
        // Without a ggml description the chip names it.
        mac.ggml = vec![ggml("", "metal", 0)];
        assert_eq!(assess(&mac).0.name.as_deref(), Some("Apple M4 Max"));
    }

    #[test]
    fn an_unnamed_gpu_still_reports_its_status() {
        let mut seen = observed(Os::Linux, true, true, Vec::new());
        seen.ggml = vec![ggml("", "vulkan", 4)];
        let (summary, _, _) = assess(&seen);
        assert_eq!(summary.status, GpuStatus::Ready);
        assert_eq!(summary.name, None);
    }

    #[test]
    fn vendors_come_from_pci_ids_and_names() {
        assert_eq!(parse_pci_id("0x10de\n"), Some(0x10de));
        assert_eq!(parse_pci_id("1002"), Some(0x1002));
        assert_eq!(parse_pci_id("nope"), None);
        assert_eq!(Vendor::from_pci(0x10de), Vendor::Nvidia);
        assert_eq!(Vendor::from_pci(0x1002), Vendor::Amd);
        assert_eq!(Vendor::from_pci(0x8086), Vendor::Intel);
        assert_eq!(Vendor::from_pci(0x1af4), Vendor::Other);
        assert_eq!(
            vendor_from_device_id(r"pci\ven_10de&dev_28e0&subsys_3c8017aa"),
            Vendor::Nvidia
        );
        assert_eq!(
            vendor_from_device_id(r"PCI\VEN_8086&DEV_A7A0"),
            Vendor::Intel
        );
        assert_eq!(vendor_from_device_id(r"root\basicrender"), Vendor::Other);
        assert_eq!(
            Vendor::from_name("NVIDIA GeForce RTX 4060 Laptop GPU"),
            Vendor::Nvidia
        );
        assert_eq!(Vendor::from_name("AMD Radeon 8060S"), Vendor::Amd);
        assert_eq!(Vendor::from_name("Apple M1 Max (G13C C0)"), Vendor::Apple);
    }

    #[test]
    fn software_adapters_are_skipped() {
        assert!(windows_software_adapter(
            "Microsoft Basic Render Driver",
            r"root\basicrender"
        ));
        assert!(windows_software_adapter(
            "Microsoft Basic Display Adapter",
            ""
        ));
        assert!(!windows_software_adapter(
            "NVIDIA GeForce RTX 4060 Laptop GPU",
            r"pci\ven_10de&dev_28e0"
        ));
        assert!(linux_adapter(None, "simpledrm").software);
        let nvidia = linux_adapter(Some(0x10de), "nvidia");
        assert_eq!(
            (nvidia.name.as_str(), nvidia.vendor, nvidia.software),
            ("NVIDIA GPU", Vendor::Nvidia, false)
        );
        assert_eq!(linux_adapter(None, "asahi").vendor, Vendor::Apple);
    }

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn icd_search_follows_the_loader() {
        let defaults = icd_search_paths(env(&[("HOME", "/home/alex")]));
        for expected in [
            "/etc/xdg/vulkan/icd.d",
            "/etc/vulkan/icd.d",
            "/home/alex/.local/share/vulkan/icd.d",
            "/usr/local/share/vulkan/icd.d",
            "/usr/share/vulkan/icd.d",
        ] {
            assert!(
                defaults.contains(&PathBuf::from(expected)),
                "{expected} in {defaults:?}"
            );
        }
        let nix = icd_search_paths(env(&[("XDG_DATA_DIRS", "/run/opengl-driver/share")]));
        assert!(nix.contains(&PathBuf::from("/run/opengl-driver/share/vulkan/icd.d")));
        assert!(nix.contains(&PathBuf::from("/usr/share/vulkan/icd.d")));
        // VK_DRIVER_FILES replaces the search; VK_ICD_FILENAMES is its old name.
        let only = icd_search_paths(env(&[("VK_DRIVER_FILES", "/opt/a.json:/opt/icd.d")]));
        assert_eq!(
            only,
            vec![PathBuf::from("/opt/a.json"), PathBuf::from("/opt/icd.d")]
        );
        let old = icd_search_paths(env(&[("VK_ICD_FILENAMES", "/opt/lvp.json")]));
        assert_eq!(old, vec![PathBuf::from("/opt/lvp.json")]);
        let added = icd_search_paths(env(&[("VK_ADD_DRIVER_FILES", "/opt/extra.json")]));
        assert_eq!(added[0], PathBuf::from("/opt/extra.json"));
        assert!(added.contains(&PathBuf::from("/usr/share/vulkan/icd.d")));
    }

    #[test]
    fn manifests_are_found_in_files_and_directories() {
        let dir = std::env::temp_dir().join(format!("wordink-icd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!has_manifest(&dir));
        let manifest = dir.join("nvidia_icd.json");
        std::fs::write(&manifest, "{}").unwrap();
        assert!(has_manifest(&dir));
        assert!(has_manifest(&manifest));
        assert!(!has_manifest(&dir.join("missing.json")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
