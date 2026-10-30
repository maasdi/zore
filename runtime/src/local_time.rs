//! The local time zone's offset from UTC, as the operating system reports it.

/// Seconds east of UTC at the Unix time `seconds`, or zero when the system cannot say.
#[unsafe(no_mangle)]
pub extern "C" fn zore_native_time_local_offset(seconds: i64) -> i64 {
    offset_at(seconds).unwrap_or(0)
}

#[cfg(unix)]
fn offset_at(seconds: i64) -> Option<i64> {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: std::ffi::c_long,
        zone: *const std::ffi::c_char,
    }
    unsafe extern "C" {
        fn tzset();
        fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm {
        sec: 0,
        min: 0,
        hour: 0,
        mday: 0,
        mon: 0,
        year: 0,
        wday: 0,
        yday: 0,
        isdst: 0,
        gmtoff: 0,
        zone: std::ptr::null(),
    };
    // SAFETY: both pointers are valid for the call; time_t is 64 bits on supported hosts.
    let converted = unsafe {
        tzset();
        localtime_r(&seconds, &mut tm)
    };
    (!converted.is_null()).then_some(tm.gmtoff)
}

#[cfg(windows)]
fn offset_at(seconds: i64) -> Option<i64> {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToSystemTime(file_time: *const [u32; 2], system_time: *mut SystemTime) -> i32;
        fn SystemTimeToTzSpecificLocalTime(
            zone: *const std::ffi::c_void,
            universal: *const SystemTime,
            local: *mut SystemTime,
        ) -> i32;
        fn SystemTimeToFileTime(system_time: *const SystemTime, file_time: *mut [u32; 2]) -> i32;
    }
    const TICKS: i64 = 10_000_000;
    const EPOCH_GAP: i64 = 11_644_473_600;
    let ticks = seconds.checked_add(EPOCH_GAP)?.checked_mul(TICKS)?;
    if ticks < 0 {
        return None;
    }
    let universal_file = [ticks as u32, (ticks >> 32) as u32];
    let (mut universal, mut local) = (SystemTime::default(), SystemTime::default());
    let mut local_file = [0u32; 2];
    // SAFETY: every pointer refers to a live local of the declared layout.
    let ok = unsafe {
        FileTimeToSystemTime(&universal_file, &mut universal) != 0
            && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &universal, &mut local) != 0
            && SystemTimeToFileTime(&local, &mut local_file) != 0
    };
    if !ok {
        return None;
    }
    let local_ticks = (i64::from(local_file[1]) << 32) | i64::from(local_file[0]);
    Some((local_ticks - ticks).div_euclid(TICKS))
}
