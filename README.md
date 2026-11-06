# Ferrum

A fast, cross-platform Git client written in Rust.

## Structure

- `crates/ferrum-core` — Git logic (built on `git2`), no UI code
- `crates/ferrum-app` — desktop app (built on `egui`/`eframe`), binary `ferrum`

## Building

The toolchain runs in a container, nothing needs to be installed on the host
besides Podman. The `dev` helper lives in `~/containers` (see its README).

```sh
dev rust cargo build            # build
dev rust cargo test             # tests
dev rust cargo clippy --all-targets
./target/debug/ferrum [path...] # run on the host
```

`FERRUM_SCREENSHOT=out.ppm ./target/debug/ferrum <repo>` saves a screenshot of
the window and quits, handy for checking UI changes.

The image is based on the same Fedora release as the host, so the binary runs
directly on the machine.

See [ROADMAP.md](ROADMAP.md) for planned features.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
