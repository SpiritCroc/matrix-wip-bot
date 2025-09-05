# matrix-wip-bot

Matrix bot written with the main goal being that I want to learn Rust.
Uses the matrix-rust-sdk directly rather than a dedicated bot framework.

This bot will forever be a work-in-progress, unless I find a better name for it at some point.

## Quick start

First, copy `example-config.yaml` to `config.yaml` and adjust it as necessary.

Then, to run natively (requires Rust/`cargo`, the dev library for `imagemagick`, and the DejaVu-Sans font installed / available in `convert -list font`):

```
cargo run
```

If you don't want to worry about dependencies, you can use a container e.g. with

```
docker-compose up
```

or

```
podman-compose up
```

## TTS

Text-to-speach uses [piper-rs](https://github.com/thewh1teagle/piper-rs/), to get it to work download a model +
config from [huggingface](https://huggingface.co/rhasspy/piper-voices/tree/main) and point to the config in your bot config.

## Verbose logging

See options for [env_logger](https://docs.rs/env_logger/latest/env_logger/), e.g. run with environment variable `RUST_LOG=trace`.
