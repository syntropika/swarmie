//! Same-user named-pipe WebSockets for Windows runtime attachment and byte proxies.
use std::{io, path::Path, time::Duration};
use swarmie::Result;
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(crate) fn user_sid() -> io::Result<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut length = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
            if length == 0 || length > 64 * 1024 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut text = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut count = 0;
            while *text.add(count) != 0 {
                count += 1;
            }
            let result = String::from_utf16(std::slice::from_raw_parts(text, count))
                .map_err(|_| io::Error::other("invalid user SID"));
            LocalFree(text.cast());
            result
        })();
        CloseHandle(token);
        result
    }
}
fn server(path: &Path, first: bool) -> io::Result<NamedPipeServer> {
    // Protect the pipe independently of HTTP bearer authentication. Reject network clients.
    let descriptor = format!("D:P(A;;GA;;;{})", user_sid()?);
    let descriptor: Vec<u16> = descriptor.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut security = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut security,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                path,
                (&attributes as *const SECURITY_ATTRIBUTES)
                    .cast_mut()
                    .cast(),
            );
        LocalFree(security);
        result
    }
}
pub(crate) async fn connect(path: &Path) -> io::Result<NamedPipeClient> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match ClientOptions::new().open(path) {
                Ok(client) => return Ok(client),
                Err(error) if error.raw_os_error() == Some(231) => {
                    tokio::time::sleep(Duration::from_millis(20)).await
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "local named pipe is busy"))?
}
pub(crate) async fn listen(
    service: protocol::RuntimeService,
    path: std::path::PathBuf,
) -> Result<crate::transport::LocalSocket> {
    let mut listener = server(&path, true)?;
    let task = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = service.wait_for_shutdown() => break,
                accepted = listener.connect() => {
                    if accepted.is_err() { break; }
                    let next = match server(&path, false) { Ok(next) => next, Err(_) => break };
                    let stream = std::mem::replace(&mut listener, next);
                    let service = service.clone();
                    clients.spawn(async move { let _ = crate::transport::connection(stream, service, None).await; });
                }
                Some(_) = clients.join_next(), if !clients.is_empty() => (),
            }
        }
        drop(listener);
        while clients.join_next().await.is_some() {}
    });
    Ok(crate::transport::LocalSocket { task: Some(task) })
}
