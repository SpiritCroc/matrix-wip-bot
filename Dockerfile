FROM docker.io/rust:1.89.0-trixie

# ImageMagick & other build deps
RUN apt-get update \
 && apt-get -y install imagemagick build-essential clang cmake

# TTS - https://github.com/thewh1teagle/piper-rs/blob/076924bcc5cdc98898dfb083c81bfa46ef498db3/examples/usage.rs
RUN wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts_r/medium/en_US-libritts_r-medium.onnx \
 && wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts_r/medium/en_US-libritts_r-medium.onnx.json

# WIP-Bot

WORKDIR /usr/src/matrix-wip-bot
COPY . .

RUN rustup component add rustfmt
RUN cargo install --path .

CMD ["matrix-wip-bot"]
