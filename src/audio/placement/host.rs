//! Reading and writing the calling thread's affinity mask, one branch per host.
//!
//! Two calls and a size, and no decision of its own: which core is reserved,
//! what an error number meant and what is put back afterwards are in
//! [`placement`](super), where a test reaches them on every host. A sweep
//! compiles one branch of this file, so the others are missed by construction
//! rather than for want of a test — which is why it is what the mutation sweep
//! excludes, and why nothing that decides a [`Grant`](super::Grant) is here.

#[cfg(target_os = "linux")]
const NAMEABLE_CORES: usize = libc::CPU_SETSIZE as usize;

#[cfg(target_os = "linux")]
pub fn owned_cores() -> Vec<usize> {
    /* SAFETY: cpu_set_t is an array of unsigned integers, for which every bit
    pattern is valid, and all zero is the empty set CPU_ZERO writes. */
    let mut mask: libc::cpu_set_t = unsafe { std::mem::zeroed() };

    /* SAFETY: sched_getaffinity fills the mask behind the pointer and returns
    zero, or returns non-zero having written nothing. The pointer is to local
    storage that outlives the call, and a pid of zero is the calling thread. */
    let outcome =
        unsafe { libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut mask) };
    if outcome != 0 {
        return Vec::new();
    }

    (0..NAMEABLE_CORES)
        /* SAFETY: CPU_ISSET reads the bit for the core it is given, and the
        range above stops at the last bit the mask has. */
        .filter(|&core| unsafe { libc::CPU_ISSET(core, &mask) })
        .collect()
}

#[cfg(not(target_os = "linux"))]
pub fn owned_cores() -> Vec<usize> {
    Vec::new()
}

#[cfg(target_os = "linux")]
pub fn pin_to_cores(cores: &[usize]) -> Result<(), i32> {
    /* SAFETY: as the zeroed mask above. */
    let mut wanted: libc::cpu_set_t = unsafe { std::mem::zeroed() };

    for &core in cores.iter().filter(|&&core| core < NAMEABLE_CORES) {
        /* SAFETY: CPU_SET indexes the mask by the core it is given, and the
        filter above leaves only a core the mask has a bit for. */
        unsafe { libc::CPU_SET(core, &mut wanted) };
    }

    /* SAFETY: sched_setaffinity reads the mask through the pointer and does not
    keep it, and the reference guarantees it is valid for the call and the size
    given. A pid of zero is the calling thread. */
    let outcome =
        unsafe { libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &wanted) };
    if outcome != 0 {
        return Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EINVAL));
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn pin_to_cores(_cores: &[usize]) -> Result<(), i32> {
    Err(libc::ENOTSUP)
}
