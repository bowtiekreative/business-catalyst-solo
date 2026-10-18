use catalyst_core::{Project, Result, create_demo};

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!("Catalyst Solo — Rust starter\n\nUsage:\n  catalyst status <folder>   Scan and print the Site Map and issues\n  catalyst history <folder>  List Catalyst commits (without source snapshots)\n  catalyst demo <new-folder> Create a disposable sample Git repository\n\nDesktop: cargo run -p catalyst-desktop -- <folder>");
        return Ok(());
    }
    if args.len() != 2 { return Err("Use catalyst --help for supported commands".into()); }
    match args[0].as_str() {
        "demo" => {
            let project = create_demo(&args[1])?;
            println!("Created sample at {}", project.root().display());
        }
        "status" => println!("{}", serde_json::to_string_pretty(&Project::open(&args[1])?.scan()?)?),
        "history" => {
            for change in Project::open(&args[1])?.history()? {
                println!("{}  {}  {}", change.commit, change.review.path, change.review.summary);
            }
        }
        _ => return Err("Unknown command. Use catalyst --help.".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
