use log::{error, info, warn};
use simple_logger::SimpleLogger;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};
use structopt::StructOpt;
use toml::Value;

#[derive(StructOpt, Debug)]
#[structopt(name = "remocom", bin_name = "cargo")]
enum Options {
    #[structopt(name = "remote")]
    Remote {
        #[structopt(short = "r", long = "remote", help = "Remote ssh build server")]
        remote: Option<String>,
    },
}

/// Tries to parse the file. Logs warnings and return [`None`] if during reading or
/// parsing errors occur. 
/// Otherwise, returns [`Some(value)`].
fn config_from_file(config_path: &Path) -> Option<Value> {
    let config_file = std::fs::read_to_string(config_path)
        .map_err(|e| {
            warn!(
                "Can't parse config file '{}' error(: {}",
                config_path.to_string_lossy(),
                e
            );
        })
        .ok()?;
    
    let value = config_file
        .parse::<Value>()
        .map_err(|e| {
            warn!(
                "Can't parse config file '{}' error(: {}",
                config_path.to_string_lossy(),
                e
            );
        })
        .ok()?;
    
        Some(value)
}

fn main() {
    SimpleLogger::new().init().unwrap();
    log::info!("Log set");

    let Options::Remote {
        remote,
    } = Options::from_args();

    let mut cli_metadata = cargo_metadata::MetadataCommand::new();
    let manifest_path = "Cargo.toml"; //TODO Implement as a cli argument option
    cli_metadata.manifest_path(manifest_path).no_deps();

    let project_metadata = cli_metadata.exec().unwrap();
    let project_dir = project_metadata.workspace_root;

    let config_options = vec![
        config_from_file(&project_dir.join("remocom-config.toml")),
        xdg::BaseDirectories::with_prefix("remocom")
            .ok()
            .and_then(|base| base.find_config_file("remocom-config.toml"))
            .and_then(|p: PathBuf| config_from_file(&p)),
    ];

    let build_server = remote
        .or_else(|| {
            config_options 
                .into_iter()
                .flat_map(|config| config.and_then(|c| c["remote"].as_str().map(String::from)))
                .next()
    })
    .unwrap_or_else(|| {
        error!("No remote server defined (use remcom-config or --remote flag)");
        exit(-3);
    });
}
