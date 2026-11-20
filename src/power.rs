//! Power button, for takeover mode. The device is GRABBED so logind doesn't
//! also act on the press: the diary draws its sleep page first, then triggers
//! the suspend itself. If the grab fails we still see the press and draw, and
//! leave the actual suspend to logind.

use std::io;
use std::os::fd::RawFd;

const EV_KEY: u16 = 1;
const KEY_POWER: u16 = 116;
const EVIOCGRAB: libc::c_ulong = 0x40044590;

pub struct PowerButton {
    fd: RawFd,
    pub grabbed: bool,
}

impl PowerButton {
    pub fn open() -> io::Result<Self> {
        for i in 0..8 {
            let name = std::fs::read_to_string(format!("/sys/class/input/event{i}/device/name"))
                .unwrap_or_default()
                .to_lowercase();
            if !name.contains("powerkey") && !name.contains("power button") {
                continue;
            }
            let cpath = std::ffi::CString::new(format!("/dev/input/event{i}")).unwrap();
            let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let grabbed = unsafe { libc::ioctl(fd, EVIOCGRAB, 1i32) } == 0;
            eprintln!("sage: power button /dev/input/event{i} (grabbed: {grabbed})");
            return Ok(Self { fd, grabbed });
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no power button device",
        ))
    }

    /// True if a power-key press (value 1) was seen since the last drain.
    pub fn drain_pressed(&mut self) -> bool {
        let mut pressed = false;
        let mut buf = [0u8; 24 * 16];
        loop {
            let n =
                unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            for chunk in buf[..n as usize].chunks_exact(24) {
                let etype = u16::from_le_bytes(chunk[16..18].try_into().unwrap());
                let code = u16::from_le_bytes(chunk[18..20].try_into().unwrap());
                let value = i32::from_le_bytes(chunk[20..24].try_into().unwrap());
                if etype == EV_KEY && code == KEY_POWER && value == 1 {
                    pressed = true;
                }
            }
        }
        pressed
    }
}

impl Drop for PowerButton {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.fd, EVIOCGRAB, 0i32);
            libc::close(self.fd);
        }
    }
}

/// The kernel's successful-suspend counter — the authoritative "we slept"
/// signal. (Clock heuristics fail here: on this kernel CLOCK_MONOTONIC keeps
/// advancing across deep sleep, verified on-device.)
pub fn suspend_count() -> u64 {
    stat("success")
}

/// The kernel's failed-suspend counter: a suspend that was tried and aborted
/// (e.g. the EPD regulator's discharge timer was still running). Only this
/// justifies trying again; silence does not.
pub fn suspend_fail_count() -> u64 {
    stat("fail")
}

fn stat(name: &str) -> u64 {
    std::fs::read_to_string(format!("/sys/power/suspend_stats/{name}"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// What happened after a suspend was requested.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SleepOutcome {
    /// The tablet slept and has woken (kernel counter, or the waking press).
    Woke,
    /// The kernel tried and gave up: worth another try shortly.
    Aborted,
    /// Nothing at all happened within the patience window.
    NoSign,
}

/// Decide from one poll what the suspend did. `pressed` is a power press seen
/// since the suspend was requested: on a grabbed button it is the press that
/// woke the tablet, and it must never be answered by sleeping again.
pub fn sleep_outcome(success0: u64, fail0: u64, success: u64, fail: u64, pressed: bool) -> Option<SleepOutcome> {
    if success > success0 || pressed {
        Some(SleepOutcome::Woke)
    } else if fail > fail0 {
        Some(SleepOutcome::Aborted)
    } else {
        None
    }
}

/// Is Wi-Fi associated? (wpa_supplicant's view; true when it can't be asked,
/// so a tablet without wpa_cli is never held back.)
pub fn wifi_ready() -> bool {
    match std::process::Command::new("wpa_cli").args(["-i", "wlan0", "status"]).output() {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == "wpa_state=COMPLETED")
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waking_press_is_never_answered_with_sleep() {
        // Counters that never move (as on some kernels) plus the waking press.
        assert_eq!(sleep_outcome(5, 0, 5, 0, true), Some(SleepOutcome::Woke));
        assert_eq!(sleep_outcome(5, 0, 6, 0, false), Some(SleepOutcome::Woke));
        assert_eq!(sleep_outcome(5, 2, 5, 3, false), Some(SleepOutcome::Aborted));
        // Silence is not a reason to suspend again.
        assert_eq!(sleep_outcome(5, 2, 5, 2, false), None);
    }
}

/// After resume, Wi-Fi is often stranded: wpa_supplicant fails a few attempts
/// while the radio settles and marks the network TEMP-DISABLED, and with
/// xochitl stopped nobody clears it. Nudge it back, detached, best-effort.
pub fn wifi_heal() {
    let script = "for i in 1 2 3 4 5 6 7 8 9 10; do \
        state=$(wpa_cli -i wlan0 status 2>/dev/null | grep ^wpa_state | cut -d= -f2); \
        [ \"$state\" = COMPLETED ] && exit 0; \
        wpa_cli -i wlan0 enable_network all >/dev/null 2>&1; \
        wpa_cli -i wlan0 reassociate >/dev/null 2>&1; \
        sleep 3; \
        done";
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
