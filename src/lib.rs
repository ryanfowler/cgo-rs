//! A library for build scripts to compile custom Go code, inspired by the
//! excellent [cc](https://docs.rs/cc/latest/cc) crate.
//!
//! It is intended that you use this library from within your `build.rs` file by
//! adding the cgo crate to your [`build-dependencies`](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#build-dependencies):
//!
//! ```toml
//! [build-dependencies]
//! cgo = "*"
//! ```
//!
//! # Examples
//!
//! The following example will statically compile the Go package and instruct
//! cargo to link the resulting library (`libexample`).
//!
//! ```no_run
//! fn main() {
//!     cgo::Build::new()
//!         .package("pkg/example/main.go")
//!         .build("example");
//! }
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::needless_doctest_main)]

use std::{
    env,
    ffi::{OsStr, OsString},
    fmt::Write,
    path::{Path, PathBuf},
    process,
};

/// A builder for the compilation of a Go library.
#[derive(Clone, Debug)]
pub struct Build {
    build_mode: BuildMode,
    cargo_metadata: bool,
    change_dir: Option<PathBuf>,
    ldflags: Option<OsString>,
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
    /// Returns a new instance of `Build` with the default configuration.
    pub fn new() -> Self {
        Build {
            build_mode: BuildMode::default(),
            cargo_metadata: true,
            change_dir: None,
            ldflags: None,
            out_dir: None,
            packages: Vec::default(),
            trimpath: false,
        }
    }

    /// Instruct the builder to use the provided build mode.
    ///
    /// For more information, see https://pkg.go.dev/cmd/go#hdr-Build_modes
    ///
    /// By default, 'CArchive' is used.
    pub fn build_mode(&mut self, build_mode: BuildMode) -> &mut Self {
        self.build_mode = build_mode;
        self
    }

    /// Instruct the builder to automatically output cargo metadata or not.
    ///
    /// By default, cargo metadata is enabled.
    pub fn cargo_metadata(&mut self, cargo_metadata: bool) -> &mut Self {
        self.cargo_metadata = cargo_metadata;
        self
    }

    /// Instruct the builder to change to `dir` before running the `go build`
    /// command. All other paths are interpreted after changing directories.
    pub fn change_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Self {
        self.change_dir = Some(dir.as_ref().to_owned());
        self
    }

    /// Instruct the builder to pass in the provided ldflags during compilation.
    pub fn ldflags<P: AsRef<OsStr>>(&mut self, ldflags: P) -> &mut Self {
        self.ldflags = Some(ldflags.as_ref().to_os_string());
        self
    }

    /// Instruct the builder to use the provided directory for output.
    ///
    /// By default, the cargo-provided `OUT_DIR` env var is used.
    pub fn out_dir<P: AsRef<Path>>(&mut self, out_dir: P) -> &mut Self {
        self.out_dir = Some(out_dir.as_ref().to_owned());
        self
    }

    /// Instruct the builder to compile the provided Go package.
    ///
    /// Note: The `go build` command can be passed multiple packages and this
    /// method may be called more than once.
    pub fn package<P: AsRef<Path>>(&mut self, package: P) -> &mut Self {
        self.packages.push(package.as_ref().to_owned());
        self
    }

    /// Instruct the builder to enable the `-trimpath` flag during compilation.
    pub fn trimpath(&mut self, trimpath: bool) -> &mut Self {
        self.trimpath = trimpath;
        self
    }

    /// Builds the Go package, generating the file `output`.
    ///
    /// # Panics
    ///
    /// Panics if any error occurs during compilation.
    pub fn build(&self, output: &str) {
        if let Err(err) = self.try_build(output) {
            eprintln!("\n\nerror occurred: {}\n", err);
            process::exit(1);
        }
    }

    /// Builds the Go package, generating the file `output`.
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
            .env("CC", get_cc())
            .env("CXX", get_cxx())
            .arg("build");
        if let Some(change_dir) = &self.change_dir {
            // This flag is required to be the first flag used in the command as
            // of Go v1.21: https://tip.golang.org/doc/go1.21#go-command
            cmd.args([&"-C".into(), change_dir]);
        }
        if let Some(ldflags) = &self.ldflags {
            cmd.args([&"-ldflags".into(), ldflags]);
        }
        if self.trimpath {
            cmd.arg("-trimpath");
        }
        cmd.args(["-buildmode", &self.build_mode.to_string()]);
        cmd.args(["-o".into(), out_path]);
        for package in &self.packages {
            cmd.arg(package);
        }

        let build_output = match cmd.output() {
            Ok(build_output) => build_output,
            Err(err) => {
                return Err(Error::new(
                    ErrorKind::ToolExecError,
                    &format!("failed to execute go command: {}", err),
                ));
            }
        };

        if self.cargo_metadata {
            let link_kind = match self.build_mode {
                BuildMode::CArchive => "static",
                BuildMode::CShared => "dylib",
            };
            println!("cargo:rustc-link-lib={}={}", link_kind, output);
            println!("cargo:rustc-link-search=native={}", out_dir.display());
        }

        if build_output.status.success() {
            return Ok(());
        }

        let mut message = format!(
            "failed to build Go library ({}). Build output:",
            build_output.status
        );

        let mut push_output = |stream_name, bytes| {
            let string = String::from_utf8_lossy(bytes);
            let string = string.trim();

            if string.is_empty() {
                return;
            }

            write!(&mut message, "\n=== {stream_name}:\n{string}").unwrap();
        };

        push_output("stdout", &build_output.stdout);
        push_output("stderr", &build_output.stderr);

        Err(Error::new(ErrorKind::ToolExecError, &message))
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

/// BuildMode to be used during compilation.
///
/// Refer to the [Go docs](https://pkg.go.dev/cmd/go#hdr-Build_modes)
/// for more information.
#[derive(Clone, Debug, Default)]
pub enum BuildMode {
    /// Build the listed main package, plus all packages it imports,
    /// into a C archive file. The only callable symbols will be those
    /// functions exported using a cgo //export comment. Requires
    /// exactly one main package to be listed.
    #[default]
    CArchive,
    /// Build the listed main package, plus all packages it imports,
    /// into a C shared library. The only callable symbols will
    /// be those functions exported using a cgo //export comment.
    /// Requires exactly one main package to be listed.
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

/// Kind of error that was encountered.
#[derive(Clone, Debug)]
enum ErrorKind {
    EnvVarNotFound,
    InvalidGOARCH,
    InvalidGOOS,
    ToolExecError,
}

/// Represents an internal error that occurred, including an explanation.
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

fn get_cc() -> PathBuf {
    cc::Build::new().get_compiler().path().to_path_buf()
}

fn get_cxx() -> PathBuf {
    cc::Build::new()
        .cpp(true)
        .get_compiler()
        .path()
        .to_path_buf()
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
