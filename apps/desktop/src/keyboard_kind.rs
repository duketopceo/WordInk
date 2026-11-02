//! Which kind of keyboard the person types on, so the Option/Alt key is
//! labelled the way its keycap reads: "Option" on Apple keyboards, "Alt"
//! everywhere else (WordInk's default dictation shortcut is that key plus
//! Space). macOS and Windows answer from the OS; Linux reads the input
//! devices the kernel lists in `/proc/bus/input/devices`.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyboardKind {
    Apple,
    Pc,
}

impl KeyboardKind {
    /// What the key beside Command (or Windows) is called.
    pub const fn option_label(self) -> &'static str {
        match self {
            Self::Apple => "Option",
            Self::Pc => "Alt",
        }
    }
}

/// The kind of keyboard attached, by the kernel's device list: Apple when any
/// real keyboard is Apple's (USB vendor 05ac, or "Apple" in its name), PC
/// otherwise. A real keyboard has a `kbd` handler and key repeat
/// (`EV_REP`), which the power button, lid switch and audio jacks lack.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn from_proc_input_devices(devices: &str) -> KeyboardKind {
    const APPLE_VENDOR: &str = "Vendor=05ac";
    // Bit 20 of the EV bitmask: the device repeats keys.
    const EV_REP: u64 = 1 << 20;
    let apple = devices.split("\n\n").any(|block| {
        let line = |prefix: &str| {
            block
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_default()
        };
        let keyboard = line("H: Handlers=").split_whitespace().any(|h| h == "kbd")
            && u64::from_str_radix(line("B: EV=").trim(), 16).is_ok_and(|ev| ev & EV_REP != 0);
        let apple = line("I: ").contains(APPLE_VENDOR)
            || line("N: Name=").to_ascii_lowercase().contains("apple");
        keyboard && apple
    });
    if apple {
        KeyboardKind::Apple
    } else {
        KeyboardKind::Pc
    }
}

/// The detected kind, read once. macOS is Apple, Windows is PC, and Linux
/// asks the kernel; an unreadable list falls back to PC.
pub fn detected() -> KeyboardKind {
    static DETECTED: std::sync::OnceLock<KeyboardKind> = std::sync::OnceLock::new();
    *DETECTED.get_or_init(detect)
}

#[cfg(all(target_os = "macos", not(test)))]
fn detect() -> KeyboardKind {
    KeyboardKind::Apple
}

/// Tests never read the host's devices, so a result doesn't depend on the
/// keyboard of the machine running them.
#[cfg(test)]
fn detect() -> KeyboardKind {
    KeyboardKind::Pc
}

#[cfg(all(target_os = "linux", not(test)))]
fn detect() -> KeyboardKind {
    std::fs::read_to_string("/proc/bus/input/devices").map_or(KeyboardKind::Pc, |devices| {
        from_proc_input_devices(&devices)
    })
}

#[cfg(all(not(any(target_os = "macos", target_os = "linux")), not(test)))]
fn detect() -> KeyboardKind {
    KeyboardKind::Pc
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASAHI: &str = "I: Bus=0000 Vendor=0000 Product=0000 Version=0000
N: Name=\"Apple SMC power/lid events\"
H: Handlers=kbd event0 
B: EV=23

I: Bus=001c Vendor=05ac Product=0343 Version=0352
N: Name=\"Apple SPI Trackpad\"
H: Handlers=mouse0 event1 
B: EV=1b

I: Bus=001c Vendor=05ac Product=0343 Version=0352
N: Name=\"Apple SPI Keyboard\"
H: Handlers=sysrq kbd leds event2 
B: EV=120013
";

    const PC: &str = "I: Bus=0019 Vendor=0000 Product=0001 Version=0000
N: Name=\"Power Button\"
H: Handlers=kbd event0 
B: EV=3

I: Bus=0003 Vendor=046d Product=c31c Version=0110
N: Name=\"Logitech USB Keyboard\"
H: Handlers=sysrq kbd leds event3 
B: EV=120013

I: Bus=0003 Vendor=05ac Product=030d Version=0300
N: Name=\"Apple Magic Trackpad\"
H: Handlers=mouse1 event4 
B: EV=1b
";

    #[test]
    fn an_apple_keyboard_is_found_by_vendor_or_name() {
        assert_eq!(from_proc_input_devices(ASAHI), KeyboardKind::Apple);
        let by_vendor = "I: Bus=0003 Vendor=05ac Product=0250 Version=0111
N: Name=\"Keychron K2\"
H: Handlers=sysrq kbd leds event5 
B: EV=120013
";
        assert_eq!(from_proc_input_devices(by_vendor), KeyboardKind::Apple);
        let by_name = by_vendor
            .replace("05ac", "1234")
            .replace("Keychron K2", "Apple Internal Keyboard");
        assert_eq!(from_proc_input_devices(&by_name), KeyboardKind::Apple);
    }

    #[test]
    fn a_pc_keyboard_or_an_apple_trackpad_alone_is_a_pc() {
        assert_eq!(from_proc_input_devices(PC), KeyboardKind::Pc);
        assert_eq!(from_proc_input_devices(""), KeyboardKind::Pc);
    }

    #[test]
    fn the_label_follows_the_keyboard() {
        assert_eq!(KeyboardKind::Apple.option_label(), "Option");
        assert_eq!(KeyboardKind::Pc.option_label(), "Alt");
    }
}
