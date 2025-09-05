FROM docker.io/rust:1.89.0-bookworm

# ImageMagick - from https://github.com/nlfiedler/magick-rust/blob/master/docker/Dockerfile
# NOTE: once Debian packages imagemagick 7.1.1-26 or later, can switch to that
# (required by https://github.com/nlfiedler/magick-rust)
RUN apt-get update \
 && apt-get -y install curl build-essential clang pkg-config libjpeg-turbo-progs libpng-dev \
 && rm -rfv /var/lib/apt/lists/*

# TTS - https://github.com/thewh1teagle/piper-rs/blob/076924bcc5cdc98898dfb083c81bfa46ef498db3/examples/usage.rs
RUN wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts_r/medium/en_US-libritts_r-medium.onnx \
 && wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts_r/medium/en_US-libritts_r-medium.onnx.json

ENV MAGICK_VERSION 7.1

RUN curl https://imagemagick.org/archive/ImageMagick.tar.gz | tar xz \
 && cd ImageMagick-${MAGICK_VERSION}* \
 && ./configure --with-magick-plus-plus=no --with-perl=no \
 && make \
 && make install \
 && cd .. \
 && rm -r ImageMagick-${MAGICK_VERSION}*
RUN ldconfig /usr/local/lib

# WIP-Bot

WORKDIR /usr/src/matrix-wip-bot
COPY . .

RUN cargo install --path .

CMD ["matrix-wip-bot"]
