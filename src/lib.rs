use std::{
    env,
    path::{Path, PathBuf},
    process,
};

pub struct Build {
    build_mode: BuildMode,
    cargo_metadata: bool,
    out_dir: Option<PathBuf>,
    packages: Vec<PathBuf>,
    trimpath: bool,
}

impl Default for Build {
    fn default() -> Self {
        Self::new()
    }
}

impl Build {
    pub fn new() -> Self {
        Build {
            build_mode: BuildMode::default(),
            cargo_metadata: true,
            out_dir: None,
            packages: Vec::default(),
            trimpath: false,
        }
    }

    pub fn cargo_metadata(&mut self, cargo_metadata: bool) -> &mut Build {
        self.cargo_metadata = cargo_metadata;
        self
    }

    pub fn out_dir<P: AsRef<Path>>(&mut self, out_dir: P) -> &mut Build {
        self.out_dir = Some(out_dir.as_ref().to_owned());
        self
    }

    pub fn package<P: AsRef<Path>>(&mut self, package: P) -> &mut Build {
        self.packages.push(package.as_ref().to_owned());
        self
    }

    pub fn trimpath(&mut self, trimpath: bool) -> &mut Build {
        self.trimpath = trimpath;
        self
    }

    pub fn build(&self, output: &str) {
        if let Err(err) = self.try_build(output) {
            eprintln!("\n\nerror occurred: {}\n", err);
            process::exit(1);
        }
    }

    pub fn try_build(&self, output: &str) -> Result<(), Error> {
        let goos = goos_from_env()?;
        let goarch = goarch_from_env()?;

        let lib_name = self.format_lib_name(output);
        let out_dir = match &self.out_dir {
            Some(out_dir) => out_dir.clone(),
            None => get_env_var("OUT_DIR")?.into(),
        };
        let out_path = out_dir.join(lib_name);

        let mut cmd = process::Command::new("go");
        cmd.env("CGO_ENABLED", "1")
            .env("GOOS", goos)
            .env("GOARCH", goarch)
            .arg("build")
            .args(["-buildmode", &self.build_mode.to_string()])
            .args(["-o".into(), out_path]);
        if self.trimpath {
            cmd.arg("-trimpath");
        }
        for package in &self.packages {
            cmd.arg(package);
        }

        let status = match cmd.output() {
            Ok(output) => output.status,
            Err(err) => {
                return Err(Error::new(
                    ErrorKind::ToolExecError,
                    &format!("failed to execute go command: {}", err),
                ));
            }
        };

        if self.cargo_metadata {
            println!("cargo:rustc-link-search=native={}", out_dir.display());
            println!("cargo:rustc-link-lib=static={}", output);
        }

        if status.success() {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::ToolExecError,
                &format!("failed to build Go library: status {}", status),
            ))
        }
    }

    fn format_lib_name(&self, output: &str) -> PathBuf {
        let mut lib = String::with_capacity(output.len() + 7);
        lib.push_str("lib");
        lib.push_str(output);
        lib.push_str(match self.build_mode {
            BuildMode::CArchive => {
                if cfg!(windows) {
                    ".lib"
                } else {
                    ".a"
                }
            }
            BuildMode::CShared => {
                if cfg!(windows) {
                    ".dll"
                } else {
                    ".so"
                }
            }
        });
        lib.into()
    }
}

#[derive(Clone, Debug, Default)]
pub enum BuildMode {
    #[default]
    CArchive,
    CShared,
}

impl std::fmt::Display for BuildMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CArchive => "c-archive",
            Self::CShared => "c-shared",
        })
    }
}

#[derive(Clone, Debug)]
enum ErrorKind {
    EnvVarNotFound,
    InvalidGOARCH,
    InvalidGOOS,
    ToolExecError,
}

#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    fn new(kind: ErrorKind, message: &str) -> Self {
        Error {
            kind,
            message: message.to_owned(),
        }
    }
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

fn goarch_from_env() -> Result<String, Error> {
    let target_arch = get_env_var("CARGO_CFG_TARGET_ARCH")?;

    // From the following references:
    // https://doc.rust-lang.org/reference/conditional-compilation.html#target_arch
    // https://go.dev/doc/install/source#environment
    let goarch = match target_arch.as_str() {
        "x86" => "386",
        "x86_64" => "amd64",
        "powerpc64" => "ppc64",
        "aarch64" => "arm64",
        "mips" | "mips64" | "arm" => &target_arch,
        _ => {
            return Err(Error::new(
                ErrorKind::InvalidGOARCH,
                &format!("unexpected target arch {}", target_arch),
            ))
        }
    };
    Ok(goarch.to_string())
}

fn goos_from_env() -> Result<String, Error> {
    let target_os = get_env_var("CARGO_CFG_TARGET_OS")?;

    // From the following references:
    // https://doc.rust-lang.org/reference/conditional-compilation.html#target_os
    // https://go.dev/doc/install/source#environment
    let goos = match target_os.as_str() {
        "macos" => "darwin",
        "windows" | "ios" | "linux" | "android" | "freebsd" | "dragonfly" | "openbsd"
        | "netbsd" => &target_os,
        _ => {
            return Err(Error::new(
                ErrorKind::InvalidGOOS,
                &format!("unexpected target os {}", target_os),
            ))
        }
    };
    Ok(goos.to_string())
}

fn get_env_var(key: &str) -> Result<String, Error> {
    env::var(key).map_err(|_| {
        Error::new(
            ErrorKind::EnvVarNotFound,
            &format!("could not find environment variable {}", key),
        )
    })
}
