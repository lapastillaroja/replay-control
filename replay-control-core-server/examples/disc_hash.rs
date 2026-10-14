//! Read-only RA hash diagnostic: cargo run -p replay-control-core-server
//! --features library --example disc_hash -- <system> <image> [<image> ...]
#[cfg(feature = "library")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use replay_control_core_server::rc_hash_disc::compute_disc_rc_hash;
    use std::{env, io, path::Path};

    let mut arguments = env::args().skip(1);
    let system = arguments.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: disc_hash <system> <image> [<image> ...]",
        )
    })?;
    for path in arguments {
        let hash = compute_disc_rc_hash(&system, Path::new(&path))?;
        println!("{}\t{path}", hash.as_deref().unwrap_or("unidentified"));
    }
    Ok(())
}

#[cfg(not(feature = "library"))]
fn main() {
    eprintln!("Enable the library feature to use this example.");
}
