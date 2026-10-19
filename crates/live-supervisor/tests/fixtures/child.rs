use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let mode = &arguments[1];
    if mode == "supervisor" {
        use autodj_live_supervisor::{
            ChildCommand, LaunchConfig, LiveSupervisor, Protection, RuntimeStore,
        };
        let root = std::path::PathBuf::from(&arguments[2]);
        let config = LaunchConfig {
            engine: ChildCommand::new(arguments[3].clone().into()),
            sidecar: ChildCommand::new(std::env::current_exe().unwrap()).args([
                "sidecar".into(),
                root.with_extension("sidecar-pid").into_os_string(),
            ]),
            runtime: RuntimeStore::new(root.clone(), Protection::Installed),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let session = LiveSupervisor::launch(config).await.unwrap();
            let identity = session.identity();
            std::fs::write(
                root.with_extension("ready"),
                format!("{} {}", identity.pid, identity.sidecar_pid),
            )
            .unwrap();
            std::future::pending::<()>().await;
        });
        return;
    }
    std::fs::write(&arguments[2], std::process::id().to_string()).unwrap();
    let token = std::env::var("AUTODJ_LIVE_TOKEN").unwrap();
    let session = std::env::var("AUTODJ_LIVE_SESSION_ID").unwrap();
    assert_eq!(token.len(), 64);
    uuid::Uuid::parse_str(&session).unwrap();
    assert!(
        arguments
            .iter()
            .all(|arg| !arg.contains(&token) && !arg.contains(&session))
    );
    match mode.as_str() {
        "malformed" => println!("secret={token} session={session}"),
        "noisy" => println!("booting\n{{}}"),
        "no-descriptor" => return,
        "oversized" => {
            print!("{}", "x".repeat(8193));
            std::io::stdout().flush().unwrap();
        }
        "timeout" => {}
        "extra-line" => println!(
            "{{\"protocol\":{{\"major\":1,\"minor\":0}},\"port\":1,\"pid\":{},\"engine_id\":\"{}\"}}\nnoise",
            std::process::id(),
            uuid::Uuid::new_v4()
        ),
        "sidecar" | "stderr-flood" | "exact-limit" | "extra-field" => {
            if mode == "stderr-flood" {
                std::thread::spawn(|| {
                    let block = [b'e'; 8192];
                    for _ in 0..1024 {
                        let _ = std::io::stderr().write_all(&block);
                    }
                });
            }
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let id = uuid::Uuid::new_v4();
            let mut descriptor = format!(
                "{{\"protocol\":{{\"major\":1,\"minor\":0}},\"port\":{},\"pid\":{},\"sidecar_id\":\"{id}\"}}",
                listener.local_addr().unwrap().port(),
                std::process::id()
            );
            if mode == "exact-limit" {
                descriptor.push_str(&" ".repeat(8191 - descriptor.len()));
            }
            if mode == "extra-field" {
                descriptor.pop();
                descriptor.push_str(",\"engine_id\":null}");
            }
            println!("{descriptor}");
            std::io::stdout().flush().unwrap();
            for mut connection in listener.incoming().flatten() {
                let mut request = Vec::new();
                connection
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if connection.read_exact(&mut byte).is_err() {
                        break;
                    }
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                let request = String::from_utf8(request).unwrap();
                let valid = request.contains(&format!("Authorization: Bearer {token}\r\n"))
                    && request.contains(&format!("X-AutoDJ-Live-Session: {session}\r\n"));
                let body = format!(
                    "{{\"protocol\":{{\"major\":1,\"minor\":0}},\"session_id\":\"{session}\",\"sidecar_id\":\"{id}\",\"status\":\"healthy\"}}"
                );
                let response = if valid {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .into()
                };
                let _ = connection.write_all(response.as_bytes());
            }
        }
        _ => panic!("unknown fixture mode"),
    }
    std::thread::sleep(Duration::from_secs(60));
}
