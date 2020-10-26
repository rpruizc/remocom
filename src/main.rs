use log::{error, info, warn};
use simple_logger::SimpleLogger;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::{Command, exit, Stdio},
};
use structopt::StructOpt;
use toml::Value;

#[derive(StructOpt, Debug)]
#[structopt(name = "remocom", bin_name = "remocom")]
enum Opts {
    #[structopt(name = "remote")]
    Remote {
        #[structopt(
            short = "r",
            long = "remote", 
            help = "Remote ssh build server")]
        remote: Option<String>,

        #[structopt(
            short = "b",
            long = "build-env",
            help = "Set remote environment variables. RUST_BACKTRACE, CC, LIB, etc. ",
            default_value = "RUST_BACKTRACE=1",
        )]
        build_env: String,

        #[structopt(
            short = "d",
            long = "rustup-default",
            help = "Rustup default (stable|beta|nightly)",
            default_value = "stable",
        )]
        rustup_default: String,

        #[structopt(
            short = "e",
            long = "env",
            help = "Environment profile. default_value = /etc/profile",
            default_value = "/etc/profile",
        )] 
        env: String,

        #[structopt(
            short = "c",
            long = "copy-back",
            help = "Transfers the target folder or file back to the local machine",
        )] 
        copy_back: Option<Option<String>>,

        #[structopt(
            long = "no-copy-lock",
            help = "Do not transfer the Cargo.lock back to the local machine",
        )] 
        no_copy_lock: bool,

        #[structopt(
            long = "manifest-path",
            help = "Path to the manifest to execute",
            default_value = "Cargo.toml",
            parse(from_os_str)
        )]
        manifest_path: PathBuf,

        #[structopt(
            short = "h",
            long = "transfer-hidden",
            help = "Transfer hidden files and directories to the build server",
        )] 
        hidden: bool,

        #[structopt(help = "cargo command that will be executed remotely")] 
        command: String,

        #[structopt(
            help = "cargo options and flags that will be applied remotely",
            name = "remote options",
        )] 
        options: Vec<String>,
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
    info!("Log set");

    let Opts::Remote {
        remote,
        build_env,
        rustup_default,
        env,
        copy_back,
        no_copy_lock,
        manifest_path,
        hidden,
        command,
        options,
    } = Opts::from_args();

    let mut cli_metadata = cargo_metadata::MetadataCommand::new();
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

    // This is a unique build path created using the project's hashed dir name.
    let mut hasher = DefaultHasher::new();
    project_dir.hash(&mut hasher);
    let build_path = format!("~/remote-builds/{}/", hasher.finish());

    info!("Sources are being transferred to your build server.");
    // Transfers the project to the user's build server
    let mut rsync_to = Command::new("rsync");

    rsync_to
        .arg("-a".to_owned())
        .arg("--delete")
        .arg("--compress")
        .arg("--info=progress2")
        .arg("--exclude")
        .arg("--target");
    
        if !hidden {
            rsync_to.arg("--exclude").arg(".*");
        }

        rsync_to
            .arg("--rsync-path")
            .arg("mkdir -p remote-builds && rsync")
            .arg(format!("{}/", project_dir.to_string_lossy()))
            .arg(format!("{}:{}", build_server, build_path))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::inherit())
            .output()
            .unwrap_or_else(|e| {
                error!("Failed to transfer project to build server (error: {})", e);
                exit(-4);
            });
        
        log::info!("Build ENV: {:?}", build_env);
        log::info!("Environment profile: {:?}", env);
        log::info!("Build path: {:?}", build_path);

        let build_command = format!(
            "source {}; rustup default {}; cd {}; {} cargo {} {}",
            env,
            rustup_default,
            build_path,
            build_env,
            command,
            options.join(" ")
        );

        info!("Starting build process...");
        let output = Command::new("ssh")
            .arg("-t")
            .arg(&build_server)
            .arg(build_command)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::inherit())
            .output()
            .unwrap_or_else(|e| {
                error!("Failed to run cargo command remotely (error: {})", e);
                exit(-5);
            });
        
        if let Some(file_name) = copy_back {
            log::info!("Transferring artifacts back to client");
            let file_name = file_name.unwrap_or_else(String::new);
            Command::new("rsync")
                .arg("-a")
                .arg("--delete")
                .arg("--compress")
                .arg("--info-progress2")
                .arg(format!("{}:{}/target/{}", build_server, build_path, file_name))
                .arg(format!("{}/target/{}", project_dir.to_string_lossy(), file_name))
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .stdin(Stdio::inherit())
                .output()
                .unwrap_or_else(|e| {
                    log::error!(
                        "Failed to transfer target back to local machine (error: {})",
                        e
                    );
                    exit(-6);
                });
        }

        if !no_copy_lock {
            log::info!("Transferring Cargo.lock file back to the client");
            Command::new("rsync")
                .arg("-a")
                .arg("--delete")
                .arg("--compress")
                .arg("--info=progress2")
                .arg(format!("{}:{}/Cargo.lock", build_server, build_path))
                .arg(format!("{}/Cargo.lock", project_dir.to_string_lossy()))
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .stdin(Stdio::inherit())
                .output()
                .unwrap_or_else(|e| {
                    log::error!(
                        "Failed to transfer Cargo.lock back to local machine (error: {})",
                        e
                    );
                    exit(-7);
                });
        }

        if !output.status.success() {
            exit(output.status.code().unwrap_or(1))
        }
}
