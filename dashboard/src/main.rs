//! `daiorama-dashboard` — the optional dashboard module (03, 09). It is a plain manager client:
//! connection management (D3), a loopback-only HTTP server (Q2), and a static UI. No manager
//! code knows this binary exists.

mod client;
mod server;
mod store;

use std::path::PathBuf;
use std::process::ExitCode;

use store::Connections;

const USAGE: &str = "\
daiorama-dashboard — optional dashboard module

USAGE:
  daiorama-dashboard serve [--bind 127.0.0.1:8760] [--ui <dir>] [--config <file>]
  daiorama-dashboard connections [--config <file>]
  daiorama-dashboard connect <address> [--config <file>]
  daiorama-dashboard remove <name> [--config <file>]
  daiorama-dashboard token set|remove <connection> [--config <file>]        (set reads stdin)
  daiorama-dashboard scope-token set|remove <connection> [--config <file>]  (set reads stdin)
";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        print!("{USAGE}");
        return Ok(());
    };
    let (command_args, config) = split_config(&args[1..]);
    match command {
        "serve" => {
            let mut bind = "127.0.0.1:8760".to_string();
            let mut ui = None;
            let mut index = 0;
            while index < command_args.len() {
                match command_args[index].as_str() {
                    "--bind" => {
                        index += 1;
                        bind = command_args
                            .get(index)
                            .cloned()
                            .ok_or("--bind needs a value")?;
                    }
                    "--ui" => {
                        index += 1;
                        ui = Some(PathBuf::from(
                            command_args.get(index).ok_or("--ui needs a value")?,
                        ));
                    }
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
                index += 1;
            }
            let store = Connections::load(&config)?;
            let ui_dir = match ui {
                Some(dir) => dir,
                None => default_ui_dir()?,
            };
            let ui = server::Ui::load(&ui_dir)?;
            let (host, port) = server::parse_bind(&bind)?;
            server::serve(
                server::App {
                    connections: std::sync::Arc::new(store),
                    ui: std::sync::Arc::new(ui),
                },
                &host,
                port,
            )
            .await
        }
        "connections" => {
            let store = Connections::load(&config)?;
            if store.connections.is_empty() {
                println!("no connections");
            }
            for connection in &store.connections {
                println!(
                    "{}\t{}\tinstance-token:{}\tscope-secret:{}",
                    connection.name,
                    connection.address,
                    if connection.token.is_empty() {
                        "missing"
                    } else {
                        "set"
                    },
                    if connection.scope_token.is_empty() {
                        "missing"
                    } else {
                        "set"
                    },
                );
            }
            Ok(())
        }
        "connect" => {
            // 04 grammar: `dashboard connect <address>` — the address is the default label.
            let address = command_args.first().ok_or("connect needs an address")?;
            if !valid_address(address) {
                return Err("address must be host:port".into());
            }
            let mut store = Connections::load(&config)?;
            match store.find_mut(address) {
                Some(connection) => connection.address = address.clone(),
                None => store.connections.push(store::Connection {
                    name: address.clone(),
                    address: address.clone(),
                    token: String::new(),
                    scope_token: String::new(),
                }),
            }
            store.save(&config)?;
            println!(
                "connection {address} added (tokens: 'token set {address}', 'scope-token set {address}')"
            );
            Ok(())
        }
        "remove" => {
            let name = command_args.first().ok_or("remove needs a name")?;
            let mut store = Connections::load(&config)?;
            let before = store.connections.len();
            store
                .connections
                .retain(|connection| connection.name != *name);
            if store.connections.len() == before {
                return Err(format!("unknown connection {name:?}"));
            }
            store.save(&config)?;
            println!("removed {name:?}");
            Ok(())
        }
        "token" | "scope-token" => {
            let field = if command == "token" {
                "token"
            } else {
                "scope_token"
            };
            let verb = command_args.first().map(String::as_str).unwrap_or_default();
            let name = command_args.get(1).ok_or("connection is required")?;
            let mut store = Connections::load(&config)?;
            match verb {
                "set" => {
                    let secret = store::read_secret()?;
                    let connection = store.find_mut(name).ok_or_else(|| {
                        format!("unknown connection {name:?}; run 'connect <address>' first")
                    })?;
                    if field == "token" {
                        connection.token = secret;
                    } else {
                        connection.scope_token = secret;
                    }
                    store.save(&config)?;
                    // Value never printed, matching the manager token rule (07).
                    println!("{field} updated for {name} (0600)");
                }
                "remove" => {
                    let connection = store.find_mut(name).ok_or_else(|| {
                        format!("unknown connection {name:?}; run 'connect <address>' first")
                    })?;
                    if field == "token" {
                        connection.token.clear();
                    } else {
                        connection.scope_token.clear();
                    }
                    store.save(&config)?;
                    println!("{field} removed for {name}");
                }
                other => {
                    return Err(format!(
                        "{command} takes `set|remove <connection>`, got {other:?}\n{USAGE}"
                    ));
                }
            }
            Ok(())
        }
        "--help" | "-h" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    }
}

fn split_config(args: &[String]) -> (Vec<String>, PathBuf) {
    let mut rest = Vec::new();
    let mut config = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--config" {
            index += 1;
            config = args.get(index).cloned();
        } else {
            rest.push(args[index].clone());
        }
        index += 1;
    }
    (
        rest,
        config.map(PathBuf::from).unwrap_or_else(store::store_path),
    )
}

fn default_ui_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| format!("current exe: {err}"))?;
    let dir = exe
        .parent()
        .ok_or("binary has no parent directory")?
        .join("ui");
    Ok(dir)
}

fn valid_address(address: &str) -> bool {
    match address.rsplit_once(':') {
        Some((host, port)) => !host.is_empty() && port.parse::<u16>().is_ok(),
        None => false,
    }
}
