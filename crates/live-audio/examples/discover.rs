#[cfg(windows)]
fn main() {
    let inventory = live_audio::discovery::discover();
    println!("Read-only audio inventory. All capabilities are UNPROBED; no streams opened.");
    for endpoint in inventory.endpoints {
        println!(
            "{:?} {:?}: {:?} [{}]",
            endpoint.direction, endpoint.state, endpoint.name, endpoint.id
        );
    }
    for driver in inventory.asio_drivers {
        println!(
            "ASIO x64 registered/unprobed: {:?} [{}]",
            driver.registration, driver.clsid
        );
    }
    for issue in &inventory.issues {
        eprintln!("Discovery issue: {issue}");
    }
    if !inventory.issues.is_empty() {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Audio discovery currently requires Windows.");
    std::process::exit(1);
}
