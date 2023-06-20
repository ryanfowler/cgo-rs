fn main() {
    cgo::Build::new()
        .trimpath(true)
        .ldflags("-s -w")
        .change_dir("./tests/example")
        .package("main.go")
        .build("integrationtest");
}
