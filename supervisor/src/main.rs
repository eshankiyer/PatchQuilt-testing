use std::env;
use std::process::{Command, ExitCode, Stdio};

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("patchquilt-supervisor requires Linux ptrace support");
    ExitCode::from(2)
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let Some(program) = arguments.next() else {
        eprintln!("usage: patchquilt-supervisor <program> [args ...]");
        return ExitCode::from(2);
    };
    let command_arguments: Vec<String> = arguments.collect();
    match supervise(&program, &command_arguments) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("patchquilt-supervisor: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(target_os = "linux")]
fn supervise(program: &str, arguments: &[String]) -> Result<ExitCode, String> {
    let child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("failed to start {program}: {error}"))?;
    let pid =
        libc::pid_t::try_from(child.id()).map_err(|_| "child pid is out of range".to_string())?;
    let status = trace_child(pid)?;
    drop(child);
    Ok(match status {
        status if libc::WIFEXITED(status) => {
            ExitCode::from(u8::try_from(libc::WEXITSTATUS(status)).map_or(1, |code| code))
        }
        _ => ExitCode::from(1),
    })
}

#[cfg(target_os = "linux")]
fn trace_child(pid: libc::pid_t) -> Result<libc::c_int, String> {
    let seize_result = unsafe {
        libc::ptrace(
            libc::PTRACE_SEIZE,
            pid,
            0,
            libc::PTRACE_O_EXITKILL | libc::PTRACE_O_TRACESYSGOOD,
        )
    };
    if seize_result == -1 {
        return Err(last_error("PTRACE_SEIZE"));
    }
    let interrupt_result = unsafe { libc::ptrace(libc::PTRACE_INTERRUPT, pid, 0, 0) };
    if interrupt_result == -1 {
        return Err(last_error("PTRACE_INTERRUPT"));
    }
    wait_for_stop(pid)?;
    let continue_result = unsafe { libc::ptrace(libc::PTRACE_CONT, pid, 0, 0) };
    if continue_result == -1 {
        return Err(last_error("PTRACE_CONT"));
    }
    loop {
        let mut status = 0;
        let waited = unsafe { libc::waitpid(pid, &raw mut status, libc::__WALL) };
        if waited == -1 {
            return Err(last_error("waitpid"));
        }
        if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
            return Ok(status);
        }
        if libc::WIFSTOPPED(status) {
            let continue_result = unsafe { libc::ptrace(libc::PTRACE_CONT, pid, 0, 0) };
            if continue_result == -1 {
                return Err(last_error("PTRACE_CONT"));
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn wait_for_stop(pid: libc::pid_t) -> Result<(), String> {
    let mut status = 0;
    let waited = unsafe { libc::waitpid(pid, &raw mut status, libc::WUNTRACED) };
    if waited == -1 {
        return Err(last_error("waitpid"));
    }
    if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
        return Err("child exited before ptrace could be resumed".to_string());
    }
    if !libc::WIFSTOPPED(status) {
        return Err("child did not enter a ptrace stop".to_string());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn last_error(operation: &str) -> String {
    let error = std::io::Error::last_os_error();
    format!("{operation} failed: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn supervises_a_short_lived_child() {
        let status = supervise("true", &[]).expect("true should run");
        assert_eq!(status, ExitCode::SUCCESS);
    }
}
