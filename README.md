# cgo-rs

[![](https://img.shields.io/crates/v/cgo.svg)](https://crates.io/crates/cgo)

A library for build scripts to compile custom Go code, inspired by the
excellent [cc](https://docs.rs/cc/latest/cc) crate.

It is intended that you use this library from within your `build.rs` file by
adding the cgo crate to your [`build-dependencies`](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#build-dependencies):

```toml
[build-dependencies]
cgo = "*"
```

# Examples

The following example will statically compile the Go package and instruct
cargo to link the resulting library (`libexample`).

```rust
fn main() {
    cgo::Build::new()
        .package("pkg/example/main.go")
        .build("example");
}
```
