use cc_statusline_rs::{setup, statusline};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => print!("{}", statusline()),
        Some("setup") => {
            let command = match args.next().as_deref() {
                Some("--command") => match args.next() {
                    Some(path) => Some(path),
                    None => usage("--command requires a path"),
                },
                Some(other) => usage(&format!("unknown argument: {other}")),
                None => None,
            };
            match setup(command.as_deref()) {
                Ok(message) => println!("{message}"),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        Some(other) => usage(&format!("unknown subcommand: {other}")),
    }
}

fn usage(complaint: &str) -> ! {
    eprintln!("{complaint}");
    eprintln!("usage: cc-statusline-rs                       render a statusline from stdin JSON");
    eprintln!("       cc-statusline-rs setup [--command <path>]  configure ~/.claude/settings.json");
    std::process::exit(2);
}
